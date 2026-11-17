//! Database operations for PR reviews.

use anyhow::{Context, Result};
use sea_orm::*;

use crate::entities::pr_review::{self, ActiveModel, Entity as ReviewEntity, Model as PrReview};
use crate::entities::user;

/// `users.kind` of a person. An agent is `"agent"`; see `rg_core::agent`.
const KIND_HUMAN: &str = "human";

/// Find a review by ID.
pub async fn find_by_id(db: &DatabaseConnection, id: i64) -> Result<Option<PrReview>> {
    ReviewEntity::find_by_id(id)
        .one(db)
        .await
        .context("db: find review by id")
}

/// List all reviews for a PR, ordered by creation time.
pub async fn list_by_pr(db: &DatabaseConnection, pr_id: i64) -> Result<Vec<PrReview>> {
    ReviewEntity::find()
        .filter(pr_review::Column::PrId.eq(pr_id))
        .order_by_asc(pr_review::Column::CreatedAt)
        .all(db)
        .await
        .context("db: list reviews by PR")
}

/// List reviews by PR and reviewer.
pub async fn list_by_pr_and_reviewer(
    db: &DatabaseConnection,
    pr_id: i64,
    reviewer_id: i64,
) -> Result<Vec<PrReview>> {
    ReviewEntity::find()
        .filter(pr_review::Column::PrId.eq(pr_id))
        .filter(pr_review::Column::ReviewerId.eq(reviewer_id))
        .order_by_asc(pr_review::Column::CreatedAt)
        .all(db)
        .await
        .context("db: list reviews by PR and reviewer")
}

// `count_approvals(db, pr_id)` lived here until #19 R19-4. Removed: it had no
// caller, and its `None` branch counts only reviews with no `commit_id`, which
// is not the merge gate's yardstick. Call
// `count_current_human_approvals` with the head SHA instead.

/// Count at most one latest approval per reviewer for the current head commit,
/// counting people only.
///
/// A later `request_changes` from the same reviewer supersedes their approval.
/// Reviews left by an agent are ignored: approving is a human act, so an
/// agent's approval never satisfies a review requirement, even when the agent
/// is not the author (its owner's PR included).
pub async fn count_current_human_approvals(
    db: &DatabaseConnection,
    pr_id: i64,
    head_sha: Option<&str>,
) -> Result<i64> {
    Ok(current_human_approver_ids(db, pr_id, head_sha).await?.len() as i64)
}

/// The people whose latest approval stands for the current head commit: the
/// reviewers [`count_current_human_approvals`] counts, by id, in ascending
/// order. The count and the list are one rule, so a caller that wants to show
/// who approved cannot drift from the number that gates a merge.
pub async fn current_human_approver_ids(
    db: &DatabaseConnection,
    pr_id: i64,
    head_sha: Option<&str>,
) -> Result<Vec<i64>> {
    let reviews = list_by_pr(db, pr_id).await?;
    let mut latest = std::collections::HashMap::new();
    for review in reviews {
        if matches!(review.action.as_str(), "approve" | "request_changes") {
            latest.insert(review.reviewer_id, review);
        }
    }
    let current: Vec<PrReview> = latest
        .into_values()
        .filter(|review| {
            review.action == "approve"
                && match head_sha {
                    Some(sha) => review.commit_id.as_deref() == Some(sha),
                    None => review.commit_id.is_none(),
                }
        })
        .collect();
    if current.is_empty() {
        return Ok(Vec::new());
    }

    let reviewer_ids: Vec<i64> = current.iter().map(|review| review.reviewer_id).collect();
    let humans: Vec<i64> = user::Entity::find()
        .filter(user::Column::Id.is_in(reviewer_ids))
        .filter(user::Column::Kind.eq(KIND_HUMAN))
        .all(db)
        .await
        .context("db: list human reviewers")?
        .into_iter()
        .map(|model| model.id)
        .collect();

    let mut ids: Vec<i64> = current
        .iter()
        .map(|review| review.reviewer_id)
        .filter(|id| humans.contains(id))
        .collect();
    ids.sort_unstable();
    Ok(ids)
}

/// Create a new review.
pub async fn create(db: &DatabaseConnection, model: ActiveModel) -> Result<PrReview> {
    model.insert(db).await.context("db: create PR review")
}

/// Delete a review by ID.
pub async fn delete_by_id(db: &DatabaseConnection, id: i64) -> Result<()> {
    ReviewEntity::delete_by_id(id)
        .exec(db)
        .await
        .context("db: delete review")?;
    Ok(())
}
