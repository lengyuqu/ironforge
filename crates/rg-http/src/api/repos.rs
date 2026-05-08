//! Repository REST API.
//!
//! POST /api/v1/repos              — create repo (auth required)
//! GET  /api/v1/repos/:owner       — list repos by owner (user or org)
//! GET  /api/v1/repos/:owner/:name — get single repo

use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use serde::Deserialize;
use utoipa::ToSchema;

use crate::{api::users::extract_bearer_claims, openapi::PaginatedRepoResponse, AppState};
use crate::pagination::{PaginationParams, PaginatedResponse};

/// POST /api/v1/repos
#[derive(Deserialize, ToSchema)]
pub struct CreateRepoRequest {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_private: Option<bool>,
    /// Organization name — if provided, create repo under this org
    #[serde(skip_serializing_if = "Option::is_none")]
    pub org: Option<String>,
}

/// Repository response (matches DB model fields exposed to API).
#[derive(serde::Serialize, ToSchema)]
pub struct RepoResponse {
    pub id: i64,
    pub owner_id: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub org_id: Option<i64>,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub is_private: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[utoipa::path(
    post,
    path = "/repos",
    tag = "Repositories",
    request_body = CreateRepoRequest,
    responses(
        (status = 201, description = "Repository created", body = RepoResponse),
        (status = 400, description = "Invalid input", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
        (status = 403, description = "Forbidden (org membership required)", body = serde_json::Value),
    )
)]
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

    // Resolve org_id if org is specified
    let org_id = match &body.org {
        Some(org_name) => {
            match rg_db::ops::org_ops::get_org_by_name(&state.db, org_name).await {
                Ok(Some(org)) => {
                    // Verify the user is a member of this org
                    match rg_db::ops::org_ops::is_org_member(&state.db, org.id, owner_id).await {
                        Ok(true) => Some(org.id),
                        _ => {
                            return (
                                StatusCode::FORBIDDEN,
                                Json(serde_json::json!({ "error": "you are not a member of this organization" })),
                            )
                                .into_response()
                        }
                    }
                }
                Ok(None) => {
                    return (
                        StatusCode::NOT_FOUND,
                        Json(serde_json::json!({ "error": "organization not found" })),
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
        }
        None => None,
    };

    match rg_core::repo::service::create_repo(
        &state.db,
        owner_id,
        &body.name,
        body.description.as_deref(),
        body.is_private.unwrap_or(false),
        &state.repo_root,
        org_id,
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
/// Lists repos for either a user or an organization.
#[derive(Deserialize)]
pub struct ListReposQuery {
    #[serde(flatten)]
    pub pagination: PaginationParams,
}

#[utoipa::path(
    get,
    path = "/repos/{owner}",
    tag = "Repositories",
    params(
        ("owner" = String, Path, description = "Username or organization name"),
        ("page" = Option<u64>, Query, description = "Page number (1-based)"),
        ("per_page" = Option<u64>, Query, description = "Items per page (1-100)"),
    ),
    responses(
        (status = 200, description = "List of repositories", body = PaginatedRepoResponse),
        (status = 404, description = "Owner not found", body = serde_json::Value),
        (status = 500, description = "Internal server error", body = serde_json::Value),
    )
)]
pub async fn list_repos(
    State(state): State<AppState>,
    Path(owner): Path<String>,
    Query(params): Query<ListReposQuery>,
) -> impl IntoResponse {
    let pagination = params.pagination.clamp();
    let offset = pagination.offset();
    let limit = pagination.limit();

    // Try user first
    if let Some(user) = rg_db::ops::user_ops::find_by_username(&state.db, &owner)
        .await
        .ok()
        .flatten()
    {
        match rg_db::ops::repo_ops::list_by_owner_paginated(&state.db, user.id, offset, limit).await {
            Ok((data, total)) => {
                return (
                    StatusCode::OK,
                    Json(PaginatedResponse::new(data, &pagination, total as u64)),
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
    }

    // Try organization
    if let Some(org) = rg_db::ops::org_ops::get_org_by_name(&state.db, &owner)
        .await
        .ok()
        .flatten()
    {
        match rg_db::ops::repo_ops::list_by_org_paginated(&state.db, org.id, offset, limit).await {
            Ok((data, total)) => {
                return (
                    StatusCode::OK,
                    Json(PaginatedResponse::new(data, &pagination, total as u64)),
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
    }

    (
        StatusCode::NOT_FOUND,
        Json(serde_json::json!({ "error": "owner not found (neither user nor organization)" })),
    )
        .into_response()
}

#[utoipa::path(
    get,
    path = "/repos/{owner}/{name}",
    tag = "Repositories",
    params(
        ("owner" = String, Path, description = "Username or organization name"),
        ("name" = String, Path, description = "Repository name"),
    ),
    responses(
        (status = 200, description = "Repository details", body = RepoResponse),
        (status = 404, description = "Repository not found", body = serde_json::Value),
        (status = 500, description = "Internal server error", body = serde_json::Value),
    )
)]
/// GET /api/v1/repos/:owner/:name
/// Gets a single repo, supporting both user and org owners.
pub async fn get_repo(
    State(state): State<AppState>,
    Path((owner, name)): Path<(String, String)>,
) -> impl IntoResponse {
    match rg_core::repo::service::find_repo_by_owner_name(&state.db, &owner, &name).await {
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

// ── Star/Watch/Delete handlers ───────────────────────────────────────────────

/// Request body for watch state.
#[derive(serde::Deserialize)]
pub struct WatchRequest {
    pub state: String,
}

/// PUT /api/v1/repos/:owner/:name/star
pub async fn star_repo(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((owner, name)): Path<(String, String)>,
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

    match rg_core::repo::service::toggle_star(&state.db, user_id, repo.id).await {
        Ok(starred) => (
            StatusCode::OK,
            Json(serde_json::json!({ "starred": starred })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// GET /api/v1/repos/:owner/:name/stargazers
pub async fn get_stargazers(
    State(state): State<AppState>,
    Path((owner, name)): Path<(String, String)>,
    Query(params): Query<PaginationParams>,
) -> impl IntoResponse {
    let pagination = params.clamp();
    let offset = pagination.offset();
    let limit = pagination.limit();

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

    match rg_core::repo::service::list_stargazers(&state.db, repo.id, offset, limit).await {
        Ok((stargazers, total)) => (
            StatusCode::OK,
            Json(PaginatedResponse::new(stargazers, &pagination, total as u64)),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// PUT /api/v1/repos/:owner/:name/watch
pub async fn watch_repo(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((owner, name)): Path<(String, String)>,
    Json(body): Json<WatchRequest>,
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

    match rg_core::repo::service::set_watch(&state.db, user_id, repo.id, &body.state).await {
        Ok(watch_state) => (
            StatusCode::OK,
            Json(serde_json::json!({ "watch_state": watch_state })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// DELETE /api/v1/repos/:owner/:name/watch
pub async fn unwatch_repo(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((owner, name)): Path<(String, String)>,
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

    match rg_core::repo::service::set_watch(&state.db, user_id, repo.id, "not_watching").await {
        Ok(_) => (
            StatusCode::OK,
            Json(serde_json::json!({ "watch_state": "not_watching" })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// DELETE /api/v1/repos/:owner/:name
pub async fn delete_repo_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((owner, name)): Path<(String, String)>,
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

    // Only owner can delete
    if repo.owner_id != user_id {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({ "error": "only repository owner can delete" })),
        )
            .into_response();
    }

    match rg_core::repo::service::delete_repo(&state.db, repo.id).await {
        Ok(()) => (StatusCode::OK, Json(serde_json::json!({ "deleted": true }))).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

// ── Fork handlers ──────────────────────────────────────────────────────

#[derive(serde::Deserialize)]
pub struct ForkRequest {
    pub org: Option<String>,
}

/// POST /api/v1/repos/:owner/:name/fork
pub async fn fork_repo_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((owner, name)): Path<(String, String)>,
) -> impl IntoResponse {
    let claims = match extract_bearer_claims(&headers, &state.jwt_secret) {
        Some(c) => c,
        None => {
            return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({ "error": "authentication required" }))).into_response();
        }
    };
    let user_id: i64 = claims.sub.parse().unwrap_or(-1);

    match rg_core::repo::service::fork_repo(&state.db, user_id, &owner, &name, &state.repo_root).await {
        Ok(repo) => (StatusCode::ACCEPTED, Json(serde_json::json!(repo))).into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "error": e.to_string() }))).into_response(),
    }
}

/// GET /api/v1/repos/:owner/:name/forks
pub async fn list_forks_handler(
    State(state): State<AppState>,
    Path((owner, name)): Path<(String, String)>,
    Query(params): Query<PaginationParams>,
) -> impl IntoResponse {
    let pagination = params.clamp();
    let offset = pagination.offset();
    let limit = pagination.limit();

    match rg_core::repo::service::list_forks(&state.db, &owner, &name, offset, limit).await {
        Ok((forks, total)) => (
            StatusCode::OK,
            Json(PaginatedResponse::new(forks, &pagination, total as u64)),
        ).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": e.to_string() }))).into_response(),
    }
}

// ── Transfer handler ──────────────────────────────────────────────────

#[derive(serde::Deserialize)]
pub struct TransferRequest {
    pub new_owner: String,
}

/// POST /api/v1/repos/:owner/:name/transfer
pub async fn transfer_repo_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((owner, name)): Path<(String, String)>,
    Json(body): Json<TransferRequest>,
) -> impl IntoResponse {
    let claims = match extract_bearer_claims(&headers, &state.jwt_secret) {
        Some(c) => c,
        None => {
            return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({ "error": "authentication required" }))).into_response();
        }
    };
    let user_id: i64 = claims.sub.parse().unwrap_or(-1);

    match rg_core::repo::service::transfer_repo(&state.db, user_id, &owner, &name, &body.new_owner, &state.repo_root).await {
        Ok(repo) => (StatusCode::OK, Json(serde_json::json!(repo))).into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "error": e.to_string() }))).into_response(),
    }
}
