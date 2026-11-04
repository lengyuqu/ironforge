//! Escalation settings (repository admins) and the lazy evaluation hook.
//!
//! Reminders are `escalation` notices, read through `GET .../notices`. There is
//! no scheduler: a session's notice or inbox read evaluates that session's
//! conditions (see `rg_core::collaboration::escalation`).

use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::Json;
use chrono::Utc;
use rg_core::collaboration::escalation as svc;
use rg_db::entities::{agent_session, repository};
use serde::Deserialize;
use utoipa::ToSchema;

use super::collab::require_collab_read;
use super::repo_access::require_admin;
use crate::error::AppError;
use crate::AppState;

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct QuietHoursRequest {
    pub start_minute: i32,
    pub end_minute: i32,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SettingsRequest {
    pub expected_version: i64,
    pub enabled: bool,
    pub blocked_after_secs: i64,
    pub question_after_secs: i64,
    pub handoff_after_secs: i64,
    pub repeat_after_secs: i64,
    pub quiet_hours: Option<QuietHoursRequest>,
}

/// Evaluate the session's escalations before it reads. A failure must not turn
/// a read of existing hints into an error, so it is logged and skipped.
pub(crate) async fn evaluate_lazily(
    state: &AppState,
    repo: &repository::Model,
    session: &agent_session::Model,
) {
    if let Err(error) = svc::evaluate_for_session(&state.db, repo, session, Utc::now()).await {
        tracing::warn!(repo_id = repo.id, %error, "collaboration escalation evaluation failed");
    }
}

#[utoipa::path(
    get,
    path = "/repos/{owner}/{name}/collaboration/escalation-settings",
    tag = "Collaboration",
    params(("owner" = String, Path), ("name" = String, Path)),
    responses(
        (status = 200, description = "Thresholds, quiet window (UTC) and version; source=default means nothing is stored and escalation is off", body = serde_json::Value),
        (status = 403, description = "Read access denied")
    )
)]
pub async fn get_settings(
    State(state): State<AppState>,
    Path((owner, name)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<Json<svc::SettingsView>, AppError> {
    let (repo, _user) = require_collab_read(&state, &headers, &owner, &name).await?;
    Ok(Json(svc::get_settings(&state.db, repo.id).await?))
}

#[utoipa::path(
    put,
    path = "/repos/{owner}/{name}/collaboration/escalation-settings",
    tag = "Collaboration",
    params(("owner" = String, Path), ("name" = String, Path)),
    request_body = SettingsRequest,
    responses(
        (status = 200, description = "Stored settings (a retry with identical values is a no-op)", body = serde_json::Value),
        (status = 400, description = "Value out of range"),
        (status = 403, description = "Repository admin access required"),
        (status = 409, description = "VERSION_CONFLICT with the current version")
    )
)]
pub async fn put_settings(
    State(state): State<AppState>,
    Path((owner, name)): Path<(String, String)>,
    headers: HeaderMap,
    super::collab_json::CollabJson(input): super::collab_json::CollabJson<SettingsRequest>,
) -> Result<Json<svc::SettingsView>, AppError> {
    let (repo, user_id) = require_admin(&state, &headers, &owner, &name).await?;
    let settings = svc::SettingsInput {
        expected_version: input.expected_version,
        enabled: input.enabled,
        blocked_after_secs: input.blocked_after_secs,
        question_after_secs: input.question_after_secs,
        handoff_after_secs: input.handoff_after_secs,
        repeat_after_secs: input.repeat_after_secs,
        quiet_hours: input.quiet_hours.map(|q| svc::QuietHours {
            start_minute: q.start_minute,
            end_minute: q.end_minute,
        }),
    };
    Ok(Json(
        svc::put_settings(&state.db, repo.id, user_id, settings, Utc::now()).await?,
    ))
}
