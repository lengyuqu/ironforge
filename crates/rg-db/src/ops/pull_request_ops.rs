//! Database operations for pull requests.

use anyhow::{Context, Result};
use sea_orm::*;

use crate::entities::pull_request::{self, ActiveModel, Entity as PrEntity, Model as PullRequest};

/// Find a PR by (repo_id, number).
pub async fn find_by_repo_and_number(
    db: &DatabaseConnection,
    repo_id: i64,
    number: i64,
) -> Result<Option<PullRequest>> {
    PrEntity::find()
        .filter(pull_request::Column::RepoId.eq(repo_id))
        .filter(pull_request::Column::Number.eq(number))
        .one(db)
        .await
        .context("db: find PR by repo and number")
}

/// List PRs for a repo, optionally filtered by state.
pub async fn list_by_repo(
    db: &DatabaseConnection,
    repo_id: i64,
    state: Option<&str>,
) -> Result<Vec<PullRequest>> {
    let mut query = PrEntity::find().filter(pull_request::Column::RepoId.eq(repo_id));
    if let Some(s) = state {
        query = query.filter(pull_request::Column::State.eq(s));
    }
    query
        .order_by_desc(pull_request::Column::CreatedAt)
        .all(db)
        .await
        .context("db: list PRs by repo")
}

/// Get the next PR number for a repo (max + 1, or 1 if no PRs).
pub async fn next_number(db: &DatabaseConnection, repo_id: i64) -> Result<i64> {
    let max = PrEntity::find()
        .filter(pull_request::Column::RepoId.eq(repo_id))
        .order_by_desc(pull_request::Column::Number)
        .one(db)
        .await
        .context("db: get max PR number")?;
    Ok(max.map(|m| m.number + 1).unwrap_or(1))
}

/// Create a new PR.
pub async fn create(db: &DatabaseConnection, model: ActiveModel) -> Result<PullRequest> {
    model.insert(db).await.context("db: create PR")
}

/// Update a PR.
pub async fn update(db: &DatabaseConnection, model: ActiveModel) -> Result<PullRequest> {
    model.update(db).await.context("db: update PR")
}

/// Delete a PR by id.
pub async fn delete_by_id(db: &DatabaseConnection, id: i64) -> Result<()> {
    PrEntity::delete_by_id(id)
        .exec(db)
        .await
        .context("db: delete PR")?;
    Ok(())
}
