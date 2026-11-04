//! Database operations for `collab_task_budgets` and `collab_task_usage`.
//!
//! Budgets are replaced under an optimistic `version` by a single statement
//! (insert, or conditional `UPDATE` as arbiter). Usage rows are append-only.
//! Nothing here adds amounts up: aggregation is the service's job, per
//! `(unit, source)`, in integer arithmetic.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use sea_orm::sea_query::Expr;
use sea_orm::*;

use crate::entities::collab_task_budget::{self, ActiveModel as BudgetActive, Model as Budget};
use crate::entities::collab_task_usage::{self, ActiveModel as UsageActive, Model as Usage};

#[derive(Debug, Clone, PartialEq)]
pub enum PutBudgetOutcome {
    Saved(Budget),
    /// The row is already what this request would have produced (its own retry).
    Replayed(Budget),
    /// `version` is not the expected one (0 = no budget for this unit/source).
    Conflict {
        current_version: i64,
    },
}

fn is_unique_violation(err: &DbErr) -> bool {
    matches!(err.sql_err(), Some(SqlErr::UniqueConstraintViolation(_)))
}

pub async fn find_budget<C: ConnectionTrait>(
    db: &C,
    task_id: &str,
    unit: &str,
    source: &str,
) -> Result<Option<Budget>> {
    collab_task_budget::Entity::find()
        .filter(collab_task_budget::Column::TaskId.eq(task_id))
        .filter(collab_task_budget::Column::Unit.eq(unit))
        .filter(collab_task_budget::Column::Source.eq(source))
        .one(db)
        .await
        .context("db: find task budget")
}

pub async fn list_budgets<C: ConnectionTrait>(
    db: &C,
    repo_id: i64,
    task_id: &str,
) -> Result<Vec<Budget>> {
    collab_task_budget::Entity::find()
        .filter(collab_task_budget::Column::RepoId.eq(repo_id))
        .filter(collab_task_budget::Column::TaskId.eq(task_id))
        .order_by_asc(collab_task_budget::Column::Unit)
        .order_by_asc(collab_task_budget::Column::Source)
        .all(db)
        .await
        .context("db: list task budgets")
}

/// Set the budget of `(task, unit, source)` iff it is still at
/// `expected_version` (0 = none yet). `model` carries the new content with
/// `version = expected_version + 1`.
pub async fn put_budget(
    db: &DatabaseConnection,
    expected_version: i64,
    model: BudgetActive,
) -> Result<PutBudgetOutcome> {
    let task_id = model.task_id.clone().unwrap();
    let unit = model.unit.clone().unwrap();
    let source = model.source.clone().unwrap();
    let hash = model.payload_hash.clone().unwrap();

    if expected_version == 0 {
        return match model.insert(db).await {
            Ok(row) => Ok(PutBudgetOutcome::Saved(row)),
            Err(err) if is_unique_violation(&err) => {
                resolve_budget(db, &task_id, &unit, &source, expected_version, &hash).await
            }
            Err(err) => Err(err).context("db: insert task budget"),
        };
    }
    let res = collab_task_budget::Entity::update_many()
        .col_expr(
            collab_task_budget::Column::Version,
            Expr::value(expected_version + 1),
        )
        .col_expr(
            collab_task_budget::Column::Amount,
            Expr::value(model.amount.clone().unwrap()),
        )
        .col_expr(
            collab_task_budget::Column::AlertAt,
            Expr::value(model.alert_at.clone().unwrap()),
        )
        .col_expr(
            collab_task_budget::Column::SamplingPeriod,
            Expr::value(model.sampling_period.clone().unwrap()),
        )
        .col_expr(
            collab_task_budget::Column::SetByKind,
            Expr::value(model.set_by_kind.clone().unwrap()),
        )
        .col_expr(
            collab_task_budget::Column::SetById,
            Expr::value(model.set_by_id.clone().unwrap()),
        )
        .col_expr(
            collab_task_budget::Column::PayloadHash,
            Expr::value(hash.clone()),
        )
        .col_expr(
            collab_task_budget::Column::UpdatedAt,
            Expr::value(model.updated_at.clone().unwrap()),
        )
        .filter(collab_task_budget::Column::TaskId.eq(task_id.clone()))
        .filter(collab_task_budget::Column::Unit.eq(unit.clone()))
        .filter(collab_task_budget::Column::Source.eq(source.clone()))
        .filter(collab_task_budget::Column::Version.eq(expected_version))
        .exec(db)
        .await
        .context("db: replace task budget")?;
    if res.rows_affected == 1 {
        let row = find_budget(db, &task_id, &unit, &source)
            .await?
            .context("db: budget vanished after update")?;
        return Ok(PutBudgetOutcome::Saved(row));
    }
    resolve_budget(db, &task_id, &unit, &source, expected_version, &hash).await
}

