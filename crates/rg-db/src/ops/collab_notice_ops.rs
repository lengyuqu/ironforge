//! Database operations for `collab_notices` and `collab_delivery_receipts`.
//!
//! Notices are written in the *same transaction* as the business change that
//! causes them ([`insert_all`] takes the caller's transaction), so a committed
//! change always has its notice and a rolled-back one never leaves a stray.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use sea_orm::sea_query::Expr;
use sea_orm::*;

use crate::entities::collab_delivery_receipt::{self, Entity as ReceiptEntity, Model as Receipt};
use crate::entities::collab_notice::{self, ActiveModel, Entity as NoticeEntity, Model as Notice};

/// Notice kinds.
pub mod kind {
    pub const HANDOFF_RECEIVED: &str = "handoff_received";
    pub const MESSAGE_TARGETED: &str = "message_targeted";
    pub const QUESTION_ASKED: &str = "question_asked";
    pub const QUESTION_ANSWERED: &str = "question_answered";
    pub const TASK_REASSIGNED_AWAY: &str = "task_reassigned_away";
    /// Reserved: emitted by the dependency feature, not by this module.
    pub const DEPENDENCY_FINISHED: &str = "dependency_finished";
    pub const ESCALATION: &str = "escalation";
}

/// What a notice points at.
pub mod source {
    pub const HANDOFF: &str = "handoff";
    pub const MESSAGE: &str = "message";
    pub const QUESTION: &str = "question";
    pub const TAKEOVER: &str = "takeover";
    pub const TASK: &str = "task";
    pub const DEPENDENCY: &str = "dependency";
}

/// A notice to be written; the server assigns the id.
#[derive(Debug, Clone, PartialEq)]
pub struct NewNotice {
    pub repo_id: i64,
    pub task_id: String,
    pub recipient_session_id: String,
    pub kind: &'static str,
    pub source_type: &'static str,
    pub source_id: String,
    pub generation: i64,
    /// Platform-derived facts only; never client-written text.
    pub hint_json: String,
    pub created_at: DateTime<Utc>,
}

fn active(new: &NewNotice) -> ActiveModel {
    ActiveModel {
        id: Set(uuid::Uuid::new_v4().to_string()),
        repo_id: Set(new.repo_id),
        task_id: Set(new.task_id.clone()),
        recipient_session_id: Set(new.recipient_session_id.clone()),
        kind: Set(new.kind.to_string()),
        source_type: Set(new.source_type.to_string()),
        source_id: Set(new.source_id.clone()),
        generation: Set(new.generation),
        hint_json: Set(new.hint_json.clone()),
        created_at: Set(new.created_at),
        ack_at: Set(None),
        resolved_at: Set(None),
    }
}

/// Insert notices on the caller's connection (normally the transaction of the
/// business write). A duplicate key is an error here: callers only emit for a
/// source row they just created.
pub async fn insert_all<C: ConnectionTrait>(db: &C, notices: &[NewNotice]) -> Result<()> {
    for notice in notices {
        active(notice).insert(db).await.context("db: insert collab notice")?;
    }
    Ok(())
}

/// Insert one notice in its own statement. `false` = the dedupe key already
/// exists (another evaluator emitted it first), which is not an error.
pub async fn insert_if_new(db: &DatabaseConnection, notice: &NewNotice) -> Result<bool> {
    match active(notice).insert(db).await {
        Ok(_) => Ok(true),
        Err(err) if matches!(err.sql_err(), Some(SqlErr::UniqueConstraintViolation(_))) => Ok(false),
        Err(err) => Err(err).context("db: insert collab notice"),
    }
}

/// Which notices of a session to list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Filter {
    /// Not acknowledged and not resolved.
    Pending,
    All,
}

