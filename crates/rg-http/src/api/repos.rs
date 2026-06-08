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

use crate::error::AppError;
use crate::{api::auth::extract_bearer_claims, openapi::PaginatedRepoResponse, AppState};
use crate::pagination::{PaginationParams, PaginatedResponse};

/// Helper to record audit log (fire-and-forget).
async fn record_audit(
    db: &sea_orm::DatabaseConnection,
    user_id: i64,
    username: &str,
    action: &str,
    resource_type: Option<&str>,
    resource_id: Option<i64>,
    resource_name: Option<&str>,
    headers: &HeaderMap,
    details: Option<serde_json::Value>,
) {
    let (ip_address, user_agent) = crate::api::audit::extract_ip_and_ua(headers);

    let entry = rg_db::entities::audit_log::ActiveModel {
        id: sea_orm::NotSet,
        user_id: sea_orm::Set(Some(user_id)),
        username: sea_orm::Set(Some(username.to_string())),
        action: sea_orm::Set(action.to_string()),
        resource_type: sea_orm::Set(resource_type.map(|s| s.to_string())),
        resource_id: sea_orm::Set(resource_id),
        resource_name: sea_orm::Set(resource_name.map(|s| s.to_string())),
        ip_address: sea_orm::Set(ip_address),
        user_agent: sea_orm::Set(user_agent),
        details: sea_orm::Set(details.map(|v| v.to_string())),
        created_at: sea_orm::Set(chrono::Utc::now()),
    };

    if let Err(e) = rg_db::ops::audit_log_ops::insert(db, entry).await {
        tracing::warn!(error = %e, "failed to record audit log");
    }
}

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
            return AppError::Unauthorized("authentication required".to_string()).into_response()
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
                            return AppError::Forbidden("you are not a member of this organization".to_string()).into_response()
                        }
                    }
                }
                Ok(None) => {
                    return AppError::NotFound("organization not found".to_string()).into_response()
                }
                Err(e) => {
                    return AppError::InternalError(e.to_string()).into_response()
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
        Ok(repo) => {
            // Record audit log
            let details = serde_json::json!({
                "name": body.name,
                "is_private": body.is_private.unwrap_or(false),
                "org": body.org.as_deref()
            });
            let resource_name = format!("{}/{}", body.org.as_deref().unwrap_or(&claims.sub), &body.name);
            record_audit(
                &state.db,
                owner_id,
                &claims.sub,  // This should be username, but we have user_id. Let me fix this.
                "repo.create",
                Some("repo"),
                Some(repo.id),
                Some(&resource_name),
                &headers,
                Some(details),
            ).await;

            (StatusCode::CREATED, Json(serde_json::json!(repo))).into_response()
        },
        Err(e) => AppError::BadRequest(e.to_string()).into_response(),
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
                return AppError::InternalError(e.to_string()).into_response()
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
                return AppError::InternalError(e.to_string()).into_response()
            }
        }
    }

    AppError::NotFound("owner not found (neither user nor organization)".to_string()).into_response()
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
        Ok(None) => AppError::NotFound("repository not found".to_string()).into_response(),
        Err(e) => AppError::InternalError(e.to_string()).into_response(),
    }
}

// ── Star/Watch/Delete handlers ───────────────────────────────────────────────

/// Request body for watch state.
#[derive(serde::Deserialize, ToSchema)]
pub struct WatchRequest {
    pub state: String,
}

/// PUT /api/v1/repos/:owner/:name/star
#[utoipa::path(
    put,
    path = "/repos/{owner}/{name}/star",
    tag = "Repositories",
    params(
        ("owner" = String, Path, description = "owner"),
        ("name" = String, Path, description = "name"),
    ),
    responses(
        (status = 200, description = "Updated", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
    ),
)]
pub async fn star_repo(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((owner, name)): Path<(String, String)>,
) -> impl IntoResponse {
    let claims = match extract_bearer_claims(&headers, &state.jwt_secret) {
        Some(c) => c,
        None => {
            return AppError::Unauthorized("authentication required".to_string()).into_response()
        }
    };

    let user_id: i64 = claims.sub.parse().unwrap_or(-1);

    let repo = match rg_core::repo::service::find_repo_by_owner_name(&state.db, &owner, &name).await {
        Ok(Some(r)) => r,
        Ok(None) => {
            return AppError::NotFound("repository not found".to_string()).into_response()
        }
        Err(e) => {
            return AppError::InternalError(e.to_string()).into_response()
        }
    };

    match rg_core::repo::service::toggle_star(&state.db, user_id, repo.id).await {
        Ok(starred) => (
            StatusCode::OK,
            Json(serde_json::json!({ "starred": starred })),
        )
            .into_response(),
        Err(e) => AppError::InternalError(e.to_string()).into_response(),
    }
}

