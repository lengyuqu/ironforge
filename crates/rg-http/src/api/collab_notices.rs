//! Durable per-session notices and delivery receipts of targeted messages.
//!
//! Notices are hints: re-read the object they point at before acting.
//! Acknowledging one changes only the notice. A receipt is the target
//! session's own claim that it read a message; it moves no read watermark.

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use chrono::{DateTime, Utc};
use rg_core::collaboration::{self, delivery, notice as svc};
use serde::{Deserialize, Serialize};

use super::collab::{acting_session, cursor_key, require_collab_read, require_collab_write};
use crate::error::AppError;
use crate::AppState;

#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct NoticeQuery {
    pub limit: Option<u64>,
    pub cursor: Option<String>,
    /// `pending` (default: not acknowledged, not resolved) or `all`.
    pub state: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct NoticeList {
    pub items: Vec<svc::NoticeView>,
    /// Pass back as `cursor`; absent on the last page of this pass.
    pub next_cursor: Option<String>,
    pub as_of: DateTime<Utc>,
    /// Notices are hints; re-read the object they point at.
    pub authority: &'static str,
}

#[utoipa::path(
    get,
    path = "/repos/{owner}/{name}/collaboration/notices",
    tag = "Collaboration",
    params(("owner" = String, Path), ("name" = String, Path), NoticeQuery),
    responses(
        (status = 200, description = "The acting session's notices, oldest first: {items, next_cursor, as_of, authority}", body = serde_json::Value),
        (status = 400, description = "Invalid state or cursor"),
        (status = 403, description = "Read access denied or invalid session credentials"),
        (status = 409, description = "SESSION_EXPIRED")
    )
)]
pub async fn list_notices(
    State(state): State<AppState>,
    Path((owner, name)): Path<(String, String)>,
    Query(query): Query<NoticeQuery>,
    headers: HeaderMap,
) -> Result<Json<NoticeList>, AppError> {
    let (repo, user_id) = require_collab_read(&state, &headers, &owner, &name).await?;
    let session = acting_session(&state, &headers, repo.id, user_id).await?;
    let filter = match query.state.as_deref() {
        None | Some("pending") => svc::StateFilter::Pending,
        Some("all") => svc::StateFilter::All,
        Some(_) => return Err(AppError::bad_request("state must be pending or all")),
    };
    super::collab_escalation::evaluate_lazily(&state, &repo, &session).await;
    let now = Utc::now();
    let page = svc::list(
        &state.db,
        cursor_key(&state),
        repo.id,
        &session,
        filter,
        collaboration::clamp_limit(query.limit),
        query.cursor.as_deref(),
    )
    .await?;
    Ok(Json(NoticeList {
        items: page.items,
        next_cursor: page.next_cursor,
        as_of: now,
        authority: svc::AUTHORITY,
    }))
}

#[utoipa::path(
    post,
    path = "/repos/{owner}/{name}/collaboration/notices/{id}/ack",
    tag = "Collaboration",
    params(("owner" = String, Path), ("name" = String, Path), ("id" = String, Path)),
    responses(
        (status = 200, description = "{notice, newly_acked}; repeating is harmless", body = serde_json::Value),
        (status = 403, description = "Read access denied or invalid session credentials"),
        (status = 404, description = "No such notice for the acting session"),
        (status = 409, description = "SESSION_EXPIRED")
    )
)]
pub async fn ack_notice(
    State(state): State<AppState>,
    Path((owner, name, id)): Path<(String, String, String)>,
    headers: HeaderMap,
) -> Result<Json<svc::Acked>, AppError> {
    let (repo, user_id) = require_collab_read(&state, &headers, &owner, &name).await?;
    let session = acting_session(&state, &headers, repo.id, user_id).await?;
    Ok(Json(svc::ack(&state.db, repo.id, &session, &id, Utc::now()).await?))
}

#[utoipa::path(
    post,
    path = "/repos/{owner}/{name}/collaboration/tasks/{id}/messages/{message_id}/read-receipt",
    tag = "Collaboration",
    params(("owner" = String, Path), ("name" = String, Path), ("id" = String, Path), ("message_id" = String, Path)),
    responses(
        (status = 201, description = "Receipt recorded", body = serde_json::Value),
        (status = 200, description = "The target had already confirmed; the original read_at is returned", body = serde_json::Value),
        (status = 403, description = "Not the target session, write access denied or invalid session credentials"),
        (status = 404, description = "No such task or message in this repository"),
        (status = 409, description = "SESSION_EXPIRED")
    )
)]
pub async fn confirm_receipt(
    State(state): State<AppState>,
    Path((owner, name, id, message_id)): Path<(String, String, String, String)>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<delivery::Receipt>), AppError> {
    let (repo, user_id) = require_collab_write(&state, &headers, &owner, &name).await?;
    let session = acting_session(&state, &headers, repo.id, user_id).await?;
    let receipt =
        delivery::confirm(&state.db, repo.id, &id, &message_id, &session, Utc::now()).await?;
    let status = if receipt.newly_confirmed { StatusCode::CREATED } else { StatusCode::OK };
    Ok((status, Json(receipt)))
}

#[utoipa::path(
    get,
    path = "/repos/{owner}/{name}/collaboration/tasks/{id}/messages/{message_id}/delivery",
    tag = "Collaboration",
    params(("owner" = String, Path), ("name" = String, Path), ("id" = String, Path), ("message_id" = String, Path)),
    responses(
        (status = 200, description = "{saved, target_session_id, read_at, replied_at, reply_message_id, status}; status is not_targeted | unknown | read | replied", body = serde_json::Value),
        (status = 403, description = "Not the author session, read access denied or invalid session credentials"),
        (status = 404, description = "No such task or message in this repository"),
        (status = 409, description = "SESSION_EXPIRED")
    )
)]
pub async fn get_delivery(
    State(state): State<AppState>,
    Path((owner, name, id, message_id)): Path<(String, String, String, String)>,
    headers: HeaderMap,
) -> Result<Json<delivery::DeliveryStatus>, AppError> {
    let (repo, user_id) = require_collab_read(&state, &headers, &owner, &name).await?;
    let author = acting_session(&state, &headers, repo.id, user_id).await?;
    Ok(Json(delivery::status(&state.db, repo.id, &id, &message_id, &author).await?))
}