async fn resolve_budget(
    db: &DatabaseConnection,
    task_id: &str,
    unit: &str,
    source: &str,
    expected_version: i64,
    hash: &str,
) -> Result<PutBudgetOutcome> {
    Ok(match find_budget(db, task_id, unit, source).await? {
        Some(row) if row.version == expected_version + 1 && row.payload_hash == hash => {
            PutBudgetOutcome::Replayed(row)
        }
        Some(row) => PutBudgetOutcome::Conflict {
            current_version: row.version,
        },
        None => PutBudgetOutcome::Conflict { current_version: 0 },
    })
}

pub async fn find_usage_by_client_id<C: ConnectionTrait>(
    db: &C,
    reporter_kind: &str,
    reporter_id: &str,
    client_usage_id: &str,
) -> Result<Option<Usage>> {
    collab_task_usage::Entity::find()
        .filter(collab_task_usage::Column::ReportedByKind.eq(reporter_kind))
        .filter(collab_task_usage::Column::ReportedById.eq(reporter_id))
        .filter(collab_task_usage::Column::ClientUsageId.eq(client_usage_id))
        .one(db)
        .await
        .context("db: find usage by client id")
}

pub async fn count_usage<C: ConnectionTrait>(db: &C, task_id: &str) -> Result<u64> {
    collab_task_usage::Entity::find()
        .filter(collab_task_usage::Column::TaskId.eq(task_id))
        .count(db)
        .await
        .context("db: count usage")
}

pub async fn count_budgets<C: ConnectionTrait>(db: &C, task_id: &str) -> Result<u64> {
    collab_task_budget::Entity::find()
        .filter(collab_task_budget::Column::TaskId.eq(task_id))
        .count(db)
        .await
        .context("db: count budgets")
}

#[derive(Debug, Clone, PartialEq)]
pub enum AppendUsageOutcome {
    Created(Usage),
    /// `(reporter, client_usage_id)` already exists.
    Existing(Usage),
}

pub async fn append_usage(
    db: &DatabaseConnection,
    model: UsageActive,
) -> Result<AppendUsageOutcome> {
    let kind = model.reported_by_kind.clone().unwrap();
    let id = model.reported_by_id.clone().unwrap();
    let client_id = model.client_usage_id.clone().unwrap();
    if let Some(existing) = find_usage_by_client_id(db, &kind, &id, &client_id).await? {
        return Ok(AppendUsageOutcome::Existing(existing));
    }
    match model.insert(db).await {
        Ok(row) => Ok(AppendUsageOutcome::Created(row)),
        Err(err) if is_unique_violation(&err) => {
            let existing = find_usage_by_client_id(db, &kind, &id, &client_id)
                .await?
                .context("db: usage client id vanished after conflict")?;
            Ok(AppendUsageOutcome::Existing(existing))
        }
        Err(err) => Err(err).context("db: insert usage"),
    }
}

/// Every usage row of the task, oldest period first (bounded by the service's
/// per-task cap).
pub async fn all_usage<C: ConnectionTrait>(
    db: &C,
    repo_id: i64,
    task_id: &str,
) -> Result<Vec<Usage>> {
    collab_task_usage::Entity::find()
        .filter(collab_task_usage::Column::RepoId.eq(repo_id))
        .filter(collab_task_usage::Column::TaskId.eq(task_id))
        .order_by_asc(collab_task_usage::Column::PeriodStart)
        .order_by_asc(collab_task_usage::Column::Id)
        .all(db)
        .await
        .context("db: list all usage")
}

/// Keyset page of usage rows by `(created_at, id)` descending, optionally
/// filtered by exact unit and source.
pub async fn list_usage_page<C: ConnectionTrait>(
    db: &C,
    repo_id: i64,
    task_id: &str,
    unit: Option<&str>,
    source: Option<&str>,
    after: Option<(DateTime<Utc>, String)>,
    limit: u64,
) -> Result<Vec<Usage>> {
    let mut query = collab_task_usage::Entity::find()
        .filter(collab_task_usage::Column::RepoId.eq(repo_id))
        .filter(collab_task_usage::Column::TaskId.eq(task_id));
    if let Some(unit) = unit {
        query = query.filter(collab_task_usage::Column::Unit.eq(unit));
    }
    if let Some(source) = source {
        query = query.filter(collab_task_usage::Column::Source.eq(source));
    }
    if let Some((created, id)) = after {
        query = query.filter(
            Condition::any()
                .add(collab_task_usage::Column::CreatedAt.lt(created))
                .add(
                    Condition::all()
                        .add(collab_task_usage::Column::CreatedAt.eq(created))
                        .add(collab_task_usage::Column::Id.lt(id)),
                ),
        );
    }
    query
        .order_by_desc(collab_task_usage::Column::CreatedAt)
        .order_by_desc(collab_task_usage::Column::Id)
        .limit(limit)
        .all(db)
        .await
        .context("db: list usage")
}
