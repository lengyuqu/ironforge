//! Reaction service — emoji reactions on issues and issue comments.
//!
//! Uniqueness (one reaction per user/content/target) is enforced by the
//! `idx_reactions_unique_target_user_content` DB index and surfaced as the
//! error string "reaction already exists" so the HTTP layer can map it to 409.

use crate::error::{CoreContext, CoreError, CoreResult};
use chrono::Utc;
use sea_orm::{DatabaseConnection, Set};

use rg_db::entities::reactions;
use rg_db::ops::{issue_comment_ops, reaction_ops};

/// Add a reaction to an issue body (comment_id = 0).
pub async fn add_issue_reaction(
    db: &DatabaseConnection,
    owner: &str,
    repo_name: &str,
    issue_number: i64,
    user_id: i64,
    content: &str,
) -> CoreResult<reactions::Model> {
    validate_content(content)?;
    let issue = super::service::get_issue(db, owner, repo_name, issue_number).await?;
    let reaction =
        insert_reaction(db, issue.id, 0, user_id, content.to_owned()).await?;

    // Notify the issue author (never the reactor itself).
    if issue.author_id != user_id {
        if let Err(e) = crate::notification::notify(
            db,
            issue.author_id,
            "issue_reaction",
            &format!(
                "{} reacted {} to your issue",
                actor_name(db, user_id).await,
                content
            ),
            Some(&issue.title),
            Some(issue.repo_id),
        )
        .await
        {
            tracing::warn!("Failed to notify issue author about reaction: {e}");
        }
    }

    Ok(reaction)
}

/// Remove the current user's reaction from an issue body.
/// Returns Ok(()) whether or not the reaction existed (idempotent).
pub async fn remove_issue_reaction(
    db: &DatabaseConnection,
    owner: &str,
    repo_name: &str,
    issue_number: i64,
    user_id: i64,
    content: &str,
) -> CoreResult<()> {
    let issue = super::service::get_issue(db, owner, repo_name, issue_number).await?;
    remove_reaction(db, issue.id, 0, user_id, content).await
}

/// List reactions on an issue body (comment_id = 0).
pub async fn list_issue_reactions(
    db: &DatabaseConnection,
    owner: &str,
    repo_name: &str,
    issue_number: i64,
) -> CoreResult<Vec<reactions::Model>> {
    let issue = super::service::get_issue(db, owner, repo_name, issue_number).await?;
    Ok(reaction_ops::list_by_target(db, issue.id, 0).await?)
}

/// Add a reaction to an issue comment.
pub async fn add_comment_reaction(
    db: &DatabaseConnection,
    comment_id: i64,
    user_id: i64,
    content: &str,
) -> CoreResult<reactions::Model> {
    validate_content(content)?;
    let comment = issue_comment_ops::find_by_id(db, comment_id)
        .await?
        .context("comment not found")?;
    let reaction =
        insert_reaction(db, comment.issue_id, comment.id, user_id, content.to_owned()).await?;

    // Notify the comment author (never the reactor itself).
    if comment.author_id != user_id {
        if let Err(e) = crate::notification::notify(
            db,
            comment.author_id,
            "comment_reaction",
            &format!("{} reacted {} to your comment", actor_name(db, user_id).await, content),
            Some(&truncate(&comment.body, 80)),
            None,
        )
        .await
        {
            tracing::warn!("Failed to notify comment author about reaction: {e}");
        }
    }

    Ok(reaction)
}

/// Remove the current user's reaction from a comment (idempotent).
pub async fn remove_comment_reaction(
    db: &DatabaseConnection,
    comment_id: i64,
    user_id: i64,
    content: &str,
) -> CoreResult<()> {
    let comment = issue_comment_ops::find_by_id(db, comment_id)
        .await?
        .context("comment not found")?;
    remove_reaction(db, comment.issue_id, comment.id, user_id, content).await
}

/// List reactions on a comment.
pub async fn list_comment_reactions(
    db: &DatabaseConnection,
    comment_id: i64,
) -> CoreResult<Vec<reactions::Model>> {
    let comment = issue_comment_ops::find_by_id(db, comment_id)
        .await?
        .context("comment not found")?;
    Ok(reaction_ops::list_by_target(db, comment.issue_id, comment.id).await?)
}

// ── Helpers ─────────────────────────────────────────────────────────────

fn validate_content(content: &str) -> CoreResult<()> {
    if reaction_ops::REACTION_CONTENTS.contains(&content) {
        Ok(())
    } else {
        Err(CoreError::InvalidInput(format!(
            "invalid reaction content, must be one of: {}",
            reaction_ops::REACTION_CONTENTS.join(", ")
        )))
    }
}

async fn insert_reaction(
    db: &DatabaseConnection,
    issue_id: i64,
    comment_id: i64,
    user_id: i64,
    content: String,
) -> CoreResult<reactions::Model> {
    // Validate the user exists to get a clean 404/500 instead of an FK error.
    if rg_db::ops::user_ops::find_by_id(db, user_id)
        .await?
        .is_none()
    {
        return Err(CoreError::NotFound("user not found".into()));
    }

    let model = reactions::ActiveModel {
        id: sea_orm::NotSet,
        issue_id: Set(issue_id),
        comment_id: Set(comment_id),
        user_id: Set(user_id),
        content: Set(content),
        created_at: Set(Utc::now()),
    };
    Ok(reaction_ops::create(db, model).await?)
}

async fn remove_reaction(
    db: &DatabaseConnection,
    issue_id: i64,
    comment_id: i64,
    user_id: i64,
    content: &str,
) -> CoreResult<()> {
    let existing = reaction_ops::list_by_target(db, issue_id, comment_id).await?;
    let Some(row) = existing
        .iter()
        .find(|r| r.user_id == user_id && r.content == content)
    else {
        return Ok(());
    };
    Ok(reaction_ops::delete_by_id(db, row.id).await?)
}

async fn actor_name(db: &DatabaseConnection, user_id: i64) -> String {
    rg_db::ops::user_ops::find_by_id(db, user_id)
        .await
        .ok()
        .flatten()
        .map(|u| u.username)
        .unwrap_or_else(|| user_id.to_string())
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_owned()
    } else {
        let cut: String = s.chars().take(max).collect();
        format!("{cut}…")
    }
}

/// Delete all reactions attached to a comment, in the caller's transaction.
/// Called from [`super::service::delete_comment`] because `comment_id` has
/// no FK (0 means "issue body").
pub async fn delete_reactions_for_comment<C: sea_orm::ConnectionTrait>(
    db: &C,
    comment_id: i64,
) -> CoreResult<u64> {
    Ok(reaction_ops::delete_by_comment(db, comment_id).await?)
}
