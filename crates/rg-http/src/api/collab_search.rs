//! O09 live collaboration search; repository authorization precedes all queries.
use super::collab::{cursor_key, require_collab_read};
use crate::{error::AppError, AppState};
use axum::{
    extract::{Path, Query, State},
    http::HeaderMap,
    Json,
};
use rg_core::collaboration::search as svc;
use serde::Deserialize;

#[derive(Debug, Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct SearchQuery {
    pub kind: String,
    pub q: String,
    pub limit: Option<u64>,
    pub cursor: Option<String>,
}

#[utoipa::path(
    get, path = "/repos/{owner}/{name}/collaboration/search", tag = "Collaboration",
    params(("owner" = String, Path), ("name" = String, Path), SearchQuery),
    responses((status = 200, description = "Live search; no total or snippets", body = serde_json::Value),
              (status = 400, description = "Invalid query or cursor"),
              (status = 403, description = "Collaboration read access denied"))
)]
pub async fn search(
    State(state): State<AppState>,
    Path((owner, name)): Path<(String, String)>,
    Query(query): Query<SearchQuery>,
    headers: HeaderMap,
) -> Result<Json<svc::SearchPage>, AppError> {
    let (repo, _) = require_collab_read(&state, &headers, &owner, &name).await?;
    Ok(Json(
        svc::search(
            &state.db,
            repo.id,
            cursor_key(&state),
            &query.kind,
            &query.q,
            query.limit,
            query.cursor.as_deref(),
        )
        .await?,
    ))
}
