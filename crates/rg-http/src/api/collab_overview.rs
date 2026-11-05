//! Repository collaboration overview for people (O02).

use axum::extract::{Query, State};
use axum::http::HeaderMap;
use axum::Json;
use chrono::Utc;
use rg_core::collaboration::{self, overview as svc};
use serde::Deserialize;

use super::collab::require_collab_read;
use crate::error::AppError;
use crate::AppState;

#[derive(Debug, Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct OverviewQuery {
    /// Comma-separated subset of `awaiting_me`, `blocked_waiting`,
    /// `pending_handoffs`, `in_progress`. Default: all, in that order.
    pub sections: Option<String>,
    /// Items per section, 1–100, default 50.
    pub limit: Option<u64>,
}

#[utoipa::path(
    get,
    path = "/repos/{owner}/{name}/collaboration/overview",
    tag = "Collaboration",
    params(("owner" = String, Path), ("name" = String, Path), OverviewQuery),
    responses(
        (status = 200, description = "{sections:[{name, available, reason, items:[{object_type, object_id, task_id, title, state, occurred_at, session_id, session_state, flags, origin}], as_of, truncated, continue_with}], as_of, consistency: per_section_live_query, notes}", body = serde_json::Value),
        (status = 400, description = "Unknown section"),
        (status = 403, description = "Read access denied")
    )
)]
pub async fn get_overview(
    State(state): State<AppState>,
    axum::extract::Path((owner, name)): axum::extract::Path<(String, String)>,
    Query(query): Query<OverviewQuery>,
    headers: HeaderMap,
) -> Result<Json<svc::Overview>, AppError> {
    let (repo, user_id) = require_collab_read(&state, &headers, &owner, &name).await?;
    let sections = svc::parse_sections(query.sections.as_deref())?;
    let can_write = rg_core::repo::service::can_write_repo(&state.db, &repo, Some(user_id)).await?;
    let overview = svc::build(
        &state.db,
        repo.id,
        user_id,
        can_write,
        &sections,
        collaboration::clamp_limit(query.limit),
        Utc::now(),
    )
    .await?;
    Ok(Json(overview))
}
