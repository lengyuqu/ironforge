//! REST API handlers for Pull Requests.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;

use crate::AppState;
use crate::pagination::{PaginationParams, PaginatedResponse};

// ── Request / Response types ────────────────────────────────────────────

#[derive(Deserialize)]
pub struct CreatePrRequest {
    pub title: String,
    #[serde(default)]
    pub body: Option<String>,
    pub head_branch: String,
    pub base_branch: String,
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
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("{:#}", e)})),
        )
            .into_response(),
    }
}

pub async fn get_pr(
    State(state): State<AppState>,
    Path((owner, repo, number)): Path<(String, String, i64)>,
) -> impl IntoResponse {
    match rg_core::pull_request::get_pr(&state.db, &owner, &repo, number).await {
        Ok(pr) => (StatusCode::OK, Json(pr)).into_response(),
        Err(e) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": format!("{:#}", e)})),
        )
            .into_response(),
    }
}

pub async fn create_pr(
    State(state): State<AppState>,
    Path((owner, repo)): Path<(String, String)>,
    headers: axum::http::HeaderMap,
    Json(req): Json<CreatePrRequest>,
) -> impl IntoResponse {
    let user_id = match extract_user_id(&state, &headers) {
        Some(id) => id,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({"error": "authentication required"})),
            )
                .into_response()
        }
    };

    let repo_id = match resolve_repo_id(&state.db, &owner, &repo).await {
        Some(id) => id,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "repository not found"})),
            )
                .into_response()
        }
    };

    match rg_core::pull_request::create_pr(
        &state.db,
        repo_id,
        user_id,
        req.title,
        req.body,
        req.head_branch,
        req.base_branch,
    )
    .await
    {
        Ok(pr) => (StatusCode::CREATED, Json(pr)).into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": format!("{:#}", e)})),
        )
            .into_response(),
    }
}

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
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": format!("{:#}", e)})),
        )
            .into_response(),
    }
}

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
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("{:#}", e)})),
        )
            .into_response(),
    }
}

pub async fn merge_pr(
    State(state): State<AppState>,
    Path((owner, repo, number)): Path<(String, String, i64)>,
    headers: axum::http::HeaderMap,
    Json(req): Json<MergePrRequest>,
) -> impl IntoResponse {
    // Require auth
    if extract_user_id(&state, &headers).is_none() {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "authentication required"})),
        )
            .into_response();
    }

    let strategy = match req.strategy.as_str() {
        "merge" => rg_core::pull_request::MergeStrategy::Merge,
        "squash" => rg_core::pull_request::MergeStrategy::Squash,
        "rebase" => rg_core::pull_request::MergeStrategy::Rebase,
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "invalid merge strategy, use: merge, squash, rebase"})),
            )
                .into_response()
        }
    };

    // Check branch protection before merging
    if let Some(repo_id) = resolve_repo_id(&state.db, &owner, &repo).await {
        if let Ok(pr) = rg_core::pull_request::get_pr(&state.db, &owner, &repo, number).await {
            if let Err(e) = rg_core::branch_protection::service::check_merge_allowed(
                &state.db, repo_id, &pr.base_branch, pr.id,
            ).await {
                return (
                    StatusCode::FORBIDDEN,
                    Json(serde_json::json!({"error": format!("{:#}", e)})),
                )
                    .into_response();
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
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": format!("{:#}", e)})),
        )
            .into_response(),
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
