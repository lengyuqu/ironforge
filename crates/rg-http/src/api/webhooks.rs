//! Webhook REST API endpoints.

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use sea_orm::DatabaseConnection;

use crate::AppState;
use crate::error::AppError;
use utoipa::ToSchema;

// ── Handlers ──────────────────────────────────────────────────────────────

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
    ),
)]
pub async fn list_webhooks(
    State(state): State<AppState>,
    Path((owner, repo)): Path<(String, String)>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let _user_id = match extract_user_id(&state, &headers) {
        Some(id) => id,
        None => return AppError::unauthorized("unauthorized").into_response(),
    };

    let repo_id = match resolve_repo_id(&state.db, &owner, &repo).await {
        Some(id) => id,
        None => return AppError::not_found("repository not found").into_response(),
    };

    match rg_core::webhook::service::list_webhooks(&state.db, repo_id).await {
        Ok(hooks) => (StatusCode::OK, Json(serde_json::json!(hooks))).into_response(),
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
    ),
)]
pub async fn create_webhook(
    State(state): State<AppState>,
    Path((owner, repo)): Path<(String, String)>,
    headers: HeaderMap,
    Json(body): Json<rg_core::webhook::service::CreateWebhookRequest>,
) -> impl IntoResponse {
    let _user_id = match extract_user_id(&state, &headers) {
        Some(id) => id,
        None => return AppError::unauthorized("unauthorized").into_response(),
    };

    let repo_id = match resolve_repo_id(&state.db, &owner, &repo).await {
        Some(id) => id,
        None => return AppError::not_found("repository not found").into_response(),
    };

    match rg_core::webhook::service::create_webhook(&state.db, repo_id, &body).await {
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
    ),
)]
pub async fn get_webhook(
    State(state): State<AppState>,
    Path((_owner, _repo, id)): Path<(String, String, i64)>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let _user_id = match extract_user_id(&state, &headers) {
        Some(id) => id,
        None => return AppError::unauthorized("unauthorized").into_response(),
    };

    match rg_core::webhook::service::get_webhook(&state.db, id).await {
        Ok(Some(hook)) => (StatusCode::OK, Json(serde_json::json!(hook))).into_response(),
        Ok(None) => AppError::not_found("webhook not found").into_response(),
        Err(e) => AppError::internal(e).into_response(),
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
    ),
)]
pub async fn update_webhook(
    State(state): State<AppState>,
    Path((_owner, _repo, id)): Path<(String, String, i64)>,
    headers: HeaderMap,
    Json(body): Json<rg_core::webhook::service::UpdateWebhookRequest>,
) -> impl IntoResponse {
    let _user_id = match extract_user_id(&state, &headers) {
        Some(id) => id,
        None => return AppError::unauthorized("unauthorized").into_response(),
    };

    let existing = match rg_core::webhook::service::get_webhook(&state.db, id).await {
        Ok(Some(h)) => h,
        Ok(None) => return AppError::not_found("webhook not found").into_response(),
        Err(e) => return AppError::internal(e).into_response(),
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
        (status = 200, description = "Deleted", body = serde_json::Value),
        (status = 204, description = "No content"),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
    ),
)]
pub async fn delete_webhook(
    State(state): State<AppState>,
    Path((_owner, _repo, id)): Path<(String, String, i64)>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let _user_id = match extract_user_id(&state, &headers) {
        Some(id) => id,
        None => return AppError::unauthorized("unauthorized").into_response(),
    };

    match rg_core::webhook::service::delete_webhook(&state.db, id).await {
        Ok(()) => (StatusCode::OK, Json(serde_json::json!({"message": "webhook deleted"}))).into_response(),
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
    ),
)]
pub async fn list_deliveries(
    State(state): State<AppState>,
    Path((_owner, _repo, id)): Path<(String, String, i64)>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let _user_id = match extract_user_id(&state, &headers) {
        Some(id) => id,
        None => return AppError::unauthorized("unauthorized").into_response(),
    };

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
    ),
)]
pub async fn redeliver(
    State(state): State<AppState>,
    Path((_owner, _repo, delivery_id)): Path<(String, String, i64)>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let _user_id = match extract_user_id(&state, &headers) {
        Some(id) => id,
        None => return AppError::unauthorized("unauthorized").into_response(),
    };

    match rg_core::webhook::service::redeliver(&state.db, delivery_id).await {
        Ok(()) => (StatusCode::OK, Json(serde_json::json!({"message": "redelivery triggered"}))).into_response(),
        Err(e) => AppError::bad_request(e).into_response(),
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────

fn extract_user_id(state: &AppState, headers: &HeaderMap) -> Option<i64> {
    let auth = headers.get("authorization")?.to_str().ok()?;
    let token = auth.strip_prefix("Bearer ")?;
    let claims = rg_core::auth::jwt::validate_token(token, &state.jwt_secret)?;
    claims.sub.parse().ok()
}

async fn resolve_repo_id(
    db: &DatabaseConnection,
    owner: &str,
    repo_name: &str,
) -> Option<i64> {
    let user = rg_db::ops::user_ops::find_by_username(db, owner)
        .await
        .ok()
        .flatten()?;
    let repo = rg_db::ops::repo_ops::find_by_owner_and_name(db, user.id, repo_name)
        .await
        .ok()
        .flatten()?;
    Some(repo.id)
}