/// Keyset page of a session's notices ordered by `(created_at, id)` ascending.
pub async fn list_page<C: ConnectionTrait>(
    db: &C,
    repo_id: i64,
    recipient_session_id: &str,
    filter: Filter,
    after: Option<(DateTime<Utc>, String)>,
    limit: u64,
) -> Result<Vec<Notice>> {
    let mut query = NoticeEntity::find()
        .filter(collab_notice::Column::RepoId.eq(repo_id))
        .filter(collab_notice::Column::RecipientSessionId.eq(recipient_session_id));
    if filter == Filter::Pending {
        query = query
            .filter(collab_notice::Column::AckAt.is_null())
            .filter(collab_notice::Column::ResolvedAt.is_null());
    }
    if let Some((created, id)) = after {
        query = query.filter(
            Condition::any()
                .add(collab_notice::Column::CreatedAt.gt(created))
                .add(
                    Condition::all()
                        .add(collab_notice::Column::CreatedAt.eq(created))
                        .add(collab_notice::Column::Id.gt(id)),
                ),
        );
    }
    query
        .order_by_asc(collab_notice::Column::CreatedAt)
        .order_by_asc(collab_notice::Column::Id)
        .limit(limit)
        .all(db)
        .await
        .context("db: list collab notices")
}

/// Notices of a session that are neither acknowledged nor resolved, with the
/// oldest creation time; used by diagnostics (counts, never contents).
pub async fn pending_summary<C: ConnectionTrait>(
    db: &C,
    repo_id: i64,
    recipient_session_id: &str,
) -> Result<(u64, Option<DateTime<Utc>>)> {
    let count = NoticeEntity::find()
        .filter(collab_notice::Column::RepoId.eq(repo_id))
        .filter(collab_notice::Column::RecipientSessionId.eq(recipient_session_id))
        .filter(collab_notice::Column::AckAt.is_null())
        .filter(collab_notice::Column::ResolvedAt.is_null())
        .count(db)
        .await
        .context("db: count pending notices")?;
    let oldest = NoticeEntity::find()
        .filter(collab_notice::Column::RepoId.eq(repo_id))
        .filter(collab_notice::Column::RecipientSessionId.eq(recipient_session_id))
        .filter(collab_notice::Column::AckAt.is_null())
        .filter(collab_notice::Column::ResolvedAt.is_null())
        .order_by_asc(collab_notice::Column::CreatedAt)
        .one(db)
        .await
        .context("db: oldest pending notice")?
        .map(|n| n.created_at);
    Ok((count, oldest))
}

/// A notice of `recipient_session_id`; another session's notice reads as absent.
pub async fn find_for_recipient<C: ConnectionTrait>(
    db: &C,
    repo_id: i64,
    recipient_session_id: &str,
    id: &str,
) -> Result<Option<Notice>> {
    NoticeEntity::find_by_id(id.to_string())
        .filter(collab_notice::Column::RepoId.eq(repo_id))
        .filter(collab_notice::Column::RecipientSessionId.eq(recipient_session_id))
        .one(db)
        .await
        .context("db: find collab notice")
}

/// The notice for one source of one kind with the highest generation.
pub async fn find_for_source<C: ConnectionTrait>(
    db: &C,
    recipient_session_id: &str,
    kind: &str,
    source_type: &str,
    source_id: &str,
) -> Result<Option<Notice>> {
    NoticeEntity::find()
        .filter(collab_notice::Column::RecipientSessionId.eq(recipient_session_id))
        .filter(collab_notice::Column::Kind.eq(kind))
        .filter(collab_notice::Column::SourceType.eq(source_type))
        .filter(collab_notice::Column::SourceId.eq(source_id))
        .order_by_desc(collab_notice::Column::Generation)
        .one(db)
        .await
        .context("db: find latest notice for source")
}

#[derive(Debug, Clone, PartialEq)]
pub enum AckOutcome {
    /// This call set `ack_at`.
    Acked(Notice),
    /// Acknowledged earlier (possibly by a racing call); the stored state.
    AlreadyAcked(Notice),
    NotFound,
}

