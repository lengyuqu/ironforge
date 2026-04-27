//! Database operations for issues.

use anyhow::{Context, Result};
use sea_orm::*;

use crate::entities::issue::{self, ActiveModel, Entity as IssueEntity, Model as Issue};

/// Find an issue by (repo_id, number).
pub async fn find_by_repo_and_number(
    db: &DatabaseConnection,
    repo_id: i64,
    number: i64,
) -> Result<Option<Issue>> {
    IssueEntity::find()
        .filter(issue::Column::RepoId.eq(repo_id))
        .filter(issue::Column::Number.eq(number))
        .one(db)
        .await
        .context("db: find issue by repo and number")
}

/// List issues for a repo, optionally filtered by state.
pub async fn list_by_repo(
    db: &DatabaseConnection,
    repo_id: i64,
    state: Option<&str>,
) -> Result<Vec<Issue>> {
    let mut query = IssueEntity::find().filter(issue::Column::RepoId.eq(repo_id));
    if let Some(s) = state {
        query = query.filter(issue::Column::State.eq(s));
    }
    query
        .order_by_desc(issue::Column::CreatedAt)
        .all(db)
        .await
        .context("db: list issues by repo")
}

/// Paginated list of issues for a repo.
/// Returns (data, total) — SQL LIMIT/OFFSET pushed to the database.
pub async fn list_by_repo_paginated(
    db: &DatabaseConnection,
    repo_id: i64,
    state: Option<&str>,
    offset: u64,
    limit: u64,
) -> Result<(Vec<Issue>, i64)> {
    let mut base = IssueEntity::find().filter(issue::Column::RepoId.eq(repo_id));
    if let Some(s) = state {
        base = base.filter(issue::Column::State.eq(s));
    }
    let query = base.order_by_desc(issue::Column::CreatedAt);

    let total = query.clone().count(db).await.context("db: count issues by repo")? as i64;
    let issues = query
        .offset(offset)
        .limit(limit)
        .all(db)
        .await
        .context("db: list issues by repo (paginated)")?;

    Ok((issues, total))
}

/// Get the next issue number for a repo (max + 1, or 1 if no issues).
pub async fn next_number(db: &DatabaseConnection, repo_id: i64) -> Result<i64> {
    let max = IssueEntity::find()
        .filter(issue::Column::RepoId.eq(repo_id))
        .order_by_desc(issue::Column::Number)
        .one(db)
        .await
        .context("db: get max issue number")?;
    Ok(max.map(|m| m.number + 1).unwrap_or(1))
}

/// Create a new issue.
pub async fn create(db: &DatabaseConnection, model: ActiveModel) -> Result<Issue> {
    model.insert(db).await.context("db: create issue")
}

/// Update an issue.
pub async fn update(db: &DatabaseConnection, model: ActiveModel) -> Result<Issue> {
    model.update(db).await.context("db: update issue")
}

/// Delete an issue by id.
pub async fn delete_by_id(db: &DatabaseConnection, id: i64) -> Result<()> {
    IssueEntity::delete_by_id(id)
        .exec(db)
        .await
        .context("db: delete issue")?;
    Ok(())
}
