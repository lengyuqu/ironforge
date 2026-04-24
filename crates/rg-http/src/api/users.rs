//! User registration and login REST API.
//!
//! POST /api/v1/users/register
//! POST /api/v1/users/login
//! GET  /api/v1/users/me  (requires Bearer token)

use axum::{
    extract::{Extension, State},
    http::{header, HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};

use crate::AppState;

/// POST /api/v1/users/register
#[derive(Deserialize)]
pub struct RegisterRequest {
    pub username: String,
    pub email: String,
    pub password: String,
}

pub async fn register(
    State(state): State<AppState>,
    Json(body): Json<RegisterRequest>,
) -> impl IntoResponse {
    match rg_core::user::service::register(
        &state.db,
        &body.username,
        &body.email,
        &body.password,
        &state.jwt_secret,
    )
    .await
    {
        Ok(resp) => (StatusCode::CREATED, Json(serde_json::json!(resp))).into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// POST /api/v1/users/login
#[derive(Deserialize)]
pub struct LoginRequest {
    /// Username or email
    pub login: String,
    pub password: String,
}

pub async fn login(
    State(state): State<AppState>,
    Json(body): Json<LoginRequest>,
) -> impl IntoResponse {
    match rg_core::user::service::login(
        &state.db,
        &body.login,
        &body.password,
        &state.jwt_secret,
    )
    .await
    {
        Ok(resp) => (StatusCode::OK, Json(serde_json::json!(resp))).into_response(),
        Err(e) => (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// GET /api/v1/users/me — returns the current user's profile.
pub async fn me(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let claims = match extract_bearer_claims(&headers, &state.jwt_secret) {
        Some(c) => c,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({ "error": "missing or invalid token" })),
            )
                .into_response()
        }
    };

    let user_id: i64 = claims.sub.parse().unwrap_or(-1);
    match rg_db::ops::user_ops::find_by_id(&state.db, user_id).await {
        Ok(Some(user)) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "id": user.id,
                "username": user.username,
                "email": user.email,
                "display_name": user.display_name,
                "avatar_url": user.avatar_url,
                "is_admin": user.is_admin,
                "created_at": user.created_at,
            })),
        )
            .into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "user not found" })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// Extract and validate a Bearer JWT from the Authorization header.
pub(crate) fn extract_bearer_claims(
    headers: &HeaderMap,
    jwt_secret: &str,
) -> Option<rg_core::auth::jwt::Claims> {
    let auth = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let token = auth.strip_prefix("Bearer ")?;
    rg_core::auth::jwt::validate_token(token, jwt_secret)
}
