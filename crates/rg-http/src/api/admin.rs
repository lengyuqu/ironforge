//! Admin REST API handlers.
//!
//! All endpoints require is_admin=true on the authenticated user.
//!
//! GET    /api/v1/admin/users          -- list all users (paginated)
//! GET    /api/v1/admin/users/:id      -- get a single user
//! PATCH  /api/v1/admin/users/:id      -- update user (display_name, bio, is_admin, is_active)
//! DELETE /api/v1/admin/users/:id      -- delete a user
//! GET    /api/v1/admin/orgs           -- list all organizations
//! GET    /api/v1/admin/orgs/:name     -- get an organization
//! DELETE /api/v1/admin/orgs/:name     -- delete an organization

use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use serde::Deserialize;
use utoipa::ToSchema;

use crate::AppState;
use crate::error::AppError;
use crate::pagination::{PaginatedResponse, PaginationParams};
use super::users::extract_bearer_claims;

// ── Request / Response types ────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct UpdateUserRequest {
    pub display_name: Option<String>,
    pub bio: Option<String>,
    pub is_admin: Option<bool>,
    pub is_active: Option<bool>,
}

// ── Admin middleware: require is_admin ────────────────────────────────

/// Extract the current user ID from Bearer token and verify is_admin=true.
/// Returns None if not authenticated or not an admin.
async fn require_admin(state: &AppState, headers: &HeaderMap) -> Option<i64> {
    let claims = extract_bearer_claims(headers, &state.jwt_secret)?;
    let user_id: i64 = claims.sub.parse().ok()?;
    let user = rg_db::ops::user_ops::find_by_id(&state.db, user_id).await.ok()??;
    if user.is_admin {
        Some(user_id)
    } else {
        None
    }
}

// ── User management endpoints ─────────────────────────────────────────

/// GET /api/v1/admin/users
#[utoipa::path(
    get,
    path = "/admin/users",
    tag = "Admin",
    responses(
        (status = 200, description = "Success", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
    ),
)]
pub async fn list_users(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<PaginationParams>,
) -> impl IntoResponse {
    if require_admin(&state, &headers).await.is_none() {
        return AppError::forbidden("admin required").into_response();
    }
    let params = params.clamp();
    match rg_core::user::service::list_users_admin(&state.db, params.offset(), params.limit()).await {
        Ok(paginated) => {
            let resp = PaginatedResponse::new(paginated.users, &params, paginated.total as u64);
            (StatusCode::OK, Json(serde_json::to_value(resp).unwrap())).into_response()
        }
        Err(e) => AppError::internal(e.to_string()).into_response(),
    }
}

/// GET /api/v1/admin/users/:id
#[utoipa::path(
    get,
    path = "/admin/users/{id}",
    tag = "Admin",
    params(
        ("id" = i64, Path, description = "id"),
    ),
    responses(
        (status = 200, description = "Success", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
    ),
)]
pub async fn get_user(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(user_id): Path<i64>,
) -> impl IntoResponse {
    if require_admin(&state, &headers).await.is_none() {
        return AppError::forbidden("admin required").into_response();
    }
    match rg_core::user::service::get_user_by_id(&state.db, user_id).await {
        Ok(Some(user)) => (StatusCode::OK, Json(serde_json::json!(user))).into_response(),
        Ok(None) => AppError::not_found("user not found").into_response(),
        Err(e) => AppError::internal(e.to_string()).into_response(),
    }
}

/// PATCH /api/v1/admin/users/:id
#[utoipa::path(
    patch,
    path = "/admin/users/{id}",
    tag = "Admin",
    params(
        ("id" = i64, Path, description = "id"),
    ),
    request_body(content = serde_json::Value),
    responses(
        (status = 200, description = "Updated", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
    ),
)]
pub async fn update_user(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(user_id): Path<i64>,
    Json(body): Json<UpdateUserRequest>,
) -> impl IntoResponse {
    if require_admin(&state, &headers).await.is_none() {
        return AppError::forbidden("admin required").into_response();
    }
    let display_name = body.display_name.map(Some);
    let bio = body.bio.map(Some);
    match rg_core::user::service::update_user_admin(
        &state.db, user_id, display_name, bio, body.is_admin, body.is_active,
    )
    .await
    {
        Ok(user) => (StatusCode::OK, Json(serde_json::json!(user))).into_response(),
        Err(e) => AppError::bad_request(e.to_string()).into_response(),
    }
}

