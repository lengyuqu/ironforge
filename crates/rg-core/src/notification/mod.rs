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

/// Paginated list of notifications. Returns (data, total).
pub async fn list_notifications_paginated(
    db: &DatabaseConnection,
    user_id: i64,
    unread_only: bool,
    offset: u64,
    limit: u64,
) -> Result<(Vec<rg_db::entities::notification::Model>, i64)> {
    notification_ops::list_notifications_paginated(db, user_id, unread_only, offset, limit).await
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

// ── Watch notification helpers ─────────────────────────────────────────

/// Notify all watchers of a repository about a push event.
pub async fn notify_watchers_push(
    db: &DatabaseConnection,
    repo_id: i64,
    repo_name: &str,
    pusher_name: &str,
    ref_name: &str,
) -> Result<()> {
    let watchers = rg_db::ops::repo_watch_ops::list_watchers(db, repo_id, 0, 1000).await?.0;
    // Resolve pusher once outside the loop to avoid N+1 queries
    let pusher_opt = rg_db::ops::user_ops::find_by_username(db, pusher_name).await.ok().flatten();
    for watcher in watchers {
        // Don't notify the pusher themselves
        if let Some(ref pusher) = pusher_opt {
            if pusher.id == watcher.user_id {
                continue;
            }
        }
        let title = format!("New push to {}", repo_name);
        let body = Some(format!("{} pushed to {}", pusher_name, ref_name));
        let _ = notification_ops::create_notification(
            db, watcher.user_id, "push", &title, body.as_deref(), Some(repo_id),
        ).await;
    }
    Ok(())
}

/// Notify all watchers of a repository about a pull request event.
pub async fn notify_watchers_pr(
    db: &DatabaseConnection,
    repo_id: i64,
    repo_name: &str,
    author_name: &str,
    pr_number: i64,
    pr_title: &str,
    action: &str,
) -> Result<()> {
    let watchers = rg_db::ops::repo_watch_ops::list_watchers(db, repo_id, 0, 1000).await?.0;
    for watcher in watchers {
        let title = format!("PR #{} {} in {}", pr_number, action, repo_name);
        let body = Some(format!("{} {}: {}", author_name, action, pr_title));
        let _ = notification_ops::create_notification(
            db, watcher.user_id, "pull_request", &title, body.as_deref(), Some(repo_id),
        ).await;
    }
    Ok(())
}

/// Notify all watchers of a repository about a milestone event.
pub async fn notify_watchers_milestone(
    db: &DatabaseConnection,
    repo_id: i64,
    repo_name: &str,
    milestone_title: &str,
    action: &str,
) -> Result<()> {
    let watchers = rg_db::ops::repo_watch_ops::list_watchers(db, repo_id, 0, 1000).await?.0;
    for watcher in watchers {
        let title = format!("Milestone {} in {}", action, repo_name);
        let body = Some(format!("Milestone '{}' {}", milestone_title, action));
        let _ = notification_ops::create_notification(
            db, watcher.user_id, "milestone", &title, body.as_deref(), Some(repo_id),
        ).await;
    }
    Ok(())
}
