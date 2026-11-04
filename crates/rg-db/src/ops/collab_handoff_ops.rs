//! Database operations for `collab_handoffs`.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use sea_orm::sea_query::Expr;
use sea_orm::*;

use super::collab_task_ops::CasOutcome;
use super::collab_notice_ops::{self, NewNotice};
use crate::entities::collab_handoff::{
    self, ActiveModel, Entity as HandoffEntity, Model as Handoff,
};

/// Handoff statuses.
pub mod status {
    pub const PENDING: &str = "pending";
    pub const ACCEPTED: &str = "accepted";
    pub const CANCELLED: &str = "cancelled";
}

#[derive(Debug, Clone, PartialEq)]
pub enum CreateOutcome {
    Created(Handoff),
    /// `(from_session_id, client_handoff_id)` already exists; compare `payload_hash`.
    Existing(Handoff),
}

/// Insert a handoff, or return the row that already owns its idempotency key.
///
/// Two concurrent creates with one key race on the unique constraint; the loser
/// re-reads the winner's row instead of surfacing the violation.
pub async fn create_idempotent(
    db: &DatabaseConnection,
    model: ActiveModel,
    from_session_id: &str,
    client_handoff_id: &str,
) -> Result<CreateOutcome> {
    create_idempotent_with_notices(db, model, from_session_id, client_handoff_id, &[]).await
}

/// [`create_idempotent`] that also writes `notices` in the handoff's
/// transaction (none for a replay). The insert is the transaction's first
/// statement; the replay lookup runs before it (CLAUDE.md pitfall 21).
pub async fn create_idempotent_with_notices(
    db: &DatabaseConnection,
    model: ActiveModel,
    from_session_id: &str,
    client_handoff_id: &str,
    notices: &[NewNotice],
) -> Result<CreateOutcome> {
    if let Some(existing) = find_by_client_id(db, from_session_id, client_handoff_id).await? {
        return Ok(CreateOutcome::Existing(existing));
    }
    let txn = db.begin().await.context("db: begin create handoff")?;
    match model.insert(&txn).await {
        Ok(created) => {
            collab_notice_ops::insert_all(&txn, notices).await?;
            txn.commit().await.context("db: commit create handoff")?;
            Ok(CreateOutcome::Created(created))
        }
        Err(err) if matches!(err.sql_err(), Some(SqlErr::UniqueConstraintViolation(_))) => {
            // Rolled back on drop; the winner's row is the answer.
            drop(txn);
            let existing = find_by_client_id(db, from_session_id, client_handoff_id)
                .await?
                .context("db: handoff idempotency key vanished after conflict")?;
            Ok(CreateOutcome::Existing(existing))
        }
        Err(err) => Err(err).context("db: create handoff"),
    }
}

pub async fn find_by_client_id<C: ConnectionTrait>(
    db: &C,
    from_session_id: &str,
    client_handoff_id: &str,
) -> Result<Option<Handoff>> {
    HandoffEntity::find()
        .filter(collab_handoff::Column::FromSessionId.eq(from_session_id))
        .filter(collab_handoff::Column::ClientHandoffId.eq(client_handoff_id))
        .one(db)
        .await
        .context("db: find handoff by client id")
}

pub async fn find_in_repo<C: ConnectionTrait>(
    db: &C,
    repo_id: i64,
    id: &str,
) -> Result<Option<Handoff>> {
    HandoffEntity::find_by_id(id.to_string())
        .filter(collab_handoff::Column::RepoId.eq(repo_id))
        .one(db)
        .await
        .context("db: find handoff")
}

/// Keyset page of a task's handoffs ordered by `(created_at, id)` ascending.
pub async fn list_page<C: ConnectionTrait>(
    db: &C,
    repo_id: i64,
    task_id: &str,
    after: Option<(DateTime<Utc>, String)>,
    limit: u64,
) -> Result<Vec<Handoff>> {
    let mut query = HandoffEntity::find()
        .filter(collab_handoff::Column::RepoId.eq(repo_id))
        .filter(collab_handoff::Column::TaskId.eq(task_id));
    if let Some((created, id)) = after {
        query = query.filter(
            Condition::any()
                .add(collab_handoff::Column::CreatedAt.gt(created))
                .add(
                    Condition::all()
                        .add(collab_handoff::Column::CreatedAt.eq(created))
                        .add(collab_handoff::Column::Id.gt(id)),
                ),
        );
    }
    query
        .order_by_asc(collab_handoff::Column::CreatedAt)
        .order_by_asc(collab_handoff::Column::Id)
        .limit(limit)
        .all(db)
        .await
        .context("db: list handoffs")
}

/// Pending handoffs addressed to `session_id`, newest first.
pub async fn list_pending_to_session<C: ConnectionTrait>(
    db: &C,
    repo_id: i64,
    session_id: &str,
    limit: u64,
) -> Result<Vec<Handoff>> {
    HandoffEntity::find()
        .filter(collab_handoff::Column::RepoId.eq(repo_id))
        .filter(collab_handoff::Column::ToSessionId.eq(session_id))
        .filter(collab_handoff::Column::Status.eq(status::PENDING))
        .order_by_desc(collab_handoff::Column::CreatedAt)
        .order_by_desc(collab_handoff::Column::Id)
        .limit(limit)
        .all(db)
        .await
        .context("db: list pending handoffs")
}

/// Move a `pending` handoff to a terminal status iff it is still at
/// `expected_version`. Accept and cancel both go through this one conditional
/// `UPDATE`, so when they race exactly one matches the row and the other gets
/// [`CasOutcome::Conflict`] carrying the winner's state.
pub async fn transition_pending(
    db: &DatabaseConnection,
    repo_id: i64,
    id: &str,
    expected_version: i64,
    to_status: &str,
    now: DateTime<Utc>,
) -> Result<CasOutcome<Handoff>> {
    let txn = db.begin().await.context("db: begin handoff transition")?;
    let accepted_at = (to_status == status::ACCEPTED).then_some(now);
    let res = HandoffEntity::update_many()
        .col_expr(collab_handoff::Column::Status, Expr::value(to_status))
        .col_expr(
            collab_handoff::Column::Version,
            Expr::col(collab_handoff::Column::Version).add(1),
        )
        .col_expr(collab_handoff::Column::AcceptedAt, Expr::value(accepted_at))
        .filter(collab_handoff::Column::Id.eq(id))
        .filter(collab_handoff::Column::RepoId.eq(repo_id))
        .filter(collab_handoff::Column::Version.eq(expected_version))
        .filter(collab_handoff::Column::Status.eq(status::PENDING))
        .exec(&txn)
        .await
        .context("db: transition handoff")?;
    let current = find_in_repo(&txn, repo_id, id).await?;
    txn.commit().await.context("db: commit handoff transition")?;
    Ok(match (res.rows_affected, current) {
        (_, None) => CasOutcome::NotFound,
        (0, Some(current)) => CasOutcome::Conflict { current },
        (_, Some(updated)) => CasOutcome::Updated(updated),
    })
}
