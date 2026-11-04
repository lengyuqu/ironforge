//! Task takeover endpoints: a session claims an abandoned task, a user reassigns.

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use chrono::Utc;
use rg_core::collaboration::{self, takeover as svc};
use serde::Deserialize;
use utoipa::ToSchema;

use super::collab::{acting_session, cursor_key, require_collab_read, require_collab_write, ListQuery};
use crate::error::AppError;
use crate::AppState;

#[derive(Debug, Deserialize, ToSchema)]
pub struct TakeoverBody {
    /// Client-chosen idempotency key, unique per acting session or user.
    pub client_takeover_id: String,
    /// The task `version` the caller last saw.
    pub expected_task_version: i64,
    /// The owner the caller last saw; omit or `null` for an unowned task.
    pub expected_owner_session_id: Option<String>,
    /// 1 byte to 2000 bytes; a claim, not verified.
    pub reason: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct ReassignBody {
    pub client_takeover_id: String,
    pub expected_task_version: i64,
    pub expected_owner_session_id: Option<String>,
    /// A live session of this repository that becomes the owner.
    pub to_session_id: String,
    pub reason: String,
}

impl From<TakeoverBody> for svc::TakeoverRequest {
    fn from(b: TakeoverBody) -> Self {
        Self {
            client_takeover_id: b.client_takeover_id,
            expected_task_version: b.expected_task_version,
            expected_owner_session_id: b.expected_owner_session_id,
            reason: b.reason,
        }
    }
}

fn status_of(result: &svc::TakeoverResult) -> StatusCode {
    if result.replayed {
        StatusCode::OK
    } else {
        StatusCode::CREATED
    }
}

#[utoipa::path(
    post,
    path = "/repos/{owner}/{name}/collaboration/tasks/{id}/takeovers",
    tag = "Collaboration",
    params(("owner" = String, Path), ("name" = String, Path), ("id" = String, Path)),
    request_body = TakeoverBody,
    responses(
        (status = 201, description = "Task now owned by the acting session: {task, takeover, replayed}", body = serde_json::Value),
        (status = 200, description = "Identical earlier request replayed", body = serde_json::Value),
        (status = 403, description = "Write access denied or invalid session credentials"),
        (status = 404, description = "No such task in this repository"),
        (status = 409, description = "OWNER_MAY_BE_ACTIVE, VERSION_CONFLICT, TASK_DONE, IDEMPOTENCY_KEY_REUSED or SESSION_EXPIRED")
    )
)]
pub async fn claim_task(
    State(state): State<AppState>,
    Path((owner, name, id)): Path<(String, String, String)>,
    headers: HeaderMap,
    Json(body): Json<TakeoverBody>,
) -> Result<(StatusCode, Json<svc::TakeoverResult>), AppError> {
    let (repo, user_id) = require_collab_write(&state, &headers, &owner, &name).await?;
    let session = acting_session(&state, &headers, repo.id, user_id).await?;
    let result = svc::claim(&state.db, repo.id, &id, &session, body.into(), Utc::now()).await?;
    Ok((status_of(&result), Json(result)))
}

#[utoipa::path(
    post,
    path = "/repos/{owner}/{name}/collaboration/tasks/{id}/reassign",
    tag = "Collaboration",
    params(("owner" = String, Path), ("name" = String, Path), ("id" = String, Path)),
    request_body = ReassignBody,
    responses(
        (status = 201, description = "Task reassigned: {task, takeover, replayed}", body = serde_json::Value),
        (status = 200, description = "Identical earlier request replayed", body = serde_json::Value),
        (status = 400, description = "to_session_id is not a live session of this repository"),
        (status = 403, description = "Write access denied"),
        (status = 404, description = "No such task in this repository"),
        (status = 409, description = "VERSION_CONFLICT or IDEMPOTENCY_KEY_REUSED")
    )
)]
pub async fn reassign_task(
    State(state): State<AppState>,
    Path((owner, name, id)): Path<(String, String, String)>,
    headers: HeaderMap,
    Json(body): Json<ReassignBody>,
) -> Result<(StatusCode, Json<svc::TakeoverResult>), AppError> {
    let (repo, user_id) = require_collab_write(&state, &headers, &owner, &name).await?;
    let to = body.to_session_id.clone();
    let req = svc::TakeoverRequest {
        client_takeover_id: body.client_takeover_id,
        expected_task_version: body.expected_task_version,
        expected_owner_session_id: body.expected_owner_session_id,
        reason: body.reason,
    };
    let result = svc::reassign(&state.db, repo.id, &id, user_id, &to, req, Utc::now()).await?;
    Ok((status_of(&result), Json(result)))
}

#[utoipa::path(
    get,
    path = "/repos/{owner}/{name}/collaboration/tasks/{id}/takeovers",
    tag = "Collaboration",
    params(("owner" = String, Path), ("name" = String, Path), ("id" = String, Path), ListQuery),
    responses(
        (status = 200, description = "Ownership changes by time: {items, next_cursor}", body = serde_json::Value),
        (status = 403, description = "Read access denied"),
        (status = 404, description = "No such task in this repository")
    )
)]
pub async fn list_takeovers(
    State(state): State<AppState>,
    Path((owner, name, id)): Path<(String, String, String)>,
    Query(query): Query<ListQuery>,
    headers: HeaderMap,
) -> Result<Json<collaboration::Page<svc::TakeoverView>>, AppError> {
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
    Ok(Json(page))
}
