//! REST API handlers for branch protection rules.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;

use crate::AppState;

// ── Request / Response types ──────────────────────────────────────────

#[derive(Deserialize)]
pub struct CreateProtectionRequest {
    pub branch_name: String,
    #[serde(default)]
    pub require_pr: bool,
    #[serde(default)]
    pub require_status_check: bool,
    #[serde(default)]
    pub required_status_checks: Option<Vec<String>>,
    #[serde(default)]
    pub require_approval: bool,
    #[serde(default)]
    pub required_approvals: Option<i64>,
    #[serde(default)]
    pub allow_force_push: bool,
    #[serde(default)]
    pub allowed_push_user_ids: Option<Vec<i64>>,
}

#[derive(Deserialize)]
pub struct UpdateProtectionRequest {
    #[serde(default)]
    pub require_pr: Option<bool>,
    #[serde(default)]
    pub require_status_check: Option<bool>,
    #[serde(default)]
    pub required_status_checks: Option<Vec<String>>,
    #[serde(default)]
    pub require_approval: Option<bool>,
    #[serde(default)]
    pub required_approvals: Option<i64>,
    #[serde(default)]
    pub allow_force_push: Option<bool>,
    #[serde(default)]
    pub allowed_push_user_ids: Option<Vec<i64>>,
}

// ── Handlers ──────────────────────────────────────────────────────────

/// List branch protection rules for a repo.
/// GET /api/v1/repos/:owner/:name/branches/protection
pub async fn list_protections(
    State(state): State<AppState>,
    Path((owner, repo)): Path<(String, String)>,
) -> impl IntoResponse {
    match rg_core::branch_protection::service::list_protections(&state.db, &owner, &repo).await {
        Ok(protections) => (StatusCode::OK, Json(protections)).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("{:#}", e)})),
        )
            .into_response(),
    }
}

/// Create a branch protection rule.
/// POST /api/v1/repos/:owner/:name/branches/protection
pub async fn create_protection(
    State(state): State<AppState>,
    Path((owner, repo)): Path<(String, String)>,
    headers: axum::http::HeaderMap,
    Json(req): Json<CreateProtectionRequest>,
) -> impl IntoResponse {
    if extract_user_id(&state, &headers).is_none() {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "authentication required"})),
        )
            .into_response();
    }

    match rg_core::branch_protection::service::create_protection(
        &state.db,
        &owner,
        &repo,
        req.branch_name,
        req.require_pr,
        req.require_status_check,
        req.required_status_checks,
        req.require_approval,
        req.required_approvals,
        req.allow_force_push,
        req.allowed_push_user_ids,
    )
    .await
    {
        Ok(protection) => (StatusCode::CREATED, Json(protection)).into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": format!("{:#}", e)})),
        )
            .into_response(),
    }
}

/// Get a branch protection rule by ID.
/// GET /api/v1/repos/:owner/:name/branches/protection/:id
pub async fn get_protection(
    State(state): State<AppState>,
    Path((_owner, _repo, id)): Path<(String, String, i64)>,
) -> impl IntoResponse {
    match rg_core::branch_protection::service::get_protection(&state.db, id).await {
        Ok(protection) => (StatusCode::OK, Json(protection)).into_response(),
        Err(e) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": format!("{:#}", e)})),
        )
            .into_response(),
    }
}

/// Update a branch protection rule.
/// PATCH /api/v1/repos/:owner/:name/branches/protection/:id
pub async fn update_protection(
    State(state): State<AppState>,
    Path((_owner, _repo, id)): Path<(String, String, i64)>,
    headers: axum::http::HeaderMap,
    Json(req): Json<UpdateProtectionRequest>,
) -> impl IntoResponse {
    if extract_user_id(&state, &headers).is_none() {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "authentication required"})),
        )
            .into_response();
    }

    match rg_core::branch_protection::service::update_protection(
        &state.db,
        id,
        req.require_pr,
        req.require_status_check,
        req.required_status_checks,
        req.require_approval,
        req.required_approvals,
        req.allow_force_push,
        req.allowed_push_user_ids,
    )
    .await
    {
        Ok(protection) => (StatusCode::OK, Json(protection)).into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": format!("{:#}", e)})),
        )
            .into_response(),
    }
}

/// Delete a branch protection rule.
/// DELETE /api/v1/repos/:owner/:name/branches/protection/:id
pub async fn delete_protection(
    State(state): State<AppState>,
    Path((_owner, _repo, id)): Path<(String, String, i64)>,
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    if extract_user_id(&state, &headers).is_none() {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "authentication required"})),
        )
            .into_response();
    }

    match rg_core::branch_protection::service::delete_protection(&state.db, id).await {
        Ok(()) => (StatusCode::NO_CONTENT, Json(serde_json::json!({}))).into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": format!("{:#}", e)})),
        )
            .into_response(),
    }
}

// ── Helpers ───────────────────────────────────────────────────────────

fn extract_user_id(state: &AppState, headers: &axum::http::HeaderMap) -> Option<i64> {
    let auth = headers.get("authorization")?.to_str().ok()?;
    let token = auth.strip_prefix("Bearer ")?;
    let claims = rg_core::auth::jwt::validate_token(token, &state.jwt_secret)?;
    claims.sub.parse().ok()
}
