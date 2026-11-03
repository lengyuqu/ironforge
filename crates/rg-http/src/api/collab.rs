//! Shared helpers of the same-account multi-agent collaboration REST API
//! (`collab_*` modules hold the endpoints).
//!
//! Design: `ironforge-docs/architecture/multi-agent-collaboration-design-2026-11.md`.
//!
//! Authorization layering for every endpoint: (1) the account credential
//! (JWT/PAT), (2) the account's repository permission, re-evaluated on every
//! request, (3) for session-bound actions, the session id + secret headers.
//! A session is a label under the account, not an isolation boundary.

use axum::http::HeaderMap;
use serde::Deserialize;

use crate::api::repo_access::resolve_repo;
use crate::error::AppError;
use crate::AppState;

/// Header naming the acting session.
pub const HEADER_SESSION: &str = "x-ironforge-session";
/// Header carrying the session secret. Never put it in a URL, log line or body.
pub const HEADER_SESSION_SECRET: &str = "x-ironforge-session-secret";

/// The session credential pair presented on a request.
pub(crate) struct SessionCreds {
    pub id: String,
    pub secret: String,
}

pub(crate) fn session_creds(headers: &HeaderMap) -> Result<SessionCreds, AppError> {
    let read = |name: &str| {
        headers
            .get(name)
            .and_then(|value| value.to_str().ok())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    };
    match (read(HEADER_SESSION), read(HEADER_SESSION_SECRET)) {
        (Some(id), Some(secret)) => Ok(SessionCreds { id, secret }),
        _ => Err(AppError::coded(
            axum::http::StatusCode::BAD_REQUEST,
            "SESSION_CREDENTIALS_REQUIRED",
            "X-Ironforge-Session and X-Ironforge-Session-Secret headers are required",
        )),
    }
}

/// Authenticate the acting session from the request headers: valid id and
/// secret, owned by `user_id`, in this repository, and still live.
pub(crate) async fn acting_session(
    state: &AppState,
    headers: &HeaderMap,
    repo_id: i64,
    user_id: i64,
) -> Result<rg_db::entities::agent_session::Model, AppError> {
    let creds = session_creds(headers)?;
    Ok(rg_core::collaboration::session::authenticate(
        &state.db,
        repo_id,
        user_id,
        &creds.id,
        &creds.secret,
        chrono::Utc::now(),
    )
    .await?)
}

/// Whether `sha` names a commit object in the repository on disk.
///
/// Only existence is checked; this says nothing about the commit being on any
/// branch, reviewed, or passing CI.
pub(crate) async fn commit_exists(
    state: &AppState,
    owner: &str,
    name: &str,
    sha: &str,
) -> Result<bool, AppError> {
    let path = state.repo_root.join(format!("{owner}/{name}.git"));
    let sha = sha.to_string();
    tokio::task::spawn_blocking(move || -> Result<bool, String> {
        let repo = gix::open(&path).map_err(|e| format!("open repository: {e}"))?;
        let Ok(id) = gix::ObjectId::from_hex(sha.as_bytes()) else {
            return Ok(false);
        };
        Ok(repo
            .find_object(id)
            .is_ok_and(|object| object.kind == gix::object::Kind::Commit))
    })
    .await
    .map_err(|e| AppError::internal(format!("commit lookup task failed: {e}")))?
    .map_err(AppError::internal)
}

/// Deserialize a field so that "absent" (`None`) and "explicit null"
/// (`Some(None)`) differ — the PATCH contract for clearing optional fields.
pub(crate) fn present_or_null<'de, D, T>(d: D) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::Deserialize<'de>,
{
    Option::<T>::deserialize(d).map(Some)
}

/// Key for cursor integrity tags.
pub(crate) fn cursor_key(state: &AppState) -> &[u8] {
    state.jwt_secret.as_bytes()
}

/// Authenticated account with read access to the repository.
///
/// Public repositories are readable by anyone, but collaboration data is not
/// public content: an account credential is required (design §2). Whether
/// readers beyond writers should see it is an open product decision (§8.1);
/// this helper is the single place to tighten it.
pub(crate) async fn require_collab_read(
    state: &AppState,
    headers: &HeaderMap,
    owner: &str,
    name: &str,
) -> Result<(rg_db::entities::repository::Model, i64), AppError> {
    let actor_id = crate::api::auth::extract_user_id(headers, &state.jwt_secret)
        .ok_or_else(|| AppError::unauthorized("authentication required"))?;
    let repo = resolve_repo(state, owner, name).await?;
    // Decide from the database, not the 30s process-local permission cache, so a
    // revocation blocks the very next collaboration request on this instance.
    rg_core::repo::service::invalidate_perm_cache_user(repo.id, actor_id);
    match rg_core::repo::service::can_read_repo(&state.db, &repo, Some(actor_id)).await {
        Ok(true) => Ok((repo, actor_id)),
        Ok(false) => Err(AppError::forbidden("access denied")),
        Err(e) => Err(AppError::from(e)),
    }
}

/// Authenticated account with write access to the repository (see
/// [`require_collab_read`] for why the permission cache is bypassed).
pub(crate) async fn require_collab_write(
    state: &AppState,
    headers: &HeaderMap,
    owner: &str,
    name: &str,
) -> Result<(rg_db::entities::repository::Model, i64), AppError> {
    let actor_id = crate::api::auth::extract_user_id(headers, &state.jwt_secret)
        .ok_or_else(|| AppError::unauthorized("authentication required"))?;
    let repo = resolve_repo(state, owner, name).await?;
    rg_core::repo::service::invalidate_perm_cache_user(repo.id, actor_id);
    match rg_core::repo::service::can_write_repo(&state.db, &repo, Some(actor_id)).await {
        Ok(true) => Ok((repo, actor_id)),
        Ok(false) => Err(AppError::forbidden("write access denied")),
        Err(e) => Err(AppError::from(e)),
    }
}

/// Query parameters shared by cursor-paginated list endpoints.
#[derive(Debug, serde::Deserialize, utoipa::IntoParams)]
pub struct ListQuery {
    pub limit: Option<u64>,
    pub cursor: Option<String>,
}
