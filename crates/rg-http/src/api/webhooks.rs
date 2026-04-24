//! Webhook REST API endpoints.

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use sea_orm::DatabaseConnection;

use crate::AppState;

// ── Handlers ──────────────────────────────────────────────────────────────

/// List webhooks for a repo.
pub async fn list_webhooks(
    State(state): State<AppState>,
    Path((owner, repo)): Path<(String, String)>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let _user_id = match extract_user_id(&state, &headers) {
        Some(id) => id,
        None => return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({"error": "unauthorized"}))),
    };

    let repo_id = match resolve_repo_id(&state.db, &owner, &repo).await {
        Some(id) => id,
        None => return (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "repository not found"}))),
    };

    match rg_core::webhook::service::list_webhooks(&state.db, repo_id).await {
        Ok(hooks) => (StatusCode::OK, Json(serde_json::json!(hooks))),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": format!("{:#}", e)}))),
    }
}

/// Create a webhook.
pub async fn create_webhook(
    State(state): State<AppState>,
    Path((owner, repo)): Path<(String, String)>,
    headers: HeaderMap,
    Json(body): Json<rg_core::webhook::service::CreateWebhookRequest>,
) -> impl IntoResponse {
    let _user_id = match extract_user_id(&state, &headers) {
        Some(id) => id,
        None => return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({"error": "unauthorized"}))),
    };

    let repo_id = match resolve_repo_id(&state.db, &owner, &repo).await {
        Some(id) => id,
        None => return (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "repository not found"}))),
    };

    match rg_core::webhook::service::create_webhook(&state.db, repo_id, &body).await {
        Ok(hook) => (StatusCode::CREATED, Json(serde_json::json!(hook))),
        Err(e) => (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": format!("{:#}", e)}))),
    }
}

/// Get a webhook by id.
pub async fn get_webhook(
    State(state): State<AppState>,
    Path((owner, repo, id)): Path<(String, String, i64)>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let _user_id = match extract_user_id(&state, &headers) {
        Some(id) => id,
        None => return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({"error": "unauthorized"}))),
    };

    match rg_core::webhook::service::get_webhook(&state.db, id).await {
        Ok(Some(hook)) => (StatusCode::OK, Json(serde_json::json!(hook))),
        Ok(None) => (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "webhook not found"}))),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": format!("{:#}", e)}))),
    }
}

/// Update a webhook.
pub async fn update_webhook(
    State(state): State<AppState>,
    Path((owner, repo, id)): Path<(String, String, i64)>,
    headers: HeaderMap,
    Json(body): Json<rg_core::webhook::service::UpdateWebhookRequest>,
) -> impl IntoResponse {
    let _user_id = match extract_user_id(&state, &headers) {
        Some(id) => id,
        None => return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({"error": "unauthorized"}))),
    };

    let existing = match rg_core::webhook::service::get_webhook(&state.db, id).await {
        Ok(Some(h)) => h,
        Ok(None) => return (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "webhook not found"}))),
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": format!("{:#}", e)}))),
    };

    match rg_core::webhook::service::update_webhook(&state.db, &existing, &body).await {
        Ok(hook) => (StatusCode::OK, Json(serde_json::json!(hook))),
        Err(e) => (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": format!("{:#}", e)}))),
    }
}

/// Delete a webhook.
pub async fn delete_webhook(
    State(state): State<AppState>,
    Path((owner, repo, id)): Path<(String, String, i64)>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let _user_id = match extract_user_id(&state, &headers) {
        Some(id) => id,
        None => return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({"error": "unauthorized"}))),
    };

    match rg_core::webhook::service::delete_webhook(&state.db, id).await {
        Ok(()) => (StatusCode::OK, Json(serde_json::json!({"message": "webhook deleted"}))),
        Err(e) => (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": format!("{:#}", e)}))),
    }
}

/// List recent deliveries for a webhook.
pub async fn list_deliveries(
    State(state): State<AppState>,
    Path((_owner, _repo, id)): Path<(String, String, i64)>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let _user_id = match extract_user_id(&state, &headers) {
        Some(id) => id,
        None => return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({"error": "unauthorized"}))),
    };

    match rg_core::webhook::service::list_deliveries(&state.db, id).await {
        Ok(deliveries) => (StatusCode::OK, Json(serde_json::json!(deliveries))),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": format!("{:#}", e)}))),
    }
}

/// Redeliver a webhook.
pub async fn redeliver(
    State(state): State<AppState>,
    Path((_owner, _repo, delivery_id)): Path<(String, String, i64)>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let _user_id = match extract_user_id(&state, &headers) {
        Some(id) => id,
        None => return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({"error": "unauthorized"}))),
    };

    match rg_core::webhook::service::redeliver(&state.db, delivery_id).await {
        Ok(()) => (StatusCode::OK, Json(serde_json::json!({"message": "redelivery triggered"}))),
        Err(e) => (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": format!("{:#}", e)}))),
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
