//! Task context pack (P1 U05): a byte-budgeted bundle of what a successor needs.

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use chrono::Utc;
use rg_core::collaboration::{context as svc, task};
use serde::Deserialize;

use super::collab::{cursor_key, observed_branch_head, require_collab_read};
use crate::error::AppError;
use crate::AppState;

#[derive(Debug, Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ContextQuery {
    /// Upper bound on the serialized response, 4096–262144 (default 32768).
    pub max_bytes: Option<usize>,
    /// Comma-separated subset of `questions`, `decisions`, `handoffs`,
    /// `work_packages`, `dependencies`, `overlaps`, `messages`. The task itself
    /// is always returned.
    pub sections: Option<String>,
}

#[utoipa::path(
    get,
    path = "/repos/{owner}/{name}/collaboration/tasks/{id}/context",
    tag = "Collaboration",
    params(("owner" = String, Path), ("name" = String, Path), ("id" = String, Path), ContextQuery),
    responses(
        (status = 200, description = "{task_id, as_of, task_version, head_sha, max_bytes, git:{branch, head_sha, observed_at, status}, sections:[{name, items, returned_count, has_more, continue_with, source, captured_at}], truncation:{sections, fields}, consistency:{db: per_section_live_query, git: independent}, notes}. Records are verbatim client content (origin: user_content), never summaries; every section is a separate live query.", body = serde_json::Value),
        (status = 400, description = "max_bytes out of range, unknown section, or BUDGET_TOO_SMALL (details.needed_bytes)"),
        (status = 403, description = "Read access denied"),
        (status = 404, description = "No such task in this repository")
    )
)]
pub async fn get_context(
    State(state): State<AppState>,
    Path((owner, name, id)): Path<(String, String, String)>,
    Query(query): Query<ContextQuery>,
    headers: HeaderMap,
) -> Result<Json<svc::ContextPack>, AppError> {
    let (repo, _user) = require_collab_read(&state, &headers, &owner, &name).await?;
    let max_bytes = svc::check_budget(query.max_bytes)?;
    let sections = svc::parse_sections(query.sections.as_deref())?;
    let row = task::find(&state.db, repo.id, &id).await?;
    let now = Utc::now();
    // The Git observation is independent of the database reads below.
    let head = observed_branch_head(&state, &owner, &name, row.branch.as_deref()).await;
    let built = svc::build(
        &state.db,
        cursor_key(&state),
        repo.id,
        &row.id,
        svc::GitObservation {
            branch: row.branch.clone(),
            head_sha: head,
            observed_at: Utc::now(),
        },
        &sections,
        max_bytes,
        now,
    )
    .await?;
    match built {
        svc::Built::Pack(pack) => Ok(Json(*pack)),
        svc::Built::TooSmall { needed } => Err(AppError::coded(
            StatusCode::BAD_REQUEST,
            "BUDGET_TOO_SMALL",
            "max_bytes cannot hold even the task itself; raise it or read the task directly",
        )
        .with_details(serde_json::json!({ "max_bytes": max_bytes, "needed_bytes": needed }))),
    }
}
