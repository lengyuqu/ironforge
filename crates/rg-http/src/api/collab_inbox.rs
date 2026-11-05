//! Inbox endpoints: a live projection of what waits for a session or the user.

use axum::extract::{Query, State};
use axum::http::HeaderMap;
use axum::Json;
use chrono::{DateTime, Utc};
use rg_core::collaboration::{self, inbox as svc, Actor};
use serde::Deserialize;
use utoipa::ToSchema;

use super::collab::{acting_session, require_collab_read, require_collab_write};
use super::collab_json::CollabJson;
use crate::error::AppError;
use crate::AppState;

#[derive(Debug, Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct InboxQuery {
    /// Only items that became relevant after this RFC 3339 time.
    pub since: Option<String>,
    pub limit: Option<u64>,
}

fn parse_since(since: Option<&str>) -> Result<Option<DateTime<Utc>>, AppError> {
    since
        .map(|s| {
            DateTime::parse_from_rfc3339(s)
                .map(|t| t.with_timezone(&Utc))
                .map_err(|_| AppError::bad_request("since must be an RFC 3339 timestamp"))
        })
        .transpose()
}

#[utoipa::path(
    get,
    path = "/repos/{owner}/{name}/collaboration/inbox",
    tag = "Collaboration",
    params(("owner" = String, Path), ("name" = String, Path), InboxQuery),
    responses(
        (status = 200, description = "Items waiting for the acting session: {items, as_of, truncated, consistency}", body = serde_json::Value),
        (status = 400, description = "Invalid since"),
        (status = 403, description = "Read access denied or invalid session credentials"),
        (status = 409, description = "SESSION_EXPIRED")
    )
)]
pub async fn session_inbox(
    State(state): State<AppState>,
    axum::extract::Path((owner, name)): axum::extract::Path<(String, String)>,
    Query(query): Query<InboxQuery>,
    headers: HeaderMap,
) -> Result<Json<svc::Inbox>, AppError> {
    let (repo, user_id) = require_collab_read(&state, &headers, &owner, &name).await?;
    let since = parse_since(query.since.as_deref())?;
    let session = acting_session(&state, &headers, repo.id, user_id).await?;
    super::collab_escalation::evaluate_lazily(&state, &repo, &session).await;
    let inbox = svc::for_session(
        &state.db,
        repo.id,
        &session.id,
        since,
        collaboration::clamp_limit(query.limit),
        Utc::now(),
    )
    .await?;
    Ok(Json(inbox))
}

#[utoipa::path(
    get,
    path = "/repos/{owner}/{name}/collaboration/inbox/user",
    tag = "Collaboration",
    params(("owner" = String, Path), ("name" = String, Path), InboxQuery),
    responses(
        (status = 200, description = "Questions waiting for the signed-in account and answers to its own questions", body = serde_json::Value),
        (status = 400, description = "Invalid since"),
        (status = 403, description = "Write access denied")
    )
)]
pub async fn user_inbox(
    State(state): State<AppState>,
    axum::extract::Path((owner, name)): axum::extract::Path<(String, String)>,
    Query(query): Query<InboxQuery>,
    headers: HeaderMap,
) -> Result<Json<svc::Inbox>, AppError> {
    // Answering needs write access, so only writers have anything to find here.
    let (repo, user_id) = require_collab_write(&state, &headers, &owner, &name).await?;
    let since = parse_since(query.since.as_deref())?;
    let inbox = svc::for_user(
        &state.db,
        repo.id,
        user_id,
        since,
        collaboration::clamp_limit(query.limit),
        Utc::now(),
    )
    .await?;
    Ok(Json(inbox))
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct InboxMarkReadRequest {
    /// `item_key` of an inbox item, e.g. `open_question:<id>`.
    pub item_key: String,
    /// The `object_version` that was read; not newer than the record's current one.
    pub observed_version: i64,
}

#[utoipa::path(
    put,
    path = "/repos/{owner}/{name}/collaboration/inbox/read",
    tag = "Collaboration",
    params(("owner" = String, Path), ("name" = String, Path)),
    request_body = InboxMarkReadRequest,
    responses(
        (status = 200, description = "{item_key, observed_version, current_version, read_state, read_at}. Monotonic and idempotent; a newer version of the item stays unread. Changes only this session's read mark, never the record", body = serde_json::Value),
        (status = 400, description = "observed_version below 1 or newer than the record's current version"),
        (status = 403, description = "Read access denied or invalid session credentials"),
        (status = 404, description = "The item is not in this session's inbox"),
        (status = 409, description = "SESSION_EXPIRED")
    )
)]
pub async fn mark_session_read(
    State(state): State<AppState>,
    axum::extract::Path((owner, name)): axum::extract::Path<(String, String)>,
    headers: HeaderMap,
    CollabJson(req): CollabJson<InboxMarkReadRequest>,
) -> Result<Json<svc::ReadMark>, AppError> {
    let (repo, user_id) = require_collab_read(&state, &headers, &owner, &name).await?;
    let session = acting_session(&state, &headers, repo.id, user_id).await?;
    let mark = svc::mark_read(
        &state.db,
        repo.id,
        &Actor::session(session.id),
        &req.item_key,
        req.observed_version,
        Utc::now(),
    )
    .await?;
    Ok(Json(mark))
}

#[utoipa::path(
    put,
    path = "/repos/{owner}/{name}/collaboration/inbox/user/read",
    tag = "Collaboration",
    params(("owner" = String, Path), ("name" = String, Path)),
    request_body = InboxMarkReadRequest,
    responses(
        (status = 200, description = "Same shape as the session door; the mark belongs to the signed-in account and is not shared with any session", body = serde_json::Value),
        (status = 400, description = "observed_version below 1 or newer than the record's current version"),
        (status = 403, description = "Write access denied"),
        (status = 404, description = "The item is not in this account's inbox")
    )
)]
pub async fn mark_user_read(
    State(state): State<AppState>,
    axum::extract::Path((owner, name)): axum::extract::Path<(String, String)>,
    headers: HeaderMap,
    CollabJson(req): CollabJson<InboxMarkReadRequest>,
) -> Result<Json<svc::ReadMark>, AppError> {
    let (repo, user_id) = require_collab_write(&state, &headers, &owner, &name).await?;
    let mark = svc::mark_read(
        &state.db,
        repo.id,
        &Actor::user(user_id),
        &req.item_key,
        req.observed_version,
        Utc::now(),
    )
    .await?;
    Ok(Json(mark))
}
