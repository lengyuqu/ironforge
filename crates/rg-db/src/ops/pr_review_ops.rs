//! Database operations for PR reviews.

use anyhow::{Context, Result};
use sea_orm::*;

use crate::entities::pr_review::{self, ActiveModel, Entity as ReviewEntity, Model as PrReview};

/// Find a review by ID.
pub async fn find_by_id(
    db: &DatabaseConnection,
    id: i64,
) -> Result<Option<PrReview>> {
    ReviewEntity::find_by_id(id)
        .one(db)
        .await
        .context("db: find review by id")
}

/// List all reviews for a PR, ordered by creation time.
pub async fn list_by_pr(
    db: &DatabaseConnection,
    pr_id: i64,
) -> Result<Vec<PrReview>> {
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

/// Count approvals for a PR.
pub async fn count_approvals(
    db: &DatabaseConnection,
    pr_id: i64,
) -> Result<i64> {
    let count = ReviewEntity::find()
        .filter(pr_review::Column::PrId.eq(pr_id))
        .filter(pr_review::Column::Action.eq("approve"))
        .count(db)
        .await
        .context("db: count PR approvals")?;
    Ok(count as i64)
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
