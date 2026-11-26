//! Shared helpers of the same-account multi-agent collaboration REST API
//! (`collab_*` modules hold the endpoints).
//!
//! Design: `docs/design/agent-collaboration.md`.
//!
//! Authorization layering for every endpoint: (1) the account credential
//! (JWT/PAT), (2) the account's repository permission, re-evaluated on every
//! request, (3) for session-bound actions, the session id + secret headers.
//! A session is a label under the account, not an isolation boundary.

use axum::http::{HeaderMap, StatusCode};
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

/// Stable `reason` of the refusal an agent gets when it writes without a session.
pub(crate) const AGENT_MUST_USE_SESSION: &str = "AGENT_MUST_USE_SESSION";

/// Refuse `user_id` when it is an agent account, with a stable `reason`.
///
/// The "user door" speaks for a person; an agent that wants to write must go
/// through a collaboration session so the record names the session, not the
/// account. An unreadable or missing account is refused too: failing to see the
/// account type must not read as "a person".
pub(crate) async fn ensure_person(
    state: &AppState,
    user_id: i64,
    reason: &'static str,
    message: &'static str,
    advice: &'static str,
) -> Result<(), AppError> {
    let user = rg_db::ops::user_ops::find_by_id(&state.db, user_id)
        .await?
        .ok_or_else(|| AppError::forbidden("no such account"))?;
    if rg_core::agent::is_agent(&user) {
        return Err(AppError::coded(StatusCode::FORBIDDEN, "FORBIDDEN", message)
            .with_reason(reason, false, advice));
    }
    Ok(())
}

/// The actor of a write: the authenticated session when session headers are
/// sent, otherwise the signed-in account through its "user door" (a UI or CLI).
///
/// The user door serves people only: an agent account without session headers
/// is refused with [`AGENT_MUST_USE_SESSION`]. Reading is not gated here.
pub(crate) async fn write_actor(
    state: &AppState,
    headers: &HeaderMap,
    repo_id: i64,
    user_id: i64,
) -> Result<rg_core::collaboration::Actor, AppError> {
    if headers.contains_key(HEADER_SESSION) || headers.contains_key(HEADER_SESSION_SECRET) {
        let session = acting_session(state, headers, repo_id, user_id).await?;
        Ok(rg_core::collaboration::Actor::session(session.id))
    } else {
        ensure_person(
            state,
            user_id,
            AGENT_MUST_USE_SESSION,
            "an agent must write collaboration data through its own session",
            "register a collaboration session and send its id and secret in the session headers",
        )
        .await?;
        Ok(rg_core::collaboration::Actor::user(user_id))
    }
}

/// Reject a commit id that does not name a commit of the repository.
pub(crate) async fn ensure_commit(
    state: &AppState,
    owner: &str,
    name: &str,
    field: &str,
    sha: Option<&str>,
) -> Result<Option<String>, AppError> {
    let Some(sha) = sha else { return Ok(None) };
    let sha = rg_core::collaboration::message::normalize_commit_sha(sha)?;
    if !commit_exists(state, owner, name, &sha).await? {
        return Err(AppError::bad_request(format!(
            "{field} does not name a commit in this repository"
        )));
    }
    Ok(Some(sha))
}

/// Current head of `refs/heads/<branch>`, or `None` when it cannot be read.
pub(crate) async fn observed_branch_head(
    state: &AppState,
    owner: &str,
    name: &str,
    branch: Option<&str>,
) -> Option<String> {
    let branch = branch?.to_string();
    let path = state.repo_root.join(format!("{owner}/{name}.git"));
    tokio::task::spawn_blocking(move || {
        let repo = gix::open(&path).ok()?;
        let id = repo
            .rev_parse_single(format!("refs/heads/{branch}").as_str())
            .ok()?;
        Some(id.to_string())
    })
    .await
    .ok()
    .flatten()
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

/// Authenticated account that belongs to the repository's working group.
///
/// Public repositories are readable by anyone, but collaboration data (agent
/// conversations, questions, work packages) is not public content: it can carry
/// rationale and context its authors never meant for strangers. Readers are the
/// owner, explicit collaborators of any level, organization members and anyone
/// who can write — a plain signed-in account on a public repository is not
/// among them (decision of 2026-11-04, design open question §8.1).
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
    if is_collab_member(state, &repo, actor_id).await? {
        Ok((repo, actor_id))
    } else {
        Err(AppError::forbidden("access denied"))
    }
}

pub(crate) async fn is_collab_member(
    state: &AppState,
    repo: &rg_db::entities::repository::Model,
    actor_id: i64,
) -> Result<bool, AppError> {
    Ok(rg_core::collaboration::access::is_collab_member(&state.db, repo, actor_id).await?)
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
#[into_params(parameter_in = Query)]
pub struct ListQuery {
    pub limit: Option<u64>,
    pub cursor: Option<String>,
}
