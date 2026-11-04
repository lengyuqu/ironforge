//! Assignee service — multi-assignee management for issues (ISSUE-105).
//!
//! The junction table `issue_assignees` holds the full set; the legacy
//! `issues.assignee_id` column is kept in sync with the first entry
//! ("primary assignee") for compatibility with existing read paths.

use crate::error::{CoreError, CoreResult};
use sea_orm::DatabaseConnection;

use rg_db::entities::issue::Model as Issue;
use rg_db::ops::{issue_assignee_ops, issue_ops, user_ops};

/// Replace the assignee set of an issue by usernames.
/// Unknown usernames are rejected with InvalidInput (→ 400).
/// Returns the effective assignee usernames (deduplicated, order preserved).
pub async fn set_issue_assignees(
    db: &DatabaseConnection,
    owner: &str,
    repo_name: &str,
    number: i64,
    actor_id: i64,
    usernames: Vec<String>,
) -> CoreResult<Vec<String>> {
    let issue = super::service::get_issue(db, owner, repo_name, number).await?;
    let mut issue = issue;

    // Resolve usernames to user IDs (fail on unknown users).
    let mut user_ids: Vec<i64> = Vec::with_capacity(usernames.len());
    let mut resolved_names: Vec<String> = Vec::with_capacity(usernames.len());
    for name in usernames {
        let name = name.trim().to_string();
        if name.is_empty() {
            continue;
        }
        if resolved_names.contains(&name) {
            continue;
        }
        let user = user_ops::find_by_username(db, &name)
            .await?
            .ok_or_else(|| CoreError::InvalidInput(format!("unknown assignee: {name}")))?;
        user_ids.push(user.id);
        resolved_names.push(user.username);
    }

    apply_assignees(db, &mut issue, &user_ids).await?;

    // Notify newly assigned users (never the actor themselves).
    for (idx, user_id) in user_ids.iter().enumerate() {
        if *user_id == actor_id {
            continue;
        }
        if let Err(e) = crate::notification::notify(
            db,
            *user_id,
            "issue_assigned",
            &format!("{} assigned you to issue #{}: {}", actor_username(db, actor_id).await, issue.number, issue.title),
            Some(&issue.title),
            Some(issue.repo_id),
        )
        .await
        {
            tracing::warn!("Failed to notify assignee {}: {e}", resolved_names[idx]);
        }
    }

    Ok(resolved_names)
}

/// List assignee usernames of an issue.
pub async fn list_issue_assignees(
    db: &DatabaseConnection,
    owner: &str,
    repo_name: &str,
    number: i64,
) -> CoreResult<Vec<String>> {
    let issue = super::service::get_issue(db, owner, repo_name, number).await?;
    assignee_names_by_issue(db, issue.id).await
}

/// Resolve assignee usernames for one issue ID (used by the HTTP layer to
/// decorate issue responses).
pub async fn assignee_names_by_issue(
    db: &DatabaseConnection,
    issue_id: i64,
) -> CoreResult<Vec<String>> {
    let user_ids = issue_assignee_ops::list_user_ids_by_issue(db, issue_id).await?;
    let mut names = Vec::with_capacity(user_ids.len());
    for user_id in user_ids {
        let name = user_ops::find_by_id(db, user_id)
            .await?
            .map(|u| u.username)
            .unwrap_or_else(|| user_id.to_string());
        names.push(name);
    }
    Ok(names)
}

/// List issues of a repo assigned to a user, paginated.
/// Mirrors the labels-filter pattern: resolve matching issue IDs, then batch
/// fetch and filter by repo/state in memory.
pub async fn list_issues_filtered_by_assignee(
    db: &DatabaseConnection,
    owner: &str,
    repo_name: &str,
    state: Option<&str>,
    assignee_username: &str,
    offset: u64,
    limit: u64,
) -> CoreResult<(Vec<Issue>, i64)> {
    let repo = crate::repo::service::find_repo_by_owner_name(db, owner, repo_name)
        .await?
        .ok_or_else(|| CoreError::NotFound("repository not found".into()))?;

    let assignee = user_ops::find_by_username(db, assignee_username)
        .await?
        .ok_or_else(|| CoreError::NotFound(format!("unknown assignee: {assignee_username}")))?;

    let matching_ids = issue_assignee_ops::find_issue_ids_by_user(db, assignee.id).await?;
    if matching_ids.is_empty() {
        return Ok((Vec::new(), 0));
    }

    let all = issue_ops::find_by_ids(db, &matching_ids).await?;
    let filtered: Vec<Issue> = all
        .into_iter()
        .filter(|issue| {
            issue.repo_id == repo.id
                && state.map(|s| issue.state == s).unwrap_or(true)
        })
        .collect();

    let total = filtered.len() as i64;
    let page = filtered
        .into_iter()
        .skip(offset as usize)
        .take(limit as usize)
        .collect();
    Ok((page, total))
}

// ── Helpers ─────────────────────────────────────────────────────────────

/// Write the assignee set to the junction table and mirror the primary
/// assignee into `issues.assignee_id`. Also bumps `updated_at`.
async fn apply_assignees(
    db: &DatabaseConnection,
    issue: &mut Issue,
    user_ids: &[i64],
) -> CoreResult<()> {
    issue_assignee_ops::set_assignees(db, issue.id, user_ids).await?;

    // Mirror primary assignee into the legacy column.
    let primary = user_ids.first().copied();
    if primary != issue.assignee_id {
        let mut active: rg_db::entities::issue::ActiveModel = issue.clone().into();
        active.assignee_id = sea_orm::Set(primary);
        active.updated_at = sea_orm::Set(chrono::Utc::now());
        *issue = issue_ops::update(db, active).await?;
    }
    Ok(())
}

async fn actor_username(db: &DatabaseConnection, user_id: i64) -> String {
    user_ops::find_by_id(db, user_id)
        .await
        .ok()
        .flatten()
        .map(|u| u.username)
        .unwrap_or_else(|| user_id.to_string())
}
