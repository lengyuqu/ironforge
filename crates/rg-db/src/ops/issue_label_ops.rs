//! Database operations for issue labels junction table.

use anyhow::{Context, Result};
use sea_orm::*;

use crate::entities::issue_label::{self, ActiveModel, Entity as IssueLabelEntity, Model as IssueLabel};

/// Set labels for an issue (replace all existing labels).
pub async fn set_labels(
    db: &DatabaseConnection,
    issue_id: i64,
    label_ids: Vec<i64>,
) -> Result<()> {
    let txn = db.begin().await.context("db: begin transaction")?;

    // Delete all existing labels for this issue
    IssueLabelEntity::delete_many()
        .filter(issue_label::Column::IssueId.eq(issue_id))
        .exec(&txn)
        .await
        .context("db: delete existing issue labels")?;

    // Insert new labels
    for label_id in label_ids {
        let model = ActiveModel {
            issue_id: Set(issue_id),
            label_id: Set(label_id),
            created_at: Set(chrono::Utc::now()),
            ..Default::default()
        };
        model.insert(&txn).await.context("db: insert issue label")?;
    }

    txn.commit().await.context("db: commit transaction")?;
    Ok(())
}

/// Get all label IDs for an issue.
pub async fn get_label_ids(db: &DatabaseConnection, issue_id: i64) -> Result<Vec<i64>> {
    let labels = IssueLabelEntity::find()
        .filter(issue_label::Column::IssueId.eq(issue_id))
        .all(db)
        .await
        .context("db: get issue label ids")?;
    Ok(labels.into_iter().map(|l| l.label_id).collect())
}

/// Get all issue labels for an issue.
pub async fn get_labels(db: &DatabaseConnection, issue_id: i64) -> Result<Vec<IssueLabel>> {
    IssueLabelEntity::find()
        .filter(issue_label::Column::IssueId.eq(issue_id))
        .all(db)
        .await
        .context("db: get issue labels")
}

/// Delete all issue labels for a label ID (used when deleting a label).
pub async fn delete_by_label_id(db: &DatabaseConnection, label_id: i64) -> Result<()> {
    IssueLabelEntity::delete_many()
        .filter(issue_label::Column::LabelId.eq(label_id))
        .exec(db)
        .await
        .context("db: delete issue labels by label id")?;
    Ok(())
}
