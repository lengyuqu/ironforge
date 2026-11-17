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

/// The personal access token a session was minted from, if it was minted from
/// one. `Ok(None)` is an ordinary login. A token that has since been revoked, has
/// expired, or belongs to someone else is an error, not "no token": deleting a
/// token also ends the sessions it already minted.
async fn session_access_token(
    db: &sea_orm::DatabaseConnection,
    claims: &Claims,
) -> Result<Option<rg_db::entities::access_token::Model>, crate::error::AppError> {
    use crate::error::AppError;
    let Some(token_id) = claims.tid else {
        return Ok(None);
    };
    let token = rg_db::ops::token_ops::find_by_id(db, token_id)
        .await
        .ok()
        .flatten();
    let Some(token) = token else {
        return Err(AppError::forbidden("access token is no longer valid"));
    };
    let expired = token
        .expires_at
        .is_some_and(|expires_at| expires_at < chrono::Utc::now());
    if expired || token.user_id.to_string() != claims.sub {
        return Err(AppError::forbidden("access token is no longer valid"));
    }
    Ok(Some(token))
}

/// Hold a request to the branch limit of the personal access token it came
/// from.
///
/// The PAT middleware swaps a token for a session JWT that carries the token's
/// id (`tid`); a handler that moves a branch calls this with the branch it is
/// about to write. A session without `tid` (an ordinary login) is not limited,
/// and neither is a token with no branch limit.
pub(crate) async fn ensure_token_may_write_branch(
    db: &sea_orm::DatabaseConnection,
    claims: &Claims,
    branch: &str,
) -> Result<(), crate::error::AppError> {
    use crate::error::AppError;
    let Some(token) = session_access_token(db, claims).await? else {
        return Ok(());
    };
    let Some(stored) = token.ref_allowlist.as_deref() else {
        return Ok(());
    };
    let allow = rg_core::auth::pat_scope::parse_ref_allowlist(stored);
    if rg_core::auth::pat_scope::ref_in_allowlist(&format!("refs/heads/{branch}"), &allow) {
        Ok(())
    } else {
        Err(AppError::forbidden(
            "this access token may not write to this branch",
        ))
    }
}

/// Stable `reason` of the refusal an agent gets when it tries to approve.
pub(crate) const AGENT_MAY_NOT_APPROVE: &str = "AGENT_MAY_NOT_APPROVE";

