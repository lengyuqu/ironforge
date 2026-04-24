//! REST API handlers for repository collaborators.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;

use crate::AppState;

// ── Request / Response types ──────────────────────────────────────────

#[derive(Deserialize)]
pub struct AddCollaboratorRequest {
    pub user_id: i64,
    /// read / write / admin
    #[serde(default = "default_permission")]
    pub permission: String,
}

fn default_permission() -> String {
    "read".to_string()
}

#[derive(Deserialize)]
pub struct UpdatePermissionRequest {
    pub permission: String,
}

// ── Handlers ──────────────────────────────────────────────────────────

/// List collaborators for a repo.
/// GET /api/v1/repos/:owner/:name/collaborators
pub async fn list_collaborators(
    State(state): State<AppState>,
    Path((owner, repo)): Path<(String, String)>,
) -> impl IntoResponse {
    match rg_core::collaborator::service::list_collaborators(&state.db, &owner, &repo).await {
        Ok(collaborators) => (StatusCode::OK, Json(collaborators)).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("{:#}", e)})),
        )
            .into_response(),
    }
}

/// Add a collaborator to a repo.
/// POST /api/v1/repos/:owner/:name/collaborators
pub async fn add_collaborator(
    State(state): State<AppState>,
    Path((owner, repo)): Path<(String, String)>,
    headers: axum::http::HeaderMap,
    Json(req): Json<AddCollaboratorRequest>,
) -> impl IntoResponse {
    if extract_user_id(&state, &headers).is_none() {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "authentication required"})),
        )
            .into_response();
    }

    match rg_core::collaborator::service::add_collaborator(
        &state.db,
        &owner,
        &repo,
        req.user_id,
        req.permission,
    )
    .await
    {
        Ok(collab) => (StatusCode::CREATED, Json(collab)).into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": format!("{:#}", e)})),
        )
            .into_response(),
    }
}

/// Update a collaborator's permission.
/// PATCH /api/v1/repos/:owner/:name/collaborators/:id
pub async fn update_permission(
    State(state): State<AppState>,
    Path((_owner, _repo, id)): Path<(String, String, i64)>,
    headers: axum::http::HeaderMap,
    Json(req): Json<UpdatePermissionRequest>,
) -> impl IntoResponse {
    if extract_user_id(&state, &headers).is_none() {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "authentication required"})),
        )
            .into_response();
    }

    match rg_core::collaborator::service::update_permission(&state.db, id, req.permission).await {
        Ok(collab) => (StatusCode::OK, Json(collab)).into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": format!("{:#}", e)})),
        )
            .into_response(),
    }
}

/// Remove a collaborator from a repo.
/// DELETE /api/v1/repos/:owner/:name/collaborators/:user_id
pub async fn remove_collaborator(
    State(state): State<AppState>,
    Path((owner, repo, user_id)): Path<(String, String, i64)>,
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    if extract_user_id(&state, &headers).is_none() {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "authentication required"})),
        )
            .into_response();
    }

    match rg_core::collaborator::service::remove_collaborator(&state.db, &owner, &repo, user_id).await {
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
