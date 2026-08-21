//! Database operations for issue assignees (multi-assignee, ISSUE-105).

use anyhow::{Context, Result};
use chrono::Utc;
use sea_orm::*;

use crate::entities::issue_assignee::{
    self, ActiveModel, Entity as IssueAssigneeEntity, Model as IssueAssignee,
};

/// Replace the full assignee set of one issue (delete-all + insert).
/// Deduplicates while preserving the caller's order (first entry is the
/// "primary assignee" mirrored into `issues.assignee_id` by the service).
pub async fn set_assignees<C: ConnectionTrait>(
    db: &C,
    issue_id: i64,
    user_ids: &[i64],
) -> Result<()> {
    let mut unique: Vec<i64> = Vec::with_capacity(user_ids.len());
    for id in user_ids {
        if !unique.contains(id) {
            unique.push(*id);
        }
    }

    let txn_manager = db;
    IssueAssigneeEntity::delete_many()
        .filter(issue_assignee::Column::IssueId.eq(issue_id))
        .exec(txn_manager)
        .await
        .context("db: clear issue assignees")?;

    if unique.is_empty() {
        return Ok(());
    }

    let now = Utc::now();
    let models: Vec<ActiveModel> = unique
        .into_iter()
        .map(|user_id| ActiveModel {
            id: sea_orm::NotSet,
            issue_id: Set(issue_id),
            user_id: Set(user_id),
            created_at: Set(now),
        })
        .collect();
    IssueAssigneeEntity::insert_many(models)
        .exec(txn_manager)
        .await
        .context("db: insert issue assignees")?;
    Ok(())
}

/// List all assignee rows of one issue (ordered by insertion id).
pub async fn list_by_issue(
    db: &DatabaseConnection,
    issue_id: i64,
) -> Result<Vec<IssueAssignee>> {
    IssueAssigneeEntity::find()
        .filter(issue_assignee::Column::IssueId.eq(issue_id))
        .order_by_asc(issue_assignee::Column::Id)
        .all(db)
        .await
        .context("db: list issue assignees")
}

/// List assignee user IDs of one issue.
pub async fn list_user_ids_by_issue(
    db: &DatabaseConnection,
    issue_id: i64,
) -> Result<Vec<i64>> {
    Ok(list_by_issue(db, issue_id)
        .await?
        .into_iter()
        .map(|row| row.user_id)
        .collect())
}

/// Find issue IDs assigned to a user (for the `?assignee=` list filter).
pub async fn find_issue_ids_by_user(
    db: &DatabaseConnection,
    user_id: i64,
) -> Result<Vec<i64>> {
    let rows: Vec<i64> = IssueAssigneeEntity::find()
        .filter(issue_assignee::Column::UserId.eq(user_id))
        .order_by_asc(issue_assignee::Column::IssueId)
        .all(db)
        .await
        .context("db: find issues by assignee")?
        .into_iter()
        .map(|row| row.issue_id)
        .collect();
    Ok(rows)
}
