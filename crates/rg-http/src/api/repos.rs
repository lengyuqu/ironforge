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

use crate::{api::users::extract_bearer_claims, AppState};
use crate::pagination::{PaginationParams, PaginatedResponse};

/// POST /api/v1/repos
#[derive(Deserialize)]
pub struct CreateRepoRequest {
    pub name: String,
    pub description: Option<String>,
    pub is_private: Option<bool>,
    /// Organization name — if provided, create repo under this org
    pub org: Option<String>,
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

pub async fn list_repos(
    State(state): State<AppState>,
    Path(owner): Path<String>,
    Query(params): Query<ListReposQuery>,
) -> impl IntoResponse {
    let pagination = params.pagination.clamp();

    // Try user first
    if let Some(user) = rg_db::ops::user_ops::find_by_username(&state.db, &owner)
        .await
        .ok()
        .flatten()
    {
        match rg_db::ops::repo_ops::list_by_owner(&state.db, user.id).await {
            Ok(repos) => {
                let total = repos.len() as u64;
                let offset = pagination.offset() as usize;
                let limit = pagination.limit() as usize;
                let data: Vec<_> = repos.into_iter().skip(offset).take(limit).collect();
                return (StatusCode::OK, Json(PaginatedResponse::new(data, &pagination, total))).into_response();
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
        match rg_db::ops::repo_ops::list_by_org(&state.db, org.id).await {
            Ok(repos) => {
                let total = repos.len() as u64;
                let offset = pagination.offset() as usize;
                let limit = pagination.limit() as usize;
                let data: Vec<_> = repos.into_iter().skip(offset).take(limit).collect();
                return (StatusCode::OK, Json(PaginatedResponse::new(data, &pagination, total))).into_response();
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
