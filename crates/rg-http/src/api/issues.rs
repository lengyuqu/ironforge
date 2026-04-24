//! REST API handlers for Issues and Issue Comments.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;

use crate::AppState;

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
}

// ── Issue handlers ──────────────────────────────────────────────────────

pub async fn list_issues(
    State(state): State<AppState>,
    Path((owner, repo)): Path<(String, String)>,
    Query(params): Query<ListQuery>,
) -> impl IntoResponse {
    let state_filter = params.state.as_deref();
    match rg_core::issue::list_issues(&state.db, &owner, &repo, state_filter).await {
        Ok(issues) => (StatusCode::OK, Json(issues)).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("{:#}", e)})),
        )
            .into_response(),
    }
}

pub async fn get_issue(
    State(state): State<AppState>,
    Path((owner, repo, number)): Path<(String, String, i64)>,
) -> impl IntoResponse {
    match rg_core::issue::get_issue(&state.db, &owner, &repo, number).await {
        Ok(issue) => (StatusCode::OK, Json(issue)).into_response(),
        Err(e) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": format!("{:#}", e)})),
        )
            .into_response(),
    }
}

pub async fn create_issue(
    State(state): State<AppState>,
    Path((owner, repo)): Path<(String, String)>,
    headers: axum::http::HeaderMap,
    Json(req): Json<CreateIssueRequest>,
) -> impl IntoResponse {
    let user_id = match extract_user_id(&state, &headers) {
        Some(id) => id,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({"error": "authentication required"})),
            )
                .into_response()
        }
    };

    let repo_id = match resolve_repo_id(&state.db, &owner, &repo).await {
        Some(id) => id,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "repository not found"})),
            )
                .into_response()
        }
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
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": format!("{:#}", e)})),
        )
            .into_response(),
    }
}

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
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": format!("{:#}", e)})),
        )
            .into_response(),
    }
}

// ── Comment handlers ────────────────────────────────────────────────────

pub async fn list_comments(
    State(state): State<AppState>,
    Path((owner, repo, number)): Path<(String, String, i64)>,
) -> impl IntoResponse {
    match rg_core::issue::list_comments(&state.db, &owner, &repo, number).await {
        Ok(comments) => (StatusCode::OK, Json(comments)).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("{:#}", e)})),
        )
            .into_response(),
    }
}

pub async fn add_comment(
    State(state): State<AppState>,
    Path((owner, repo, number)): Path<(String, String, i64)>,
    headers: axum::http::HeaderMap,
    Json(req): Json<CreateCommentRequest>,
) -> impl IntoResponse {
    let user_id = match extract_user_id(&state, &headers) {
        Some(id) => id,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({"error": "authentication required"})),
            )
                .into_response()
        }
    };

    match rg_core::issue::add_comment(&state.db, &owner, &repo, number, user_id, req.body).await {
        Ok(comment) => (StatusCode::CREATED, Json(comment)).into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": format!("{:#}", e)})),
        )
            .into_response(),
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────

fn extract_user_id(state: &AppState, headers: &axum::http::HeaderMap) -> Option<i64> {
    let auth = headers.get("authorization")?.to_str().ok()?;
    let token = auth.strip_prefix("Bearer ")?;
    let claims = rg_core::auth::jwt::validate_token(token, &state.jwt_secret)?;
    claims.sub.parse().ok()
}

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
