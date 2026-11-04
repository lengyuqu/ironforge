//! Fault diagnosis: explains from stored records why a task or session looks
//! stuck. Read-only; findings carry their source and a confidence, and what the
//! records cannot show is reported as `unknown`, never blamed on an agent.

use axum::extract::{Query, State};
use axum::http::HeaderMap;
use axum::Json;
use chrono::Utc;
use rg_core::collaboration::diagnostics as svc;
use serde::Deserialize;

use super::collab::require_collab_read;
use crate::error::AppError;
use crate::AppState;

#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct DiagnosticsQuery {
    /// Diagnose this task (exactly one of `task_id` and `session_id`).
    pub task_id: Option<String>,
    /// Diagnose this session.
    pub session_id: Option<String>,
}

#[utoipa::path(
    get,
    path = "/repos/{owner}/{name}/collaboration/diagnostics",
    tag = "Collaboration",
    params(("owner" = String, Path), ("name" = String, Path), DiagnosticsQuery),
    responses(
        (status = 200, description = "{subject, as_of, findings:[{code, confidence: established|unknown, severity, statement, source, details}], not_covered, authority}", body = serde_json::Value),
        (status = 400, description = "Not exactly one of task_id and session_id"),
        (status = 403, description = "Read access denied"),
        (status = 404, description = "No such task or session in this repository")
    )
)]
pub async fn get_diagnostics(
    State(state): State<AppState>,
    axum::extract::Path((owner, name)): axum::extract::Path<(String, String)>,
    Query(query): Query<DiagnosticsQuery>,
    headers: HeaderMap,
) -> Result<Json<svc::Diagnosis>, AppError> {
    let (repo, _user) = require_collab_read(&state, &headers, &owner, &name).await?;
    let now = Utc::now();
    let diagnosis = match (query.task_id.as_deref(), query.session_id.as_deref()) {
        (Some(task_id), None) => svc::for_task(&state.db, &repo, task_id, now).await?,
        (None, Some(session_id)) => svc::for_session(&state.db, &repo, session_id, now).await?,
        _ => {
            return Err(AppError::bad_request(
                "pass exactly one of task_id and session_id",
            ))
        }
    };
    Ok(Json(diagnosis))
}
