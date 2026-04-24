//! REST API handlers for notifications.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::AppState;
use crate::ws;
use crate::pagination::{PaginationParams, PaginatedResponse};

// ── Response types ───────────────────────────────────────────

#[derive(Serialize)]
struct NotificationResponse {
    id: i64,
    user_id: i64,
    event_type: String,
    title: String,
    body: Option<String>,
    repo_id: Option<i64>,
    is_read: bool,
    created_at: String,
}

#[derive(Deserialize)]
pub struct ListNotificationsQuery {
    user_id: Option<i64>,
    unread_only: Option<bool>,
    #[serde(flatten)]
    pagination: PaginationParams,
}

#[derive(Deserialize)]
pub struct UserQuery {
    user_id: Option<i64>,
}

// ── Handlers ─────────────────────────────────────────────────

/// GET /api/v1/notifications
/// List notifications for a user.
pub async fn list_notifications(
    State(state): State<AppState>,
    Query(params): Query<ListNotificationsQuery>,
) -> impl IntoResponse {
    let user_id = params.user_id.unwrap_or(1); // TODO: extract from JWT
    let unread_only = params.unread_only.unwrap_or(false);
    let pagination = params.pagination.clamp();

    match rg_core::notification::list_notifications(&state.db, user_id, unread_only).await {
        Ok(notifications) => {
            let total = notifications.len() as u64;
            let offset = pagination.offset() as usize;
            let limit = pagination.limit() as usize;
            let resp: Vec<NotificationResponse> = notifications
                .into_iter()
                .skip(offset)
                .take(limit)
                .map(|n| NotificationResponse {
                    id: n.id,
                    user_id: n.user_id,
                    event_type: n.event_type,
                    title: n.title,
                    body: n.body,
                    repo_id: n.repo_id,
                    is_read: n.is_read,
                    created_at: n.created_at.to_string(),
                })
                .collect();
            Json(PaginatedResponse::new(resp, &pagination, total)).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
        .into_response(),
    }
}

/// GET /api/v1/notifications/unread-count
pub async fn unread_count(
    State(state): State<AppState>,
    Query(params): Query<UserQuery>,
) -> impl IntoResponse {
    let user_id = params.user_id.unwrap_or(1); // TODO: extract from JWT

    match rg_core::notification::unread_count(&state.db, user_id).await {
        Ok(count) => Json(serde_json::json!({"unread_count": count})).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
        .into_response(),
    }
}

/// POST /api/v1/notifications/:id/read
pub async fn mark_read(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    match rg_core::notification::mark_read(&state.db, id).await {
        Ok(()) => Json(serde_json::json!({"id": id, "is_read": true})).into_response(),
        Err(e) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

/// POST /api/v1/notifications/mark-all-read
pub async fn mark_all_read(
    State(state): State<AppState>,
    Query(params): Query<UserQuery>,
) -> impl IntoResponse {
    let user_id = params.user_id.unwrap_or(1); // TODO: extract from JWT

    match rg_core::notification::mark_all_read(&state.db, user_id).await {
        Ok(count) => Json(serde_json::json!({"marked_read": count})).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
        .into_response(),
    }
}

/// DELETE /api/v1/notifications/:id
pub async fn delete_notification(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    match rg_core::notification::delete_notification(&state.db, id).await {
        Ok(()) => Json(serde_json::json!({"deleted": true})).into_response(),
        Err(e) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}
