//! Per-session read watermarks of a task's message stream.

use chrono::{DateTime, Utc};
use rg_db::entities::agent_session;
use rg_db::ops::collab_read_ops::{self, AdvanceOutcome};
use sea_orm::DatabaseConnection;

use super::error::{CollabError, CollabResult};
use super::task;

/// Raise `session`'s watermark on a task and return the resulting value.
///
/// Monotonic: a lower `last_read_seq` leaves the watermark where it is and the
/// current value is returned. It can never exceed the last committed message.
/// Unread = messages with `seq > watermark`; the watermark is independent per
/// session and independent of handoff state.
pub async fn mark(
    db: &DatabaseConnection,
    repo_id: i64,
    task_id: &str,
    session: &agent_session::Model,
    last_read_seq: i64,
    now: DateTime<Utc>,
) -> CollabResult<i64> {
    if last_read_seq < 0 {
        return Err(CollabError::bad_request(
            "last_read_seq must not be negative",
        ));
    }
    let task = task::find(db, repo_id, task_id).await?;
    match collab_read_ops::advance(db, repo_id, &task.id, &session.id, last_read_seq, now).await? {
        AdvanceOutcome::Current(value) => Ok(value),
        AdvanceOutcome::OutOfRange { max_seq } => Err(CollabError::bad_request(format!(
            "last_read_seq exceeds the last message of the task ({max_seq})"
        ))),
        AdvanceOutcome::TaskNotFound => Err(CollabError::not_found("task not found")),
    }
}
