//! Database operations for `collab_reads` (per-session read watermarks).

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use sea_orm::sea_query::Expr;
use sea_orm::*;

use crate::entities::collab_read::{self, ActiveModel, Entity as ReadEntity, Model as Read};
use crate::entities::collab_task;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdvanceOutcome {
    /// The watermark after the call (unchanged when `last_read_seq` was already higher).
    Current(i64),
    /// The requested value exceeds the last committed message of the task.
    OutOfRange { max_seq: i64 },
    /// No such task in the repository.
    TaskNotFound,
}

pub async fn get<C: ConnectionTrait>(
    db: &C,
    task_id: &str,
    session_id: &str,
) -> Result<Option<Read>> {
    ReadEntity::find_by_id((task_id.to_string(), session_id.to_string()))
        .one(db)
        .await
        .context("db: find collab read")
}

/// Raise the watermark to `seq` (monotonic: a lower value never moves it back).
///
/// The upper bound is the task's `next_seq`, i.e. the last *committed* message,
/// so a watermark can never point at a message that does not exist. Concurrent
/// callers converge on the maximum: the conditional `UPDATE` only matches when
/// the stored value is lower, and the first insert wins a unique-key race.
pub async fn advance(
    db: &DatabaseConnection,
    repo_id: i64,
    task_id: &str,
    session_id: &str,
    seq: i64,
    now: DateTime<Utc>,
) -> Result<AdvanceOutcome> {
    let Some(task) = collab_task::Entity::find_by_id(task_id.to_string())
        .filter(collab_task::Column::RepoId.eq(repo_id))
        .one(db)
        .await
        .context("db: find task for read watermark")?
    else {
        return Ok(AdvanceOutcome::TaskNotFound);
    };
    if seq > task.next_seq {
        return Ok(AdvanceOutcome::OutOfRange {
            max_seq: task.next_seq,
        });
    }

    for _ in 0..3 {
        let updated = ReadEntity::update_many()
            .col_expr(collab_read::Column::LastReadSeq, Expr::value(seq))
            .col_expr(collab_read::Column::UpdatedAt, Expr::value(now))
            .filter(collab_read::Column::TaskId.eq(task_id))
            .filter(collab_read::Column::SessionId.eq(session_id))
            .filter(collab_read::Column::LastReadSeq.lt(seq))
            .exec(db)
            .await
            .context("db: advance read watermark")?;
        if updated.rows_affected > 0 {
            break;
        }
        if get(db, task_id, session_id).await?.is_some() {
            // Row exists and is already >= seq.
            break;
        }
        // A plain insert whose unique violation means "someone else created the
        // row first"; `INSERT .. ON CONFLICT DO NOTHING` is not portable to MySQL.
        match (ActiveModel {
            task_id: Set(task_id.to_string()),
            session_id: Set(session_id.to_string()),
            last_read_seq: Set(seq),
            updated_at: Set(now),
        })
        .insert(db)
        .await
        {
            Ok(_) => break,
            Err(err) if matches!(err.sql_err(), Some(SqlErr::UniqueConstraintViolation(_))) => {}
            Err(err) => return Err(err).context("db: insert read watermark"),
        }
        // Lost the insert race; loop to apply our value on top of the winner's.
    }

    let current = get(db, task_id, session_id)
        .await?
        .map(|row| row.last_read_seq)
        .unwrap_or(0);
    Ok(AdvanceOutcome::Current(current))
}
