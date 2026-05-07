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
    Json,
};
use serde::Deserialize;

use crate::AppState;
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
pub async fn list_users(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<PaginationParams>,
) -> (StatusCode, Json<serde_json::Value>) {
    if require_admin(&state, &headers).await.is_none() {
        return (StatusCode::FORBIDDEN, Json(serde_json::json!({"error": "admin required"})));
    }
    let params = params.clamp();
    match rg_core::user::service::list_users_admin(&state.db, params.offset(), params.limit()).await {
        Ok(paginated) => {
            let resp = PaginatedResponse::new(paginated.users, &params, paginated.total as u64);
            (StatusCode::OK, Json(serde_json::to_value(resp).unwrap()))
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))),
    }
}

/// GET /api/v1/admin/users/:id
pub async fn get_user(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(user_id): Path<i64>,
) -> (StatusCode, Json<serde_json::Value>) {
    if require_admin(&state, &headers).await.is_none() {
        return (StatusCode::FORBIDDEN, Json(serde_json::json!({"error": "admin required"})));
    }
    match rg_core::user::service::get_user_by_id(&state.db, user_id).await {
        Ok(Some(user)) => (StatusCode::OK, Json(serde_json::json!(user))),
        Ok(None) => (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "user not found"}))),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))),
    }
}

/// PATCH /api/v1/admin/users/:id
pub async fn update_user(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(user_id): Path<i64>,
    Json(body): Json<UpdateUserRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    if require_admin(&state, &headers).await.is_none() {
        return (StatusCode::FORBIDDEN, Json(serde_json::json!({"error": "admin required"})));
    }
    let display_name = body.display_name.map(Some);
    let bio = body.bio.map(Some);
    match rg_core::user::service::update_user_admin(
        &state.db, user_id, display_name, bio, body.is_admin, body.is_active,
    )
    .await
    {
        Ok(user) => (StatusCode::OK, Json(serde_json::json!(user))),
        Err(e) => (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": e.to_string()}))),
    }
}

/// DELETE /api/v1/admin/users/:id
pub async fn delete_user(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(user_id): Path<i64>,
) -> (StatusCode, Json<serde_json::Value>) {
    let current_id = match require_admin(&state, &headers).await {
        Some(id) => id,
        None => return (StatusCode::FORBIDDEN, Json(serde_json::json!({"error": "admin required"}))),
    };
    if current_id == user_id {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "cannot delete your own account"})));
    }
    match rg_core::user::service::delete_user(&state.db, user_id).await {
        Ok(()) => (StatusCode::OK, Json(serde_json::json!({"deleted": true}))),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))),
    }
}

// ── Organization management endpoints ────────────────────────────────

/// GET /api/v1/admin/orgs
pub async fn list_orgs(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<PaginationParams>,
) -> (StatusCode, Json<serde_json::Value>) {
    if require_admin(&state, &headers).await.is_none() {
        return (StatusCode::FORBIDDEN, Json(serde_json::json!({"error": "admin required"})));
    }
    let params = params.clamp();
    match rg_db::ops::org_ops::list_all_orgs(&state.db, params.offset(), params.limit()).await {
        Ok((orgs, total)) => {
            let resp: Vec<_> = orgs.iter().map(org_response).collect();
            let page = PaginatedResponse::new(resp, &params, total as u64);
            (StatusCode::OK, Json(serde_json::to_value(page).unwrap()))
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))),
    }
}

/// GET /api/v1/admin/orgs/:name
pub async fn get_org(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(name): Path<String>,
) -> (StatusCode, Json<serde_json::Value>) {
    if require_admin(&state, &headers).await.is_none() {
        return (StatusCode::FORBIDDEN, Json(serde_json::json!({"error": "admin required"})));
    }
    match rg_core::org::get_org_by_name(&state.db, &name).await {
        Ok(Some(org)) => (StatusCode::OK, Json(serde_json::json!(org_response(&org)))),
        Ok(None) => (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "organization not found"}))),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))),
    }
}

/// DELETE /api/v1/admin/orgs/:name
pub async fn delete_org(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(name): Path<String>,
) -> (StatusCode, Json<serde_json::Value>) {
    if require_admin(&state, &headers).await.is_none() {
        return (StatusCode::FORBIDDEN, Json(serde_json::json!({"error": "admin required"})));
    }
    match rg_core::org::get_org_by_name(&state.db, &name).await {
        Ok(Some(org)) => {
            match rg_core::org::delete_org(&state.db, org.id, org.id).await {
                Ok(()) => (StatusCode::OK, Json(serde_json::json!({"deleted": true}))),
                Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))),
            }
        }
        Ok(None) => (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "organization not found"}))),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))),
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
