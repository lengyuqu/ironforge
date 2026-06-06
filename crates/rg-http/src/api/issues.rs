//! REST API handlers for Issues and Issue Comments.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;

use crate::error::AppError;
use crate::AppState;
use crate::pagination::{PaginationParams, PaginatedResponse};
use crate::api::auth::extract_bearer_claims;

// ── Request / Response types ────────────────────────────────────────────

#[derive(Deserialize)]
pub struct CreateIssueRequest {
    pub title: String,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub labels: Option<Vec<String>>,
    #[serde(default)]
    pub milestone_id: Option<i64>,
}

#[derive(Deserialize)]
pub struct UpdateIssueRequest {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub state: Option<String>,
    #[serde(default)]
    pub labels: Option<Vec<String>>,
    #[serde(default)]
    pub assignee_id: Option<Option<i64>>,
    #[serde(default)]
    pub milestone_id: Option<Option<i64>>,
}

#[derive(Deserialize)]
pub struct CreateCommentRequest {
    pub body: String,
}

#[derive(Deserialize)]
pub struct ListQuery {
    pub state: Option<String>,
    #[serde(default)]
    pub labels: Option<String>,
    #[serde(flatten)]
    pub pagination: PaginationParams,
}

// ── Issue handlers ──────────────────────────────────────────────────────

#[utoipa::path(
    get,
    path = "/repos/{owner}/{name}/issues",
    tag = "Issues",
    params(
        ("owner" = String, Path, description = "owner"),
        ("name" = String, Path, description = "name"),
    ),
    responses(
        (status = 200, description = "Success", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
    ),
)]
pub async fn list_issues(
    State(state): State<AppState>,
    Path((owner, repo)): Path<(String, String)>,
    Query(params): Query<ListQuery>,
) -> impl IntoResponse {
    let state_filter = params.state.as_deref();
    let pagination = params.pagination.clamp();

    // If labels filter is present, use filtered query
    if let Some(ref labels_str) = params.labels {
        let label_names: Vec<String> = labels_str.split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        if !label_names.is_empty() {
            return match rg_core::issue::list_issues_filtered_by_labels(
                &state.db,
                &owner,
                &repo,
                state_filter,
                &label_names,
                pagination.offset(),
                pagination.limit(),
            )
            .await
            {
                Ok((data, total)) => (
                    StatusCode::OK,
                    Json(PaginatedResponse::new(data, &pagination, total as u64)),
                )
                    .into_response(),
                Err(e) => { tracing::error!(%e, "handler error"); AppError::internal(e).into_response() },
            };
        }
    }

    match rg_core::issue::list_issues_paginated(
        &state.db,
        &owner,
        &repo,
        state_filter,
        pagination.offset(),
        pagination.limit(),
    )
    .await
    {
        Ok((data, total)) => (
            StatusCode::OK,
            Json(PaginatedResponse::new(data, &pagination, total as u64)),
        )
            .into_response(),
        Err(e) => { tracing::error!(%e, "handler error"); AppError::internal(e).into_response() },
    }
}

#[utoipa::path(
    get,
    path = "/repos/{owner}/{name}/issues/{number}",
    tag = "Issues",
    params(
        ("owner" = String, Path, description = "owner"),
        ("name" = String, Path, description = "name"),
        ("number" = i64, Path, description = "number"),
    ),
    responses(
        (status = 200, description = "Success", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
    ),
)]
pub async fn get_issue(
    State(state): State<AppState>,
    Path((owner, repo, number)): Path<(String, String, i64)>,
) -> impl IntoResponse {
    match rg_core::issue::get_issue(&state.db, &owner, &repo, number).await {
        Ok(issue) => (StatusCode::OK, Json(issue)).into_response(),
        Err(e) => AppError::NotFound(e.to_string()).into_response(),
    }
}

#[utoipa::path(
    post,
    path = "/repos/{owner}/{name}/issues",
    tag = "Issues",
    params(
        ("owner" = String, Path, description = "owner"),
        ("name" = String, Path, description = "name"),
    ),
    request_body(content = serde_json::Value),
    responses(
        (status = 201, description = "Created", body = serde_json::Value),
        (status = 400, description = "Bad request", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
    ),
)]
pub async fn create_issue(
    State(state): State<AppState>,
    Path((owner, repo)): Path<(String, String)>,
    headers: axum::http::HeaderMap,
    Json(req): Json<CreateIssueRequest>,
) -> impl IntoResponse {
    let user_id = match super::auth::extract_user_id(&headers, &state.jwt_secret) {
        Some(id) => id,
        None => return AppError::Unauthorized("authentication required".to_string()).into_response(),
    };

    let repo_id = match resolve_repo_id(&state.db, &owner, &repo).await {
        Some(id) => id,
        None => return AppError::NotFound("repository not found".to_string()).into_response(),
    };

    match rg_core::issue::create_issue(
        &state.db,
        repo_id,
        user_id,
        req.title,
        req.body,
        req.labels,
        req.milestone_id,
    )
    .await
    {
        Ok(issue) => (StatusCode::CREATED, Json(issue)).into_response(),
        Err(e) => AppError::BadRequest(e.to_string()).into_response(),
    }
}

