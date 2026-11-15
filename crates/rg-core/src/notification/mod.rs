//! Notification service — create and manage user notifications.
//!
//! Business code should not call [`notify`] directly: it writes a row
//! unconditionally. Use [`dispatch::dispatch`], the single exit that honours the
//! recipient's notification preferences and can also send the email channel.

use crate::error::CoreResult;
use sea_orm::DatabaseConnection;

use rg_db::ops::notification_ops;

pub mod dispatch;

/// Create a notification for a user, unconditionally.
///
/// Low level: no preference check, no email. Prefer
/// [`dispatch::dispatch`] in business code; this remains for tests and for
/// callers that already decided the row must exist.
pub async fn notify(
    db: &DatabaseConnection,
    user_id: i64,
    event_type: &str,
    title: &str,
    body: Option<&str>,
    repo_id: Option<i64>,
) -> CoreResult<rg_db::entities::notification::Model> {
    Ok(
        notification_ops::create_notification(db, user_id, event_type, title, body, repo_id)
            .await?,
    )
}

/// List notifications for a user.
pub async fn list_notifications(
    db: &DatabaseConnection,
    user_id: i64,
    unread_only: bool,
) -> CoreResult<Vec<rg_db::entities::notification::Model>> {
    Ok(notification_ops::list_notifications(db, user_id, unread_only).await?)
}

/// Paginated list of notifications. Returns (data, total).
pub async fn list_notifications_paginated(
    db: &DatabaseConnection,
    user_id: i64,
    unread_only: bool,
    offset: u64,
    limit: u64,
) -> CoreResult<(Vec<rg_db::entities::notification::Model>, i64)> {
    Ok(
        notification_ops::list_notifications_paginated(db, user_id, unread_only, offset, limit)
            .await?,
    )
}

/// Mark a notification as read.
pub async fn mark_read(db: &DatabaseConnection, id: i64) -> CoreResult<()> {
    notification_ops::mark_notification_read(db, id)
        .await
        .map_err(Into::into)
}

/// Mark a notification as read for its owning user.
pub async fn mark_read_for_user(db: &DatabaseConnection, id: i64, user_id: i64) -> CoreResult<()> {
    notification_ops::mark_notification_read_for_user(db, id, user_id)
        .await
        .map_err(Into::into)
}

/// Mark all notifications as read for a user.
pub async fn mark_all_read(db: &DatabaseConnection, user_id: i64) -> CoreResult<u64> {
    notification_ops::mark_all_read(db, user_id)
        .await
        .map_err(Into::into)
}

/// Get unread notification count.
pub async fn unread_count(db: &DatabaseConnection, user_id: i64) -> CoreResult<u64> {
    notification_ops::unread_count(db, user_id)
        .await
        .map_err(Into::into)
}

/// Delete a notification.
pub async fn delete_notification(db: &DatabaseConnection, id: i64) -> CoreResult<()> {
    notification_ops::delete_notification(db, id)
        .await
        .map_err(Into::into)
}

/// Delete a notification for its owning user.
pub async fn delete_notification_for_user(
    db: &DatabaseConnection,
    id: i64,
    user_id: i64,
) -> CoreResult<()> {
    notification_ops::delete_notification_for_user(db, id, user_id)
        .await
        .map_err(Into::into)
}

// ── Watch notification helpers ─────────────────────────────────────────

/// Notify all watchers of a repository about an event.
///
/// Goes through [`dispatch::dispatch`] so a watcher who muted this event type
/// (globally or for this repository) does not get the row. No mailer is
/// threaded in here on purpose: watcher fan-out is the highest-volume path in
/// the product and every watcher is subscribed to mail by default, so wiring a
/// mailer here would start mass-mailing. Pass `None` and keep it quiet.
pub async fn notify_watchers(
    db: &DatabaseConnection,
    repo_id: i64,
    author_name: &str,
    title: &str,
    notification_type: &str,
    body: Option<String>,
) -> CoreResult<()> {
    let watchers = rg_db::ops::repo_watch_ops::list_watchers(db, repo_id, 0, 1000)
        .await?
        .0;
    // Resolve author once outside the loop to avoid N+1 queries
    let author_opt = if author_name.is_empty() {
        None
    } else {
        rg_db::ops::user_ops::find_by_username(db, author_name)
            .await
            .ok()
            .flatten()
    };
    // Don't notify the author themselves.
    let recipients: Vec<i64> = watchers
        .into_iter()
        .map(|watcher| watcher.user_id)
        .filter(|user_id| {
            author_opt
                .as_ref()
                .is_none_or(|author| author.id != *user_id)
        })
        .collect();
    if recipients.is_empty() {
        return Ok(());
    }
    dispatch::dispatch(
        db,
        None,
        dispatch::NotifyRequest {
            recipients,
            event_type: notification_type.to_string(),
            repo_id: Some(repo_id),
            title: title.to_string(),
            body,
            action_url: None,
        },
    )
    .await;
    Ok(())
}
