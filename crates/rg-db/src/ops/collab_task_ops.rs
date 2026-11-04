//! Database operations for `collab_tasks`.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use sea_orm::sea_query::Expr;
use sea_orm::*;

use super::collab_metric_ops;
use super::collab_notice_ops::{self, kind, source, NewNotice};
use crate::entities::collab_task::{self, ActiveModel, Entity as TaskEntity, Model as Task};
use crate::entities::collab_task_dependency::{self, Entity as EdgeEntity};

/// The only status that ends a dependency wait.
const DONE: &str = "done";

/// Notices for the owners of unfinished tasks that were waiting on `finished`.
///
/// Written in the same transaction as the status change, so a committed
/// `done` always has them and a rolled-back one never leaves a stray. The
/// finished task's `version` is the generation: a task that is reopened and
/// finished again yields a new notice, while a repeated write of the same
/// version cannot. Statuses are declarations, so this is a hint to re-read
/// the dependency, not evidence the work is complete.
async fn dependency_finished_notices(
    txn: &DatabaseTransaction,
    finished: &Task,
    now: DateTime<Utc>,
) -> Result<()> {
    let edges = EdgeEntity::find()
        .filter(collab_task_dependency::Column::RepoId.eq(finished.repo_id))
        .filter(collab_task_dependency::Column::DependsOnTaskId.eq(finished.id.as_str()))
        .all(txn)
        .await
        .context("db: find dependents of finished task")?;
    if edges.is_empty() {
        return Ok(());
    }
    let dependents = TaskEntity::find()
        .filter(collab_task::Column::RepoId.eq(finished.repo_id))
        .filter(collab_task::Column::Id.is_in(edges.iter().map(|e| e.task_id.clone())))
        .all(txn)
        .await
        .context("db: load dependent tasks")?;
    let mut notices = Vec::new();
    for dependent in dependents {
        let Some(owner) = dependent.owner_session_id.clone() else {
            continue;
        };
        if dependent.status == DONE {
            continue;
        }
        notices.push(NewNotice {
            repo_id: finished.repo_id,
            task_id: dependent.id.clone(),
            recipient_session_id: owner,
            kind: kind::DEPENDENCY_FINISHED,
            source_type: source::DEPENDENCY,
            source_id: finished.id.clone(),
            generation: finished.version,
            hint_json: serde_json::json!({
                "dependency_task_id": finished.id,
                "dependent_task_id": dependent.id,
                "declared_status": DONE,
            })
            .to_string(),
            created_at: now,
        });
    }
    collab_notice_ops::insert_all(txn, &notices).await
}

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
pub async fn create(db: &DatabaseConnection, model: ActiveModel) -> Result<Task> {
    let txn = db.begin().await.context("db: begin task create")?;
    let task = model.insert(&txn).await.context("db: create collab task")?;
    collab_metric_ops::record_task_created(&txn, &task).await?;
    txn.commit().await.context("db: commit task create")?;
    Ok(task)
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

/// Tasks of the repository whose status is one of `statuses`, most recently
/// updated first. For summary views; use [`list_page`] to page through tasks.
pub async fn list_with_statuses<C: ConnectionTrait>(
    db: &C,
    repo_id: i64,
    statuses: &[&str],
    limit: u64,
) -> Result<Vec<Task>> {
    TaskEntity::find()
        .filter(collab_task::Column::RepoId.eq(repo_id))
        .filter(collab_task::Column::Status.is_in(statuses.iter().copied()))
        .order_by_desc(collab_task::Column::UpdatedAt)
        .order_by_desc(collab_task::Column::Id)
        .limit(limit)
        .all(db)
        .await
        .context("db: list tasks by status")
}

/// Unfinished tasks of the repository that declared at least one path, other
/// than `exclude_id`, in ascending id order after `after_id`. Candidates for
/// path-overlap warnings (P1 U03); the paths themselves are compared by the caller.
pub async fn list_overlap_candidates<C: ConnectionTrait>(
    db: &C,
    repo_id: i64,
    exclude_id: &str,
    after_id: Option<&str>,
    limit: u64,
) -> Result<Vec<Task>> {
    let mut query = TaskEntity::find()
        .filter(collab_task::Column::RepoId.eq(repo_id))
        .filter(collab_task::Column::Id.ne(exclude_id))
        .filter(collab_task::Column::Status.ne(DONE))
        .filter(collab_task::Column::PathsJson.ne("[]"));
    if let Some(after) = after_id {
        query = query.filter(collab_task::Column::Id.gt(after));
    }
    query
        .order_by_asc(collab_task::Column::Id)
        .limit(limit)
        .all(db)
        .await
        .context("db: list overlap candidates")
}

/// Unfinished tasks currently owned by `session_id`, newest update first.
pub async fn list_owned_open<C: ConnectionTrait>(
    db: &C,
    repo_id: i64,
    session_id: &str,
    limit: u64,
) -> Result<Vec<Task>> {
    TaskEntity::find()
        .filter(collab_task::Column::RepoId.eq(repo_id))
        .filter(collab_task::Column::OwnerSessionId.eq(session_id))
        .filter(collab_task::Column::Status.ne("done"))
        .order_by_desc(collab_task::Column::UpdatedAt)
        .order_by_desc(collab_task::Column::Id)
        .limit(limit)
        .all(db)
        .await
        .context("db: list owned open tasks")
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
    let new_status = patch.status.clone();
    // A status change needs the status it replaces for the metric event. The
    // no-op write comes first (SQLite: a read-first transaction cannot upgrade
    // under concurrent writers) and, matching only `expected_version`, locks the
    // row, so the status read next is the one this update replaces.
    let mut prior_status = None;
    if new_status.is_some() {
        TaskEntity::update_many()
            .col_expr(
                collab_task::Column::UpdatedAt,
                Expr::col(collab_task::Column::UpdatedAt).into(),
            )
            .filter(collab_task::Column::Id.eq(id))
            .filter(collab_task::Column::RepoId.eq(repo_id))
            .filter(collab_task::Column::Version.eq(expected_version))
            .exec(&txn)
            .await
            .context("db: lock collab task")?;
        prior_status = find_in_repo(&txn, repo_id, id)
            .await?
            .filter(|t| t.version == expected_version)
            .map(|t| t.status);
    }

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
        // Only a request that carries `paths` re-declares them; no other field
        // refreshes this timestamp.
        update = update.col_expr(collab_task::Column::PathsDeclaredAt, Expr::value(now));
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
    if let (1.., Some(task), Some(to), Some(from)) =
        (res.rows_affected, &current, &new_status, &prior_status)
    {
        if from != to {
            collab_metric_ops::record_task_status_change(&txn, task, from, now).await?;
            if to == DONE {
                dependency_finished_notices(&txn, task, now).await?;
            }
        }
    }
    txn.commit().await.context("db: commit task update")?;
    Ok(match (res.rows_affected, current) {
        (_, None) => CasOutcome::NotFound,
        (0, Some(current)) => CasOutcome::Conflict { current },
        (_, Some(updated)) => CasOutcome::Updated(updated),
    })
}
