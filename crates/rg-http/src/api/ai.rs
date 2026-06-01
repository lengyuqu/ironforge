//! AI Agent 专用 REST API 端点。
//!
//! 这些端点在 `/api/v1/ai/` 下注册，提供比通用 REST API
//! 更适合 AI Agent 消费的高层语义数据。

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::AppState;
use crate::error::AppError;

// ── Response types ───────────────────────────────────

/// 仓库摘要响应（AI 友好格式）
#[derive(Serialize, ToSchema)]
pub struct RepoSummary {
    pub full_name: String,
    pub description: Option<String>,
    pub default_branch: String,
    pub stars_count: i64,
    pub forks_count: i64,
    pub created_at: String,
    pub updated_at: String,
}

/// Issue 摘要（AI 友好格式）
#[derive(Serialize, ToSchema)]
pub struct IssueSummary {
    pub number: i64,
    pub title: String,
    pub state: String,
    pub author_id: i64,
    pub created_at: String,
}

/// PR 摘要（AI 友好格式）
#[derive(Serialize, ToSchema)]
pub struct PrSummary {
    pub number: i64,
    pub title: String,
    pub state: String,
    pub author_id: i64,
    pub head_branch: String,
    pub base_branch: String,
    pub created_at: String,
}

// ── Query structs ────────────────────────────────────

#[derive(Deserialize)]
pub struct IssueListQuery {
    #[serde(default)]
    pub state: Option<String>,
    #[serde(default)]
    pub limit: Option<i64>,
}

#[derive(Deserialize)]
pub struct PrListQuery {
    #[serde(default)]
    pub state: Option<String>,
    #[serde(default)]
    pub limit: Option<i64>,
}

/// Query params for tree endpoint
#[derive(Deserialize)]
pub struct TreeQuery {
    #[serde(default)]
    pub r#ref: Option<String>,
    #[serde(default)]
    pub path: Option<String>,
}

/// Query params for code search endpoint
#[derive(Deserialize)]
pub struct SearchCodeQuery {
    pub q: String,
    #[serde(default)]
    pub r#ref: Option<String>,
    #[serde(default)]
    pub limit: Option<i64>,
}

// ── Helpers ─────────────────────────────────────

async fn resolve_repo(
    state: &AppState,
    owner: &str,
    name: &str,
) -> Result<rg_db::entities::repository::Model, AppError> {
    rg_core::repo::service::find_repo_by_owner_name(&state.db, owner, name)
        .await
        .map_err(|e| AppError::not_found(format!("repository not found: {}/{}: {}", owner, name, e)))?
        .ok_or_else(|| AppError::not_found(format!("repository not found: {}/{}", owner, name)))
}

// ── Handlers ──────────────────────────────────────

/// GET /api/v1/ai/repos/{owner}/{name}/summary
#[utoipa::path(
    get,
    path = "/api/v1/ai/repos/{owner}/{name}/summary",
    params(
        ("owner" = String, Path, description = "Repository owner"),
        ("name" = String, Path, description = "Repository name"),
    ),
    responses(
        (status = 200, description = "Repository summary", body = RepoSummary),
        (status = 404, description = "Repository not found"),
    ),
    tag = "ai",
)]
pub async fn ai_repo_summary(
    State(state): State<AppState>,
    Path((owner, name)): Path<(String, String)>,
) -> Result<(StatusCode, Json<RepoSummary>), AppError> {
    let repo = resolve_repo(&state, &owner, &name).await?;

    let summary = RepoSummary {
        full_name: format!("{}/{}", owner, name),
        description: repo.description,
        default_branch: repo.default_branch.clone(),
        stars_count: repo.stars_count,
        forks_count: repo.forks_count,
        created_at: repo.created_at.to_rfc3339(),
        updated_at: repo.updated_at.to_rfc3339(),
    };

    Ok((StatusCode::OK, Json(summary)))
}

