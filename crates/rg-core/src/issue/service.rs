//! Issue service — business logic for Issue CRUD, labels, milestones, comments.

use anyhow::{bail, Context, Result};
use chrono::Utc;
use sea_orm::{DatabaseConnection, Set};

use rg_db::entities::issue::{self, Model as Issue};
use rg_db::entities::issue_comment::{self, Model as Comment};
use rg_db::ops::{issue_ops, issue_comment_ops, repo_ops};

// ── Issue CRUD ──────────────────────────────────────────────────────────

/// Create a new issue in the given repo.
pub async fn create_issue(
    db: &DatabaseConnection,
    repo_id: i64,
    author_id: i64,
    title: String,
    body: Option<String>,
    labels: Option<Vec<String>>,
    milestone_id: Option<i64>,
) -> Result<Issue> {
    if title.trim().is_empty() {
        bail!("issue title cannot be empty");
    }

    let number = issue_ops::next_number(db, repo_id).await?;
    let labels_json = labels.map(|l| serde_json::to_string(&l).unwrap_or_else(|_| "[]".into()));

    let model = issue::ActiveModel {
        id: sea_orm::NotSet,
        repo_id: Set(repo_id),
        number: Set(number),
        title: Set(title),
        body: Set(body),
        state: Set("open".to_string()),
        author_id: Set(author_id),
        assignee_id: Set(None),
        milestone_id: Set(milestone_id),
        labels: Set(labels_json),
        created_at: Set(Utc::now()),
        updated_at: Set(Utc::now()),
        closed_at: Set(None),
    };

    issue_ops::create(db, model).await
}

/// List issues for a repo, optionally filtered by state.
pub async fn list_issues(
    db: &DatabaseConnection,
    owner: &str,
    repo_name: &str,
    state: Option<&str>,
) -> Result<Vec<Issue>> {
    let repo = resolve_repo(db, owner, repo_name).await?;
    issue_ops::list_by_repo(db, repo.id, state).await
}

/// Paginated list of issues. Returns (issues, total).
pub async fn list_issues_paginated(
    db: &DatabaseConnection,
    owner: &str,
    repo_name: &str,
    state: Option<&str>,
    offset: u64,
    limit: u64,
) -> Result<(Vec<Issue>, i64)> {
    let repo = resolve_repo(db, owner, repo_name).await?;
    issue_ops::list_by_repo_paginated(db, repo.id, state, offset, limit).await
}

/// Get a single issue by repo owner/name and issue number.
pub async fn get_issue(
    db: &DatabaseConnection,
    owner: &str,
    repo_name: &str,
    number: i64,
) -> Result<Issue> {
    let repo = resolve_repo(db, owner, repo_name).await?;
    issue_ops::find_by_repo_and_number(db, repo.id, number)
        .await?
        .context("issue not found")
}

/// Update an issue's title, body, state, labels, assignee, or milestone.
pub async fn update_issue(
    db: &DatabaseConnection,
    owner: &str,
    repo_name: &str,
    number: i64,
    title: Option<String>,
    body: Option<String>,
    state: Option<String>,
    labels: Option<Vec<String>>,
    assignee_id: Option<Option<i64>>,
    milestone_id: Option<Option<i64>>,
) -> Result<Issue> {
    let mut issue = get_issue(db, owner, repo_name, number).await?;

    if let Some(t) = title {
        if t.trim().is_empty() {
            bail!("issue title cannot be empty");
        }
        issue.title = t;
    }
    if let Some(b) = body {
        issue.body = Some(b);
    }
    if let Some(s) = &state {
        if s != "open" && s != "closed" {
            bail!("invalid issue state: {}, must be open or closed", s);
        }
        issue.state = s.clone();
        if s == "closed" {
            issue.closed_at = Some(Utc::now());
        } else {
            issue.closed_at = None;
        }
    }
    if let Some(l) = labels {
        issue.labels = Some(serde_json::to_string(&l).unwrap_or_else(|_| "[]".into()));
    }
    if let Some(a) = assignee_id {
        issue.assignee_id = a;
    }
    if let Some(m) = milestone_id {
        issue.milestone_id = m;
    }

    issue.updated_at = Utc::now();

    let active: issue::ActiveModel = issue.into();
    issue_ops::update(db, active).await
}

// ── Issue Comments ──────────────────────────────────────────────────────

/// Add a comment to an issue.
pub async fn add_comment(
    db: &DatabaseConnection,
    owner: &str,
    repo_name: &str,
    issue_number: i64,
    author_id: i64,
    body: String,
) -> Result<Comment> {
    if body.trim().is_empty() {
        bail!("comment body cannot be empty");
    }

    let issue = get_issue(db, owner, repo_name, issue_number).await?;

    let model = issue_comment::ActiveModel {
        id: sea_orm::NotSet,
        issue_id: Set(issue.id),
        author_id: Set(author_id),
        body: Set(body),
        created_at: Set(Utc::now()),
        updated_at: Set(Utc::now()),
    };

    issue_comment_ops::create(db, model).await
}

/// List comments for an issue.
pub async fn list_comments(
    db: &DatabaseConnection,
    owner: &str,
    repo_name: &str,
    issue_number: i64,
) -> Result<Vec<Comment>> {
    let issue = get_issue(db, owner, repo_name, issue_number).await?;
    issue_comment_ops::list_by_issue(db, issue.id).await
}

/// Update a comment.
pub async fn update_comment(
    db: &DatabaseConnection,
    comment_id: i64,
    body: String,
) -> Result<Comment> {
    if body.trim().is_empty() {
        bail!("comment body cannot be empty");
    }

    let comment = issue_comment_ops::find_by_id(db, comment_id)
        .await?
        .context("comment not found")?;

    let mut active: issue_comment::ActiveModel = comment.into();
    active.body = Set(body);
    active.updated_at = Set(Utc::now());

    issue_comment_ops::update(db, active).await
}

/// Delete a comment.
pub async fn delete_comment(db: &DatabaseConnection, comment_id: i64) -> Result<()> {
    issue_comment_ops::delete_by_id(db, comment_id).await
}

// ── Helpers ─────────────────────────────────────────────────────────────

async fn resolve_repo(
    db: &DatabaseConnection,
    owner: &str,
    repo_name: &str,
) -> Result<rg_db::entities::repository::Model> {
    let user = rg_db::ops::user_ops::find_by_username(db, owner)
        .await?
        .context("owner not found")?;
    repo_ops::find_by_owner_and_name(db, user.id, repo_name)
        .await?
        .context("repository not found")
}
