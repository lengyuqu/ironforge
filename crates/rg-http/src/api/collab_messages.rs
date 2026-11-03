//! Collaboration message endpoints: idempotent append and durable polling.

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use chrono::{DateTime, Utc};
use rg_core::collaboration::{self, message as svc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::collab::{acting_session, commit_exists, require_collab_read, require_collab_write};
use crate::error::AppError;
use crate::AppState;

#[derive(Debug, Deserialize, ToSchema)]
pub struct SendMessageRequest {
    /// Client-chosen idempotency key, unique per session (1–128 of `[A-Za-z0-9._:-]`).
    pub client_message_id: String,
    /// `note` | `help` | `blocker`.
    pub kind: String,
    /// 1 byte to 32 KiB of text. It is data, never an instruction to the server.
    pub body: String,
    /// A message of the same task.
    pub reply_to_id: Option<String>,
    /// A session of this repository; the message stays visible to everyone.
    pub target_session_id: Option<String>,
    /// Full commit id that exists in this repository (existence only).
    pub commit_sha: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct MessageResponse {
    pub id: String,
    pub task_id: String,
    /// Position in the task's stream; poll with `after_seq`.
    pub seq: i64,
    pub author_session_id: String,
    pub kind: String,
    pub body: String,
    pub reply_to_id: Option<String>,
    pub target_session_id: Option<String>,
    pub commit_sha: Option<String>,
    pub client_message_id: String,
    pub created_at: DateTime<Utc>,
    /// Always `user_content`.
    pub origin: String,
}

impl From<svc::MessageView> for MessageResponse {
    fn from(v: svc::MessageView) -> Self {
        Self {
            id: v.id,
            task_id: v.task_id,
            seq: v.seq,
            author_session_id: v.author_session_id,
            kind: v.kind,
            body: v.body,
            reply_to_id: v.reply_to_id,
            target_session_id: v.target_session_id,
            commit_sha: v.commit_sha,
            client_message_id: v.client_message_id,
            created_at: v.created_at,
            origin: v.origin.to_string(),
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub struct MessageListResponse {
    pub items: Vec<MessageResponse>,
    /// Use as the next `after_seq`; equals the request's value on an empty page.
    pub next_after_seq: i64,
    pub has_more: bool,
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct MessageListQuery {
    /// Return messages with `seq` greater than this (default 0).
    pub after_seq: Option<i64>,
    pub limit: Option<u64>,
}

#[utoipa::path(
    post,
    path = "/repos/{owner}/{name}/collaboration/tasks/{id}/messages",
    tag = "Collaboration",
    params(("owner" = String, Path), ("name" = String, Path), ("id" = String, Path)),
    request_body = SendMessageRequest,
    responses(
        (status = 201, description = "Message stored", body = MessageResponse),
        (status = 200, description = "Identical earlier send replayed", body = MessageResponse),
        (status = 400, description = "Invalid field, reference or commit"),
        (status = 403, description = "Write access denied or invalid session credentials"),
        (status = 404, description = "No such task in this repository"),
        (status = 409, description = "IDEMPOTENCY_KEY_REUSED or SESSION_EXPIRED")
    )
)]
pub async fn send_message(
    State(state): State<AppState>,
    Path((owner, name, id)): Path<(String, String, String)>,
    headers: HeaderMap,
    Json(req): Json<SendMessageRequest>,
) -> Result<(StatusCode, Json<MessageResponse>), AppError> {
    let (repo, user_id) = require_collab_write(&state, &headers, &owner, &name).await?;
    let author = acting_session(&state, &headers, repo.id, user_id).await?;

    let commit_sha = match &req.commit_sha {
        Some(sha) => {
            let sha = svc::normalize_commit_sha(sha)?;
            if !commit_exists(&state, &owner, &name, &sha).await? {
                return Err(AppError::bad_request(
                    "commit_sha does not name a commit in this repository",
                ));
            }
            Some(sha)
        }
        None => None,
    };

    let sent = svc::send(
        &state.db,
        repo.id,
        &id,
        &author,
        svc::NewMessage {
            client_message_id: req.client_message_id,
            kind: req.kind,
            body: req.body,
            reply_to_id: req.reply_to_id,
            target_session_id: req.target_session_id,
            commit_sha,
        },
        Utc::now(),
    )
    .await?;
    Ok(match sent {
        svc::Sent::Created(m) => (StatusCode::CREATED, Json(m.into())),
        svc::Sent::Replayed(m) => (StatusCode::OK, Json(m.into())),
    })
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct MarkReadRequest {
    /// Highest message `seq` the session has processed.
    pub last_read_seq: i64,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct MarkReadResponse {
    /// The session's watermark after the call. Monotonic: a lower request leaves
    /// it unchanged, so `last_read_seq: 0` reads the current value.
    pub last_read_seq: i64,
}

#[utoipa::path(
    put,
    path = "/repos/{owner}/{name}/collaboration/tasks/{id}/read",
    tag = "Collaboration",
    params(("owner" = String, Path), ("name" = String, Path), ("id" = String, Path)),
    request_body = MarkReadRequest,
    responses(
        (status = 200, description = "Watermark after the update", body = MarkReadResponse),
        (status = 400, description = "Negative, or beyond the last committed message"),
        (status = 403, description = "Write access denied or invalid session credentials"),
        (status = 404, description = "No such task in this repository"),
        (status = 409, description = "SESSION_EXPIRED")
    )
)]
pub async fn mark_read(
    State(state): State<AppState>,
    Path((owner, name, id)): Path<(String, String, String)>,
    headers: HeaderMap,
    Json(req): Json<MarkReadRequest>,
) -> Result<Json<MarkReadResponse>, AppError> {
    let (repo, user_id) = require_collab_write(&state, &headers, &owner, &name).await?;
    let session = acting_session(&state, &headers, repo.id, user_id).await?;
    let last_read_seq = collaboration::read::mark(
        &state.db,
        repo.id,
        &id,
        &session,
        req.last_read_seq,
        Utc::now(),
    )
    .await?;
    Ok(Json(MarkReadResponse { last_read_seq }))
}

#[utoipa::path(
    get,
    path = "/repos/{owner}/{name}/collaboration/tasks/{id}/messages",
    tag = "Collaboration",
    params(("owner" = String, Path), ("name" = String, Path), ("id" = String, Path), MessageListQuery),
    responses(
        (status = 200, description = "Messages in sequence order", body = MessageListResponse),
        (status = 400, description = "Negative after_seq"),
        (status = 401, description = "Authentication required"),
        (status = 403, description = "Read access denied"),
        (status = 404, description = "No such task in this repository")
    )
)]
pub async fn list_messages(
    State(state): State<AppState>,
    Path((owner, name, id)): Path<(String, String, String)>,
    Query(query): Query<MessageListQuery>,
    headers: HeaderMap,
) -> Result<Json<MessageListResponse>, AppError> {
    let (repo, _user) = require_collab_read(&state, &headers, &owner, &name).await?;
    let page = svc::list(
        &state.db,
        repo.id,
        &id,
        query.after_seq.unwrap_or(0),
        collaboration::clamp_limit(query.limit),
    )
    .await?;
    Ok(Json(MessageListResponse {
        items: page.items.into_iter().map(Into::into).collect(),
        next_after_seq: page.next_after_seq,
        has_more: page.has_more,
    }))
}