/// GET /api/v1/ai/repos/{owner}/{name}/issues
#[utoipa::path(
    get,
    path = "/api/v1/ai/repos/{owner}/{name}/issues",
    params(
        ("owner" = String, Path, description = "Repository owner"),
        ("name" = String, Path, description = "Repository name"),
        ("state" = Option<String>, Query, description = "open | closed (default: open)"),
        ("limit" = Option<i64>, Query, description = "Max results (default 20)"),
    ),
    responses(
        (status = 200, description = "Issue list", body = Vec<IssueSummary>),
        (status = 404, description = "Repository not found"),
    ),
    tag = "ai",
)]
pub async fn ai_list_issues(
    State(state): State<AppState>,
    Path((owner, name)): Path<(String, String)>,
    Query(params): Query<IssueListQuery>,
) -> Result<(StatusCode, Json<Vec<IssueSummary>>), AppError> {
    let _repo = resolve_repo(&state, &owner, &name).await?;

    let state_filter = params.state.as_deref().unwrap_or("open");

    let issues = rg_core::issue::service::list_issues(&state.db, &owner, &name, Some(state_filter))
        .await
        .map_err(|e| AppError::internal(format!("DB error: {}", e)))?;

    let limit = params.limit.unwrap_or(20) as usize;
    let summaries = issues.into_iter().take(limit).map(|issue| {
        IssueSummary {
            number: issue.number,
            title: issue.title,
            state: issue.state,
            author_id: issue.author_id,
            created_at: issue.created_at.to_rfc3339(),
        }
    }).collect();

    Ok((StatusCode::OK, Json(summaries)))
}

/// GET /api/v1/ai/repos/{owner}/{name}/prs
#[utoipa::path(
    get,
    path = "/api/v1/ai/repos/{owner}/{name}/prs",
    params(
        ("owner" = String, Path, description = "Repository owner"),
        ("name" = String, Path, description = "Repository name"),
        ("state" = Option<String>, Query, description = "open | closed | merged (default: open)"),
        ("limit" = Option<i64>, Query, description = "Max results (default 20)"),
    ),
    responses(
        (status = 200, description = "PR list", body = Vec<PrSummary>),
        (status = 404, description = "Repository not found"),
    ),
    tag = "ai",
)]
pub async fn ai_list_prs(
    State(state): State<AppState>,
    Path((owner, name)): Path<(String, String)>,
    Query(params): Query<PrListQuery>,
) -> Result<(StatusCode, Json<Vec<PrSummary>>), AppError> {
    let _repo = resolve_repo(&state, &owner, &name).await?;

    let state_filter = params.state.as_deref().unwrap_or("open");

    let prs = rg_core::pull_request::service::list_prs(&state.db, &owner, &name, Some(state_filter))
        .await
        .map_err(|e| AppError::internal(format!("DB error: {}", e)))?;

    let limit = params.limit.unwrap_or(20) as usize;
    let summaries = prs.into_iter().take(limit).map(|pr| {
        PrSummary {
            number: pr.number,
            title: pr.title,
            state: pr.state,
            author_id: pr.author_id,
            head_branch: pr.head_branch,
            base_branch: pr.base_branch,
            created_at: pr.created_at.to_rfc3339(),
        }
    }).collect();

    Ok((StatusCode::OK, Json(summaries)))
}

// ── Stub handlers (NOT IMPLEMENTED) ─────────────────

/// GET /api/v1/ai/repos/{owner}/{name}/tree
#[utoipa::path(
    get,
    path = "/api/v1/ai/repos/{owner}/{name}/tree",
    params(
        ("owner" = String, Path, description = "Repository owner"),
        ("name" = String, Path, description = "Repository name"),
        ("ref" = Option<String>, Query, description = "Branch/tag/SHA (default: default branch)"),
        ("path" = Option<String>, Query, description = "Subdirectory path (default: root)"),
    ),
    responses(
        (status = 200, description = "Repository file tree"),
        (status = 404, description = "Repository not found"),
        (status = 501, description = "Not yet implemented"),
    ),
    tag = "ai",
)]
pub async fn ai_repo_tree(
    State(_state): State<AppState>,
    Path((_owner, _name)): Path<(String, String)>,
    Query(_params): Query<TreeQuery>,
) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(serde_json::json!({"error": "repo_tree not yet implemented"})),
    )
}

/// GET /api/v1/ai/repos/{owner}/{name}/search/code
#[utoipa::path(
    get,
    path = "/api/v1/ai/repos/{owner}/{name}/search/code",
    params(
        ("owner" = String, Path, description = "Repository owner"),
        ("name" = String, Path, description = "Repository name"),
        ("q" = String, Query, description = "Search query"),
        ("ref" = Option<String>, Query, description = "Branch/tag/SHA (default: default branch)"),
        ("limit" = Option<i64>, Query, description = "Max results (default 20)"),
    ),
    responses(
        (status = 200, description = "Code search results"),
        (status = 404, description = "Repository not found"),
        (status = 501, description = "Not yet implemented"),
    ),
    tag = "ai",
)]
pub async fn ai_search_code(
    State(_state): State<AppState>,
    Path((_owner, _name)): Path<(String, String)>,
    Query(_params): Query<SearchCodeQuery>,
) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(serde_json::json!({"error": "search_code not yet implemented"})),
    )
}
