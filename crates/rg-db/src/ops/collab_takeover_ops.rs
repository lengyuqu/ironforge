//! Database operations for `collab_takeovers`.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use sea_orm::sea_query::Expr;
use sea_orm::*;

use super::collab_metric_ops;
use super::collab_notice_ops::{self, NewNotice};
use super::collab_task_ops::find_in_repo as find_task;
use crate::entities::collab_takeover::{
    self, ActiveModel, Entity as TakeoverEntity, Model as Takeover,
};
use crate::entities::collab_task::{self, Entity as TaskEntity, Model as Task};

// Results are consumed immediately; boxing the rows would only add noise.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq)]
pub enum TakeoverOutcome {
    /// Ownership changed and the history row was written in one transaction.
    Done { task: Task, takeover: Takeover },
    /// The idempotency key already owns a takeover; compare `payload_hash`.
    Existing(Takeover),
    /// Task version or owner no longer matches; `current` is the latest state.
    Conflict { current: Task },
    NotFound,
}

pub async fn find_by_client_id<C: ConnectionTrait>(
    db: &C,
    actor_kind: &str,
    actor_id: &str,
    client_takeover_id: &str,
) -> Result<Option<Takeover>> {
    TakeoverEntity::find()
        .filter(collab_takeover::Column::ActorKind.eq(actor_kind))
        .filter(collab_takeover::Column::ActorId.eq(actor_id))
        .filter(collab_takeover::Column::ClientTakeoverId.eq(client_takeover_id))
        .one(db)
        .await
        .context("db: find takeover by client id")
}

/// Keyset page of a task's takeovers ordered by `(created_at, id)` ascending.
pub async fn list_page<C: ConnectionTrait>(
    db: &C,
    repo_id: i64,
    task_id: &str,
    after: Option<(DateTime<Utc>, String)>,
    limit: u64,
) -> Result<Vec<Takeover>> {
    let mut query = TakeoverEntity::find()
        .filter(collab_takeover::Column::RepoId.eq(repo_id))
        .filter(collab_takeover::Column::TaskId.eq(task_id));
    if let Some((created, id)) = after {
        query = query.filter(
            Condition::any()
                .add(collab_takeover::Column::CreatedAt.gt(created))
                .add(
                    Condition::all()
                        .add(collab_takeover::Column::CreatedAt.eq(created))
                        .add(collab_takeover::Column::Id.gt(id)),
                ),
        );
    }
    query
        .order_by_asc(collab_takeover::Column::CreatedAt)
        .order_by_asc(collab_takeover::Column::Id)
        .limit(limit)
        .all(db)
        .await
        .context("db: list takeovers")
}

/// The state a takeover is conditional on, and the owner it installs.
#[derive(Debug, Clone, Copy)]
pub struct OwnerChange<'a> {
    pub expected_version: i64,
    /// `None` = the task must currently be unowned.
    pub expected_owner: Option<&'a str>,
    pub new_owner: &'a str,
}

/// Move a task to `change.new_owner` iff it is still at `change.expected_version`
/// *and* still owned by `change.expected_owner`, and record the takeover — all in
/// one transaction.
///
/// The conditional `UPDATE` is the arbiter: of two racing claimants with the
/// same expectations exactly one matches a row; the other gets
/// [`TakeoverOutcome::Conflict`] and the history never disagrees with the task.
pub async fn takeover_cas(
    db: &DatabaseConnection,
    repo_id: i64,
    task_id: &str,
    change: OwnerChange<'_>,
    model: ActiveModel,
    now: DateTime<Utc>,
) -> Result<TakeoverOutcome> {
    takeover_cas_with_notices(db, repo_id, task_id, change, model, now, &[]).await
}

/// [`takeover_cas`] that also writes `notices` in the takeover's transaction:
/// only the winning claimant of a race emits them.
pub async fn takeover_cas_with_notices(
    db: &DatabaseConnection,
    repo_id: i64,
    task_id: &str,
    change: OwnerChange<'_>,
    model: ActiveModel,
    now: DateTime<Utc>,
    notices: &[NewNotice],
) -> Result<TakeoverOutcome> {
    let actor_kind = model.actor_kind.clone().unwrap();
    let actor_id = model.actor_id.clone().unwrap();
    let client_id = model.client_takeover_id.clone().unwrap();

    // Replay check outside the transaction, and the transaction's first
    // statement is the write: a read first would take a snapshot that SQLite
    // cannot upgrade once another writer commits (SQLITE_BUSY instead of
    // waiting), which turned racing claims into 500s.
    if let Some(existing) = find_by_client_id(db, &actor_kind, &actor_id, &client_id).await? {
        return Ok(TakeoverOutcome::Existing(existing));
    }
    let txn = db.begin().await.context("db: begin takeover")?;

    let mut update = TaskEntity::update_many()
        .col_expr(collab_task::Column::OwnerSessionId, Expr::value(change.new_owner))
        .col_expr(collab_task::Column::UpdatedAt, Expr::value(now))
        .col_expr(
            collab_task::Column::Version,
            Expr::col(collab_task::Column::Version).add(1),
        )
        .filter(collab_task::Column::Id.eq(task_id))
        .filter(collab_task::Column::RepoId.eq(repo_id))
        .filter(collab_task::Column::Version.eq(change.expected_version));
    update = match change.expected_owner {
        Some(owner) => update.filter(collab_task::Column::OwnerSessionId.eq(owner)),
        None => update.filter(collab_task::Column::OwnerSessionId.is_null()),
    };
    let res = update.exec(&txn).await.context("db: update task owner")?;

    let current = find_task(&txn, repo_id, task_id).await?;
    let Some(current) = current else {
        txn.commit().await.context("db: commit takeover")?;
        return Ok(TakeoverOutcome::NotFound);
    };
    if res.rows_affected == 0 {
        txn.commit().await.context("db: commit takeover")?;
        return Ok(TakeoverOutcome::Conflict { current });
    }
    match model.insert(&txn).await {
        Ok(takeover) => {
            collab_metric_ops::record_takeover(&txn, repo_id, task_id, &takeover.id, &takeover.outcome, now)
                .await?;
            collab_notice_ops::insert_all(&txn, notices).await?;
            txn.commit().await.context("db: commit takeover")?;
            Ok(TakeoverOutcome::Done {
                task: current,
                takeover,
            })
        }
        Err(err) if matches!(err.sql_err(), Some(SqlErr::UniqueConstraintViolation(_))) => {
            // Same idempotency key committed concurrently: undo our owner change
            // and answer with the winner's row.
            drop(txn);
            let existing = find_by_client_id(db, &actor_kind, &actor_id, &client_id)
                .await?
                .context("db: takeover idempotency key vanished after conflict")?;
            Ok(TakeoverOutcome::Existing(existing))
        }
        Err(err) => Err(err).context("db: insert takeover"),
    }
}