#[utoipa::path(
    patch,
    path = "/repos/{owner}/{name}/issues/{number}",
    tag = "Issues",
    params(
        ("owner" = String, Path, description = "owner"),
        ("name" = String, Path, description = "name"),
        ("number" = i64, Path, description = "number"),
    ),
    request_body(content = serde_json::Value),
    responses(
        (status = 200, description = "Updated", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
    ),
)]
pub async fn update_issue(
    State(state): State<AppState>,
    Path((owner, repo, number)): Path<(String, String, i64)>,
    Json(req): Json<UpdateIssueRequest>,
) -> impl IntoResponse {
    match rg_core::issue::update_issue(
        &state.db,
        &owner,
        &repo,
        number,
        req.title,
        req.body,
        req.state,
        req.labels,
        req.assignee_id,
        req.milestone_id,
    )
    .await
    {
        Ok(issue) => (StatusCode::OK, Json(issue)).into_response(),
        Err(e) => AppError::BadRequest(e.to_string()).into_response(),
    }
}

// ── Comment handlers ────────────────────────────────────────────────────

#[utoipa::path(
    get,
    path = "/repos/{owner}/{name}/issues/{number}/comments",
    tag = "Issues",
    params(
        ("owner" = String, Path, description = "owner"),
        ("name" = String, Path, description = "name"),
        ("number" = i64, Path, description = "number"),
    ),
    responses(
        (status = 200, description = "Success", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
    ),
)]
pub async fn list_comments(
    State(state): State<AppState>,
    Path((owner, repo, number)): Path<(String, String, i64)>,
) -> impl IntoResponse {
    match rg_core::issue::list_comments(&state.db, &owner, &repo, number).await {
        Ok(comments) => (StatusCode::OK, Json(comments)).into_response(),
        Err(e) => { tracing::error!(%e, "handler error"); AppError::internal(e).into_response() },
    }
}

#[utoipa::path(
    post,
    path = "/repos/{owner}/{name}/issues/{number}/comments",
    tag = "Issues",
    params(
        ("owner" = String, Path, description = "owner"),
        ("name" = String, Path, description = "name"),
        ("number" = i64, Path, description = "number"),
    ),
    request_body(content = serde_json::Value),
    responses(
        (status = 201, description = "Created", body = serde_json::Value),
        (status = 400, description = "Bad request", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
    ),
)]
pub async fn add_comment(
    State(state): State<AppState>,
    Path((owner, repo, number)): Path<(String, String, i64)>,
    headers: axum::http::HeaderMap,
    Json(req): Json<CreateCommentRequest>,
) -> impl IntoResponse {
    let user_id = match super::auth::extract_user_id(&headers, &state.jwt_secret) {
        Some(id) => id,
        None => return AppError::Unauthorized("authentication required".to_string()).into_response(),
    };

    match rg_core::issue::add_comment(&state.db, &owner, &repo, number, user_id, req.body).await {
        Ok(comment) => (StatusCode::CREATED, Json(comment)).into_response(),
        Err(e) => AppError::BadRequest(e.to_string()).into_response(),
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────


async fn resolve_repo_id(
    db: &sea_orm::DatabaseConnection,
    owner: &str,
    repo_name: &str,
) -> Option<i64> {
    let user = rg_db::ops::user_ops::find_by_username(db, owner)
        .await
        .ok()
        .flatten()?;
    let repo = rg_db::ops::repo_ops::find_by_owner_and_name(db, user.id, repo_name)
        .await
        .ok()
        .flatten()?;
    Some(repo.id)
}

// ── Milestone handlers ──────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct ListMilestonesQuery {
    pub state: Option<String>,
}

#[utoipa::path(
    get,
    path = "/repos/{owner}/{name}/milestones",
    tag = "Issues",
    params(
        ("owner" = String, Path, description = "owner"),
        ("name" = String, Path, description = "name"),
    ),
    responses(
        (status = 200, description = "Success", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
    ),
)]
pub async fn list_milestones(
    State(state): State<AppState>,
    Path((owner, name)): Path<(String, String)>,
    Query(params): Query<ListMilestonesQuery>,
) -> impl IntoResponse {
    let repo = match rg_core::repo::service::find_repo_by_owner_name(&state.db, &owner, &name).await {
        Ok(Some(r)) => r,
        Ok(None) => return AppError::NotFound("repository not found".to_string()).into_response(),
        Err(e) => return { tracing::error!(%e, "handler error"); AppError::internal(e).into_response() },
    };
    match rg_db::ops::milestone_ops::list_by_repo(&state.db, repo.id, params.state.as_deref()).await {
        Ok(milestones) => (StatusCode::OK, Json(serde_json::json!(milestones))).into_response(),
        Err(e) => { tracing::error!(%e, "handler error"); AppError::internal(e).into_response() },
    }
}

