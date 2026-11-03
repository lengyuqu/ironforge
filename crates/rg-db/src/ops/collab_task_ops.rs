//! Database operations for `collab_tasks`.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use sea_orm::sea_query::Expr;
use sea_orm::*;

use crate::entities::collab_task::{self, ActiveModel, Entity as TaskEntity, Model as Task};

/// Result of a compare-and-swap update guarded by a `version` column.
#[derive(Debug, Clone, PartialEq)]
pub enum CasOutcome<T> {
    /// The row matched the expected version and was updated (new state inside).
    Updated(T),
    /// The row exists but its version/state no longer matches; `current` is the latest state.
    Conflict { current: T },
    /// No such row in the repository.
    NotFound,
}

/// Field changes for [`update_cas`]. `None` leaves a field unchanged; for
/// nullable fields `Some(None)` clears the value.
#[derive(Debug, Clone, Default)]
pub struct TaskPatch {
    pub title: Option<String>,
    pub description: Option<Option<String>>,
    pub status: Option<String>,
    pub owner_session_id: Option<Option<String>>,
    pub branch: Option<Option<String>>,
    pub paths_json: Option<String>,
    pub issue_number: Option<Option<i64>>,
    pub pr_number: Option<Option<i64>>,
}

/// Insert a task.
pub async fn create<C: ConnectionTrait>(db: &C, model: ActiveModel) -> Result<Task> {
    model.insert(db).await.context("db: create collab task")
}

/// Find a task by id, scoped to a repository.
pub async fn find_in_repo<C: ConnectionTrait>(
    db: &C,
    repo_id: i64,
    id: &str,
) -> Result<Option<Task>> {
    TaskEntity::find_by_id(id.to_string())
        .filter(collab_task::Column::RepoId.eq(repo_id))
        .one(db)
        .await
        .context("db: find collab task")
}

/// Keyset page of tasks ordered by `(updated_at DESC, id DESC)`.
pub async fn list_page<C: ConnectionTrait>(
    db: &C,
    repo_id: i64,
    status: Option<&str>,
    after: Option<(DateTime<Utc>, String)>,
    limit: u64,
) -> Result<Vec<Task>> {
    let mut query = TaskEntity::find().filter(collab_task::Column::RepoId.eq(repo_id));
    if let Some(status) = status {
        query = query.filter(collab_task::Column::Status.eq(status));
    }
    if let Some((updated, id)) = after {
        query = query.filter(
            Condition::any()
                .add(collab_task::Column::UpdatedAt.lt(updated))
                .add(
                    Condition::all()
                        .add(collab_task::Column::UpdatedAt.eq(updated))
                        .add(collab_task::Column::Id.lt(id)),
                ),
        );
    }
    query
        .order_by_desc(collab_task::Column::UpdatedAt)
        .order_by_desc(collab_task::Column::Id)
        .limit(limit)
        .all(db)
        .await
        .context("db: list collab tasks")
}

/// Apply `patch` iff the task is still at `expected_version`; bumps `version`.
///
/// The conditional `UPDATE` is the only writer of these columns, so of two
/// concurrent callers with the same expected version exactly one matches a row.
pub async fn update_cas(
    db: &DatabaseConnection,
    repo_id: i64,
    id: &str,
    expected_version: i64,
    patch: TaskPatch,
    now: DateTime<Utc>,
) -> Result<CasOutcome<Task>> {
    let txn = db.begin().await.context("db: begin task update")?;

    let mut update = TaskEntity::update_many()
        .col_expr(collab_task::Column::UpdatedAt, Expr::value(now))
        .col_expr(
            collab_task::Column::Version,
            Expr::col(collab_task::Column::Version).add(1),
        );
    if let Some(title) = patch.title {
        update = update.col_expr(collab_task::Column::Title, Expr::value(title));
    }
    if let Some(description) = patch.description {
        update = update.col_expr(collab_task::Column::Description, Expr::value(description));
    }
    if let Some(status) = patch.status {
        update = update.col_expr(collab_task::Column::Status, Expr::value(status));
    }
    if let Some(owner) = patch.owner_session_id {
        update = update.col_expr(collab_task::Column::OwnerSessionId, Expr::value(owner));
    }
    if let Some(branch) = patch.branch {
        update = update.col_expr(collab_task::Column::Branch, Expr::value(branch));
    }
    if let Some(paths) = patch.paths_json {
        update = update.col_expr(collab_task::Column::PathsJson, Expr::value(paths));
    }
    if let Some(issue) = patch.issue_number {
        update = update.col_expr(collab_task::Column::IssueNumber, Expr::value(issue));
    }
    if let Some(pr) = patch.pr_number {
        update = update.col_expr(collab_task::Column::PrNumber, Expr::value(pr));
    }
    let res = update
        .filter(collab_task::Column::Id.eq(id))
        .filter(collab_task::Column::RepoId.eq(repo_id))
        .filter(collab_task::Column::Version.eq(expected_version))
        .exec(&txn)
        .await
        .context("db: update collab task")?;

    let current = find_in_repo(&txn, repo_id, id).await?;
    txn.commit().await.context("db: commit task update")?;
    Ok(match (res.rows_affected, current) {
        (_, None) => CasOutcome::NotFound,
        (0, Some(current)) => CasOutcome::Conflict { current },
        (_, Some(updated)) => CasOutcome::Updated(updated),
    })
}
