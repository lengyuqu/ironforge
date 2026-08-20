//! Webhook REST API endpoints.

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use sea_orm::DatabaseConnection;

use crate::api::repo_access::{require_read, require_write};
use crate::error::AppError;
use crate::AppState;

// -- Handlers ---------------------------------------------------------------

/// List webhooks for a repo.
#[utoipa::path(
    get,
    path = "/repos/{owner}/{name}/hooks",
    tag = "Webhooks",
    params(
        ("owner" = String, Path, description = "owner"),
        ("name" = String, Path, description = "name"),
    ),
    responses(
        (status = 200, description = "Success", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
        (status = 403, description = "Forbidden", body = serde_json::Value),
    ),
)]
pub async fn list_webhooks(
    State(state): State<AppState>,
    Path((owner, repo)): Path<(String, String)>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let repo_model = match require_read(&state, &headers, &owner, &repo).await {
        Ok(r) => r,
        Err(e) => return e.into_response(),
    };

    match rg_core::webhook::service::list_webhooks(&state.db, repo_model.id).await {
        Ok(mut hooks) => {
            for hook in &mut hooks {
                hook.secret = None;
            }
            (StatusCode::OK, Json(serde_json::json!(hooks))).into_response()
        }
        Err(e) => AppError::internal(e).into_response(),
    }
}

/// Create a webhook.
#[utoipa::path(
    post,
    path = "/repos/{owner}/{name}/hooks",
    tag = "Webhooks",
    params(
        ("owner" = String, Path, description = "owner"),
        ("name" = String, Path, description = "name"),
    ),
    request_body(content = serde_json::Value),
    responses(
        (status = 201, description = "Created", body = serde_json::Value),
        (status = 400, description = "Bad request", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
        (status = 403, description = "Forbidden", body = serde_json::Value),
    ),
)]
pub async fn create_webhook(
    State(state): State<AppState>,
    Path((owner, repo)): Path<(String, String)>,
    headers: HeaderMap,
    Json(body): Json<rg_core::webhook::service::CreateWebhookRequest>,
) -> impl IntoResponse {
    let (repo_model, _user_id) =
        match require_write(&state, &headers, &owner, &repo).await {
            Ok(r) => r,
            Err(e) => return e.into_response(),
        };

    match rg_core::webhook::service::create_webhook(&state.db, repo_model.id, &body).await {
        Ok(hook) => (StatusCode::CREATED, Json(serde_json::json!(hook))).into_response(),
        Err(e) => AppError::bad_request(e).into_response(),
    }
}

/// Get a webhook by id.
#[utoipa::path(
    get,
    path = "/repos/{owner}/{name}/hooks/{id}",
    tag = "Webhooks",
    params(
        ("owner" = String, Path, description = "owner"),
        ("name" = String, Path, description = "name"),
        ("id" = i64, Path, description = "id"),
    ),
    responses(
        (status = 200, description = "Success", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
        (status = 403, description = "Forbidden", body = serde_json::Value),
    ),
)]
pub async fn get_webhook(
    State(state): State<AppState>,
    Path((owner, repo, id)): Path<(String, String, i64)>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let repo_model = match require_read(&state, &headers, &owner, &repo).await {
        Ok(r) => r,
        Err(e) => return e.into_response(),
    };

    match resolve_webhook_in_repo(&state.db, repo_model.id, id).await {
        Ok(mut hook) => {
            hook.secret = None;
            (StatusCode::OK, Json(serde_json::json!(hook))).into_response()
        }
        Err(e) => e.into_response(),
    }
}

/// Update a webhook.
#[utoipa::path(
    patch,
    path = "/repos/{owner}/{name}/hooks/{id}",
    tag = "Webhooks",
    params(
        ("owner" = String, Path, description = "owner"),
        ("name" = String, Path, description = "name"),
        ("id" = i64, Path, description = "id"),
    ),
    request_body(content = serde_json::Value),
    responses(
        (status = 200, description = "Updated", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
        (status = 403, description = "Forbidden", body = serde_json::Value),
    ),
)]
pub async fn update_webhook(
    State(state): State<AppState>,
    Path((owner, repo, id)): Path<(String, String, i64)>,
    headers: HeaderMap,
    Json(body): Json<rg_core::webhook::service::UpdateWebhookRequest>,
) -> impl IntoResponse {
    let (repo_model, _user_id) =
        match require_write(&state, &headers, &owner, &repo).await {
            Ok(r) => r,
            Err(e) => return e.into_response(),
        };

    let existing = match resolve_webhook_in_repo(&state.db, repo_model.id, id).await {
        Ok(hook) => hook,
        Err(e) => return e.into_response(),
    };

    match rg_core::webhook::service::update_webhook(&state.db, &existing, &body).await {
        Ok(hook) => (StatusCode::OK, Json(serde_json::json!(hook))).into_response(),
        Err(e) => AppError::bad_request(e).into_response(),
    }
}