/// Acknowledge a notice. The conditional `UPDATE ... WHERE ack_at IS NULL` is
/// the arbiter: of N racing acknowledgements exactly one changes the row and
/// the rest read the winner's `ack_at`. Touches only `collab_notices`.
pub async fn ack(
    db: &DatabaseConnection,
    repo_id: i64,
    recipient_session_id: &str,
    id: &str,
    now: DateTime<Utc>,
) -> Result<AckOutcome> {
    let res = NoticeEntity::update_many()
        .col_expr(collab_notice::Column::AckAt, Expr::value(now))
        .filter(collab_notice::Column::Id.eq(id))
        .filter(collab_notice::Column::RepoId.eq(repo_id))
        .filter(collab_notice::Column::RecipientSessionId.eq(recipient_session_id))
        .filter(collab_notice::Column::AckAt.is_null())
        .exec(db)
        .await
        .context("db: ack collab notice")?;
    let current = find_for_recipient(db, repo_id, recipient_session_id, id).await?;
    Ok(match (res.rows_affected, current) {
        (_, None) => AckOutcome::NotFound,
        (0, Some(row)) => AckOutcome::AlreadyAcked(row),
        (_, Some(row)) => AckOutcome::Acked(row),
    })
}

/// Mark a pending notice resolved (its condition ended). Idempotent.
pub async fn resolve(db: &DatabaseConnection, id: &str, now: DateTime<Utc>) -> Result<bool> {
    let res = NoticeEntity::update_many()
        .col_expr(collab_notice::Column::ResolvedAt, Expr::value(now))
        .filter(collab_notice::Column::Id.eq(id))
        .filter(collab_notice::Column::ResolvedAt.is_null())
        .exec(db)
        .await
        .context("db: resolve collab notice")?;
    Ok(res.rows_affected > 0)
}

/// Unresolved, unacknowledged escalation notices of a session.
pub async fn pending_escalations(
    db: &DatabaseConnection,
    repo_id: i64,
    recipient_session_id: &str,
) -> Result<Vec<Notice>> {
    NoticeEntity::find()
        .filter(collab_notice::Column::RepoId.eq(repo_id))
        .filter(collab_notice::Column::RecipientSessionId.eq(recipient_session_id))
        .filter(collab_notice::Column::Kind.eq(kind::ESCALATION))
        .filter(collab_notice::Column::AckAt.is_null())
        .filter(collab_notice::Column::ResolvedAt.is_null())
        .all(db)
        .await
        .context("db: pending escalations")
}

// ------------------------------------------------------------------ receipts

pub async fn find_receipt<C: ConnectionTrait>(
    db: &C,
    message_id: &str,
    target_session_id: &str,
) -> Result<Option<Receipt>> {
    ReceiptEntity::find_by_id((message_id.to_string(), target_session_id.to_string()))
        .one(db)
        .await
        .context("db: find delivery receipt")
}

#[derive(Debug, Clone, PartialEq)]
pub enum ReceiptOutcome {
    Created(Receipt),
    /// The target confirmed earlier; the original `read_at` is kept.
    Existing(Receipt),
}

/// Record that `target_session_id` read `message_id`. Idempotent: the primary
/// key `(message_id, target_session_id)` decides a race and the loser returns
/// the winner's row.
pub async fn confirm_receipt(
    db: &DatabaseConnection,
    repo_id: i64,
    task_id: &str,
    message_id: &str,
    target_session_id: &str,
    now: DateTime<Utc>,
) -> Result<ReceiptOutcome> {
    if let Some(existing) = find_receipt(db, message_id, target_session_id).await? {
        return Ok(ReceiptOutcome::Existing(existing));
    }
    let insert = collab_delivery_receipt::ActiveModel {
        message_id: Set(message_id.to_string()),
        target_session_id: Set(target_session_id.to_string()),
        repo_id: Set(repo_id),
        task_id: Set(task_id.to_string()),
        read_at: Set(now),
    }
    .insert(db)
    .await;
    match insert {
        Ok(row) => Ok(ReceiptOutcome::Created(row)),
        Err(err) if matches!(err.sql_err(), Some(SqlErr::UniqueConstraintViolation(_))) => {
            let existing = find_receipt(db, message_id, target_session_id)
                .await?
                .context("db: receipt vanished after conflict")?;
            Ok(ReceiptOutcome::Existing(existing))
        }
        Err(err) => Err(err).context("db: insert delivery receipt"),
    }
}
