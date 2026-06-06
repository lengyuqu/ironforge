//! Issue service — business logic for Issue CRUD, labels, milestones, comments.

use anyhow::{bail, Context, Result};
use chrono::Utc;
use sea_orm::{DatabaseConnection, EntityTrait, Set};

use rg_db::entities::issue::{self, Model as Issue};
use rg_db::entities::issue_comment::{self, Model as Comment};
use rg_db::ops::{issue_ops, issue_comment_ops, label_ops, issue_label_ops, repo_ops};

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
    let labels_json = labels.as_ref().map(|l| serde_json::to_string(l).unwrap_or_else(|_| "[]".into()));

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

    let issue = issue_ops::create(db, model).await?;

    // Trigger issue.opened webhook
    let payload = serde_json::json!({
        "id": issue.id,
        "repo_id": issue.repo_id,
        "number": issue.number,
        "title": issue.title,
        "state": issue.state,
        "author_id": issue.author_id,
    });
    if let Err(e) = crate::webhook::service::trigger_issue_opened(db, repo_id, &payload).await {
        tracing::warn!("Failed to trigger issue.opened webhook: {e}");
    }

    // Dual-write: sync labels to issue_labels junction table
    if let Some(ref label_names) = labels {
        if let Ok(all_labels) = label_ops::list_by_repo(db, repo_id).await {
            let label_ids: Vec<i64> = all_labels
                .iter()
                .filter(|l| label_names.contains(&l.name))
                .map(|l| l.id)
                .collect();
            if let Err(e) = issue_label_ops::set_labels(db, issue.id, label_ids).await {
                tracing::warn!("Failed to set labels for issue {}: {e}", issue.id);
            }
        }
    }

    Ok(issue)
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

/// Paginated list of issues filtered by labels. Returns issues that have ALL specified labels.
pub async fn list_issues_filtered_by_labels(
    db: &DatabaseConnection,
    owner: &str,
    repo_name: &str,
    state: Option<&str>,
    label_names: &[String],
    offset: u64,
    limit: u64,
) -> Result<(Vec<Issue>, i64)> {
    let repo = resolve_repo(db, owner, repo_name).await?;

    // Resolve label names to IDs
    let all_labels = label_ops::list_by_repo(db, repo.id).await?;
    let required_label_ids: Vec<i64> = all_labels
        .iter()
        .filter(|l| label_names.contains(&l.name))
        .map(|l| l.id)
        .collect();

    if required_label_ids.is_empty() {
        return Ok((Vec::new(), 0));
    }

    // Find issue IDs that have ALL required labels
    let (matching_issue_ids, total) = issue_label_ops::find_issues_with_all_labels(
        db,
        &required_label_ids,
        offset,
        limit,
    )
    .await?;

    if matching_issue_ids.is_empty() {
        return Ok((Vec::new(), total));
    }

    // Fetch the actual issue models
    let mut issues = Vec::new();
    for issue_id in matching_issue_ids {
        if let Some(issue) = issue_ops::find_by_id(db, issue_id).await? {
            // Apply state filter in-memory
            if let Some(s) = state {
                if issue.state != s {
                    continue;
                }
            }
            // Only include issues from this repo
            if issue.repo_id == repo.id {
                issues.push(issue);
            }
        }
    }

    Ok((issues, total))
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
        let was_open = issue.state == "open";
        issue.state = s.clone();
        if s == "closed" {
            issue.closed_at = Some(Utc::now());
        } else {
            issue.closed_at = None;
        }

        // Trigger issue.closed webhook when transitioning to closed
        if was_open && s == "closed" {
            let close_payload = serde_json::json!({
                "id": issue.id,
                "repo_id": issue.repo_id,
                "number": issue.number,
                "title": issue.title,
                "state": s,
            });
            if let Err(e) = crate::webhook::service::trigger_issue_closed(db, issue.repo_id, &close_payload).await {
                tracing::warn!("Failed to trigger issue.closed webhook: {e}");
            }

            // Check if milestone should be notified (all issues in milestone are closed)
            if let Some(mid) = issue.milestone_id {
                if let Ok(remaining) = rg_db::ops::milestone_ops::count_open_by_milestone(db, mid).await {
                    if remaining == 0 {
                        if let Err(e) = notify_milestone_closed(db, issue.repo_id, mid).await {
                            tracing::warn!("Failed to notify milestone {} closed: {e}", mid);
                        }
                    }
                }
            }
        }
    }
    if let Some(l) = labels {
        issue.labels = Some(serde_json::to_string(&l).unwrap_or_else(|_| "[]".into()));
        // Dual-write: sync labels to issue_labels junction table
        if let Ok(all_labels) = label_ops::list_by_repo(db, issue.repo_id).await {
            let label_ids: Vec<i64> = all_labels
                .iter()
                .filter(|label| l.contains(&label.name))
                .map(|l| l.id)
                .collect();
            if let Err(e) = issue_label_ops::set_labels(db, issue.id, label_ids).await {
                tracing::warn!("Failed to set labels for issue {}: {e}", issue.id);
            }
        }
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

    let comment = issue_comment_ops::create(db, model).await?;

    // Trigger issue.comment webhook
    let issue_payload = serde_json::json!({
        "id": issue.id,
        "repo_id": issue.repo_id,
        "number": issue.number,
        "title": issue.title,
    });
    let comment_payload = serde_json::json!({
        "id": comment.id,
        "body": comment.body,
        "author_id": comment.author_id,
    });
    if let Err(e) = crate::webhook::service::trigger_issue_comment(db, issue.repo_id, &issue_payload, &comment_payload).await {
        tracing::warn!("Failed to trigger issue.comment webhook: {e}");
    }

    Ok(comment)
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

/// Notify watchers and trigger webhook when all issues in a milestone are closed.
async fn notify_milestone_closed(
    db: &DatabaseConnection,
    repo_id: i64,
    milestone_id: i64,
) -> Result<()> {
    // Trigger milestone.closed webhook
    let payload = serde_json::json!({
        "id": milestone_id,
        "repo_id": repo_id,
    });
    if let Err(e) = crate::webhook::service::trigger_milestone_closed(db, repo_id, &payload).await {
        tracing::warn!("Failed to trigger milestone.closed webhook: {e}");
    }

    // Notify watchers about milestone completion
    if let Ok(Some(milestone)) = rg_db::ops::milestone_ops::find_by_id(db, milestone_id).await {
        // Look up repo name for notification
        if let Ok(Some(repo)) = rg_db::entities::repository::Entity::find_by_id(repo_id).one(db).await {
            if let Err(e) = crate::notification::notify_watchers(
                db,
                repo_id,
                "",
                &format!("Milestone {} in {}", "closed", repo.name),
                "milestone",
                Some(format!("Milestone '{}' {}", milestone.title, "closed")),
            )
            .await {
                tracing::warn!("Failed to notify watchers about milestone: {e}");
            }
        }
    }

    Ok(())
}

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
