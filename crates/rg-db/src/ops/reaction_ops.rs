//! Database operations for issue / issue-comment reactions.

use anyhow::{Context, Result};
use sea_orm::*;

use crate::entities::reactions::{
    self, ActiveModel, Entity as ReactionEntity, Model as Reaction,
};

/// Allowed reaction contents (Gitea-compatible emoji set).
pub const REACTION_CONTENTS: [&str; 8] = [
    "+1", "-1", "laugh", "confused", "heart", "hooray", "rocket", "eyes",
];

/// Find a reaction by id.
pub async fn find_by_id(db: &DatabaseConnection, id: i64) -> Result<Option<Reaction>> {
    ReactionEntity::find_by_id(id)
        .one(db)
        .await
        .context("db: find reaction by id")
}

/// Insert a reaction. Fails with a unique-constraint error when the same
/// (target, user, content) already exists.
pub async fn create<C: ConnectionTrait>(db: &C, model: ActiveModel) -> Result<Reaction> {
    model
        .insert(db)
        .await
        .context("db: create reaction")
        .map_err(map_unique_violation)
}

/// List all reactions for one reaction target (issue body when
/// `comment_id == 0`, otherwise that comment).
pub async fn list_by_target(
    db: &DatabaseConnection,
    issue_id: i64,
    comment_id: i64,
) -> Result<Vec<Reaction>> {
    ReactionEntity::find()
        .filter(reactions::Column::IssueId.eq(issue_id))
        .filter(reactions::Column::CommentId.eq(comment_id))
        .order_by_asc(reactions::Column::Id)
        .all(db)
        .await
        .context("db: list reactions by target")
}

/// List all reactions attached to an issue (body + comments).
pub async fn list_by_issue(
    db: &DatabaseConnection,
    issue_id: i64,
) -> Result<Vec<Reaction>> {
    ReactionEntity::find()
        .filter(reactions::Column::IssueId.eq(issue_id))
        .order_by_asc(reactions::Column::Id)
        .all(db)
        .await
        .context("db: list reactions by issue")
}

/// Delete a reaction row by id.
pub async fn delete_by_id(db: &DatabaseConnection, id: i64) -> Result<()> {
    ReactionEntity::delete_by_id(id)
        .exec(db)
        .await
        .context("db: delete reaction")?;
    Ok(())
}

/// Delete all reactions attached to a comment (called when the comment is
/// deleted — there is no FK from `comment_id` because 0 means "issue body").
pub async fn delete_by_comment<C: ConnectionTrait>(db: &C, comment_id: i64) -> Result<u64> {
    let res = ReactionEntity::delete_many()
        .filter(reactions::Column::CommentId.eq(comment_id))
        .exec(db)
        .await
        .context("db: delete reactions by comment")?;
    Ok(res.rows_affected)
}

/// Translate a unique-index violation into a stable, user-facing error so the
/// API layer can return 409 instead of 500. Walks the whole error chain
/// because the DB error is wrapped by a `context(...)` message.
fn map_unique_violation(e: anyhow::Error) -> anyhow::Error {
    let violated = e
        .chain()
        .any(|cause| {
            let msg = cause.to_string();
            msg.contains("idx_reactions_unique_target_user_content")
                || msg.contains("UNIQUE constraint failed")
                || msg.contains("Duplicate entry")
                || msg.contains("duplicate key value violates unique constraint")
        });
    if violated {
        anyhow::anyhow!("reaction already exists")
    } else {
        e
    }
}
