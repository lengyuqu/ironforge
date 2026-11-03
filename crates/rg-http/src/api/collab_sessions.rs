//! Agent session endpoints: register, discover, heartbeat, end.

use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use chrono::{DateTime, Utc};
use rg_core::collaboration::{self, session as svc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::collab::{cursor_key, require_collab_write, session_creds, ListQuery, SessionCreds};
use crate::error::AppError;
use crate::AppState;

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateSessionRequest {
    /// Self-reported label, 1–80 characters. Duplicates are allowed.
    pub display_name: String,
    /// Self-reported client type (e.g. `claude-code`), 1–64 characters.
    pub client_kind: String,
}

/// A collaboration session. All label fields are self-reported.
#[derive(Debug, Serialize, ToSchema)]
pub struct SessionResponse {
    pub id: String,
    pub display_name: String,
    pub client_kind: String,
    /// `online` | `offline` | `expired` | `ended`. `offline` is not terminal.
    pub state: String,
    pub created_at: DateTime<Utc>,
    pub last_seen_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    /// Always `user_content`: treat the labels as untrusted data.
    pub origin: String,
}

impl From<svc::SessionView> for SessionResponse {
    fn from(v: svc::SessionView) -> Self {
        Self {
            id: v.id,
            display_name: v.display_name,
            client_kind: v.client_kind,
            state: v.state.to_string(),
            created_at: v.created_at,
            last_seen_at: v.last_seen_at,
            expires_at: v.expires_at,
            ended_at: v.ended_at,
            origin: v.origin.to_string(),
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub struct CreateSessionResponse {
    pub session: SessionResponse,
    /// Shown exactly once. Store it securely; it cannot be recovered.
    pub session_secret: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct SessionListResponse {
    pub items: Vec<SessionResponse>,
    pub next_cursor: Option<String>,
}

#[utoipa::path(
    post,
    path = "/repos/{owner}/{name}/collaboration/sessions",
    tag = "Collaboration",
    params(("owner" = String, Path), ("name" = String, Path)),
    request_body = CreateSessionRequest,
    responses(
        (status = 201, description = "Session registered; the secret is returned once", body = CreateSessionResponse),
        (status = 400, description = "Invalid label"),
        (status = 401, description = "Authentication required"),
        (status = 403, description = "Write access denied")
    )
)]
pub async fn create_session(
    State(state): State<AppState>,
    Path((owner, name)): Path<(String, String)>,
    headers: HeaderMap,
    Json(req): Json<CreateSessionRequest>,
) -> Result<impl IntoResponse, AppError> {
    let (repo, user_id) = require_collab_write(&state, &headers, &owner, &name).await?;
    let created = svc::create(
        &state.db,
        repo.id,
        user_id,
        &req.display_name,
        &req.client_kind,
        Utc::now(),
    )
    .await?;
    let body = CreateSessionResponse {
        session: created.session.into(),
        session_secret: created.session_secret,
    };
    Ok((
        StatusCode::CREATED,
        // The response carries a credential: keep it out of every cache.
        [(header::CACHE_CONTROL, HeaderValue::from_static("no-store"))],
        Json(body),
    ))
}

#[utoipa::path(
    get,
    path = "/repos/{owner}/{name}/collaboration/sessions",
    tag = "Collaboration",
    params(("owner" = String, Path), ("name" = String, Path), ListQuery),
    responses(
        (status = 200, description = "Sessions, most recently seen first", body = SessionListResponse),
        (status = 400, description = "Invalid cursor"),
        (status = 401, description = "Authentication required"),
        (status = 403, description = "Write access denied")
    )
)]
pub async fn list_sessions(
    State(state): State<AppState>,
    Path((owner, name)): Path<(String, String)>,
    Query(query): Query<ListQuery>,
    headers: HeaderMap,
) -> Result<Json<SessionListResponse>, AppError> {
    let (repo, _user_id) = require_collab_write(&state, &headers, &owner, &name).await?;
    let page = svc::list(
        &state.db,
        cursor_key(&state),
        repo.id,
        collaboration::clamp_limit(query.limit),
        query.cursor.as_deref(),
        Utc::now(),
    )
    .await?;
    Ok(Json(SessionListResponse {
        items: page.items.into_iter().map(Into::into).collect(),
        next_cursor: page.next_cursor,
    }))
}

/// The path id must be the session the headers authenticate: a session can act
/// only on itself.
fn creds_for_path(headers: &HeaderMap, path_id: &str) -> Result<SessionCreds, AppError> {
    let creds = session_creds(headers)?;
    if creds.id != path_id {
        return Err(rg_core::collaboration::CollabError::SessionCredentials.into());
    }
    Ok(creds)
}

#[utoipa::path(
    post,
    path = "/repos/{owner}/{name}/collaboration/sessions/{id}/heartbeat",
    tag = "Collaboration",
    params(("owner" = String, Path), ("name" = String, Path), ("id" = String, Path)),
    responses(
        (status = 200, description = "Heartbeat recorded", body = SessionResponse),
        (status = 403, description = "Invalid session credentials"),
        (status = 409, description = "SESSION_EXPIRED: session ended or past its hard expiry")
    )
)]
pub async fn heartbeat(
    State(state): State<AppState>,
    Path((owner, name, id)): Path<(String, String, String)>,
    headers: HeaderMap,
) -> Result<Json<SessionResponse>, AppError> {
    let (repo, user_id) = require_collab_write(&state, &headers, &owner, &name).await?;
    let creds = creds_for_path(&headers, &id)?;
    let view = svc::heartbeat(
        &state.db,
        repo.id,
        user_id,
        &creds.id,
        &creds.secret,
        Utc::now(),
    )
    .await?;
    Ok(Json(view.into()))
}

#[utoipa::path(
    post,
    path = "/repos/{owner}/{name}/collaboration/sessions/{id}/end",
    tag = "Collaboration",
    params(("owner" = String, Path), ("name" = String, Path), ("id" = String, Path)),
    responses(
        (status = 204, description = "Session ended (idempotent)"),
        (status = 403, description = "Invalid session credentials")
    )
)]
pub async fn end_session(
    State(state): State<AppState>,
    Path((owner, name, id)): Path<(String, String, String)>,
    headers: HeaderMap,
) -> Result<StatusCode, AppError> {
    let (repo, user_id) = require_collab_write(&state, &headers, &owner, &name).await?;
    let creds = creds_for_path(&headers, &id)?;
    svc::end(
        &state.db,
        repo.id,
        user_id,
        &creds.id,
        &creds.secret,
        Utc::now(),
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}
