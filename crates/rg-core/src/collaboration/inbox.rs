//! Inbox: what is waiting for a session or a user in one repository.
//!
//! The inbox is a *live projection* of the authoritative records (owned tasks,
//! pending handoffs, open and answered questions), computed on every request.
//! It is not a snapshot and has no read marks: an item disappears when its
//! record stops being actionable, and clients that want "only new" pass
//! `since`. Items only locate records; read the record before acting on it.

use chrono::{DateTime, Utc};
use rg_db::ops::{collab_handoff_ops, collab_question_ops, collab_task_ops};
use sea_orm::{ColumnTrait, Condition, DatabaseConnection};
use serde::Serialize;

use super::error::CollabResult;
use super::{Actor, ORIGIN_USER_CONTENT};
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
    /// Always `live_query`: no snapshot, no cursor, no read marks.
    pub consistency: &'static str,
}

fn item(
    r#type: &'static str,
    task_id: String,
    object_id: String,
    occurred_at: DateTime<Utc>,
    state: String,
    title: String,
) -> InboxItem {
    InboxItem {
        item_key: format!("{type}:{object_id}"),
        r#type,
        task_id,
        object_id,
        occurred_at,
        state,
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
    item(kind, q.task_id, q.id, at, q.status, q.title)
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
    Ok(finish(items, since, limit, now))
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
    Ok(finish(items, since, limit, now))
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
