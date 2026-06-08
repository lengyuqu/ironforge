//! Shared JWT authentication helpers.
//! Provides centralized Bearer token extraction to eliminate duplicate
//! auth patterns across API handlers.
//!
//! ## Token types supported
//!
//! - **User tokens**: Full-access tokens with username claim, issued at login.
//!   Validated by `extract_user_id` and `extract_bearer_claims`.
//! - **CI Job tokens** (`CI_JOB_TOKEN`): Least-privilege tokens scoped to a
//!   specific repository. Validated by `extract_ci_job_claims` and
//!   `extract_ci_or_user_id`. Used by CI jobs to call the IronForge API.

use axum::http::HeaderMap;
use rg_core::auth::jwt::Claims;

/// Extract authenticated user_id from the Authorization: Bearer header.
/// Returns Some(user_id) if the JWT is a valid **user token**, None otherwise.
///
/// CI job tokens are intentionally rejected — use `extract_ci_or_user_id` for
/// repository-scoped operations during CI job execution.
pub(crate) fn extract_user_id(headers: &HeaderMap, jwt_secret: &str) -> Option<i64> {
    extract_bearer_claims(headers, jwt_secret)
        .and_then(|c| c.sub.parse::<i64>().ok())
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

/// Extract an authenticated actor for repo-scoped operations.
///
/// This function accepts **both** user tokens (returns user_id) and
/// CI job tokens (returns 0 = machine actor). Use this for operations
/// that should be accessible from CI jobs (e.g., status checks, file reads,
/// package downloads).
///
/// For operations that require human authorization (admin, user settings,
/// org management), use `extract_user_id` instead.
pub(crate) fn extract_ci_or_user_id(
    headers: &HeaderMap,
    jwt_secret: &str,
    repo_id: i64,
    required_scope: &str,
) -> Option<i64> {
    let auth = headers.get("authorization")?.to_str().ok()?;
    let token = auth.strip_prefix("Bearer ")?;

    // Try user token first
    if let Some(claims) = rg_core::auth::jwt::validate_token(token, jwt_secret) {
        return claims.sub.parse::<i64>().ok();
    }

    // Try CI job token
    if rg_core::auth::ci_token::validate_ci_token(token, jwt_secret, repo_id, required_scope).is_some() {
        return Some(0); // machine actor
    }

    None
}
