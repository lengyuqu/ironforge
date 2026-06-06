//! Global search API handler.

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::Deserialize;

use crate::AppState;
use crate::error::AppError;
use crate::pagination::PaginationParams;

#[derive(Deserialize)]
pub struct SearchQuery {
    pub q: String,
    #[serde(default = "default_search_type")]
    pub r#type: String,
    #[serde(flatten)]
    pub pagination: PaginationParams,
}

fn default_search_type() -> String {
    "all".to_string()
}

/// GET /api/v1/search?q=keyword&type=all|repos|issues|wiki&page=1&per_page=20
#[utoipa::path(
    get,
    path = "/search",
    tag = "Search",
    responses(
        (status = 200, description = "Success", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
    ),
)]
pub async fn search(
    State(state): State<AppState>,
    Query(params): Query<SearchQuery>,
) -> impl IntoResponse {
    let pagination = params.pagination.clamp();

    if params.q.trim().is_empty() {
        return AppError::bad_request("search query 'q' parameter is required").into_response();
    }

    let valid_types = ["all", "repos", "issues", "wiki"];
    if !valid_types.contains(&params.r#type.as_str()) {
        return AppError::bad_request(format!(
            "invalid type '{}', must be one of: {:?}",
            params.r#type, valid_types
        )).into_response();
    }

    match rg_core::search::service::search(
        &state.db,
        &params.q,
        &params.r#type,
        pagination.page,
        pagination.per_page,
    )
    .await
    {
        Ok((results, total)) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "results": results,
                "total": total,
                "page": pagination.page,
                "per_page": pagination.per_page,
            })),
        )
            .into_response(),
        Err(e) => AppError::internal(e.to_string()).into_response(),
    }
}