#[derive(Deserialize)]
pub struct CreateMilestoneRequest {
    pub title: String,
    pub description: Option<String>,
    pub due_date: Option<String>,
    pub state: Option<String>,
}

#[utoipa::path(
    post,
    path = "/repos/{owner}/{name}/milestones",
    tag = "Issues",
    params(
        ("owner" = String, Path, description = "owner"),
        ("name" = String, Path, description = "name"),
    ),
    request_body(content = serde_json::Value),
    responses(
        (status = 201, description = "Created", body = serde_json::Value),
        (status = 400, description = "Bad request", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
    ),
)]
pub async fn create_milestone(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path((owner, name)): Path<(String, String)>,
    Json(body): Json<CreateMilestoneRequest>,
) -> impl IntoResponse {
    let claims = match extract_bearer_claims(&headers, &state.jwt_secret) {
        Some(c) => c,
        None => return AppError::Unauthorized("authentication required".to_string()).into_response(),
    };
    let user_id: i64 = claims.sub.parse().unwrap_or(-1);
    let repo = match rg_core::repo::service::find_repo_by_owner_name(&state.db, &owner, &name).await {
        Ok(Some(r)) => r,
        Ok(None) => return AppError::NotFound("repository not found".to_string()).into_response(),
        Err(e) => return { tracing::error!(%e, "handler error"); AppError::internal(e).into_response() },
    };
    if !rg_core::repo::service::can_write(&state.db, &owner, &name, Some(user_id)).await.unwrap_or(false) {
        return AppError::Forbidden("forbidden".to_string()).into_response();
    }
    let now = chrono::Utc::now();
    let due_date = body.due_date.as_deref().and_then(|d| chrono::DateTime::parse_from_rfc3339(d).ok()).map(|dt| dt.with_timezone(&chrono::Utc));
    let model = rg_db::entities::milestone::ActiveModel {
        id: sea_orm::NotSet,
        repo_id: sea_orm::Set(repo.id),
        title: sea_orm::Set(body.title),
        description: sea_orm::Set(body.description),
        state: sea_orm::Set(body.state.unwrap_or_else(|| "open".to_string())),
        due_date: sea_orm::Set(due_date),
        created_at: sea_orm::Set(now),
        updated_at: sea_orm::Set(now),
    };
    match rg_db::ops::milestone_ops::create(&state.db, model).await {
        Ok(m) => (StatusCode::CREATED, Json(serde_json::json!(m))).into_response(),
        Err(e) => AppError::BadRequest(e.to_string()).into_response(),
    }
}

#[utoipa::path(
    get,
    path = "/repos/{owner}/{name}/milestones/{id}",
    tag = "Issues",
    params(
        ("owner" = String, Path, description = "owner"),
        ("name" = String, Path, description = "name"),
        ("id" = i64, Path, description = "id"),
    ),
    responses(
        (status = 200, description = "Success", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
    ),
)]
pub async fn get_milestone(
    State(state): State<AppState>,
    Path(((owner, name), id)): Path<((String, String), i64)>,
) -> impl IntoResponse {
    let _repo = match rg_core::repo::service::find_repo_by_owner_name(&state.db, &owner, &name).await {
        Ok(Some(r)) => r,
        Ok(None) => return AppError::NotFound("repository not found".to_string()).into_response(),
        Err(e) => return { tracing::error!(%e, "handler error"); AppError::internal(e).into_response() },
    };
    match rg_db::ops::milestone_ops::find_by_id(&state.db, id).await {
        Ok(Some(m)) => (StatusCode::OK, Json(serde_json::json!(m))).into_response(),
        Ok(None) => AppError::NotFound("milestone not found".to_string()).into_response(),
        Err(e) => { tracing::error!(%e, "handler error"); AppError::internal(e).into_response() },
    }
}

#[derive(Deserialize)]
pub struct UpdateMilestoneRequest {
    pub title: Option<String>,
    pub description: Option<Option<String>>,
    pub state: Option<String>,
    pub due_date: Option<Option<String>>,
}

