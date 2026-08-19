//! Shared JWT authentication helpers.
//! Provides centralized Bearer token extraction to eliminate duplicate
//! auth patterns across API handlers.
//!
//! ## Token types supported
//!
//! - **User tokens**: Full-access tokens with username claim, issued at login.
//!   Validated by `extract_user_id` and `extract_bearer_claims`.
//! - **CI Job tokens** (`CI_JOB_TOKEN`): Least-privilege tokens scoped to a
//!   specific repository. Validated by `extract_ci_job_claims`. Used by CI jobs
//!   to call selected read-only IronForge APIs.
//!
//! ## H-3: Unified Axum Extractor
//!
//! `AuthenticatedUser` implements `FromRequestParts<AppState>`, allowing handlers
//! to declare authentication at the signature level:
//!
//! ```ignore
//! pub async fn handler(
//!     State(state): State<AppState>,
//!     AuthUser(user_id): AuthUser,
//! ) -> impl IntoResponse { ... }
//! ```
//!
//! This provides compile-time auth guarantees — handlers that need auth simply
//! include `AuthUser` in their signature. The legacy `extract_user_id()` helper
//! remains for cases where conditional auth is needed (e.g., anonymous-read repos).

use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::http::{HeaderMap, StatusCode};
use rg_core::auth::jwt::Claims;

/// H-3: Unified auth extractor — handlers include this in their signature
/// to get compile-time authentication guarantees.
///
/// Extracts user_id from the HttpOnly auth cookie or `Authorization: Bearer <jwt>` header.
/// Returns 401 if the token is missing, invalid, or not a user token.
#[derive(Debug, Clone, Copy)]
pub struct AuthUser(pub i64);

