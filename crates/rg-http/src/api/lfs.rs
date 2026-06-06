//! Git LFS REST API endpoints.
//!
//! Implements the LFS batch API and object upload/download endpoints.

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::Json;

use crate::api::users::extract_bearer_claims;
use crate::AppState;
use crate::error::AppError;
use utoipa::ToSchema;

/// LFS batch API: POST /repos/:owner/:name/lfs/objects/batch
#[utoipa::path(
    post,
    path = "/repos/{owner}/{name}/lfs/objects/batch",
    tag = "LFS",
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
pub async fn batch(
    State(state): State<AppState>,
    Path((owner, repo)): Path<(String, String)>,
    headers: HeaderMap,
    Json(req): Json<rg_core::lfs::service::LfsBatchRequest>,
) -> impl IntoResponse {
    // LFS client sends Accept: application/vnd.git-lfs+json
    let repo_model = match rg_core::repo::service::find_repo_by_owner_name(
        &state.db, &owner, &repo,
    )
    .await
    {
        Ok(Some(r)) => r,
        Ok(None) => return AppError::not_found("repository not found").into_response(),
        Err(e) => return AppError::internal(e).into_response(),
    };

    // H-01: Auth check for private repos
    if repo_model.is_private {
        let claims = match extract_bearer_claims(&headers, &state.jwt_secret) {
            Some(c) => c,
            None => {
                return AppError::unauthorized("authentication required").into_response()
            }
        };
        let user_id: i64 = claims.sub.parse().unwrap_or(-1);
        if !rg_core::repo::service::can_read_repo(&state.db, &repo_model, Some(user_id))
            .await
            .unwrap_or(false)
        {
            return AppError::forbidden("access denied").into_response();
        }
    }

    let repo_id = repo_model.id;
    let lfs_root = rg_core::lfs::service::lfs_root(&state.repo_root, &owner, &repo);

    // Build base URL from request headers
    let base_url = headers
        .get("host")
        .and_then(|v| v.to_str().ok())
        .map(|h| format!("http://{}", h))
        .unwrap_or_else(|| "http://localhost:8080".to_string());

    match rg_core::lfs::service::batch(
        &state.db,
        repo_id,
        &lfs_root,
        &base_url,
        &owner,
        &repo,
        &req,
    )
    .await
    {
        Ok(resp) => (StatusCode::OK, Json(serde_json::json!(resp))).into_response(),
        Err(e) => AppError::internal(e).into_response(),
    }
}

/// Upload an LFS object: PUT /repos/:owner/:name/lfs/objects/:oid
#[utoipa::path(
    put,
    path = "/repos/{owner}/{name}/lfs/objects/{oid}",
    tag = "LFS",
    params(
        ("owner" = String, Path, description = "owner"),
        ("name" = String, Path, description = "name"),
        ("oid" = String, Path, description = "oid"),
    ),
    request_body(content = serde_json::Value),
    responses(
        (status = 200, description = "Updated", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
    ),
)]
pub async fn upload_object(
    State(state): State<AppState>,
    Path((owner, repo, oid)): Path<(String, String, String)>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    let repo_model = match rg_core::repo::service::find_repo_by_owner_name(
        &state.db, &owner, &repo,
    )
    .await
    {
        Ok(Some(r)) => r,
        Ok(None) => return AppError::not_found("repository not found").into_response(),
        Err(e) => return AppError::internal(e).into_response(),
    };

    // H-01: Auth check for private repos
    if repo_model.is_private {
        let claims = match extract_bearer_claims(&headers, &state.jwt_secret) {
            Some(c) => c,
            None => {
                return AppError::unauthorized("authentication required").into_response()
            }
        };
        let user_id: i64 = claims.sub.parse().unwrap_or(-1);
        if !rg_core::repo::service::can_read_repo(&state.db, &repo_model, Some(user_id))
            .await
            .unwrap_or(false)
        {
            return AppError::forbidden("access denied").into_response();
        }
    }

    let repo_id = repo_model.id;
    let lfs_root = rg_core::lfs::service::lfs_root(&state.repo_root, &owner, &repo);

    match rg_core::lfs::service::store_object(
        &state.db,
        repo_id,
        &lfs_root,
        &oid,
        &body,
    )
    .await
    {
        Ok(()) => StatusCode::OK.into_response(),
        Err(e) => AppError::internal(e).into_response(),
    }
}

/// Download an LFS object: GET /repos/:owner/:name/lfs/objects/:oid
#[utoipa::path(
    get,
    path = "/repos/{owner}/{name}/lfs/objects/{oid}",
    tag = "LFS",
    params(
        ("owner" = String, Path, description = "owner"),
        ("name" = String, Path, description = "name"),
        ("oid" = String, Path, description = "oid"),
    ),
    responses(
        (status = 200, description = "Success", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
    ),
)]
pub async fn download_object(
    State(state): State<AppState>,
    Path((owner, repo, oid)): Path<(String, String, String)>,
    headers: HeaderMap,
) -> impl IntoResponse {
    // H-01: Auth check for private repos
    let repo_model = match rg_core::repo::service::find_repo_by_owner_name(
        &state.db, &owner, &repo,
    )
    .await
    {
        Ok(Some(r)) => r,
        Ok(None) => return AppError::not_found("repository not found").into_response(),
        Err(e) => return AppError::internal(e).into_response(),
    };

    if repo_model.is_private {
        let claims = match extract_bearer_claims(&headers, &state.jwt_secret) {
            Some(c) => c,
            None => {
                return AppError::unauthorized("authentication required").into_response()
            }
        };
        let user_id: i64 = claims.sub.parse().unwrap_or(-1);
        if !rg_core::repo::service::can_read_repo(&state.db, &repo_model, Some(user_id))
            .await
            .unwrap_or(false)
        {
            return AppError::forbidden("access denied").into_response();
        }
    }

    let lfs_root = rg_core::lfs::service::lfs_root(&state.repo_root, &owner, &repo);

    match rg_core::lfs::service::read_object(&lfs_root, &oid).await {
        Ok(data) => (
            StatusCode::OK,
            [(axum::http::header::CONTENT_TYPE, "application/octet-stream")],
            data,
        ).into_response(),
        Err(e) => (
            StatusCode::NOT_FOUND,
            [(axum::http::header::CONTENT_TYPE, "text/plain")],
            e.to_string().into_bytes(),
        ).into_response(),
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────
