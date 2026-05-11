//! REST API handlers for Pull Requests.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;

use crate::error::AppError;
use crate::AppState;
use crate::pagination::{PaginationParams, PaginatedResponse};
use utoipa::ToSchema;

// ── Request / Response types ────────────────────────────────────────────

#[derive(Deserialize)]
pub struct CreatePrRequest {
    pub title: String,
    #[serde(default)]
    pub body: Option<String>,
    /// Head branch reference. Supports "owner:branch" for fork PRs, or just "branch" for same-repo.
    pub head: String,
    pub base: String,
}

#[derive(Deserialize)]
pub struct UpdatePrRequest {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub state: Option<String>,
}

#[derive(Deserialize)]
pub struct MergePrRequest {
    /// merge / squash / rebase
    pub strategy: String,
}

#[derive(Deserialize)]
pub struct ListQuery {
    pub state: Option<String>,
    #[serde(flatten)]
    pub pagination: PaginationParams,
}

// ── PR handlers ─────────────────────────────────────────────────────────

#[utoipa::path(
    get,
    path = "/repos/{owner}/{name}/pulls",
    tag = "Pull Requests",
    params(
        ("owner" = String, Path, description = "owner"),
        ("name" = String, Path, description = "name"),
    ),
    responses(
        (status = 200, description = "Success", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
    ),
)]
pub async fn list_prs(
    State(state): State<AppState>,
    Path((owner, repo)): Path<(String, String)>,
    Query(params): Query<ListQuery>,
) -> impl IntoResponse {
    let state_filter = params.state.as_deref();
    let pagination = params.pagination.clamp();
    match rg_core::pull_request::list_prs_paginated(
        &state.db,
        &owner,
        &repo,
        state_filter,
        pagination.offset(),
        pagination.limit(),
    )
    .await
    {
        Ok((data, total)) => (
            StatusCode::OK,
            Json(PaginatedResponse::new(data, &pagination, total as u64)),
        )
            .into_response(),
        Err(e) => AppError::InternalError(e.to_string()).into_response(),
    }
}

#[utoipa::path(
    get,
    path = "/repos/{owner}/{name}/pulls/{number}",
    tag = "Pull Requests",
    params(
        ("owner" = String, Path, description = "owner"),
        ("name" = String, Path, description = "name"),
        ("number" = i64, Path, description = "number"),
    ),
    responses(
        (status = 200, description = "Success", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
    ),
)]
pub async fn get_pr(
    State(state): State<AppState>,
    Path((owner, repo, number)): Path<(String, String, i64)>,
) -> impl IntoResponse {
    match rg_core::pull_request::get_pr(&state.db, &owner, &repo, number).await {
        Ok(pr) => (StatusCode::OK, Json(pr)).into_response(),
        Err(e) => AppError::NotFound(e.to_string()).into_response(),
    }
}

#[utoipa::path(
    post,
    path = "/repos/{owner}/{name}/pulls",
    tag = "Pull Requests",
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
pub async fn create_pr(
    State(state): State<AppState>,
    Path((owner, repo)): Path<(String, String)>,
    headers: axum::http::HeaderMap,
    Json(req): Json<CreatePrRequest>,
) -> impl IntoResponse {
    let user_id = match extract_user_id(&state, &headers) {
        Some(id) => id,
        None => return AppError::Unauthorized("authentication required".to_string()).into_response(),
    };

    let repo_id = match resolve_repo_id(&state.db, &owner, &repo).await {
        Some(id) => id,
        None => return AppError::NotFound("repository not found".to_string()).into_response(),
    };

    match rg_core::pull_request::resolve_head_ref(&state.db, repo_id, &req.head).await {
        Ok((head_branch, head_repo_id)) => {
            match rg_core::pull_request::create_pr(
                &state.db,
                repo_id,
                user_id,
                req.title,
                req.body,
                head_branch,
                req.base,
                head_repo_id,
            )
            .await
            {
                Ok(pr) => (StatusCode::CREATED, Json(pr)).into_response(),
                Err(e) => AppError::BadRequest(e.to_string()).into_response(),
            }
        }
        Err(e) => AppError::BadRequest(e.to_string()).into_response(),
    }
}

