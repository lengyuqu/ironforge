//! Declared-path overlap warnings (P1 U03). A hint, never a lock.

use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::Json;
use chrono::Utc;
use rg_core::collaboration::{self, overlap as svc};
use serde::Deserialize;

use super::collab::{cursor_key, require_collab_read};
use crate::error::AppError;
use crate::AppState;

#[derive(Debug, Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct OverlapQuery {
    /// 1–100, default 50.
    pub limit: Option<u64>,
    /// Opaque `next_cursor` from the previous page.
    pub cursor: Option<String>,
}

#[utoipa::path(
    get,
    path = "/repos/{owner}/{name}/collaboration/tasks/{id}/overlaps",
    tag = "Collaboration",
    params(("owner" = String, Path), ("name" = String, Path), ("id" = String, Path), OverlapQuery),
    responses(
        (status = 200, description = "{items:[{task_id, status, owner_session_id, owner_state, branch, overlaps:[{mine, theirs, reason}], overlaps_truncated, task_version, declared_at, confidence: declared_current|declaration_stale, origin}], next_cursor, as_of, authority: hint, not_covered}. A page may be short and still have next_cursor.", body = serde_json::Value),
        (status = 400, description = "Invalid cursor"),
        (status = 403, description = "Read access denied"),
        (status = 404, description = "No such task in this repository")
    )
)]
pub async fn get_overlaps(
    State(state): State<AppState>,
    Path((owner, name, id)): Path<(String, String, String)>,
    Query(query): Query<OverlapQuery>,
    headers: HeaderMap,
) -> Result<Json<svc::OverlapPage>, AppError> {
    let (repo, _user) = require_collab_read(&state, &headers, &owner, &name).await?;
    let page = svc::list(
        &state.db,
        cursor_key(&state),
        repo.id,
        &id,
        collaboration::clamp_limit(query.limit),
        query.cursor.as_deref(),
        Utc::now(),
    )
    .await?;
    Ok(Json(page))
}