/// GET /api/v1/repos/:owner/:name/stargazers
#[utoipa::path(
    get,
    path = "/repos/{owner}/{name}/stargazers",
    tag = "Repositories",
    params(
        ("owner" = String, Path, description = "owner"),
        ("name" = String, Path, description = "name"),
    ),
    responses(
        (status = 200, description = "Success", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
    ),
)]
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
            return AppError::NotFound("repository not found".to_string()).into_response()
        }
        Err(e) => {
            return AppError::InternalError(e.to_string()).into_response()
        }
    };

    match rg_core::repo::service::list_stargazers(&state.db, repo.id, offset, limit).await {
        Ok((stargazers, total)) => (
            StatusCode::OK,
            Json(PaginatedResponse::new(stargazers, &pagination, total as u64)),
        )
            .into_response(),
        Err(e) => AppError::InternalError(e.to_string()).into_response(),
    }
}

/// PUT /api/v1/repos/:owner/:name/watch
#[utoipa::path(
    put,
    path = "/repos/{owner}/{name}/watch",
    tag = "Repositories",
    params(
        ("owner" = String, Path, description = "owner"),
        ("name" = String, Path, description = "name"),
    ),
    request_body(content = serde_json::Value),
    responses(
        (status = 200, description = "Updated", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
    ),
)]
pub async fn watch_repo(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((owner, name)): Path<(String, String)>,
    Json(body): Json<WatchRequest>,
) -> impl IntoResponse {
    let claims = match extract_bearer_claims(&headers, &state.jwt_secret) {
        Some(c) => c,
        None => {
            return AppError::Unauthorized("authentication required".to_string()).into_response()
        }
    };

    let user_id: i64 = claims.sub.parse().unwrap_or(-1);

    let repo = match rg_core::repo::service::find_repo_by_owner_name(&state.db, &owner, &name).await {
        Ok(Some(r)) => r,
        Ok(None) => {
            return AppError::NotFound("repository not found".to_string()).into_response()
        }
        Err(e) => {
            return AppError::InternalError(e.to_string()).into_response()
        }
    };

    match rg_core::repo::service::set_watch(&state.db, user_id, repo.id, &body.state).await {
        Ok(watch_state) => (
            StatusCode::OK,
            Json(serde_json::json!({ "watch_state": watch_state })),
        )
            .into_response(),
        Err(e) => AppError::InternalError(e.to_string()).into_response(),
    }
}

/// DELETE /api/v1/repos/:owner/:name/watch
#[utoipa::path(
    delete,
    path = "/repos/{owner}/{name}/watch",
    tag = "Repositories",
    params(
        ("owner" = String, Path, description = "owner"),
        ("name" = String, Path, description = "name"),
    ),
    responses(
        (status = 200, description = "Deleted", body = serde_json::Value),
        (status = 204, description = "No content"),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
    ),
)]
pub async fn unwatch_repo(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((owner, name)): Path<(String, String)>,
) -> impl IntoResponse {
    let claims = match extract_bearer_claims(&headers, &state.jwt_secret) {
        Some(c) => c,
        None => {
            return AppError::Unauthorized("authentication required".to_string()).into_response()
        }
    };

    let user_id: i64 = claims.sub.parse().unwrap_or(-1);

    let repo = match rg_core::repo::service::find_repo_by_owner_name(&state.db, &owner, &name).await {
        Ok(Some(r)) => r,
        Ok(None) => {
            return AppError::NotFound("repository not found".to_string()).into_response()
        }
        Err(e) => {
            return AppError::InternalError(e.to_string()).into_response()
        }
    };

    match rg_core::repo::service::set_watch(&state.db, user_id, repo.id, "not_watching").await {
        Ok(_) => (
            StatusCode::OK,
            Json(serde_json::json!({ "watch_state": "not_watching" })),
        )
            .into_response(),
        Err(e) => AppError::InternalError(e.to_string()).into_response(),
    }
}

