//! Inbox: what is waiting for a session or a user in one repository.
//!
//! The inbox is a *live projection* of the authoritative records (owned tasks,
//! pending handoffs, open and answered questions), computed on every request.
//! It is not a snapshot: an item disappears when its record stops being
//! actionable, and clients that want "only new" pass `since`. Items only locate
//! records; read the record before acting on it.
//!
//! Each item also carries the caller's *personal read mark* (`read_state`). A
//! mark names the object version that was read; a newer version of the same
//! object is unread again. Marks only express reading: they never answer,
//! accept, cancel or close anything, and they are not shared between actors.

use chrono::{DateTime, Utc};
use rg_db::ops::{collab_handoff_ops, collab_inbox_read_ops, collab_question_ops, collab_task_ops};
use sea_orm::{ColumnTrait, Condition, DatabaseConnection};
use serde::Serialize;

use super::error::{CollabError, CollabResult};
use super::{validation, Actor, ORIGIN_USER_CONTENT};
use rg_db::entities::collab_question;

pub const ASSIGNED_TASK: &str = "assigned_task";
pub const PENDING_HANDOFF: &str = "pending_handoff";
pub const OPEN_QUESTION: &str = "open_question";
pub const QUESTION_ANSWER: &str = "question_answer";

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct InboxItem {
    /// `<type>:<object id>`; stable across requests.
    pub item_key: String,
    /// `assigned_task` | `pending_handoff` | `open_question` | `question_answer`.
    pub r#type: &'static str,
    pub task_id: String,
    pub object_id: String,
    /// Moment the record last became relevant (update or creation time).
    pub occurred_at: DateTime<Utc>,
    /// Current state of the record (task status, handoff status, ...).
    pub state: String,
    /// The record's version now; `observed_version` of a read mark refers to it.
    pub object_version: i64,
    /// `read` when the caller marked this version (or a newer one) as read,
    /// otherwise `unread`. Reading never changes the record or its state.
    pub read_state: &'static str,
    /// Highest version the caller marked as read, when it ever did.
    pub last_read_version: Option<i64>,
    pub last_read_at: Option<DateTime<Utc>>,
    /// Title or summary text written by a client.
    pub title: String,
    pub origin: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct Inbox {
    pub items: Vec<InboxItem>,
    pub as_of: DateTime<Utc>,
    /// More items matched than `limit`; narrow with `since` or work the list down.
    pub truncated: bool,
    /// Always `live_query`: no snapshot and no cursor. Read marks are the
    /// caller's own and are looked up on every request.
    pub consistency: &'static str,
}

fn item(
    r#type: &'static str,
    task_id: String,
    object_id: String,
    occurred_at: DateTime<Utc>,
    state: String,
    object_version: i64,
    title: String,
) -> InboxItem {
    InboxItem {
        item_key: format!("{type}:{object_id}"),
        r#type,
        task_id,
        object_id,
        occurred_at,
        state,
        object_version,
        read_state: "unread",
        last_read_version: None,
        last_read_at: None,
        title,
        origin: ORIGIN_USER_CONTENT,
    }
}

fn finish(
    mut items: Vec<InboxItem>,
    since: Option<DateTime<Utc>>,
    limit: u64,
    now: DateTime<Utc>,
) -> Inbox {
    if let Some(since) = since {
        items.retain(|i| i.occurred_at > since);
    }
    items.sort_by(|a, b| {
        b.occurred_at
            .cmp(&a.occurred_at)
            .then_with(|| a.item_key.cmp(&b.item_key))
    });
    let truncated = items.len() as u64 > limit;
    items.truncate(limit as usize);
    Inbox {
        items,
        as_of: now,
        truncated,
        consistency: "live_query",
    }
}

fn question_item(q: collab_question::Model) -> InboxItem {
    let (kind, at) = if q.status == "open" {
        (OPEN_QUESTION, q.created_at)
    } else {
        (QUESTION_ANSWER, q.updated_at)
    };
    item(kind, q.task_id, q.id, at, q.status, q.version, q.title)
}

/// What is waiting for one session: tasks it owns, handoffs addressed to it,
/// open questions addressed to it or to repository writers, and answers to the
/// questions it asked.
pub async fn for_session(
    db: &DatabaseConnection,
    repo_id: i64,
    session_id: &str,
    since: Option<DateTime<Utc>>,
    limit: u64,
    now: DateTime<Utc>,
) -> CollabResult<Inbox> {
    let cap = limit + 1;
    let mut items = Vec::new();
    for t in collab_task_ops::list_owned_open(db, repo_id, session_id, cap).await? {
        items.push(item(
            ASSIGNED_TASK,
            t.id.clone(),
            t.id,
            t.updated_at,
            t.status,
            t.version,
            t.title,
        ));
    }
    for h in collab_handoff_ops::list_pending_to_session(db, repo_id, session_id, cap).await? {
        items.push(item(
            PENDING_HANDOFF,
            h.task_id,
            h.id,
            h.created_at,
            h.status,
            h.version,
            h.summary,
        ));
    }
    let addressed = Condition::any()
        .add(
            Condition::all()
                .add(collab_question::Column::RecipientKind.eq("session"))
                .add(collab_question::Column::RecipientId.eq(session_id)),
        )
        .add(
            Condition::all()
                .add(collab_question::Column::RecipientKind.eq("repo_writers"))
                .add(
                    Condition::any()
                        .add(collab_question::Column::CreatorKind.ne("session"))
                        .add(collab_question::Column::CreatorId.ne(session_id)),
                ),
        );
    for q in collab_question_ops::list_open(db, repo_id, addressed, cap).await? {
        items.push(question_item(q));
    }
    for q in collab_question_ops::list_answered_by_creator(db, repo_id, "session", session_id, cap)
        .await?
    {
        items.push(question_item(q));
    }
    annotate(
        db,
        repo_id,
        &Actor::session(session_id),
        finish(items, since, limit, now),
    )
    .await
}

/// What is waiting for the signed-in account's user door: open questions
/// addressed to it or to repository writers, and answers to its own questions.
pub async fn for_user(
    db: &DatabaseConnection,
    repo_id: i64,
    user_id: i64,
    since: Option<DateTime<Utc>>,
    limit: u64,
    now: DateTime<Utc>,
) -> CollabResult<Inbox> {
    let me = Actor::user(user_id);
    let cap = limit + 1;
    let addressed = Condition::any()
        .add(
            Condition::all()
                .add(collab_question::Column::RecipientKind.eq("user"))
                .add(collab_question::Column::RecipientId.eq(me.id.clone())),
        )
        .add(
            Condition::all()
                .add(collab_question::Column::RecipientKind.eq("repo_writers"))
                .add(
                    Condition::any()
                        .add(collab_question::Column::CreatorKind.ne("user"))
                        .add(collab_question::Column::CreatorId.ne(me.id.clone())),
                ),
        );
    let mut items = Vec::new();
    for q in collab_question_ops::list_open(db, repo_id, addressed, cap).await? {
        items.push(question_item(q));
    }
    for q in collab_question_ops::list_answered_by_creator(db, repo_id, "user", &me.id, cap).await?
    {
        items.push(question_item(q));
    }
    annotate(db, repo_id, &me, finish(items, since, limit, now)).await
}

/// Fill in the caller's read marks.
async fn annotate(
    db: &DatabaseConnection,
    repo_id: i64,
    actor: &Actor,
    mut inbox: Inbox,
) -> CollabResult<Inbox> {
    let keys: Vec<String> = inbox.items.iter().map(|i| i.item_key.clone()).collect();
    let marks =
        collab_inbox_read_ops::list_for_keys(db, repo_id, actor.kind, &actor.id, &keys).await?;
    for item in &mut inbox.items {
        if let Some(mark) = marks.iter().find(|m| m.item_key == item.item_key) {
            item.last_read_version = Some(mark.observed_version);
            item.last_read_at = Some(mark.read_at);
            if mark.observed_version >= item.object_version {
                item.read_state = "read";
            }
        }
    }
    Ok(inbox)
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ReadMark {
    pub item_key: String,
    /// Highest version marked as read after this call.
    pub observed_version: i64,
    /// The record's version now.
    pub current_version: i64,
    /// `read` when the mark covers the current version.
    pub read_state: &'static str,
    pub read_at: DateTime<Utc>,
}

const NOT_IN_INBOX: &str = "inbox item not found";

fn addressed_to(q: &collab_question::Model, actor: &Actor) -> bool {
    let me = |kind: &str, id: &str| kind == actor.kind && id == actor.id;
    match q.recipient_kind.as_str() {
        "repo_writers" => !me(&q.creator_kind, &q.creator_id),
        kind => q.recipient_id.as_deref().is_some_and(|id| me(kind, id)),
    }
}

/// The version of the record behind `item_key`, if that item is in `actor`'s
/// inbox right now. Everything else reads as absent, so a mark is no way to
/// probe other actors' records.
async fn current_version(
    db: &DatabaseConnection,
    repo_id: i64,
    actor: &Actor,
    item_key: &str,
) -> CollabResult<i64> {
    let absent = || CollabError::not_found(NOT_IN_INBOX);
    let (kind, id) = item_key.split_once(':').ok_or_else(absent)?;
    if !validation::is_uuid(id) {
        return Err(absent());
    }
    match kind {
        ASSIGNED_TASK if actor.kind == "session" => {
            let t = collab_task_ops::find_in_repo(db, repo_id, id)
                .await?
                .ok_or_else(absent)?;
            (t.owner_session_id.as_deref() == Some(actor.id.as_str()) && t.status != "done")
                .then_some(t.version)
                .ok_or_else(absent)
        }
        PENDING_HANDOFF if actor.kind == "session" => {
            let h = collab_handoff_ops::find_in_repo(db, repo_id, id)
                .await?
                .ok_or_else(absent)?;
            (h.to_session_id == actor.id && h.status == "pending")
                .then_some(h.version)
                .ok_or_else(absent)
        }
        OPEN_QUESTION | QUESTION_ANSWER => {
            let q = collab_question_ops::find_in_repo(db, repo_id, id)
                .await?
                .ok_or_else(absent)?;
            let visible = if kind == OPEN_QUESTION {
                q.status == "open" && addressed_to(&q, actor)
            } else {
                q.status == "answered" && q.creator_kind == actor.kind && q.creator_id == actor.id
            };
            visible.then_some(q.version).ok_or_else(absent)
        }
        _ => Err(absent()),
    }
}

/// Mark the object version `observed_version` of an inbox item as read by
/// `actor`. Only an item currently in the actor's inbox can be marked, and only
/// up to the record's current version: a newer version stays unread, a version
/// that does not exist yet is refused. Marking is monotonic and idempotent.
pub async fn mark_read(
    db: &DatabaseConnection,
    repo_id: i64,
    actor: &Actor,
    item_key: &str,
    observed_version: i64,
    now: DateTime<Utc>,
) -> CollabResult<ReadMark> {
    if observed_version < 1 {
        return Err(CollabError::bad_request(
            "observed_version must be at least 1",
        ));
    }
    let current = current_version(db, repo_id, actor, item_key).await?;
    if observed_version > current {
        return Err(CollabError::bad_request(
            "observed_version is newer than the item's current version",
        ));
    }
    let mark = collab_inbox_read_ops::raise(
        db,
        repo_id,
        actor.kind,
        &actor.id,
        item_key,
        observed_version,
        now,
    )
    .await?;
    Ok(ReadMark {
        item_key: mark.item_key,
        observed_version: mark.observed_version,
        current_version: current,
        read_state: if mark.observed_version >= current {
            "read"
        } else {
            "unread"
        },
        read_at: mark.read_at,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    fn at(minutes: i64, key: &str) -> InboxItem {
        let t =
            DateTime::<Utc>::from_timestamp(1_700_000_000, 0).unwrap() + Duration::minutes(minutes);
        item(
            OPEN_QUESTION,
            "t".into(),
            key.into(),
            t,
            "open".into(),
            1,
            "x".into(),
        )
    }

    #[test]
    fn items_sort_newest_first_with_stable_ties_and_truncate_honestly() {
        let now = Utc::now();
        let inbox = finish(
            vec![at(1, "b"), at(3, "z"), at(3, "a"), at(2, "c")],
            None,
            3,
            now,
        );
        let keys: Vec<_> = inbox.items.iter().map(|i| i.object_id.as_str()).collect();
        assert_eq!(keys, vec!["a", "z", "c"]);
        assert!(inbox.truncated);
        assert_eq!(inbox.consistency, "live_query");
    }

    #[test]
    fn since_drops_items_at_or_before_the_watermark() {
        let now = Utc::now();
        let watermark = at(2, "w").occurred_at;
        let inbox = finish(
            vec![at(1, "old"), at(2, "same"), at(3, "new")],
            Some(watermark),
            10,
            now,
        );
        assert_eq!(inbox.items.len(), 1);
        assert_eq!(inbox.items[0].object_id, "new");
        assert!(!inbox.truncated);
    }
}