/// Delete a webhook.
#[utoipa::path(
    delete,
    path = "/repos/{owner}/{name}/hooks/{id}",
    tag = "Webhooks",
    params(
        ("owner" = String, Path, description = "owner"),
        ("name" = String, Path, description = "name"),
        ("id" = i64, Path, description = "id"),
    ),
    responses(
        (status = 204, description = "No content"),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
        (status = 403, description = "Forbidden", body = serde_json::Value),
    ),
)]
pub async fn delete_webhook(
    State(state): State<AppState>,
    Path((owner, repo, id)): Path<(String, String, i64)>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let (repo_model, _user_id) =
        match require_write(&state, &headers, &owner, &repo).await {
            Ok(r) => r,
            Err(e) => return e.into_response(),
        };

    if let Err(e) = resolve_webhook_in_repo(&state.db, repo_model.id, id).await {
        return e.into_response();
    }

    match rg_core::webhook::service::delete_webhook(&state.db, id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => AppError::bad_request(e).into_response(),
    }
}

/// List recent deliveries for a webhook.
#[utoipa::path(
    get,
    path = "/repos/{owner}/{name}/hooks/{id}/deliveries",
    tag = "Webhooks",
    params(
        ("owner" = String, Path, description = "owner"),
        ("name" = String, Path, description = "name"),
        ("id" = i64, Path, description = "id"),
    ),
    responses(
        (status = 200, description = "Success", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
        (status = 403, description = "Forbidden", body = serde_json::Value),
    ),
)]
pub async fn list_deliveries(
    State(state): State<AppState>,
    Path((owner, repo, id)): Path<(String, String, i64)>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let repo_model = match require_read(&state, &headers, &owner, &repo).await {
        Ok(r) => r,
        Err(e) => return e.into_response(),
    };

    if let Err(e) = resolve_webhook_in_repo(&state.db, repo_model.id, id).await {
        return e.into_response();
    }

    match rg_core::webhook::service::list_deliveries(&state.db, id).await {
        Ok(deliveries) => (StatusCode::OK, Json(serde_json::json!(deliveries))).into_response(),
        Err(e) => AppError::internal(e).into_response(),
    }
}

/// Redeliver a webhook.
#[utoipa::path(
    post,
    path = "/repos/{owner}/{name}/hooks/{id}/deliveries/{delivery_id}/redeliver",
    tag = "Webhooks",
    params(
        ("owner" = String, Path, description = "owner"),
        ("name" = String, Path, description = "name"),
        ("id" = i64, Path, description = "id"),
        ("delivery_id" = i64, Path, description = "delivery_id"),
    ),
    responses(
        (status = 201, description = "Created", body = serde_json::Value),
        (status = 400, description = "Bad request", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
        (status = 403, description = "Forbidden", body = serde_json::Value),
    ),
)]
pub async fn redeliver(
    State(state): State<AppState>,
    Path((owner, repo, id, delivery_id)): Path<(String, String, i64, i64)>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let (repo_model, _user_id) =
        match require_write(&state, &headers, &owner, &repo).await {
            Ok(r) => r,
            Err(e) => return e.into_response(),
        };

    let hook = match resolve_webhook_in_repo(&state.db, repo_model.id, id).await {
        Ok(hook) => hook,
        Err(e) => return e.into_response(),
    };

    match rg_core::webhook::service::get_delivery(&state.db, delivery_id).await {
        Ok(Some(delivery)) if delivery.webhook_id == hook.id => {}
        Ok(Some(_)) | Ok(None) => return AppError::not_found("delivery not found").into_response(),
        Err(e) => return AppError::internal(e).into_response(),
    }

    match rg_core::webhook::service::redeliver(&state.db, delivery_id).await {
        Ok(()) => (
            StatusCode::OK,
            Json(serde_json::json!({"message": "redelivery triggered"})),
        )
            .into_response(),
        Err(e) => AppError::bad_request(e).into_response(),
    }
}

// -- Helpers ----------------------------------------------------------------

async fn resolve_webhook_in_repo(
    db: &DatabaseConnection,
    repo_id: i64,
    webhook_id: i64,
) -> Result<rg_db::entities::webhook::Model, AppError> {
    match rg_core::webhook::service::get_webhook(db, webhook_id).await {
        Ok(Some(hook)) if hook.repo_id == repo_id => Ok(hook),
        Ok(Some(_)) | Ok(None) => Err(AppError::not_found("webhook not found")),
        Err(e) => Err(AppError::internal(e)),
    }
}