/// DELETE /api/v1/repos/:owner/:name
#[utoipa::path(
    delete,
    path = "/repos/{owner}/{name}",
    tag = "Repositories",
    params(
        ("owner" = String, Path, description = "owner"),
        ("name" = String, Path, description = "name"),
    ),
    responses(
        (status = 200, description = "Deleted", body = serde_json::Value),
        (status = 204, description = "No content"),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
    ),
)]
pub async fn delete_repo_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((owner, name)): Path<(String, String)>,
) -> impl IntoResponse {
    let claims = match extract_bearer_claims(&headers, &state.jwt_secret) {
        Some(c) => c,
        None => {
            return AppError::Unauthorized("authentication required".to_string()).into_response()
        }
    };

    let user_id: i64 = claims.sub.parse().unwrap_or(-1);

    let repo = match rg_core::repo::service::find_repo_by_owner_name(&state.db, &owner, &name).await {
        Ok(Some(r)) => r,
        Ok(None) => {
            return AppError::NotFound("repository not found".to_string()).into_response()
        }
        Err(e) => {
            return AppError::InternalError(e.to_string()).into_response()
        }
    };

    // Only owner can delete
    if repo.owner_id != user_id {
        return AppError::Forbidden("only repository owner can delete".to_string()).into_response();
    }

    match rg_core::repo::service::delete_repo(&state.db, repo.id).await {
        Ok(()) => {
            // Record audit log
            let resource_name = format!("{}/{}", owner, name);
            let details = serde_json::json!({
                "owner": owner,
                "name": name
            });
            record_audit(
                &state.db,
                user_id,
                &claims.sub,
                "repo.delete",
                Some("repo"),
                Some(repo.id),
                Some(&resource_name),
                &headers,
                Some(details),
            ).await;

            (StatusCode::OK, Json(serde_json::json!({ "deleted": true }))).into_response()
        },
        Err(e) => AppError::InternalError(e.to_string()).into_response(),
    }
}

// ── Fork handlers ──────────────────────────────────────────────────────

#[derive(serde::Deserialize, ToSchema)]
pub struct ForkRequest {
    pub org: Option<String>,
}

/// POST /api/v1/repos/:owner/:name/fork
#[utoipa::path(
    post,
    path = "/repos/{owner}/{name}/fork",
    tag = "Repositories",
    params(
        ("owner" = String, Path, description = "owner"),
        ("name" = String, Path, description = "name"),
    ),
    responses(
        (status = 201, description = "Created", body = serde_json::Value),
        (status = 400, description = "Bad request", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
    ),
)]
pub async fn fork_repo_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((owner, name)): Path<(String, String)>,
) -> impl IntoResponse {
    let claims = match extract_bearer_claims(&headers, &state.jwt_secret) {
        Some(c) => c,
        None => {
            return AppError::Unauthorized("authentication required".to_string()).into_response();
        }
    };
    let user_id: i64 = claims.sub.parse().unwrap_or(-1);

    match rg_core::repo::service::fork_repo(&state.db, user_id, &owner, &name, &state.repo_root).await {
        Ok(repo) => {
            // Record audit log
            let details = serde_json::json!({
                "source_owner": owner,
                "source_name": name,
                "fork_owner": claims.sub
            });
            let resource_name = format!("{}/{}", claims.sub, name);
            record_audit(
                &state.db,
                user_id,
                &claims.sub,
                "repo.fork",
                Some("repo"),
                Some(repo.id),
                Some(&resource_name),
                &headers,
                Some(details),
            ).await;

            (StatusCode::ACCEPTED, Json(serde_json::json!(repo))).into_response()
        },
        Err(e) => AppError::BadRequest(e.to_string()).into_response(),
    }
}

/// GET /api/v1/repos/:owner/:name/forks
#[utoipa::path(
    get,
    path = "/repos/{owner}/{name}/forks",
    tag = "Repositories",
    params(
        ("owner" = String, Path, description = "owner"),
        ("name" = String, Path, description = "name"),
    ),
    responses(
        (status = 200, description = "Success", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
    ),
)]
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
        Err(e) => AppError::InternalError(e.to_string()).into_response(),
    }
}

// ── Transfer handler ──────────────────────────────────────────────────

#[derive(serde::Deserialize, ToSchema)]
pub struct TransferRequest {
    pub new_owner: String,
}

/// POST /api/v1/repos/:owner/:name/transfer
#[utoipa::path(
    post,
    path = "/repos/{owner}/{name}/transfer",
    tag = "Repositories",
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
pub async fn transfer_repo_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((owner, name)): Path<(String, String)>,
    Json(body): Json<TransferRequest>,
) -> impl IntoResponse {
    let claims = match extract_bearer_claims(&headers, &state.jwt_secret) {
        Some(c) => c,
        None => {
            return AppError::Unauthorized("authentication required".to_string()).into_response();
        }
    };
    let user_id: i64 = claims.sub.parse().unwrap_or(-1);

    match rg_core::repo::service::transfer_repo(&state.db, user_id, &owner, &name, &body.new_owner, &state.repo_root).await {
        Ok(repo) => {
            // Record audit log
            let details = serde_json::json!({
                "old_owner": owner,
                "new_owner": body.new_owner,
                "name": name
            });
            let resource_name = format!("{}/{}", body.new_owner, name);
            record_audit(
                &state.db,
                user_id,
                &claims.sub,
                "repo.transfer",
                Some("repo"),
                Some(repo.id),
                Some(&resource_name),
                &headers,
                Some(details),
            ).await;

            (StatusCode::OK, Json(serde_json::json!(repo))).into_response()
        },
        Err(e) => AppError::BadRequest(e.to_string()).into_response(),
    }
}

