//! Database operations for background jobs (QUEUE-001).

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use sea_orm::sea_query::Expr;
use sea_orm::*;

use crate::entities::background_job::{self, status, ActiveModel, Entity as JobEntity, Model};

/// Insert a new job row.
pub async fn insert(db: &DatabaseConnection, model: ActiveModel) -> Result<Model> {
    model.insert(db).await.context("db: insert background job")
}

/// Find a job by id.
pub async fn find_by_id(db: &DatabaseConnection, id: i64) -> Result<Option<Model>> {
    JobEntity::find_by_id(id)
        .one(db)
        .await
        .context("db: find background job by id")
}

/// List jobs with the given status, oldest `run_at` first.
pub async fn list_by_status(
    db: &DatabaseConnection,
    status_value: &str,
    limit: u64,
) -> Result<Vec<Model>> {
    JobEntity::find()
        .filter(background_job::Column::Status.eq(status_value))
        .order_by_asc(background_job::Column::RunAt)
        .limit(limit)
        .all(db)
        .await
        .context("db: list background jobs by status")
}

/// List pending jobs whose `run_at` is due, oldest first (claim candidates).
pub async fn list_pending_due(
    db: &DatabaseConnection,
    limit: u64,
) -> Result<Vec<Model>> {
    JobEntity::find()
        .filter(background_job::Column::Status.eq(status::PENDING))
        .filter(background_job::Column::RunAt.lte(Utc::now()))
        .order_by_asc(background_job::Column::Id)
        .limit(limit)
        .all(db)
        .await
        .context("db: list due background jobs")
}

/// List running jobs claimed before the given timestamp (stale claims).
pub async fn list_running_locked_before(
    db: &DatabaseConnection,
    before: DateTime<Utc>,
    limit: u64,
) -> Result<Vec<Model>> {
    JobEntity::find()
        .filter(background_job::Column::Status.eq(status::RUNNING))
        .filter(background_job::Column::LockedAt.lt(before))
        .limit(limit)
        .all(db)
        .await
        .context("db: list stale running background jobs")
}

/// Update a job row.
pub async fn update(db: &DatabaseConnection, model: ActiveModel) -> Result<Model> {
    model.update(db).await.context("db: update background job")
}

/// Atomically flip a pending job to running for `worker_id`.
///
/// Returns the updated model when this caller won the claim, `None` when
/// another worker claimed the row first. The `status = 'pending'` predicate
/// makes the check-and-set portable across SQLite/PostgreSQL/MySQL without
/// `RETURNING` support.
pub async fn claim(
    db: &DatabaseConnection,
    id: i64,
    worker_id: &str,
) -> Result<Option<Model>> {
    let now = Utc::now();
    let result = JobEntity::update_many()
        .col_expr(background_job::Column::Status, Expr::value(status::RUNNING))
        .col_expr(background_job::Column::LockedBy, Expr::value(worker_id))
        .col_expr(background_job::Column::LockedAt, Expr::value(now))
        .col_expr(background_job::Column::UpdatedAt, Expr::value(now))
        .filter(background_job::Column::Id.eq(id))
        .filter(background_job::Column::Status.eq(status::PENDING))
        .exec(db)
        .await
        .context("db: claim background job")?;
    if result.rows_affected == 0 {
        return Ok(None);
    }
    Ok(find_by_id(db, id).await?)
}
