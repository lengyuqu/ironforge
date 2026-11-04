//! Inbox endpoints: a live projection of what waits for a session or the user.

use axum::extract::{Query, State};
use axum::http::HeaderMap;
use axum::Json;
use chrono::{DateTime, Utc};
use rg_core::collaboration::{self, inbox as svc};
use serde::Deserialize;

use super::collab::{acting_session, require_collab_read, require_collab_write};
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
