//! Database operations for notifications.

use anyhow::{Context, Result};
use sea_orm::*;

use crate::entities::notification;

/// Create a notification.
pub async fn create_notification(
    db: &DatabaseConnection,
    user_id: i64,
    event_type: &str,
    title: &str,
    body: Option<&str>,
    repo_id: Option<i64>,
) -> Result<notification::Model> {
    let model = notification::ActiveModel {
        user_id: Set(user_id),
        event_type: Set(event_type.to_string()),
        title: Set(title.to_string()),
        body: Set(body.map(|s| s.to_string())),
        repo_id: Set(repo_id),
        is_read: Set(false),
        created_at: Set(chrono::Utc::now()),
        ..Default::default()
    };
    model.insert(db).await.context("db: create notification")
}

/// List notifications for a user.
pub async fn list_notifications(
    db: &DatabaseConnection,
    user_id: i64,
    unread_only: bool,
) -> Result<Vec<notification::Model>> {
    let mut query = notification::Entity::find()
        .filter(notification::Column::UserId.eq(user_id));

    if unread_only {
        query = query.filter(notification::Column::IsRead.eq(false));
    }

    query
        .order_by_desc(notification::Column::CreatedAt)
        .all(db)
        .await
        .context("db: list notifications")
}

/// Mark a notification as read.
pub async fn mark_notification_read(db: &DatabaseConnection, id: i64) -> Result<()> {
    let model = notification::Entity::find_by_id(id)
        .one(db)
        .await
        .context("db: find notification")?
        .ok_or_else(|| anyhow::anyhow!("notification {} not found", id))?;

    let mut active: notification::ActiveModel = model.into();
    active.is_read = Set(true);
    active.update(db).await.context("db: mark notification read")?;
    Ok(())
}

/// Mark all notifications as read for a user.
pub async fn mark_all_read(db: &DatabaseConnection, user_id: i64) -> Result<u64> {
    // Find all unread notifications for this user, then update each one
    let unread = notification::Entity::find()
        .filter(notification::Column::UserId.eq(user_id))
        .filter(notification::Column::IsRead.eq(false))
        .all(db)
        .await
        .context("db: find unread notifications")?;

    let mut count: u64 = 0;
    for n in unread {
        let mut active: notification::ActiveModel = n.into();
        active.is_read = Set(true);
        active.update(db).await.context("db: mark notification read")?;
        count += 1;
    }

    Ok(count)
}

/// Get unread notification count for a user.
pub async fn unread_count(db: &DatabaseConnection, user_id: i64) -> Result<u64> {
    let count = notification::Entity::find()
        .filter(notification::Column::UserId.eq(user_id))
        .filter(notification::Column::IsRead.eq(false))
        .count(db)
        .await
        .context("db: unread notification count")?;

    Ok(count)
}

/// Delete a notification.
pub async fn delete_notification(db: &DatabaseConnection, id: i64) -> Result<()> {
    let model = notification::Entity::find_by_id(id)
        .one(db)
        .await
        .context("db: find notification for delete")?
        .ok_or_else(|| anyhow::anyhow!("notification {} not found", id))?;

    model.delete(db).await.context("db: delete notification")?;
    Ok(())
}