/// DELETE /api/v1/admin/users/:id
#[utoipa::path(
    delete,
    path = "/admin/users/{id}",
    tag = "Admin",
    params(
        ("id" = i64, Path, description = "id"),
    ),
    responses(
        (status = 200, description = "Deleted", body = serde_json::Value),
        (status = 204, description = "No content"),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
    ),
)]
pub async fn delete_user(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(user_id): Path<i64>,
) -> impl IntoResponse {
    let current_id = match require_admin(&state, &headers).await {
        Some(id) => id,
        None => return AppError::forbidden("admin required").into_response(),
    };
    if current_id == user_id {
        return AppError::bad_request("cannot delete your own account").into_response();
    }
    match rg_core::user::service::delete_user(&state.db, user_id).await {
        Ok(()) => (StatusCode::OK, Json(serde_json::json!({"deleted": true}))).into_response(),
        Err(e) => AppError::internal(e.to_string()).into_response(),
    }
}

// ── Organization management endpoints ────────────────────────────────

/// GET /api/v1/admin/orgs
#[utoipa::path(
    get,
    path = "/admin/orgs",
    tag = "Admin",
    responses(
        (status = 200, description = "Success", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
    ),
)]
pub async fn list_orgs(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<PaginationParams>,
) -> impl IntoResponse {
    if require_admin(&state, &headers).await.is_none() {
        return AppError::forbidden("admin required").into_response();
    }
    let params = params.clamp();
    match rg_db::ops::org_ops::list_all_orgs(&state.db, params.offset(), params.limit()).await {
        Ok((orgs, total)) => {
            let resp: Vec<_> = orgs.iter().map(org_response).collect();
            let page = PaginatedResponse::new(resp, &params, total as u64);
            (StatusCode::OK, Json(serde_json::to_value(page).unwrap())).into_response()
        }
        Err(e) => AppError::internal(e.to_string()).into_response(),
    }
}

/// GET /api/v1/admin/orgs/:name
#[utoipa::path(
    get,
    path = "/admin/orgs/{name}",
    tag = "Admin",
    params(
        ("name" = String, Path, description = "name"),
    ),
    responses(
        (status = 200, description = "Success", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
    ),
)]
pub async fn get_org(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(name): Path<String>,
) -> impl IntoResponse {
    if require_admin(&state, &headers).await.is_none() {
        return AppError::forbidden("admin required").into_response();
    }
    match rg_core::org::get_org_by_name(&state.db, &name).await {
        Ok(Some(org)) => (StatusCode::OK, Json(serde_json::json!(org_response(&org)))).into_response(),
        Ok(None) => AppError::not_found("organization not found").into_response(),
        Err(e) => AppError::internal(e.to_string()).into_response(),
    }
}

/// DELETE /api/v1/admin/orgs/:name
#[utoipa::path(
    delete,
    path = "/admin/orgs/{name}",
    tag = "Admin",
    params(
        ("name" = String, Path, description = "name"),
    ),
    responses(
        (status = 200, description = "Deleted", body = serde_json::Value),
        (status = 204, description = "No content"),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
    ),
)]
pub async fn delete_org(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(name): Path<String>,
) -> impl IntoResponse {
    if require_admin(&state, &headers).await.is_none() {
        return AppError::forbidden("admin required").into_response();
    }
    match rg_core::org::get_org_by_name(&state.db, &name).await {
        Ok(Some(org)) => {
            match rg_core::org::delete_org(&state.db, org.id, org.id).await {
                Ok(()) => (StatusCode::OK, Json(serde_json::json!({"deleted": true}))).into_response(),
                Err(e) => AppError::internal(e.to_string()).into_response(),
            }
        }
        Ok(None) => AppError::not_found("organization not found").into_response(),
        Err(e) => AppError::internal(e.to_string()).into_response(),
    }
}

// ── Helpers ──────────────────────────────────────────────────────────

fn org_response(org: &rg_db::entities::organization::Model) -> serde_json::Value {
    serde_json::json!({
        "id": org.id,
        "name": org.name,
        "display_name": org.display_name,
        "description": org.description,
        "owner_id": org.owner_id,
        "visibility": org.visibility,
        "created_at": org.created_at.to_string(),
        "updated_at": org.updated_at.to_string(),
    })
}
