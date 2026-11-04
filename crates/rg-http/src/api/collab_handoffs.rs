//! Collaboration handoff endpoints.

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use chrono::{DateTime, Utc};
use rg_core::collaboration::{self, handoff as svc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::collab::{
    acting_session, commit_exists, cursor_key, require_collab_read, require_collab_write, ListQuery,
};
use super::collab_json::CollabJson;
use crate::error::AppError;
use crate::AppState;

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateHandoffRequest {
    /// Client-chosen idempotency key, unique per session.
    pub client_handoff_id: String,
    /// Another session of this repository; it may be offline or expired.
    pub to_session_id: String,
    /// 1 byte to 16 KiB.
    pub summary: String,
    /// Up to 16 KiB.
    #[serde(default)]
    pub completed: String,
    /// Up to 16 KiB.
    #[serde(default)]
    pub next_steps: String,
    /// Up to 16 KiB.
    #[serde(default)]
    pub blockers: String,
    /// Full commit id that exists in this repository (existence only).
    pub commit_sha: Option<String>,
    /// Up to 20 URIs or ids, each a single line of at most 2048 bytes. Claims,
    /// not verification; readers must check them themselves.
    #[serde(default)]
    pub evidence: Vec<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct TransitionRequest {
    /// The handoff `version` the caller last saw.
    pub expected_version: i64,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct HandoffResponse {
    pub id: String,
    pub task_id: String,
    pub from_session_id: String,
    pub to_session_id: String,
    pub summary: String,
    pub completed: String,
    pub next_steps: String,
    pub blockers: String,
    pub commit_sha: Option<String>,
    pub evidence: Vec<String>,
    pub client_handoff_id: String,
    /// `pending` | `accepted` | `cancelled`.
    pub status: String,
    pub version: i64,
    pub created_at: DateTime<Utc>,
    pub accepted_at: Option<DateTime<Utc>>,
    /// Always `user_content`: every text section and evidence entry is a claim.
    pub origin: String,
}

impl From<svc::HandoffView> for HandoffResponse {
    fn from(v: svc::HandoffView) -> Self {
        Self {
            id: v.id,
            task_id: v.task_id,
            from_session_id: v.from_session_id,
            to_session_id: v.to_session_id,
            summary: v.summary,
            completed: v.completed,
            next_steps: v.next_steps,
            blockers: v.blockers,
            commit_sha: v.commit_sha,
            evidence: v.evidence,
            client_handoff_id: v.client_handoff_id,
            status: v.status,
            version: v.version,
            created_at: v.created_at,
            accepted_at: v.accepted_at,
            origin: v.origin.to_string(),
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub struct HandoffListResponse {
    pub items: Vec<HandoffResponse>,
    pub next_cursor: Option<String>,
}

#[utoipa::path(
    post,
    path = "/repos/{owner}/{name}/collaboration/tasks/{id}/handoffs",
    tag = "Collaboration",
    params(("owner" = String, Path), ("name" = String, Path), ("id" = String, Path)),
    request_body = CreateHandoffRequest,
    responses(
        (status = 201, description = "Handoff created (pending)", body = HandoffResponse),
        (status = 200, description = "Identical earlier request replayed", body = HandoffResponse),
        (status = 400, description = "Invalid field, target or commit"),
        (status = 403, description = "Write access denied or invalid session credentials"),
        (status = 404, description = "No such task in this repository"),
        (status = 409, description = "IDEMPOTENCY_KEY_REUSED or SESSION_EXPIRED")
    )
)]
pub async fn create_handoff(
    State(state): State<AppState>,
    Path((owner, name, id)): Path<(String, String, String)>,
    headers: HeaderMap,
    CollabJson(req): CollabJson<CreateHandoffRequest>,
) -> Result<(StatusCode, Json<HandoffResponse>), AppError> {
    let (repo, user_id) = require_collab_write(&state, &headers, &owner, &name).await?;
    let from = acting_session(&state, &headers, repo.id, user_id).await?;

    let commit_sha = match &req.commit_sha {
        Some(sha) => {
            let sha = collaboration::message::normalize_commit_sha(sha)?;
            if !commit_exists(&state, &owner, &name, &sha).await? {
                return Err(AppError::bad_request(
                    "commit_sha does not name a commit in this repository",
                ));
            }
            Some(sha)
        }
        None => None,
    };

    let created = svc::create(
        &state.db,
        repo.id,
        &id,
        &from,
        svc::NewHandoff {
            client_handoff_id: req.client_handoff_id,
            to_session_id: req.to_session_id,
            summary: req.summary,
            completed: req.completed,
            next_steps: req.next_steps,
            blockers: req.blockers,
            commit_sha,
            evidence: req.evidence,
        },
        Utc::now(),
    )
    .await?;
    Ok(match created {
        svc::Created::New(h) => (StatusCode::CREATED, Json(h.into())),
        svc::Created::Replayed(h) => (StatusCode::OK, Json(h.into())),
    })
}

#[utoipa::path(
    get,
    path = "/repos/{owner}/{name}/collaboration/tasks/{id}/handoffs",
    tag = "Collaboration",
    params(("owner" = String, Path), ("name" = String, Path), ("id" = String, Path), ListQuery),
    responses(
        (status = 200, description = "Handoffs by creation time", body = HandoffListResponse),
        (status = 400, description = "Invalid cursor"),
        (status = 403, description = "Read access denied"),
        (status = 404, description = "No such task in this repository")
    )
)]
pub async fn list_handoffs(
    State(state): State<AppState>,
    Path((owner, name, id)): Path<(String, String, String)>,
    Query(query): Query<ListQuery>,
    headers: HeaderMap,
) -> Result<Json<HandoffListResponse>, AppError> {
    let (repo, _user) = require_collab_read(&state, &headers, &owner, &name).await?;
    let page = svc::list(
        &state.db,
        cursor_key(&state),
        repo.id,
        &id,
        collaboration::clamp_limit(query.limit),
        query.cursor.as_deref(),
    )
    .await?;
    Ok(Json(HandoffListResponse {
        items: page.items.into_iter().map(Into::into).collect(),
        next_cursor: page.next_cursor,
    }))
}

#[utoipa::path(
    post,
    path = "/repos/{owner}/{name}/collaboration/handoffs/{id}/accept",
    tag = "Collaboration",
    params(("owner" = String, Path), ("name" = String, Path), ("id" = String, Path)),
    request_body = TransitionRequest,
    responses(
        (status = 200, description = "Handoff accepted", body = HandoffResponse),
        (status = 403, description = "Not the addressed session, or invalid session credentials"),
        (status = 404, description = "No such handoff in this repository"),
        (status = 409, description = "Already accepted/cancelled (CONFLICT), VERSION_CONFLICT or SESSION_EXPIRED")
    )
)]
pub async fn accept_handoff(
    State(state): State<AppState>,
    Path((owner, name, id)): Path<(String, String, String)>,
    headers: HeaderMap,
    CollabJson(req): CollabJson<TransitionRequest>,
) -> Result<Json<HandoffResponse>, AppError> {
    let (repo, user_id) = require_collab_write(&state, &headers, &owner, &name).await?;
    let actor = acting_session(&state, &headers, repo.id, user_id).await?;
    let handoff = svc::accept(
        &state.db,
        repo.id,
        &id,
        &actor,
        req.expected_version,
        Utc::now(),
    )
    .await?;
    Ok(Json(handoff.into()))
}

