//! Label REST API.
//!
//! GET    /api/v1/repos/:owner/:name/labels      — list labels
//! POST   /api/v1/repos/:owner/:name/labels      — create label
//! GET    /api/v1/repos/:owner/:name/labels/:id  — get label
//! PATCH  /api/v1/repos/:owner/:name/labels/:id  — update label
//! DELETE /api/v1/repos/:owner/:name/labels/:id  — delete label

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use serde::Deserialize;
use utoipa::ToSchema;

use crate::api::users::extract_bearer_claims;
use crate::AppState;

/// Request body for creating a label.
#[derive(Deserialize, ToSchema)]
pub struct CreateLabelRequest {
    pub name: String,
    pub color: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Request body for updating a label.
#[derive(Deserialize, ToSchema)]
pub struct UpdateLabelRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// GET /api/v1/repos/:owner/:name/labels
pub async fn list_labels(
    State(state): State<AppState>,
    Path((owner, name)): Path<(String, String)>,
) -> impl IntoResponse {
    match rg_core::label::service::list_labels(&state.db, &owner, &name).await {
        Ok(labels) => (StatusCode::OK, Json(serde_json::json!(labels))).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// GET /api/v1/repos/:owner/:name/labels/:id
pub async fn get_label(
    State(state): State<AppState>,
    Path((owner, name, id)): Path<(String, String, i64)>,
) -> impl IntoResponse {
    match rg_core::label::service::get_label(&state.db, &owner, &name, id).await {
        Ok(label) => (StatusCode::OK, Json(serde_json::json!(label))).into_response(),
        Err(_) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "label not found" })),
        )
            .into_response(),
    }
}

/// POST /api/v1/repos/:owner/:name/labels
pub async fn create_label(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((owner, name)): Path<(String, String)>,
    Json(body): Json<CreateLabelRequest>,
) -> impl IntoResponse {
    let claims = match extract_bearer_claims(&headers, &state.jwt_secret) {
        Some(c) => c,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({ "error": "authentication required" })),
            )
                .into_response()
        }
    };

    let user_id: i64 = claims.sub.parse().unwrap_or(-1);

    // Check write permission
    if !rg_core::repo::service::can_write(&state.db, &owner, &name, Some(user_id))
        .await
        .unwrap_or(false)
    {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({ "error": "forbidden" })),
        )
            .into_response();
    }

    match rg_core::label::service::create_label(
        &state.db,
        &owner,
        &name,
        body.name,
        body.color,
        body.description,
    )
    .await
    {
        Ok(label) => (StatusCode::CREATED, Json(serde_json::json!(label))).into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// PATCH /api/v1/repos/:owner/:name/labels/:id
pub async fn update_label(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((owner, name, id)): Path<(String, String, i64)>,
    Json(body): Json<UpdateLabelRequest>,
) -> impl IntoResponse {
    let claims = match extract_bearer_claims(&headers, &state.jwt_secret) {
        Some(c) => c,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({ "error": "authentication required" })),
            )
                .into_response()
        }
    };

    let user_id: i64 = claims.sub.parse().unwrap_or(-1);

    // Check write permission
    if !rg_core::repo::service::can_write(&state.db, &owner, &name, Some(user_id))
        .await
        .unwrap_or(false)
    {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({ "error": "forbidden" })),
        )
            .into_response();
    }

    match rg_core::label::service::update_label(
        &state.db,
        id,
        body.name,
        body.color,
        Some(body.description),
    )
    .await
    {
        Ok(label) => (StatusCode::OK, Json(serde_json::json!(label))).into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// DELETE /api/v1/repos/:owner/:name/labels/:id
pub async fn delete_label(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((owner, name, id)): Path<(String, String, i64)>,
) -> impl IntoResponse {
    let claims = match extract_bearer_claims(&headers, &state.jwt_secret) {
        Some(c) => c,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({ "error": "authentication required" })),
            )
                .into_response()
        }
    };

    let user_id: i64 = claims.sub.parse().unwrap_or(-1);

    // Check write permission
    if !rg_core::repo::service::can_write(&state.db, &owner, &name, Some(user_id))
        .await
        .unwrap_or(false)
    {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({ "error": "forbidden" })),
        )
            .into_response();
    }

    match rg_core::label::service::delete_label(&state.db, id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}
