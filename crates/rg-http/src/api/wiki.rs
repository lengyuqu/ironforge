//! Wiki REST API endpoints.

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};
use sea_orm::DatabaseConnection;

use crate::AppState;
use crate::error::AppError;
use utoipa::ToSchema;

// ── Request / Response types ──────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct CreateWikiPageRequest {
    pub title: String,
    pub content: String,
    pub message: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateWikiPageRequest {
    pub content: String,
    pub message: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct WikiPageResponse {
    pub id: i64,
    pub title: String,
    pub content: String,
    pub message: Option<String>,
    pub author_id: Option<i64>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Serialize)]
pub struct WikiPageSummary {
    pub id: i64,
    pub title: String,
    pub updated_at: String,
}

fn page_to_response(p: &rg_db::entities::wiki_page::Model) -> WikiPageResponse {
    WikiPageResponse {
        id: p.id,
        title: p.title.clone(),
        content: p.content.clone(),
        message: p.message.clone(),
        author_id: p.author_id,
        created_at: p.created_at.to_rfc3339(),
        updated_at: p.updated_at.to_rfc3339(),
    }
}

fn page_to_summary(p: &rg_db::entities::wiki_page::Model) -> WikiPageSummary {
    WikiPageSummary {
        id: p.id,
        title: p.title.clone(),
        updated_at: p.updated_at.to_rfc3339(),
    }
}

// ── Handlers ──────────────────────────────────────────────────────────────

#[utoipa::path(
    get,
    path = "/repos/{owner}/{name}/wiki",
    tag = "Wiki",
    params(
        ("owner" = String, Path, description = "owner"),
        ("name" = String, Path, description = "name"),
    ),
    responses(
        (status = 200, description = "Success", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
    ),
)]
pub async fn list_pages(
    State(state): State<AppState>,
    Path((owner, repo)): Path<(String, String)>,
    _headers: HeaderMap,
) -> impl IntoResponse {
    let _user_id = extract_user_id(&state, &_headers);
    let repo_id = match resolve_repo_id(&state.db, &owner, &repo).await {
        Some(id) => id,
        None => return AppError::not_found("repository not found").into_response(),
    };

    match rg_core::wiki::service::list_pages(&state.db, repo_id).await {
        Ok(pages) => {
            let summaries: Vec<WikiPageSummary> = pages.iter().map(page_to_summary).collect();
            (StatusCode::OK, Json(serde_json::json!(summaries))).into_response()
        }
        Err(e) => AppError::internal(e).into_response(),
    }
}

#[utoipa::path(
    get,
    path = "/repos/{owner}/{name}/wiki/{title}",
    tag = "Wiki",
    params(
        ("owner" = String, Path, description = "owner"),
        ("name" = String, Path, description = "name"),
        ("title" = String, Path, description = "title"),
    ),
    responses(
        (status = 200, description = "Success", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
    ),
)]
pub async fn get_page(
    State(state): State<AppState>,
    Path((owner, repo, title)): Path<(String, String, String)>,
    _headers: HeaderMap,
) -> impl IntoResponse {
    let repo_id = match resolve_repo_id(&state.db, &owner, &repo).await {
        Some(id) => id,
        None => return AppError::not_found("repository not found").into_response(),
    };

    match rg_core::wiki::service::get_page(&state.db, repo_id, &title).await {
        Ok(Some(page)) => (StatusCode::OK, Json(serde_json::json!(page_to_response(&page)))).into_response(),
        Ok(None) => AppError::not_found("page not found").into_response(),
        Err(e) => AppError::internal(e).into_response(),
    }
}

#[utoipa::path(
    post,
    path = "/repos/{owner}/{name}/wiki",
    tag = "Wiki",
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
pub async fn create_page(
    State(state): State<AppState>,
    Path((owner, repo)): Path<(String, String)>,
    headers: HeaderMap,
    Json(body): Json<CreateWikiPageRequest>,
) -> impl IntoResponse {
    let user_id = match extract_user_id(&state, &headers) {
        Some(id) => id,
        None => return AppError::unauthorized("unauthorized").into_response(),
    };

    let repo_id = match resolve_repo_id(&state.db, &owner, &repo).await {
        Some(id) => id,
        None => return AppError::not_found("repository not found").into_response(),
    };

    match rg_core::wiki::service::create_page(
        &state.db,
        repo_id,
        &body.title,
        &body.content,
        body.message.as_deref(),
        Some(user_id),
    )
    .await
    {
        Ok(page) => (StatusCode::CREATED, Json(serde_json::json!(page_to_response(&page)))).into_response(),
        Err(e) => AppError::bad_request(e).into_response(),
    }
}

#[utoipa::path(
    patch,
    path = "/repos/{owner}/{name}/wiki/{title}",
    tag = "Wiki",
    params(
        ("owner" = String, Path, description = "owner"),
        ("name" = String, Path, description = "name"),
        ("title" = String, Path, description = "title"),
    ),
    request_body(content = serde_json::Value),
    responses(
        (status = 200, description = "Updated", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
    ),
)]
pub async fn update_page(
    State(state): State<AppState>,
    Path((owner, repo, title)): Path<(String, String, String)>,
    headers: HeaderMap,
    Json(body): Json<UpdateWikiPageRequest>,
) -> impl IntoResponse {
    let user_id = match extract_user_id(&state, &headers) {
        Some(id) => id,
        None => return AppError::unauthorized("unauthorized").into_response(),
    };

    let repo_id = match resolve_repo_id(&state.db, &owner, &repo).await {
        Some(id) => id,
        None => return AppError::not_found("repository not found").into_response(),
    };

    match rg_core::wiki::service::update_page(
        &state.db,
        repo_id,
        &title,
        &body.content,
        body.message.as_deref(),
        Some(user_id),
    )
    .await
    {
        Ok(page) => (StatusCode::OK, Json(serde_json::json!(page_to_response(&page)))).into_response(),
        Err(e) => AppError::bad_request(e).into_response(),
    }
}

#[utoipa::path(
    delete,
    path = "/repos/{owner}/{name}/wiki/{title}",
    tag = "Wiki",
    params(
        ("owner" = String, Path, description = "owner"),
        ("name" = String, Path, description = "name"),
        ("title" = String, Path, description = "title"),
    ),
    responses(
        (status = 200, description = "Deleted", body = serde_json::Value),
        (status = 204, description = "No content"),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
    ),
)]
pub async fn delete_page(
    State(state): State<AppState>,
    Path((owner, repo, title)): Path<(String, String, String)>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let _user_id = match extract_user_id(&state, &headers) {
        Some(id) => id,
        None => return AppError::unauthorized("unauthorized").into_response(),
    };

    let repo_id = match resolve_repo_id(&state.db, &owner, &repo).await {
        Some(id) => id,
        None => return AppError::not_found("repository not found").into_response(),
    };

    match rg_core::wiki::service::delete_page(&state.db, repo_id, &title).await {
        Ok(()) => (StatusCode::OK, Json(serde_json::json!({"message": "page deleted"}))).into_response(),
        Err(e) => AppError::bad_request(e).into_response(),
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────

fn extract_user_id(state: &AppState, headers: &axum::http::HeaderMap) -> Option<i64> {
    let auth = headers.get("authorization")?.to_str().ok()?;
    let token = auth.strip_prefix("Bearer ")?;
    let claims = rg_core::auth::jwt::validate_token(token, &state.jwt_secret)?;
    claims.sub.parse().ok()
}

async fn resolve_repo_id(
    db: &DatabaseConnection,
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
