//! REST API handlers for notifications.

use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::error::AppError;
use crate::pagination::{PaginatedResponse, PaginationParams};
use crate::AppState;

// ── Response types ───────────────────────────────────────────

#[derive(Serialize)]
struct NotificationResponse {
    id: i64,
    user_id: i64,
    event_type: String,
    title: String,
    body: Option<String>,
    repo_id: Option<i64>,
    /// `owner/name` of the repository, only when it still exists and the
    /// caller can read it. Lets a client link to the repository.
    repo: Option<String>,
    is_read: bool,
    created_at: String,
}

/// Event types written by the collaboration feature. Their text comes from
/// agent clients, so it is hidden once the reader may no longer see the
/// repository it was about.
fn is_collab_event(event_type: &str) -> bool {
    event_type.starts_with("collab_")
}

/// `owner/name` for each repository id the caller can read right now.
/// Repositories that are gone or not readable are simply absent.
async fn readable_repo_names(
    state: &AppState,
    user_id: i64,
    ids: &std::collections::HashSet<i64>,
) -> std::collections::HashMap<i64, String> {
    let mut out = std::collections::HashMap::new();
    for &id in ids {
        let Ok(Some(repo)) = rg_db::ops::repo_ops::find_by_id(&state.db, id).await else {
            continue;
        };
        rg_core::repo::service::invalidate_perm_cache_user(repo.id, user_id);
        if !matches!(
            rg_core::repo::service::can_read_repo(&state.db, &repo, Some(user_id)).await,
            Ok(true)
        ) {
            continue;
        }
        let owner = match repo.org_id {
            Some(org_id) => rg_db::ops::org_ops::get_org(&state.db, org_id)
                .await
                .ok()
                .flatten()
                .map(|o| o.name),
            None => rg_db::ops::user_ops::find_by_id(&state.db, repo.owner_id)
                .await
                .ok()
                .flatten()
                .map(|u| u.username),
        };
        if let Some(owner) = owner {
            out.insert(id, format!("{owner}/{}", repo.name));
        }
    }
    out
}

#[derive(Deserialize)]
pub struct ListNotificationsQuery {
    unread_only: Option<bool>,
    #[serde(flatten)]
    pagination: PaginationParams,
}

// ── Handlers ─────────────────────────────────────────────────

/// GET /api/v1/notifications
/// List notifications for the authenticated user.
#[utoipa::path(
    get,
    path = "/notifications",
    tag = "Notifications",
    responses(
        (status = 200, description = "Success", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
    ),
)]
pub async fn list_notifications(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<ListNotificationsQuery>,
) -> impl IntoResponse {
    let user_id = match super::auth::extract_user_id(&headers, &state.jwt_secret) {
        Some(id) => id,
        None => {
            return AppError::unauthorized("authentication required").into_response();
        }
    };
    let unread_only = params.unread_only.unwrap_or(false);
    let pagination = params.pagination.clamp();
    let offset = pagination.offset();
    let limit = pagination.limit();

    match rg_core::notification::list_notifications_paginated(
        &state.db,
        user_id,
        unread_only,
        offset,
        limit,
    )
    .await
    {
        Ok((notifications, total)) => {
            let ids: std::collections::HashSet<i64> =
                notifications.iter().filter_map(|n| n.repo_id).collect();
            let names = readable_repo_names(&state, user_id, &ids).await;
            let resp: Vec<NotificationResponse> = notifications
                .into_iter()
                .map(|n| {
                    let repo = n.repo_id.and_then(|id| names.get(&id).cloned());
                    let hidden = is_collab_event(&n.event_type) && repo.is_none();
                    NotificationResponse {
                        id: n.id,
                        user_id: n.user_id,
                        event_type: n.event_type,
                        title: if hidden {
                            "(no longer available)".to_string()
                        } else {
                            n.title
                        },
                        body: if hidden { None } else { n.body },
                        repo_id: n.repo_id,
                        repo,
                        is_read: n.is_read,
                        created_at: n.created_at.to_string(),
                    }
                })
                .collect();
            Json(PaginatedResponse::new(resp, &pagination, total as u64)).into_response()
        }
        Err(e) => AppError::internal(e.to_string()).into_response(),
    }
}

/// GET /api/v1/notifications/unread-count
#[utoipa::path(
    get,
    path = "/notifications/unread-count",
    tag = "Notifications",
    responses(
        (status = 200, description = "Success", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
    ),
)]
pub async fn unread_count(State(state): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    let user_id = match super::auth::extract_user_id(&headers, &state.jwt_secret) {
        Some(id) => id,
        None => {
            return AppError::unauthorized("authentication required").into_response();
        }
    };

    match rg_core::notification::unread_count(&state.db, user_id).await {
        Ok(count) => Json(serde_json::json!({"unread_count": count})).into_response(),
        Err(e) => AppError::internal(e.to_string()).into_response(),
    }
}

/// POST /api/v1/notifications/:id/read
#[utoipa::path(
    post,
    path = "/notifications/{id}/read",
    tag = "Notifications",
    params(
        ("id" = i64, Path, description = "id"),
    ),
    request_body(content = serde_json::Value),
    responses(
        (status = 201, description = "Created", body = serde_json::Value),
        (status = 400, description = "Bad request", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
    ),
)]
pub async fn mark_read(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    let user_id = match super::auth::extract_user_id(&headers, &state.jwt_secret) {
        Some(id) => id,
        None => {
            return AppError::unauthorized("authentication required").into_response();
        }
    };

    match rg_core::notification::mark_read_for_user(&state.db, id, user_id).await {
        Ok(()) => Json(serde_json::json!({"id": id, "is_read": true})).into_response(),
        Err(e) => AppError::not_found(e.to_string()).into_response(),
    }
}

/// POST /api/v1/notifications/mark-all-read
#[utoipa::path(
    post,
    path = "/notifications/mark-all-read",
    tag = "Notifications",
    responses(
        (status = 201, description = "Created", body = serde_json::Value),
        (status = 400, description = "Bad request", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
    ),
)]
pub async fn mark_all_read(State(state): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    let user_id = match super::auth::extract_user_id(&headers, &state.jwt_secret) {
        Some(id) => id,
        None => {
            return AppError::unauthorized("authentication required").into_response();
        }
    };

    match rg_core::notification::mark_all_read(&state.db, user_id).await {
        Ok(count) => Json(serde_json::json!({"marked_read": count})).into_response(),
        Err(e) => AppError::internal(e.to_string()).into_response(),
    }
}

/// DELETE /api/v1/notifications/:id
#[utoipa::path(
    delete,
    path = "/notifications/{id}",
    tag = "Notifications",
    params(
        ("id" = i64, Path, description = "id"),
    ),
    responses(
        (status = 200, description = "Deleted", body = serde_json::Value),
        (status = 204, description = "No content"),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
    ),
)]
pub async fn delete_notification(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    let user_id = match super::auth::extract_user_id(&headers, &state.jwt_secret) {
        Some(id) => id,
        None => {
            return AppError::unauthorized("authentication required").into_response();
        }
    };

    match rg_core::notification::delete_notification_for_user(&state.db, id, user_id).await {
        Ok(()) => Json(serde_json::json!({"deleted": true})).into_response(),
        Err(e) => AppError::not_found(e.to_string()).into_response(),
    }
}
