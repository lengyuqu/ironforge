//! Durable per-session notices (P1 U04).
//!
//! A notice says "something addressed to you changed". It is a *hint*: the
//! consumer must re-read the object it points at before acting, and acknowledging
//! a notice changes only the notice. Notices are written in the same transaction
//! as the business change (see the `*_with_notices` ops) and are never moved to
//! a successor of an ended or expired session: only the live recipient session,
//! authenticated with its own secret, can read them.

use chrono::{DateTime, Utc};
use rg_db::entities::{agent_session, collab_notice};
use rg_db::ops::agent_session_ops;
use rg_db::ops::collab_notice_ops::{self, AckOutcome, Filter, NewNotice};
use sea_orm::DatabaseConnection;
use serde::Serialize;

pub use rg_db::ops::collab_notice_ops::{kind, source};

use super::cursor::{self, KeysetPosition};
use super::error::{CollabError, CollabResult};
use super::session::state_of;
use super::{validation, Page};

/// Authority of a notice: it never replaces reading the source object.
pub const AUTHORITY: &str = "hint";

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct NoticeView {
    pub id: String,
    pub task_id: String,
    pub kind: String,
    pub source_type: String,
    pub source_id: String,
    pub generation: i64,
    /// Platform-derived facts (ids, enumerations); never client text.
    pub hint: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub acked_at: Option<DateTime<Utc>>,
    pub resolved_at: Option<DateTime<Utc>>,
    /// `pending` | `acknowledged` | `resolved`.
    pub state: &'static str,
    /// Always `hint`: re-read the source before acting.
    pub authority: &'static str,
}

pub(crate) fn view(row: collab_notice::Model) -> NoticeView {
    let state = if row.ack_at.is_some() {
        "acknowledged"
    } else if row.resolved_at.is_some() {
        "resolved"
    } else {
        "pending"
    };
    NoticeView {
        hint: serde_json::from_str(&row.hint_json).unwrap_or(serde_json::Value::Null),
        id: row.id,
        task_id: row.task_id,
        kind: row.kind,
        source_type: row.source_type,
        source_id: row.source_id,
        generation: row.generation,
        created_at: row.created_at,
        acked_at: row.ack_at,
        resolved_at: row.resolved_at,
        state,
        authority: AUTHORITY,
    }
}

/// Whether `session_id` can still read notices: it exists in the repository and
/// has neither ended nor passed its hard expiry. An `offline` session is alive.
pub async fn is_live(
    db: &DatabaseConnection,
    repo_id: i64,
    session_id: &str,
    now: DateTime<Utc>,
) -> CollabResult<bool> {
    if !validation::is_uuid(session_id) {
        return Ok(false);
    }
    Ok(agent_session_ops::find_in_repo(db, repo_id, session_id)
        .await?
        .is_some_and(|s| matches!(state_of(&s, now), "online" | "offline")))
}

/// Build a notice for the `source` row of `task_id`, or `None` when the
/// recipient is the actor itself or no longer live (an ended or expired session
/// gets nothing, and nothing is handed to its successor).
#[allow(clippy::too_many_arguments)]
pub(crate) async fn prepare(
    db: &DatabaseConnection,
    repo_id: i64,
    task_id: &str,
    recipient_session_id: &str,
    actor_session_id: Option<&str>,
    kind: &'static str,
    source_type: &'static str,
    source_id: &str,
    hint: serde_json::Value,
    now: DateTime<Utc>,
) -> CollabResult<Option<NewNotice>> {
    if actor_session_id == Some(recipient_session_id) {
        return Ok(None);
    }
    if !is_live(db, repo_id, recipient_session_id, now).await? {
        return Ok(None);
    }
    Ok(Some(NewNotice {
        repo_id,
        task_id: task_id.to_string(),
        recipient_session_id: recipient_session_id.to_string(),
        kind,
        source_type,
        source_id: source_id.to_string(),
        generation: 1,
        hint_json: hint.to_string(),
        created_at: now,
    }))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateFilter {
    Pending,
    All,
}

fn list_scope(repo_id: i64, session_id: &str, filter: StateFilter) -> String {
    format!(
        "notices:v1:repo={repo_id}:session={session_id}:filter={}:order=created_asc,id_asc",
        if filter == StateFilter::Pending {
            "pending"
        } else {
            "all"
        }
    )
}

/// One page of `session`'s notices, oldest first.
///
/// The default (`Pending`) is a *state* query, so it cannot miss a notice
/// regardless of commit order: anything not yet acknowledged is returned on a
/// fresh read. A cursor only walks one pass; a notice committed behind the
/// cursor shows up on the next fresh read. Re-reading is always safe and
/// duplicates are removed by notice id.
pub async fn list(
    db: &DatabaseConnection,
    cursor_key: &[u8],
    repo_id: i64,
    session: &agent_session::Model,
    filter: StateFilter,
    limit: u64,
    cursor_token: Option<&str>,
) -> CollabResult<Page<NoticeView>> {
    let scope = list_scope(repo_id, &session.id, filter);
    let after = cursor_token
        .map(|token| cursor::decode(cursor_key, &scope, token))
        .transpose()?
        .map(|p| (p.at, p.id));
    let db_filter = match filter {
        StateFilter::Pending => Filter::Pending,
        StateFilter::All => Filter::All,
    };
    let mut rows =
        collab_notice_ops::list_page(db, repo_id, &session.id, db_filter, after, limit + 1).await?;
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

#[derive(Debug, Clone, Serialize)]
pub struct Acked {
    pub notice: NoticeView,
    /// `false` when it was already acknowledged (an earlier or racing call).
    pub newly_acked: bool,
}

/// Acknowledge a notice of `session`. Changes nothing but the notice; another
/// session's notice reads as absent.
pub async fn ack(
    db: &DatabaseConnection,
    repo_id: i64,
    session: &agent_session::Model,
    notice_id: &str,
    now: DateTime<Utc>,
) -> CollabResult<Acked> {
    if !validation::is_uuid(notice_id) {
        return Err(CollabError::not_found("notice not found"));
    }
    match collab_notice_ops::ack(db, repo_id, &session.id, notice_id, now).await? {
        AckOutcome::Acked(row) => Ok(Acked {
            notice: view(row),
            newly_acked: true,
        }),
        AckOutcome::AlreadyAcked(row) => Ok(Acked {
            notice: view(row),
            newly_acked: false,
        }),
        AckOutcome::NotFound => Err(CollabError::not_found("notice not found")),
    }
}
