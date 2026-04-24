//! Notification service — create and manage user notifications.

use anyhow::Result;
use sea_orm::DatabaseConnection;

use rg_db::ops::notification_ops;

/// Create a notification for a user.
pub async fn notify(
    db: &DatabaseConnection,
    user_id: i64,
    event_type: &str,
    title: &str,
    body: Option<&str>,
    repo_id: Option<i64>,
) -> Result<rg_db::entities::notification::Model> {
    notification_ops::create_notification(db, user_id, event_type, title, body, repo_id).await
}

/// List notifications for a user.
pub async fn list_notifications(
    db: &DatabaseConnection,
    user_id: i64,
    unread_only: bool,
) -> Result<Vec<rg_db::entities::notification::Model>> {
    notification_ops::list_notifications(db, user_id, unread_only).await
}

/// Mark a notification as read.
pub async fn mark_read(db: &DatabaseConnection, id: i64) -> Result<()> {
    notification_ops::mark_notification_read(db, id).await
}

/// Mark all notifications as read for a user.
pub async fn mark_all_read(db: &DatabaseConnection, user_id: i64) -> Result<u64> {
    notification_ops::mark_all_read(db, user_id).await
}

/// Get unread notification count.
pub async fn unread_count(db: &DatabaseConnection, user_id: i64) -> Result<u64> {
    notification_ops::unread_count(db, user_id).await
}

/// Delete a notification.
pub async fn delete_notification(db: &DatabaseConnection, id: i64) -> Result<()> {
    notification_ops::delete_notification(db, id).await
}