/// Approving is a human act. An agent's approval never satisfies a review
/// requirement, whatever shape its credential is: a token with no branch limit
/// is still an agent's token when the account behind it is an agent.
///
/// The account type decides; the branch limit of the token the session came
/// from is kept as a second gate, so a branch-limited token still cannot
/// approve even for a person.
pub(crate) async fn ensure_reviewer_may_approve(
    db: &sea_orm::DatabaseConnection,
    user_id: i64,
    claims: Option<&Claims>,
) -> Result<(), crate::error::AppError> {
    use crate::error::AppError;
    // An account that cannot be read must not read as "not an agent": that is
    // exactly the failure being closed here. `is_human_account` answers `false`
    // for an agent, an unknown and a soft-deleted account alike, and propagates
    // a database error instead of guessing.
    match rg_core::agent::is_human_account(db, user_id).await {
        Ok(true) => {}
        Ok(false) => {
            return Err(AppError::coded(
                axum::http::StatusCode::FORBIDDEN,
                "FORBIDDEN",
                "an agent may not approve a pull request",
            )
            .with_reason(
                AGENT_MAY_NOT_APPROVE,
                false,
                "ask a person to review and approve",
            ));
        }
        Err(error) => return Err(error.into()),
    }
    if let Some(claims) = claims {
        if session_access_token(db, claims)
            .await?
            .is_some_and(|token| token.ref_allowlist.is_some())
        {
            return Err(AppError::forbidden(
                "an access token limited to some branches may not approve a pull request",
            ));
        }
    }
    Ok(())
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

// Repo-scoped access helpers live in [`crate::api::repo_access`]
// (`require_read` / `require_write` / `require_admin`).

// ── #5: JWT session revocation (token_version) ──────────────────────

/// Which credential a session JWT was presented in.
enum SessionSource {
    Cookie,
    Bearer,
}

/// Validate that a signature-valid JWT still represents a live session (#5).
///
/// Compares the token's `ver` claim against `users.token_version` and rejects
/// disabled accounts. Returns the user id when the session is still valid;
/// `None` when the token has been revoked (password reset, MFA change,
/// deactivation) or the account no longer exists / is disabled.
///
/// Pre-migration tokens carry no `ver` claim (decodes to 0) and match the
/// column default, so sessions issued before this feature survive deploys.
pub(crate) async fn validate_session(
    db: &sea_orm::DatabaseConnection,
    claims: &Claims,
) -> Option<i64> {
    let user_id = claims.sub.parse::<i64>().ok()?;
    let user = rg_db::ops::user_ops::find_by_id(db, user_id).await.ok()??;
    if !user.is_active || user.token_version != claims.ver {
        return None;
    }
    Some(user_id)
}

/// Extract the session JWT claims using the same priority as
/// `extract_user_id` (HttpOnly cookie first, then Bearer header).
/// Returns `None` when no signature-valid user JWT is presented.
fn extract_session_claims(
    headers: &HeaderMap,
    jwt_secret: &str,
) -> Option<(Claims, SessionSource)> {
    if let Some(token) = extract_token_from_cookie(headers) {
        if let Some(claims) = rg_core::auth::jwt::validate_token(&token, jwt_secret) {
            return Some((claims, SessionSource::Cookie));
        }
    }
    extract_bearer_claims(headers, jwt_secret).map(|claims| (claims, SessionSource::Bearer))
}

/// Remove the auth cookie from the `Cookie` header, preserving other cookies.
fn strip_auth_cookie(headers: &mut HeaderMap) {
    let Some(raw) = headers
        .get(axum::http::header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
    else {
        return;
    };
    let prefix = format!("{}=", AUTH_COOKIE_NAME);
    let remaining: Vec<&str> = raw
        .split(';')
        .map(str::trim)
        .filter(|c| !c.starts_with(&prefix))
        .collect();
    if remaining.is_empty() {
        headers.remove(axum::http::header::COOKIE);
    } else if let Ok(joined) = remaining.join("; ").parse() {
        headers.insert(axum::http::header::COOKIE, joined);
    }
}

/// #5: Session revocation guard for the REST API.
///
/// Runs on every `/api/v1` request. When a signature-valid JWT is presented
/// whose `ver` claim no longer matches `users.token_version`, the credential
/// is **stripped from the request** (and the auth cookie cleared) instead of
/// short-circuiting with 401:
///
/// - Auth-free endpoints (login, register, public repo reads) keep working,
///   so a user with a revoked browser session can still re-authenticate.
/// - Every authenticated handler then sees "no credentials" and returns its
///   normal 401, which the frontend global handler routes to the login page.
///
/// Requests without a signature-valid JWT (anonymous, PAT, CI token) pass
/// through untouched — only live-session requests incur the primary-key
/// user lookup.
pub(crate) async fn session_guard_middleware(
    axum::extract::State(state): axum::extract::State<crate::AppState>,
    mut req: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let Some((claims, source)) = extract_session_claims(req.headers(), &state.jwt_secret) else {
        return next.run(req).await;
    };
    if validate_session(&state.db, &claims).await.is_some() {
        return next.run(req).await;
    }

    tracing::info!(user_id = %claims.sub, "revoked session token presented; stripping credentials");
    let mut clear_cookie = None;
    match source {
        SessionSource::Cookie => {
            strip_auth_cookie(req.headers_mut());
            let is_https = req
                .headers()
                .get("x-forwarded-proto")
                .and_then(|v| v.to_str().ok())
                .is_some_and(|v| v == "https");
            clear_cookie = Some(format!(
                "{}=; HttpOnly; Path=/; SameSite=Strict; Max-Age=0{}",
                AUTH_COOKIE_NAME,
                if is_https { "; Secure" } else { "" }
            ));
        }
        SessionSource::Bearer => {
            req.headers_mut().remove(axum::http::header::AUTHORIZATION);
        }
    }

    let mut response = next.run(req).await;
    if let Some(cookie) = clear_cookie {
        if let Ok(value) = axum::http::HeaderValue::from_str(&cookie) {
            response
                .headers_mut()
                .append(axum::http::header::SET_COOKIE, value);
        }
    }
    response
}
