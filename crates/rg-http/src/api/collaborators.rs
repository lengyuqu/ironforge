//! REST API handlers for repository collaborators.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;

use crate::AppState;
use crate::error::AppError;

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
#[utoipa::path(
    get,
    path = "/repos/{owner}/{name}/collaborators",
    tag = "Collaborators",
    params(
        ("owner" = String, Path, description = "owner"),
        ("name" = String, Path, description = "name"),
    ),
    responses(
        (status = 200, description = "Success", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
    ),
)]
pub async fn list_collaborators(
    State(state): State<AppState>,
    Path((owner, repo)): Path<(String, String)>,
) -> impl IntoResponse {
    match rg_core::collaborator::service::list_collaborators(&state.db, &owner, &repo).await {
        Ok(collaborators) => (StatusCode::OK, Json(collaborators)).into_response(),
        Err(e) => AppError::internal(e.to_string()).into_response(),
    }
}

/// Add a collaborator to a repo.
/// POST /api/v1/repos/:owner/:name/collaborators
#[utoipa::path(
    post,
    path = "/repos/{owner}/{name}/collaborators",
    tag = "Collaborators",
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
pub async fn add_collaborator(
    State(state): State<AppState>,
    Path((owner, repo)): Path<(String, String)>,
    headers: axum::http::HeaderMap,
    Json(req): Json<AddCollaboratorRequest>,
) -> impl IntoResponse {
    if super::auth::extract_user_id(&headers, &state.jwt_secret).is_none() {
        return AppError::unauthorized("authentication required").into_response();
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
        Err(e) => AppError::bad_request(e.to_string()).into_response(),
    }
}

/// Update a collaborator's permission.
/// PATCH /api/v1/repos/:owner/:name/collaborators/:id
#[utoipa::path(
    patch,
    path = "/repos/{owner}/{name}/collaborators/{id}",
    tag = "Collaborators",
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
pub async fn update_permission(
    State(state): State<AppState>,
    Path((_owner, _repo, id)): Path<(String, String, i64)>,
    headers: axum::http::HeaderMap,
    Json(req): Json<UpdatePermissionRequest>,
) -> impl IntoResponse {
    if super::auth::extract_user_id(&headers, &state.jwt_secret).is_none() {
        return AppError::unauthorized("authentication required").into_response();
    }

    match rg_core::collaborator::service::update_permission(&state.db, id, req.permission).await {
        Ok(collab) => (StatusCode::OK, Json(collab)).into_response(),
        Err(e) => AppError::bad_request(e.to_string()).into_response(),
    }
}

/// Remove a collaborator from a repo.
/// DELETE /api/v1/repos/:owner/:name/collaborators/:user_id
#[utoipa::path(
    post,
    path = "/repos/{owner}/{name}/collaborators/{user_id}/remove",
    tag = "Collaborators",
    params(
        ("owner" = String, Path, description = "owner"),
        ("name" = String, Path, description = "name"),
        ("user_id" = i64, Path, description = "user_id"),
    ),
    responses(
        (status = 201, description = "Created", body = serde_json::Value),
        (status = 400, description = "Bad request", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
    ),
)]
pub async fn remove_collaborator(
    State(state): State<AppState>,
    Path((owner, repo, user_id)): Path<(String, String, i64)>,
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    if super::auth::extract_user_id(&headers, &state.jwt_secret).is_none() {
        return AppError::unauthorized("authentication required").into_response();
    }

    match rg_core::collaborator::service::remove_collaborator(&state.db, &owner, &repo, user_id).await {
        Ok(()) => (StatusCode::NO_CONTENT, Json(serde_json::json!({}))).into_response(),
        Err(e) => AppError::bad_request(e.to_string()).into_response(),
    }
}

