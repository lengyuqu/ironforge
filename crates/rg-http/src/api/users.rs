//! User registration and login REST API.
//!
//! POST /api/v1/users/register
//! POST /api/v1/users/login
//! GET  /api/v1/users/me  (requires Bearer token)

use axum::{
    extract::{Path, State},
    http::{header, HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use serde::Deserialize;
use utoipa::ToSchema;

use crate::AppState;

/// POST /api/v1/users/register
#[derive(Deserialize, ToSchema)]
pub struct RegisterRequest {
    pub username: String,
    pub email: String,
    pub password: String,
}

/// Login request body.
#[derive(Deserialize, ToSchema)]
pub struct LoginRequest {
    /// Username or email
    pub login: String,
    pub password: String,
}

/// Login/Register success response.
#[derive(serde::Serialize, ToSchema)]
pub struct AuthResponse {
    pub token: String,
    pub user_id: i64,
    pub username: String,
}

/// User profile response.
#[derive(serde::Serialize, ToSchema)]
pub struct UserProfile {
    pub id: i64,
    pub username: String,
    pub email: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avatar_url: Option<String>,
    pub is_admin: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[utoipa::path(
    post,
    path = "/users/register",
    tag = "Users",
    request_body = RegisterRequest,
    responses(
        (status = 201, description = "User registered successfully", body = AuthResponse),
        (status = 400, description = "Invalid input", body = serde_json::Value),
    )
)]
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

#[utoipa::path(
    post,
    path = "/users/login",
    tag = "Users",
    request_body = LoginRequest,
    responses(
        (status = 200, description = "Login successful", body = AuthResponse),
        (status = 401, description = "Invalid credentials", body = serde_json::Value),
    )
)]
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

#[utoipa::path(
    get,
    path = "/users/me",
    tag = "Users",
    responses(
        (status = 200, description = "Current user profile", body = UserProfile),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
        (status = 404, description = "User not found", body = serde_json::Value),
    )
)]
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

// ── PAT (Personal Access Token) handlers ────────────────────────────────

use sha2::{Sha256, Digest};

#[derive(serde::Deserialize)]
pub struct CreateTokenRequest {
    pub name: String,
    pub scopes: Option<String>,
    pub expires_at: Option<String>,
}

fn generate_token() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
    let entropy: u128 = {
        let mut buf = [0u8; 16];
        if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
            let _ = std::io::Read::read_exact(&mut f, &mut buf);
            u128::from_le_bytes(buf)
        } else {
            let pid = std::process::id() as u128;
            (nanos << 32) | pid
        }
    };
    format!("ifp_{:016x}{:032x}", nanos, entropy)
}

fn hash_token(token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// GET /api/v1/users/tokens
pub async fn list_tokens(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let claims = match extract_bearer_claims(&headers, &state.jwt_secret) {
        Some(c) => c,
        None => { return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({ "error": "authentication required" }))).into_response(); }
    };
    let user_id: i64 = claims.sub.parse().unwrap_or(-1);
    match rg_db::ops::token_ops::list_by_user(&state.db, user_id).await {
        Ok(tokens) => (StatusCode::OK, Json(serde_json::json!(tokens))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": e.to_string() }))).into_response(),
    }
}

/// POST /api/v1/users/tokens
pub async fn create_token(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<CreateTokenRequest>,
) -> impl IntoResponse {
    let claims = match extract_bearer_claims(&headers, &state.jwt_secret) {
        Some(c) => c,
        None => { return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({ "error": "authentication required" }))).into_response(); }
    };
    let user_id: i64 = claims.sub.parse().unwrap_or(-1);
    if body.name.trim().is_empty() {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "error": "token name cannot be empty" }))).into_response();
    }
    let raw_token = generate_token();
    let token_hash = hash_token(&raw_token);
    let scopes = body.scopes.unwrap_or_else(|| "repo".to_string());
    let expires_at = body.expires_at.as_deref()
        .and_then(|d| chrono::DateTime::parse_from_rfc3339(d).ok())
        .map(|dt| dt.with_timezone(&chrono::Utc));
    let now = chrono::Utc::now();
    let model = rg_db::entities::access_token::ActiveModel {
        id: sea_orm::NotSet,
        user_id: sea_orm::Set(user_id),
        name: sea_orm::Set(body.name),
        token_hash: sea_orm::Set(token_hash),
        scopes: sea_orm::Set(scopes),
        expires_at: sea_orm::Set(expires_at),
        last_used_at: sea_orm::Set(None),
        created_at: sea_orm::Set(now),
    };
    match rg_db::ops::token_ops::create(&state.db, model).await {
        Ok(token) => (StatusCode::CREATED, Json(serde_json::json!({
            "id": token.id,
            "name": token.name,
            "token": raw_token,
            "scopes": token.scopes,
            "expires_at": token.expires_at,
            "created_at": token.created_at,
        }))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": e.to_string() }))).into_response(),
    }
}

/// DELETE /api/v1/users/tokens/:id
pub async fn delete_token(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    let claims = match extract_bearer_claims(&headers, &state.jwt_secret) {
        Some(c) => c,
        None => { return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({ "error": "authentication required" }))).into_response(); }
    };
    let user_id: i64 = claims.sub.parse().unwrap_or(-1);
    let token = match rg_db::ops::token_ops::find_by_id(&state.db, id).await {
        Ok(Some(t)) => t,
        _ => { return (StatusCode::NOT_FOUND, Json(serde_json::json!({ "error": "token not found" }))).into_response(); }
    };
    if token.user_id != user_id {
        return (StatusCode::FORBIDDEN, Json(serde_json::json!({ "error": "you can only revoke your own tokens" }))).into_response();
    }
    match rg_db::ops::token_ops::delete_by_id(&state.db, id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": e.to_string() }))).into_response(),
    }
}