impl FromRequestParts<crate::AppState> for AuthUser {
    type Rejection = (StatusCode, &'static str);

    async fn from_request_parts(
        parts: &mut Parts,
        state: &crate::AppState,
    ) -> Result<Self, Self::Rejection> {
        let user_id = extract_user_id(&parts.headers, &state.jwt_secret)
            .ok_or((StatusCode::UNAUTHORIZED, "authentication required"))?;
        Ok(AuthUser(user_id))
    }
}

/// Cookie name used for HttpOnly JWT storage (M-4).
pub(crate) const AUTH_COOKIE_NAME: &str = "ironforge_token";

/// Extract a JWT from the `Cookie` header (M-4: HttpOnly cookie auth).
///
/// Returns the raw token string if a valid `ironforge_token` cookie is present.
fn extract_token_from_cookie(headers: &HeaderMap) -> Option<String> {
    let cookie_header = headers.get("cookie")?.to_str().ok()?;
    for cookie in cookie_header.split(';') {
        let cookie = cookie.trim();
        if let Some(token) = cookie.strip_prefix(&format!("{}=", AUTH_COOKIE_NAME)) {
            if !token.is_empty() {
                return Some(token.to_string());
            }
        }
    }
    None
}

/// Extract authenticated user_id from either the HttpOnly cookie (M-4, preferred)
/// or the `Authorization: Bearer` header (fallback for API clients / Git).
/// Returns Some(user_id) if the JWT is a valid **user token**, None otherwise.
///
/// CI job tokens are intentionally rejected — use `extract_ci_or_user_id` for
/// repository-scoped operations during CI job execution.
pub(crate) fn extract_user_id(headers: &HeaderMap, jwt_secret: &str) -> Option<i64> {
    // M-4: Check HttpOnly cookie first, then fall back to Bearer header
    if let Some(token) = extract_token_from_cookie(headers) {
        if let Some(claims) = rg_core::auth::jwt::validate_token(&token, jwt_secret) {
            return claims.sub.parse::<i64>().ok();
        }
    }
    extract_bearer_claims(headers, jwt_secret).and_then(|c| c.sub.parse::<i64>().ok())
}

/// Extract and validate the Bearer JWT Claims from the Authorization header.
/// Returns Some(Claims) for valid user tokens, None for invalid or CI tokens.
pub(crate) fn extract_bearer_claims(headers: &HeaderMap, jwt_secret: &str) -> Option<Claims> {
    let auth = headers.get("authorization")?.to_str().ok()?;
    let token = auth.strip_prefix("Bearer ")?;
    rg_core::auth::jwt::validate_token(token, jwt_secret)
}

/// Extract a CI job token and verify it has the required scope for the target repo.
///
/// Returns the job token claims if valid and authorized. Returns None if the token
/// is missing, invalid, expired, or lacks the required scope/repo access.
pub(crate) fn extract_ci_job_claims(
    headers: &HeaderMap,
    jwt_secret: &str,
    repo_id: i64,
    required_scope: &str,
) -> Option<rg_core::auth::ci_token::CiJobClaims> {
    let auth = headers.get("authorization")?.to_str().ok()?;
    let token = auth.strip_prefix("Bearer ")?;
    rg_core::auth::ci_token::validate_ci_token(token, jwt_secret, repo_id, required_scope)
}

// ── Shared repo access helpers ────────────────────────────────────────
//
// These functions consolidate repo resolution + permission checking into
// reusable helpers. All repo-scoped handlers should use them instead of
// manual auth logic.

/// Resolve a repo by owner/name and enforce **read** access.
///
/// - Public repos: accessible to everyone (anonymous included).
/// - Private repos: require a valid user JWT (cookie or Bearer) with read
///   permission, OR a valid CI job token with `repo:read` scope.
///
/// Returns `(repo_model, actor_id)` where `actor_id` is `None` for anonymous
/// users. Use [`resolve_repo_write_access`] for write operations.
pub(crate) async fn resolve_repo_read_access(
    state: &crate::AppState,
    headers: &HeaderMap,
    owner: &str,
    repo_name: &str,
) -> Result<(rg_db::entities::repository::Model, Option<i64>), crate::error::AppError> {
    let repo_model = rg_core::repo::service::find_repo_by_owner_name(&state.db, owner, repo_name)
        .await
        .map_err(crate::error::AppError::internal)?
        .ok_or_else(|| crate::error::AppError::not_found("repository not found"))?;

    let actor_id = extract_user_id(headers, &state.jwt_secret);
    match rg_core::repo::service::can_read_repo(&state.db, &repo_model, actor_id).await {
        Ok(true) => {}
        Ok(false)
            if actor_id.is_none()
                && extract_ci_job_claims(
                    headers,
                    &state.jwt_secret,
                    repo_model.id,
                    "repo:read",
                )
                .is_some() => {}
        Ok(false) if repo_model.is_private && actor_id.is_none() => {
            return Err(crate::error::AppError::unauthorized("authentication required"));
        }
        Ok(false) => {
            return Err(crate::error::AppError::forbidden("access denied"));
        }
        Err(e) => return Err(crate::error::AppError::internal(e)),
    }

    Ok((repo_model, actor_id))
}

/// Resolve a repo by owner/name and enforce **write** access.
///
/// Always requires authentication (anonymous users can never write).
/// The caller must have `write` or `admin` permission on the repo
/// (owner, collaborator with write/admin, or org member with write team).
///
/// Returns `(repo_model, user_id)`.
pub(crate) async fn resolve_repo_write_access(
    state: &crate::AppState,
    headers: &HeaderMap,
    owner: &str,
    repo_name: &str,
) -> Result<(rg_db::entities::repository::Model, i64), crate::error::AppError> {
    let repo_model = rg_core::repo::service::find_repo_by_owner_name(&state.db, owner, repo_name)
        .await
        .map_err(crate::error::AppError::internal)?
        .ok_or_else(|| crate::error::AppError::not_found("repository not found"))?;

    let user_id = extract_user_id(headers, &state.jwt_secret)
        .ok_or_else(|| crate::error::AppError::unauthorized("authentication required"))?;

    match rg_core::repo::service::can_write_repo(&state.db, &repo_model, Some(user_id)).await {
        Ok(true) => {}
        Ok(false) => {
            return Err(crate::error::AppError::forbidden("write access denied"));
        }
        Err(e) => {
            tracing::error!(error = %e, repo_id = repo_model.id, "can_write_repo check failed");
            return Err(crate::error::AppError::internal("permission check failed"));
        }
    }

    Ok((repo_model, user_id))
}
