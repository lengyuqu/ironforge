//! Task takeovers: a session claims an abandoned task, or a user reassigns it.
//!
//! A takeover only moves the declared owner of the task and writes an immutable
//! history row in the same transaction. It is not a lock: it grants no Git, CI
//! or approval rights, and it leaves pending handoffs exactly as they were.
//!
//! A claim is refused while the current owner session is still `online` or
//! merely `offline` (a missed heartbeat proves nothing); only a task without
//! owner, or whose owner session ended or expired, can be claimed. A user may
//! reassign explicitly and leaves a reason.

use chrono::{DateTime, Utc};
use rg_db::entities::{agent_session, collab_takeover};
use rg_db::ops::agent_session_ops;
use rg_db::ops::collab_takeover_ops::{self, OwnerChange, TakeoverOutcome};
use sea_orm::{DatabaseConnection, Set};
use serde::Serialize;
use sha2::{Digest, Sha256};

use super::cursor::{self, KeysetPosition};
use super::error::{CollabError, CollabResult};
use super::session::state_of;
use super::task::{self, TaskView};
use super::{validation, Page, ORIGIN_USER_CONTENT};

pub const MAX_REASON_BYTES: usize = 2000;

#[derive(Debug, Clone)]
pub struct TakeoverRequest {
    pub client_takeover_id: String,
    pub expected_task_version: i64,
    /// The owner the caller believes the task has (`None` = unowned).
    pub expected_owner_session_id: Option<String>,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct TakeoverView {
    pub id: String,
    pub task_id: String,
    pub from_session_id: Option<String>,
    pub to_session_id: String,
    /// `session` or `user`: the door used, not proof of who used it.
    pub actor_kind: String,
    pub actor_id: String,
    pub previous_task_version: i64,
    pub new_task_version: i64,
    /// `claimed` or `reassigned`.
    pub outcome: String,
    pub reason: String,
    pub created_at: DateTime<Utc>,
    pub origin: &'static str,
}

fn view(row: collab_takeover::Model) -> TakeoverView {
    TakeoverView {
        id: row.id,
        task_id: row.task_id,
        from_session_id: row.from_session_id,
        to_session_id: row.to_session_id,
        actor_kind: row.actor_kind,
        actor_id: row.actor_id,
        previous_task_version: row.previous_task_version,
        new_task_version: row.new_task_version,
        outcome: row.outcome,
        reason: row.reason,
        created_at: row.created_at,
        origin: ORIGIN_USER_CONTENT,
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct TakeoverResult {
    pub task: TaskView,
    pub takeover: TakeoverView,
    /// The same client id was already applied; nothing changed this time.
    pub replayed: bool,
}

fn payload_hash(task_id: &str, kind: &str, to_session: &str, req: &TakeoverRequest) -> String {
    let canonical = serde_json::json!([
        task_id,
        kind,
        to_session,
        req.expected_task_version,
        req.expected_owner_session_id,
        req.reason
    ]);
    hex::encode(Sha256::digest(canonical.to_string().as_bytes()))
}

fn validate(req: &TakeoverRequest) -> CollabResult<()> {
    validation::client_key("client_takeover_id", &req.client_takeover_id)?;
    validation::text("reason", &req.reason, 1, MAX_REASON_BYTES)?;
    if let Some(owner) = &req.expected_owner_session_id {
        if !validation::is_uuid(owner) {
            return Err(CollabError::bad_request(
                "expected_owner_session_id must be a session id",
            ));
        }
    }
    Ok(())
}

/// Who is moving the task, through which door, to whom.
struct Mover<'a> {
    actor_kind: &'a str,
    actor_id: &'a str,
    /// `claimed` or `reassigned`.
    outcome: &'a str,
    to_session_id: &'a str,
}

async fn apply(
    db: &DatabaseConnection,
    repo_id: i64,
    task_id: &str,
    req: &TakeoverRequest,
    mover: &Mover<'_>,
    now: DateTime<Utc>,
) -> CollabResult<TakeoverResult> {
    let Mover { actor_kind, actor_id, outcome, to_session_id } = *mover;
    let hash = payload_hash(task_id, outcome, to_session_id, req);
    let model = collab_takeover::ActiveModel {
        id: Set(uuid::Uuid::new_v4().to_string()),
        repo_id: Set(repo_id),
        task_id: Set(task_id.to_string()),
        from_session_id: Set(req.expected_owner_session_id.clone()),
        to_session_id: Set(to_session_id.to_string()),
        actor_kind: Set(actor_kind.to_string()),
        actor_id: Set(actor_id.to_string()),
        previous_task_version: Set(req.expected_task_version),
        new_task_version: Set(req.expected_task_version + 1),
        outcome: Set(outcome.to_string()),
        reason: Set(req.reason.clone()),
        client_takeover_id: Set(req.client_takeover_id.clone()),
        payload_hash: Set(hash.clone()),
        created_at: Set(now),
    };
    match collab_takeover_ops::takeover_cas(
        db,
        repo_id,
        task_id,
        OwnerChange {
            expected_version: req.expected_task_version,
            expected_owner: req.expected_owner_session_id.as_deref(),
            new_owner: to_session_id,
        },
        model,
        now,
    )
    .await?
    {
        TakeoverOutcome::Done { task, takeover } => Ok(TakeoverResult {
            task: task::view(task),
            takeover: view(takeover),
            replayed: false,
        }),
        TakeoverOutcome::Existing(row) if row.payload_hash == hash => {
            let task = task::get(db, repo_id, task_id).await?;
            Ok(TakeoverResult {
                task,
                takeover: view(row),
                replayed: true,
            })
        }
        TakeoverOutcome::Existing(_) => Err(CollabError::IdempotencyKeyReused),
        TakeoverOutcome::Conflict { current } => Err(CollabError::VersionConflict {
            current_version: current.version,
        }),
        TakeoverOutcome::NotFound => Err(CollabError::not_found("task not found")),
    }
}

/// An earlier request with the same client id decides the answer before any
/// precondition is looked at: a retry of a claim that already succeeded must be
/// a replay, not "the session already owns this task".
async fn replay_or_reuse(
    db: &DatabaseConnection,
    repo_id: i64,
    task_id: &str,
    mover: &Mover<'_>,
    req: &TakeoverRequest,
) -> CollabResult<Option<TakeoverResult>> {
    let Some(row) = collab_takeover_ops::find_by_client_id(
        db,
        mover.actor_kind,
        mover.actor_id,
        &req.client_takeover_id,
    )
    .await?
    else {
        return Ok(None);
    };
    if row.payload_hash != payload_hash(task_id, mover.outcome, mover.to_session_id, req) {
        return Err(CollabError::IdempotencyKeyReused);
    }
    let task = task::get(db, repo_id, task_id).await?;
    Ok(Some(TakeoverResult {
        task,
        takeover: view(row),
        replayed: true,
    }))
}

/// `session` claims the task for itself.
pub async fn claim(
    db: &DatabaseConnection,
    repo_id: i64,
    task_id: &str,
    session: &agent_session::Model,
    req: TakeoverRequest,
    now: DateTime<Utc>,
) -> CollabResult<TakeoverResult> {
    validate(&req)?;
    let current = task::find(db, repo_id, task_id).await?;
    let mover = Mover {
        actor_kind: "session",
        actor_id: &session.id,
        outcome: "claimed",
        to_session_id: &session.id,
    };
    if let Some(replay) = replay_or_reuse(db, repo_id, &current.id, &mover, &req).await? {
        return Ok(replay);
    }
    if current.owner_session_id.as_deref() == Some(session.id.as_str()) {
        return Err(CollabError::bad_request("the session already owns this task"));
    }
    if current.status == "done" {
        return Err(CollabError::coded_conflict(
            "TASK_DONE",
            "a finished task cannot be taken over; reopen it first",
        ));
    }
    if let Some(owner_id) = &current.owner_session_id {
        let owner = agent_session_ops::find_in_repo(db, repo_id, owner_id).await?;
        if let Some(owner) = owner {
            let state = state_of(&owner, now);
            if state == "online" || state == "offline" {
                return Err(CollabError::coded_conflict(
                    "OWNER_MAY_BE_ACTIVE",
                    "the current owner session has not ended or expired; coordinate with it \
                     (message or handoff) or ask a user to reassign",
                )
                .with_details(serde_json::json!({
                    "owner_session_id": owner.id,
                    "owner_state": state,
                    "owner_last_seen_at": owner.last_seen_at,
                })));
            }
        }
    }
    apply(db, repo_id, &current.id, &req, &mover, now).await
}

/// A user explicitly assigns the task to a live session of the repository.
pub async fn reassign(
    db: &DatabaseConnection,
    repo_id: i64,
    task_id: &str,
    user_id: i64,
    to_session_id: &str,
    req: TakeoverRequest,
    now: DateTime<Utc>,
) -> CollabResult<TakeoverResult> {
    validate(&req)?;
    let current = task::find(db, repo_id, task_id).await?;
    let actor_id = user_id.to_string();
    let mover = Mover {
        actor_kind: "user",
        actor_id: &actor_id,
        outcome: "reassigned",
        to_session_id,
    };
    if let Some(replay) = replay_or_reuse(db, repo_id, &current.id, &mover, &req).await? {
        return Ok(replay);
    }
    task::require_live_owner(db, repo_id, to_session_id, now).await?;
    if current.owner_session_id.as_deref() == Some(to_session_id) {
        return Err(CollabError::bad_request("the session already owns this task"));
    }
    apply(db, repo_id, &current.id, &req, &mover, now).await
}

fn list_scope(repo_id: i64, task_id: &str) -> String {
    format!("takeovers:v1:repo={repo_id}:task={task_id}:order=created_asc,id_asc")
}

pub async fn list(
    db: &DatabaseConnection,
    cursor_key: &[u8],
    repo_id: i64,
    task_id: &str,
    limit: u64,
    cursor_token: Option<&str>,
) -> CollabResult<Page<TakeoverView>> {
    let task = task::find(db, repo_id, task_id).await?;
    let scope = list_scope(repo_id, &task.id);
    let after = cursor_token
        .map(|token| cursor::decode(cursor_key, &scope, token))
        .transpose()?
        .map(|p| (p.at, p.id));
    let mut rows = collab_takeover_ops::list_page(db, repo_id, &task.id, after, limit + 1).await?;
    let has_more = rows.len() as u64 > limit;
    rows.truncate(limit as usize);
    let next_cursor = has_more.then(|| rows.last()).flatten().map(|last| {
        cursor::encode(
            cursor_key,
            &scope,
            &KeysetPosition {
                at: last.created_at,
                id: last.id.clone(),
            },
        )
    });
    Ok(Page {
        items: rows.into_iter().map(view).collect(),
        next_cursor,
    })
}