#[utoipa::path(
    patch,
    path = "/repos/{owner}/{name}/pulls/{number}",
    tag = "Pull Requests",
    params(
        ("owner" = String, Path, description = "owner"),
        ("name" = String, Path, description = "name"),
        ("number" = i64, Path, description = "number"),
    ),
    request_body(content = serde_json::Value),
    responses(
        (status = 200, description = "Updated", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
    ),
)]
pub async fn update_pr(
    State(state): State<AppState>,
    Path((owner, repo, number)): Path<(String, String, i64)>,
    Json(req): Json<UpdatePrRequest>,
) -> impl IntoResponse {
    match rg_core::pull_request::update_pr(
        &state.db,
        &owner,
        &repo,
        number,
        req.title,
        req.body,
        req.state,
    )
    .await
    {
        Ok(pr) => (StatusCode::OK, Json(pr)).into_response(),
        Err(e) => AppError::BadRequest(e.to_string()).into_response(),
    }
}

#[utoipa::path(
    get,
    path = "/repos/{owner}/{name}/pulls/{number}/diff",
    tag = "Pull Requests",
    params(
        ("owner" = String, Path, description = "owner"),
        ("name" = String, Path, description = "name"),
        ("number" = i64, Path, description = "number"),
    ),
    responses(
        (status = 200, description = "Success", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
    ),
)]
pub async fn get_diff(
    State(state): State<AppState>,
    Path((owner, repo, number)): Path<(String, String, i64)>,
) -> impl IntoResponse {
    match rg_core::pull_request::compute_diff(
        &state.db,
        &state.repo_root,
        &owner,
        &repo,
        number,
    )
    .await
    {
        Ok(diff) => (StatusCode::OK, Json(diff)).into_response(),
        Err(e) => AppError::InternalError(e.to_string()).into_response(),
    }
}

#[utoipa::path(
    post,
    path = "/repos/{owner}/{name}/pulls/{number}/merge",
    tag = "Pull Requests",
    params(
        ("owner" = String, Path, description = "owner"),
        ("name" = String, Path, description = "name"),
        ("number" = i64, Path, description = "number"),
    ),
    request_body(content = serde_json::Value),
    responses(
        (status = 201, description = "Created", body = serde_json::Value),
        (status = 400, description = "Bad request", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
    ),
)]
pub async fn merge_pr(
    State(state): State<AppState>,
    Path((owner, repo, number)): Path<(String, String, i64)>,
    headers: axum::http::HeaderMap,
    Json(req): Json<MergePrRequest>,
) -> impl IntoResponse {
    // Require auth
    if extract_user_id(&state, &headers).is_none() {
        return AppError::Unauthorized("authentication required".to_string()).into_response();
    }

    let strategy = match req.strategy.as_str() {
        "merge" => rg_core::pull_request::MergeStrategy::Merge,
        "squash" => rg_core::pull_request::MergeStrategy::Squash,
        "rebase" => rg_core::pull_request::MergeStrategy::Rebase,
        _ => {
            return AppError::BadRequest("invalid merge strategy, use: merge, squash, rebase".to_string()).into_response();
        }
    };

    // Check branch protection before merging
    if let Some(repo_id) = resolve_repo_id(&state.db, &owner, &repo).await {
        if let Ok(pr) = rg_core::pull_request::get_pr(&state.db, &owner, &repo, number).await {
            if let Err(e) = rg_core::branch_protection::service::check_merge_allowed(
                &state.db, repo_id, &pr.base_branch, pr.id,
            ).await {
                return AppError::Forbidden(e.to_string()).into_response();
            }
        }
    }

    match rg_core::pull_request::merge_pr(
        &state.db,
        &state.repo_root,
        &owner,
        &repo,
        number,
        strategy,
    )
    .await
    {
        Ok(result) => (StatusCode::OK, Json(result)).into_response(),
        Err(e) => AppError::BadRequest(e.to_string()).into_response(),
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────

fn extract_user_id(state: &AppState, headers: &axum::http::HeaderMap) -> Option<i64> {
    let auth = headers.get("authorization")?.to_str().ok()?;
    let token = auth.strip_prefix("Bearer ")?;
    let claims = rg_core::auth::jwt::validate_token(token, &state.jwt_secret)?;
    claims.sub.parse().ok()
}

async fn resolve_repo_id(
    db: &sea_orm::DatabaseConnection,
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
