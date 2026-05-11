//! Release REST API.
//!
//! GET    /api/v1/repos/:owner/:name/releases                      — list releases
//! POST   /api/v1/repos/:owner/:name/releases                      — create release
//! GET    /api/v1/repos/:owner/:name/releases/:id                 — get release
//! PATCH  /api/v1/repos/:owner/:name/releases/:id                 — update release
//! DELETE /api/v1/repos/:owner/:name/releases/:id                 — delete release
//! GET    /api/v1/repos/:owner/:name/releases/:release_id/assets   — list assets
//! POST   /api/v1/repos/:owner/:name/releases/:release_id/assets   — upload asset
//! GET    /api/v1/repos/:owner/:name/releases/assets/:asset_id     — get asset
//! GET    /api/v1/repos/:owner/:name/releases/assets/:asset_id/download — download asset
//! DELETE /api/v1/repos/:owner/:name/releases/assets/:asset_id     — delete asset

use axum::{
    extract::{Path, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use axum::body::Body;
use serde::Deserialize;
use utoipa::ToSchema;

use crate::api::users::extract_bearer_claims;
use crate::pagination::{PaginationParams, PaginatedResponse};
use crate::AppState;

/// Request body for creating a release.
#[derive(Deserialize, ToSchema)]
pub struct CreateReleaseRequest {
    pub tag_name: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_commitish: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_draft: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_prerelease: Option<bool>,
}

/// Request body for updating a release.
#[derive(Deserialize, ToSchema)]
pub struct UpdateReleaseRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_draft: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_prerelease: Option<bool>,
}

/// GET /api/v1/repos/:owner/:name/releases
pub async fn list_releases(
    State(state): State<AppState>,
    Path((owner, name)): Path<(String, String)>,
    Query(params): Query<PaginationParams>,
) -> impl IntoResponse {
    let pagination = params.clamp();
    let offset = pagination.offset();
    let limit = pagination.limit();

    // Find repo
    let repo = match rg_core::repo::service::find_repo_by_owner_name(&state.db, &owner, &name).await {
        Ok(Some(r)) => r,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": "repository not found" })),
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

    match rg_core::release::service::list_releases(&state.db, repo.id, offset, limit).await {
        Ok((releases, total)) => (
            StatusCode::OK,
            Json(PaginatedResponse::new(releases, &pagination, total as u64)),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// POST /api/v1/repos/:owner/:name/releases
pub async fn create_release(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((owner, name)): Path<(String, String)>,
    Json(body): Json<CreateReleaseRequest>,
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

    let user_id: i64 = claims.sub.parse().unwrap_or(-1);

    // Find repo
    let repo = match rg_core::repo::service::find_repo_by_owner_name(&state.db, &owner, &name).await {
        Ok(Some(r)) => r,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": "repository not found" })),
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

    // Check write permission
    match rg_core::repo::service::can_write(&state.db, &owner, &name, Some(user_id)).await {
        Ok(true) => {}
        Ok(false) => {
            return (
                StatusCode::FORBIDDEN,
                Json(serde_json::json!({ "error": "permission denied" })),
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
    }

    let repo_path = state.repo_root.join(format!("{}/{}.git", owner, name));

    match rg_core::release::service::create_release(
        &state.db,
        repo.id,
        user_id,
        &body.tag_name,
        &body.title,
        body.body.as_deref(),
        body.target_commitish.as_deref().unwrap_or("main"),
        body.is_draft.unwrap_or(false),
        body.is_prerelease.unwrap_or(false),
        &repo_path,
    )
    .await
    {
        Ok(release) => (StatusCode::CREATED, Json(serde_json::json!(release))).into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// GET /api/v1/repos/:owner/:name/releases/:id
pub async fn get_release(
    State(state): State<AppState>,
    Path((owner, name, id)): Path<(String, String, i64)>,
) -> impl IntoResponse {
    // Verify repo exists
    match rg_core::repo::service::find_repo_by_owner_name(&state.db, &owner, &name).await {
        Ok(Some(_)) => {}
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": "repository not found" })),
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
    }

    match rg_core::release::service::get_release(&state.db, id).await {
        Ok(release) => (StatusCode::OK, Json(serde_json::json!(release))).into_response(),
        Err(_) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "release not found" })),
        )
            .into_response(),
    }
}

/// PATCH /api/v1/repos/:owner/:name/releases/:id
pub async fn update_release(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((owner, name, id)): Path<(String, String, i64)>,
    Json(body): Json<UpdateReleaseRequest>,
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

    let user_id: i64 = claims.sub.parse().unwrap_or(-1);

    // Check write permission
    match rg_core::repo::service::can_write(&state.db, &owner, &name, Some(user_id)).await {
        Ok(true) => {}
        Ok(false) => {
            return (
                StatusCode::FORBIDDEN,
                Json(serde_json::json!({ "error": "permission denied" })),
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
    }

    match rg_core::release::service::update_release(
        &state.db,
        id,
        body.title.as_deref(),
        body.body.as_deref(),
        body.is_draft,
        body.is_prerelease,
    )
    .await
    {
        Ok(release) => (StatusCode::OK, Json(serde_json::json!(release))).into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// DELETE /api/v1/repos/:owner/:name/releases/:id
pub async fn delete_release(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((owner, name, id)): Path<(String, String, i64)>,
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

    let user_id: i64 = claims.sub.parse().unwrap_or(-1);

    // Check write permission
    match rg_core::repo::service::can_write(&state.db, &owner, &name, Some(user_id)).await {
        Ok(true) => {}
        Ok(false) => {
            return (
                StatusCode::FORBIDDEN,
                Json(serde_json::json!({ "error": "permission denied" })),
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
    }

    match rg_core::release::service::delete_release(&state.db, id).await {
        Ok(()) => (StatusCode::NO_CONTENT, Json(serde_json::json!({ "deleted": true }))).into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

// ─── Release Assets ────────────────────────────────────────────────────────

/// GET /api/v1/repos/:owner/:name/releases/:release_id/assets
pub async fn list_assets(
    State(state): State<AppState>,
    Path((owner, name, release_id)): Path<(String, String, i64)>,
) -> impl IntoResponse {
    // Verify repo exists
    match rg_core::repo::service::find_repo_by_owner_name(&state.db, &owner, &name).await {
        Ok(Some(_)) => {}
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": "repository not found" })),
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
    }

    match rg_core::release::service::list_assets(&state.db, release_id).await {
        Ok(assets) => (StatusCode::OK, Json(serde_json::json!(assets))).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// POST /api/v1/repos/:owner/:name/releases/:release_id/assets
///
/// Upload a release asset. The request body is the raw file content.
/// Required headers:
///   - `Content-Type`: MIME type of the file (used as asset content_type)
pub async fn upload_asset(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((owner, name, release_id)): Path<(String, String, i64)>,
    body: Body,
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

    let user_id: i64 = claims.sub.parse().unwrap_or(-1);

    // Check write permission
    match rg_core::repo::service::can_write(&state.db, &owner, &name, Some(user_id)).await {
        Ok(true) => {}
        Ok(false) => {
            return (
                StatusCode::FORBIDDEN,
                Json(serde_json::json!({ "error": "permission denied" })),
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
    }

    // Extract filename from query param or Content-Disposition header
    let filename = headers
        .get("x-asset-filename")
        .and_then(|v| v.to_str().ok())
        .map(String::from);

    let filename = match filename {
        Some(f) if !f.is_empty() => f,
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": "missing required header: x-asset-filename"
                })),
            )
                .into_response()
        }
    };

    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/octet-stream")
        .to_string();

    // Collect body bytes
    let bytes = match axum::body::to_bytes(body, 512 * 1024 * 1024).await {
        // 512 MB max
        Ok(b) => b,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": format!("failed to read body: {}", e) })),
            )
                .into_response()
        }
    };

    let size = bytes.len() as i64;

    match rg_core::release::service::upload_asset(
        &state.db,
        release_id,
        &state.repo_root,
        &owner,
        &name,
        &filename,
        size,
        &content_type,
        user_id,
        &bytes,
    )
    .await
    {
        Ok(asset) => (StatusCode::CREATED, Json(serde_json::json!(asset))).into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// GET /api/v1/repos/:owner/:name/releases/assets/:asset_id
pub async fn get_asset(
    State(state): State<AppState>,
    Path((owner, name, asset_id)): Path<(String, String, i64)>,
) -> impl IntoResponse {
    // Verify repo exists
    match rg_core::repo::service::find_repo_by_owner_name(&state.db, &owner, &name).await {
        Ok(Some(_)) => {}
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": "repository not found" })),
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
    }

    match rg_core::release::service::get_asset(&state.db, asset_id).await {
        Ok(asset) => (StatusCode::OK, Json(serde_json::json!(asset))).into_response(),
        Err(_) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "asset not found" })),
        )
            .into_response(),
    }
}

/// GET /api/v1/repos/:owner/:name/releases/assets/:asset_id/download
///
/// Downloads the release asset file and increments the download count.
pub async fn download_asset(
    State(state): State<AppState>,
    Path((owner, name, asset_id)): Path<(String, String, i64)>,
) -> impl IntoResponse {
    // Verify repo exists
    match rg_core::repo::service::find_repo_by_owner_name(&state.db, &owner, &name).await {
        Ok(Some(_)) => {}
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": "repository not found" })),
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
    }

    match rg_core::release::service::download_asset(
        &state.db,
        asset_id,
        &state.repo_root,
        &owner,
        &name,
    )
    .await
    {
        Ok((asset, data)) => {
            let content_disposition = format!("attachment; filename=\"{}\"", asset.filename);
            let content_length = asset.size.to_string();
            let response = (
                StatusCode::OK,
                [
                    (header::CONTENT_TYPE, asset.content_type.as_str()),
                    (header::CONTENT_DISPOSITION, content_disposition.as_str()),
                    (header::CONTENT_LENGTH, content_length.as_str()),
                ],
                data,
            );
            response.into_response()
        }
        Err(e) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// DELETE /api/v1/repos/:owner/:name/releases/assets/:asset_id
pub async fn delete_asset(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((owner, name, asset_id)): Path<(String, String, i64)>,
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

    let user_id: i64 = claims.sub.parse().unwrap_or(-1);

    // Check write permission
    match rg_core::repo::service::can_write(&state.db, &owner, &name, Some(user_id)).await {
        Ok(true) => {}
        Ok(false) => {
            return (
                StatusCode::FORBIDDEN,
                Json(serde_json::json!({ "error": "permission denied" })),
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
    }

    match rg_core::release::service::delete_asset(
        &state.db,
        asset_id,
        &state.repo_root,
        &owner,
        &name,
    )
    .await
    {
        Ok(()) => {
            (
                StatusCode::NO_CONTENT,
                Json(serde_json::json!({ "deleted": true })),
            )
                .into_response()
        }
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}