#[utoipa::path(
    post,
    path = "/repos/{owner}/{name}/collaboration/handoffs/{id}/cancel",
    tag = "Collaboration",
    params(("owner" = String, Path), ("name" = String, Path), ("id" = String, Path)),
    request_body = TransitionRequest,
    responses(
        (status = 200, description = "Handoff cancelled", body = HandoffResponse),
        (status = 403, description = "Not the creating session, or invalid session credentials"),
        (status = 404, description = "No such handoff in this repository"),
        (status = 409, description = "Already accepted/cancelled (CONFLICT), VERSION_CONFLICT or SESSION_EXPIRED")
    )
)]
pub async fn cancel_handoff(
    State(state): State<AppState>,
    Path((owner, name, id)): Path<(String, String, String)>,
    headers: HeaderMap,
    CollabJson(req): CollabJson<TransitionRequest>,
) -> Result<Json<HandoffResponse>, AppError> {
    let (repo, user_id) = require_collab_write(&state, &headers, &owner, &name).await?;
    let actor = acting_session(&state, &headers, repo.id, user_id).await?;
    let handoff = svc::cancel(
        &state.db,
        repo.id,
        &id,
        &actor,
        req.expected_version,
        Utc::now(),
    )
    .await?;
    Ok(Json(handoff.into()))
}
