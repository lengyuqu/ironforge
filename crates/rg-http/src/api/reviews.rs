//! REST API handlers for PR code reviews.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;

use crate::AppState;

// ── Request / Response types ──────────────────────────────────────────

#[derive(Deserialize)]
pub struct SubmitReviewRequest {
    /// comment / approve / request_changes / dismiss
    pub action: String,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub commit_id: Option<String>,
}

#[derive(Deserialize)]
pub struct CreateReviewCommentRequest {
    pub review_id: i64,
    pub path: String,
    #[serde(default)]
    pub line: Option<i64>,
    #[serde(default)]
    pub side: Option<String>,
    pub body: String,
    #[serde(default)]
    pub commit_id: Option<String>,
    #[serde(default)]
    pub reply_to_id: Option<i64>,
}

// ── Review handlers ───────────────────────────────────────────────────

/// List reviews for a PR.
/// GET /api/v1/repos/:owner/:name/pulls/:number/reviews
pub async fn list_reviews(
    State(state): State<AppState>,
    Path((owner, repo, number)): Path<(String, String, i64)>,
) -> impl IntoResponse {
    match rg_core::review::service::list_reviews(&state.db, &owner, &repo, number).await {
        Ok(reviews) => (StatusCode::OK, Json(reviews)).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("{:#}", e)})),
        )
            .into_response(),
    }
}

/// Submit a review on a PR.
/// POST /api/v1/repos/:owner/:name/pulls/:number/reviews
pub async fn submit_review(
    State(state): State<AppState>,
    Path((owner, repo, number)): Path<(String, String, i64)>,
    headers: axum::http::HeaderMap,
    Json(req): Json<SubmitReviewRequest>,
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

    let action = match rg_core::review::service::ReviewAction::from_str(&req.action) {
        Ok(a) => a,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": format!("{:#}", e)})),
            )
                .into_response()
        }
    };

    match rg_core::review::service::submit_review(
        &state.db,
        repo_id,
        number,
        user_id,
        action,
        req.body,
        req.commit_id,
    )
    .await
    {
        Ok(review) => (StatusCode::CREATED, Json(review)).into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": format!("{:#}", e)})),
        )
            .into_response(),
    }
}

/// Get a single review.
/// GET /api/v1/repos/:owner/:name/pulls/:number/reviews/:id
pub async fn get_review(
    State(state): State<AppState>,
    Path((_owner, _repo, _number, id)): Path<(String, String, i64, i64)>,
) -> impl IntoResponse {
    match rg_core::review::service::get_review(&state.db, id).await {
        Ok(review) => (StatusCode::OK, Json(review)).into_response(),
        Err(e) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": format!("{:#}", e)})),
        )
            .into_response(),
    }
}

/// Dismiss a review.
/// POST /api/v1/repos/:owner/:name/pulls/:number/reviews/:id/dismiss
pub async fn dismiss_review(
    State(state): State<AppState>,
    Path((_owner, _repo, _number, id)): Path<(String, String, i64, i64)>,
    headers: axum::http::HeaderMap,
    Json(req): Json<DismissReviewRequest>,
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

    match rg_core::review::service::dismiss_review(&state.db, id, user_id, req.message).await {
        Ok(review) => (StatusCode::OK, Json(review)).into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": format!("{:#}", e)})),
        )
            .into_response(),
    }
}

// ── Review comment handlers ───────────────────────────────────────────

/// List review comments for a PR.
/// GET /api/v1/repos/:owner/:name/pulls/:number/comments
pub async fn list_review_comments(
    State(state): State<AppState>,
    Path((owner, repo, number)): Path<(String, String, i64)>,
) -> impl IntoResponse {
    match rg_core::review::service::list_review_comments(&state.db, &owner, &repo, number).await {
        Ok(comments) => (StatusCode::OK, Json(comments)).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("{:#}", e)})),
        )
            .into_response(),
    }
}

/// Create a review comment.
/// POST /api/v1/repos/:owner/:name/pulls/:number/comments
pub async fn create_review_comment(
    State(state): State<AppState>,
    Path((owner, repo, number)): Path<(String, String, i64)>,
    headers: axum::http::HeaderMap,
    Json(req): Json<CreateReviewCommentRequest>,
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

    match rg_core::review::service::create_review_comment(
        &state.db,
        repo_id,
        number,
        req.review_id,
        user_id,
        req.path,
        req.line,
        req.side,
        req.body,
        req.commit_id,
        req.reply_to_id,
    )
    .await
    {
        Ok(comment) => (StatusCode::CREATED, Json(comment)).into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": format!("{:#}", e)})),
        )
            .into_response(),
    }
}

// ── Extra request types ───────────────────────────────────────────────

#[derive(Deserialize)]
pub struct DismissReviewRequest {
    pub message: String,
}

// ── Helpers ───────────────────────────────────────────────────────────

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