#[utoipa::path(
    patch,
    path = "/repos/{owner}/{name}/milestones/{id}",
    tag = "Issues",
    params(
        ("owner" = String, Path, description = "owner"),
        ("name" = String, Path, description = "name"),
        ("id" = i64, Path, description = "id"),
    ),
    request_body(content = serde_json::Value),
    responses(
        (status = 200, description = "Updated", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
    ),
)]
pub async fn update_milestone(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(((owner, name), id)): Path<((String, String), i64)>,
    Json(body): Json<UpdateMilestoneRequest>,
) -> impl IntoResponse {
    let claims = match extract_bearer_claims(&headers, &state.jwt_secret) {
        Some(c) => c,
        None => return AppError::Unauthorized("authentication required".to_string()).into_response(),
    };
    let user_id: i64 = claims.sub.parse().unwrap_or(-1);
    if !rg_core::repo::service::can_write(&state.db, &owner, &name, Some(user_id)).await.unwrap_or(false) {
        return AppError::Forbidden("forbidden".to_string()).into_response();
    }
    let mut m = match rg_db::ops::milestone_ops::find_by_id(&state.db, id).await {
        Ok(Some(m)) => m,
        Ok(None) => return AppError::NotFound("milestone not found".to_string()).into_response(),
        Err(e) => return { tracing::error!(%e, "handler error"); AppError::internal(e).into_response() },
    };
    if let Some(t) = body.title { m.title = t; }
    if let Some(d) = body.description { m.description = d; }
    if let Some(s) = body.state { if s == "open" || s == "closed" { m.state = s; } }
    if let Some(d) = body.due_date {
        m.due_date = d.as_deref().and_then(|dt| chrono::DateTime::parse_from_rfc3339(dt).ok()).map(|dt| dt.with_timezone(&chrono::Utc));
    }
    m.updated_at = chrono::Utc::now();
    let active: rg_db::entities::milestone::ActiveModel = m.into();
    match rg_db::ops::milestone_ops::update(&state.db, active).await {
        Ok(m) => (StatusCode::OK, Json(serde_json::json!(m))).into_response(),
        Err(e) => { tracing::error!(%e, "handler error"); AppError::internal(e).into_response() },
    }
}

#[utoipa::path(
    delete,
    path = "/repos/{owner}/{name}/milestones/{id}",
    tag = "Issues",
    params(
        ("owner" = String, Path, description = "owner"),
        ("name" = String, Path, description = "name"),
        ("id" = i64, Path, description = "id"),
    ),
    responses(
        (status = 200, description = "Deleted", body = serde_json::Value),
        (status = 204, description = "No content"),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
    ),
)]
pub async fn delete_milestone(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(((owner, name), id)): Path<((String, String), i64)>,
) -> impl IntoResponse {
    let claims = match extract_bearer_claims(&headers, &state.jwt_secret) {
        Some(c) => c,
        None => return AppError::Unauthorized("authentication required".to_string()).into_response(),
    };
    let user_id: i64 = claims.sub.parse().unwrap_or(-1);
    if !rg_core::repo::service::can_write(&state.db, &owner, &name, Some(user_id)).await.unwrap_or(false) {
        return AppError::Forbidden("forbidden".to_string()).into_response();
    }
    match rg_db::ops::milestone_ops::delete_by_id(&state.db, id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => { tracing::error!(%e, "handler error"); AppError::internal(e).into_response() },
    }
}

// ── Issue Labels handlers ───────────────────────────────────────────────

/// GET /api/v1/repos/:owner/:name/issues/:number/labels
#[utoipa::path(
    get,
    path = "/repos/{owner}/{name}/issues/{number}/labels",
    tag = "Issues",
    params(
        ("owner" = String, Path, description = "owner"),
        ("name" = String, Path, description = "name"),
        ("number" = i64, Path, description = "number"),
    ),
    responses(
        (status = 200, description = "Success", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
    ),
)]
pub async fn get_issue_labels(
    State(state): State<AppState>,
    Path((owner, repo, number)): Path<(String, String, i64)>,
) -> impl IntoResponse {
    match rg_core::issue::get_issue(&state.db, &owner, &repo, number).await {
        Ok(issue) => {
            match rg_core::label::service::get_issue_labels(&state.db, issue.id).await {
                Ok(labels) => (StatusCode::OK, Json(serde_json::json!(labels))).into_response(),
                Err(e) => { tracing::error!(%e, "handler error"); AppError::internal(e).into_response() },
            }
        }
        Err(e) => AppError::NotFound(e.to_string()).into_response(),
    }
}