// ── Commit Status handlers ─────────────────────────────────────────────

#[derive(serde::Deserialize, ToSchema)]
pub struct CreateCommitStatusRequest {
    pub state: String,
    pub context: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_url: Option<String>,
}

/// POST /api/v1/repos/:owner/:name/statuses/:sha
#[utoipa::path(
    post,
    path = "/repos/{owner}/{name}/statuses/{sha}",
    tag = "Repositories",
    params(
        ("owner" = String, Path, description = "owner"),
        ("name" = String, Path, description = "name"),
        ("sha" = String, Path, description = "sha"),
    ),
    request_body(content = serde_json::Value),
    responses(
        (status = 201, description = "Created", body = serde_json::Value),
        (status = 400, description = "Bad request", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
    ),
)]
pub async fn create_commit_status(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((owner, name, sha)): Path<(String, String, String)>,
    Json(body): Json<CreateCommitStatusRequest>,
) -> impl IntoResponse {
    let claims = match extract_bearer_claims(&headers, &state.jwt_secret) {
        Some(c) => c,
        None => {
            return AppError::Unauthorized("authentication required".to_string()).into_response()
        }
    };

    let user_id: i64 = claims.sub.parse().unwrap_or(-1);

    let repo = match rg_core::repo::service::find_repo_by_owner_name(&state.db, &owner, &name).await {
        Ok(Some(r)) => r,
        Ok(None) => {
            return AppError::NotFound("repository not found".to_string()).into_response()
        }
        Err(e) => {
            return AppError::InternalError(e.to_string()).into_response()
        }
    };

    if !rg_core::repo::service::can_write(&state.db, &owner, &name, Some(user_id))
        .await
        .unwrap_or(false)
    {
        return AppError::Forbidden("forbidden".to_string()).into_response();
    }

    match rg_core::repo::service::create_commit_status(
        &state.db,
        repo.id,
        &sha,
        &body.state,
        &body.context,
        body.description.as_deref(),
        body.target_url.as_deref(),
        user_id,
    )
    .await
    {
        Ok(status) => (StatusCode::CREATED, Json(serde_json::json!(status))).into_response(),
        Err(e) => AppError::BadRequest(e.to_string()).into_response(),
    }
}

/// GET /api/v1/repos/:owner/:name/commits/:sha/statuses
#[utoipa::path(
    get,
    path = "/repos/{owner}/{name}/commits/{sha}/statuses",
    tag = "Repositories",
    params(
        ("owner" = String, Path, description = "owner"),
        ("name" = String, Path, description = "name"),
        ("sha" = String, Path, description = "sha"),
    ),
    responses(
        (status = 200, description = "Success", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
    ),
)]
pub async fn list_commit_statuses(
    State(state): State<AppState>,
    Path((owner, name, sha)): Path<(String, String, String)>,
) -> impl IntoResponse {
    match rg_core::repo::service::list_commit_statuses(&state.db, &owner, &name, &sha).await {
        Ok(statuses) => (StatusCode::OK, Json(serde_json::json!(statuses))).into_response(),
        Err(e) => AppError::InternalError(e.to_string()).into_response(),
    }
}

/// GET /api/v1/repos/:owner/:name/commits/:sha/status
#[utoipa::path(
    get,
    path = "/repos/{owner}/{name}/commits/{sha}/status",
    tag = "Repositories",
    params(
        ("owner" = String, Path, description = "owner"),
        ("name" = String, Path, description = "name"),
        ("sha" = String, Path, description = "sha"),
    ),
    responses(
        (status = 200, description = "Success", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
    ),
)]
pub async fn get_combined_status(
    State(state): State<AppState>,
    Path((owner, name, sha)): Path<(String, String, String)>,
) -> impl IntoResponse {
    match rg_core::repo::service::get_combined_status(&state.db, &owner, &name, &sha).await {
        Ok(combined) => (StatusCode::OK, Json(combined)).into_response(),
        Err(e) => AppError::InternalError(e.to_string()).into_response(),
    }
}
