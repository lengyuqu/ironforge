//! Repository REST API.
//!
//! POST /api/v1/repos              — create repo (auth required)
//! GET  /api/v1/repos/:owner       — list repos by owner
//! GET  /api/v1/repos/:owner/:name — get single repo

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use serde::Deserialize;
use std::path::PathBuf;

use crate::{api::users::extract_bearer_claims, AppState};

/// POST /api/v1/repos
#[derive(Deserialize)]
pub struct CreateRepoRequest {
    pub name: String,
    pub description: Option<String>,
    pub is_private: Option<bool>,
}

pub async fn create_repo(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<CreateRepoRequest>,
) -> impl IntoResponse {
    let claims = match extract_bearer_claims(&headers, &state.jwt_secret) {
        Some(c) => c,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({ "error": "authentication required" })),
            )
                .into_response()
        }
    };

    let owner_id: i64 = claims.sub.parse().unwrap_or(-1);

    match rg_core::repo::service::create_repo(
        &state.db,
        owner_id,
        &body.name,
        body.description.as_deref(),
        body.is_private.unwrap_or(false),
        &state.repo_root,
    )
    .await
    {
        Ok(repo) => (StatusCode::CREATED, Json(serde_json::json!(repo))).into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// GET /api/v1/repos/:owner
pub async fn list_repos(
    State(state): State<AppState>,
    Path(owner): Path<String>,
) -> impl IntoResponse {
    let owner_user = match rg_db::ops::user_ops::find_by_username(&state.db, &owner).await {
        Ok(Some(u)) => u,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": "user not found" })),
            )
                .into_response()
        }
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
                .into_response()
        }
    };

    match rg_db::ops::repo_ops::list_by_owner(&state.db, owner_user.id).await {
        Ok(repos) => (StatusCode::OK, Json(serde_json::json!(repos))).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// GET /api/v1/repos/:owner/:name
pub async fn get_repo(
    State(state): State<AppState>,
    Path((owner, name)): Path<(String, String)>,
) -> impl IntoResponse {
    let owner_user = match rg_db::ops::user_ops::find_by_username(&state.db, &owner).await {
        Ok(Some(u)) => u,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": "user not found" })),
            )
                .into_response()
        }
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
                .into_response()
        }
    };

    match rg_db::ops::repo_ops::find_by_owner_and_name(&state.db, owner_user.id, &name).await {
        Ok(Some(repo)) => (StatusCode::OK, Json(serde_json::json!(repo))).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "repository not found" })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}
