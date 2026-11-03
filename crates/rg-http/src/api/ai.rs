//! AI Agent 专用 REST API 端点。
//!
//! 这些端点在 `/api/v1/ai/` 下注册，提供比通用 REST API
//! 更适合 AI Agent 消费的高层语义数据。

use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::api::repo_access;
use crate::error::AppError;
use crate::AppState;
use sea_orm::{ConnectionTrait, Statement};

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
    #[serde(default)]
    pub depth: Option<u32>,
    #[serde(default)]
    pub limit: Option<usize>,
    #[serde(default)]
    pub cursor: Option<String>,
}

/// Query params for code search endpoint
#[derive(Deserialize)]
pub struct SearchCodeQuery {
    pub q: String,
    #[serde(default)]
    pub r#ref: Option<String>,
    #[serde(default)]
    pub limit: Option<i64>,
    #[serde(default)]
    pub offset: Option<i64>,
}

// ── Handlers ──────────────────────────────────────
//
// Repo resolution + read authorization is centralized in
// [`crate::api::repo_access`] (`require_read`).

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
    headers: HeaderMap,
) -> Result<(StatusCode, Json<RepoSummary>), AppError> {
    let repo = repo_access::require_read(&state, &headers, &owner, &name).await?;

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
    headers: HeaderMap,
    Query(params): Query<IssueListQuery>,
) -> Result<(StatusCode, Json<Vec<IssueSummary>>), AppError> {
    let _repo = repo_access::require_read(&state, &headers, &owner, &name).await?;

    let state_filter = params.state.as_deref().unwrap_or("open");

    let issues = rg_core::issue::service::list_issues(&state.db, &owner, &name, Some(state_filter))
        .await
        .map_err(|e| AppError::internal(format!("DB error: {}", e)))?;

    let limit = params.limit.unwrap_or(20) as usize;
    let summaries = issues
        .into_iter()
        .take(limit)
        .map(|issue| IssueSummary {
            number: issue.number,
            title: issue.title,
            state: issue.state,
            author_id: issue.author_id,
            created_at: issue.created_at.to_rfc3339(),
        })
        .collect();

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
    headers: HeaderMap,
    Query(params): Query<PrListQuery>,
) -> Result<(StatusCode, Json<Vec<PrSummary>>), AppError> {
    let _repo = repo_access::require_read(&state, &headers, &owner, &name).await?;

    let state_filter = params.state.as_deref().unwrap_or("open");

    let prs =
        rg_core::pull_request::service::list_prs(&state.db, &owner, &name, Some(state_filter))
            .await
            .map_err(|e| AppError::internal(format!("DB error: {}", e)))?;

    let limit = params.limit.unwrap_or(20) as usize;
    let summaries = prs
        .into_iter()
        .take(limit)
        .map(|pr| PrSummary {
            number: pr.number,
            title: pr.title,
            state: pr.state,
            author_id: pr.author_id,
            head_branch: pr.head_branch,
            base_branch: pr.base_branch,
            created_at: pr.created_at.to_rfc3339(),
        })
        .collect();

    Ok((StatusCode::OK, Json(summaries)))
}

// ── File tree ───────────────────────────────────────

const TREE_DEFAULT_LIMIT: usize = 200;
const TREE_MAX_LIMIT: usize = 1000;
const TREE_MAX_DEPTH: u32 = 8;

/// One entry of the file tree. `path` is relative to the repository root.
#[derive(Serialize, ToSchema)]
pub struct AiTreeEntry {
    pub path: String,
    /// `blob` | `tree` | `symlink` | `submodule`
    #[serde(rename = "type")]
    pub kind: &'static str,
    /// Byte size, blobs only.
    pub size: Option<u64>,
    pub sha: String,
}

/// File tree page.
#[derive(Serialize, ToSchema)]
pub struct AiTreeResponse {
    /// The ref as requested (or the default branch).
    #[serde(rename = "ref")]
    pub git_ref: String,
    /// Commit the ref resolved to; `None` for a repository without commits.
    pub commit: Option<String>,
    pub path: String,
    pub entries: Vec<AiTreeEntry>,
    /// More entries exist beyond this page.
    pub truncated: bool,
    /// Pass back as `cursor` to fetch the next page.
    pub next_cursor: Option<String>,
}

fn entry_kind(mode: gix::object::tree::EntryMode) -> &'static str {
    if mode.is_tree() {
        "tree"
    } else if mode.is_commit() {
        "submodule"
    } else if mode.is_link() {
        "symlink"
    } else {
        "blob"
    }
}

/// Depth-first walk collecting at most `limit` entries; returns `true` once
/// the limit was hit so the caller can stop descending.
fn collect_tree(
    repo: &gix::Repository,
    tree_id: gix::ObjectId,
    prefix: &str,
    depth_left: u32,
    limit: usize,
    out: &mut Vec<AiTreeEntry>,
) -> rg_core::error::CoreResult<bool> {
    use rg_core::error::CoreError;

    let tree = repo
        .find_tree(tree_id)
        .map_err(|e| CoreError::internal(format!("failed to read tree: {e}")))?;
    for entry in tree.iter() {
        let entry = entry.map_err(|e| CoreError::internal(format!("corrupt tree entry: {e}")))?;
        if out.len() >= limit {
            return Ok(true);
        }
        let oid = entry.oid().to_owned();
        let mode = entry.mode();
        let name = entry.filename().to_string();
        let path = if prefix.is_empty() {
            name
        } else {
            format!("{prefix}/{name}")
        };
        let kind = entry_kind(mode);
        let size = if kind == "blob" {
            repo.find_header(oid).ok().map(|h| h.size())
        } else {
            None
        };
        out.push(AiTreeEntry {
            path: path.clone(),
            kind,
            size,
            sha: oid.to_string(),
        });
        if kind == "tree"
            && depth_left > 1
            && collect_tree(repo, oid, &path, depth_left - 1, limit, out)?
        {
            return Ok(true);
        }
    }
    Ok(false)
}

fn list_ai_tree(
    repo_path: &std::path::Path,
    git_ref: &str,
    sub_path: &str,
    depth: u32,
    skip: usize,
    take: usize,
) -> rg_core::error::CoreResult<AiTreeResponse> {
    use rg_core::error::CoreError;

    let empty = |git_ref: &str| AiTreeResponse {
        git_ref: git_ref.to_string(),
        commit: None,
        path: sub_path.to_string(),
        entries: Vec::new(),
        truncated: false,
        next_cursor: None,
    };

    let repo = crate::api::repo_content::open_repo(repo_path)?;
    let commit_id = match repo.rev_parse_single(git_ref) {
        Ok(id) => id.detach(),
        // A repository without commits has an unborn HEAD: an empty tree,
        // not an error.
        Err(_) if crate::api::repo_content::is_empty_repo(repo_path) => {
            return Ok(empty(git_ref));
        }
        Err(e) => {
            tracing::debug!(error = %e, git_ref, "ref did not resolve");
            return Err(CoreError::not_found(format!("ref '{git_ref}' not found")));
        }
    };
    let commit = repo
        .find_commit(commit_id)
        .map_err(|e| CoreError::internal(format!("failed to find commit: {e}")))?;
    let mut tree_id = commit
        .tree_id()
        .map_err(|e| CoreError::internal(format!("failed to read commit tree: {e}")))?
        .detach();

    for component in sub_path.split('/').filter(|c| !c.is_empty()) {
        let tree = repo
            .find_tree(tree_id)
            .map_err(|e| CoreError::internal(format!("failed to read tree: {e}")))?;
        let next = tree
            .iter()
            .filter_map(|e| e.ok())
            .find(|e| e.filename() == component && e.mode().is_tree())
            .map(|e| e.oid().to_owned())
            .ok_or_else(|| CoreError::not_found(format!("path not found: {sub_path}")))?;
        tree_id = next;
    }

    // One past the page so we can tell whether more exists.
    let mut collected = Vec::new();
    collect_tree(
        &repo,
        tree_id,
        sub_path.trim_matches('/'),
        depth,
        skip.saturating_add(take).saturating_add(1),
        &mut collected,
    )?;
    let truncated = collected.len() > skip + take;
    let entries: Vec<AiTreeEntry> = collected.into_iter().skip(skip).take(take).collect();
    let next_cursor = truncated.then(|| (skip + entries.len()).to_string());

    Ok(AiTreeResponse {
        git_ref: git_ref.to_string(),
        commit: Some(commit_id.to_string()),
        path: sub_path.to_string(),
        entries,
        truncated,
        next_cursor,
    })
}

/// GET /api/v1/ai/repos/{owner}/{name}/tree
#[utoipa::path(
    get,
    path = "/api/v1/ai/repos/{owner}/{name}/tree",
    params(
        ("owner" = String, Path, description = "Repository owner"),
        ("name" = String, Path, description = "Repository name"),
        ("ref" = Option<String>, Query, description = "Branch/tag/SHA (default: default branch)"),
        ("path" = Option<String>, Query, description = "Subdirectory path (default: root)"),
        ("depth" = Option<u32>, Query, description = "Directory levels to descend (default 1, max 8)"),
        ("limit" = Option<usize>, Query, description = "Max entries per page (default 200, max 1000)"),
        ("cursor" = Option<String>, Query, description = "next_cursor from the previous page"),
    ),
    responses(
        (status = 200, description = "Repository file tree page", body = AiTreeResponse),
        (status = 400, description = "Invalid cursor"),
        (status = 404, description = "Repository, ref or path not found"),
    ),
    tag = "ai",
)]
pub async fn ai_repo_tree(
    State(state): State<AppState>,
    Path((owner, name)): Path<(String, String)>,
    headers: HeaderMap,
    Query(params): Query<TreeQuery>,
) -> Result<(StatusCode, Json<AiTreeResponse>), AppError> {
    let repo = repo_access::require_read(&state, &headers, &owner, &name).await?;

    let skip = match params.cursor.as_deref() {
        None | Some("") => 0,
        Some(c) => c
            .parse::<usize>()
            .map_err(|_| AppError::bad_request("invalid cursor"))?,
    };
    let take = params
        .limit
        .unwrap_or(TREE_DEFAULT_LIMIT)
        .clamp(1, TREE_MAX_LIMIT);
    let depth = params.depth.unwrap_or(1).clamp(1, TREE_MAX_DEPTH);
    let git_ref = params.r#ref.unwrap_or_else(|| repo.default_branch.clone());
    let sub_path = params
        .path
        .unwrap_or_default()
        .trim_matches('/')
        .to_string();

    let repo_path = state.repo_root.join(format!("{}/{}.git", owner, name));
    if !repo_path.exists() {
        return Err(AppError::not_found("repository not found on disk"));
    }

    let listing = crate::api::repo_content::run_blocking(move || {
        list_ai_tree(&repo_path, &git_ref, &sub_path, depth, skip, take)
    })
    .await?;
    Ok((StatusCode::OK, Json(listing)))
}

/// Code search result for AI API
#[derive(Serialize, ToSchema)]
pub struct CodeSearchResult {
    pub repo_id: i64,
    pub file_path: String,
    pub file_name: String,
    pub language: String,
    pub snippet: String,
}

/// GET /api/v1/ai/repos/{owner}/{name}/search/code
#[utoipa::path(
    get,
    path = "/api/v1/ai/repos/{owner}/{name}/search/code",
    params(
        ("owner" = String, Path, description = "Repository owner"),
        ("name" = String, Path, description = "Repository name"),
        ("q" = String, Query, description = "Search query"),
        ("ref" = Option<String>, Query, description = "Only the default branch is indexed; any other ref is rejected with 400"),
        ("limit" = Option<i64>, Query, description = "Max results (default 20, max 100)"),
        ("offset" = Option<i64>, Query, description = "Results to skip (default 0); total in the x-total-count header"),
    ),
    responses(
        (status = 200, description = "Code search results; x-total-count carries the total match count", body = Vec<CodeSearchResult>),
        (status = 400, description = "Repository not indexed, or ref is not the indexed branch"),
        (status = 404, description = "Repository not found"),
    ),
    tag = "ai",
)]
pub async fn ai_search_code(
    State(state): State<AppState>,
    Path((owner, name)): Path<(String, String)>,
    headers: HeaderMap,
    Query(params): Query<SearchCodeQuery>,
) -> Result<axum::response::Response, AppError> {
    let repo = repo_access::require_read(&state, &headers, &owner, &name).await?;

    // The index is built from the default branch only; silently answering a
    // query about another ref from it would return results for the wrong tree.
    if let Some(requested) = params.r#ref.as_deref() {
        let requested = requested.strip_prefix("refs/heads/").unwrap_or(requested);
        if !requested.is_empty() && requested != repo.default_branch {
            return Err(AppError::bad_request(format!(
                "code search only covers the default branch '{}', not '{}'",
                repo.default_branch, requested
            )));
        }
    }

    let limit = params.limit.unwrap_or(20).clamp(1, 100) as u64;
    let offset = params.offset.unwrap_or(0).max(0) as u64;

    let indexer = rg_core::search::code_indexer::CodeIndexer::new(state.db.clone());

    // Check if repo is indexed
    let backend = state.db.get_database_backend();
    let check_sql = rg_db::prepare_sql(
        backend,
        "SELECT COUNT(*) as cnt FROM code_fts WHERE repo_id = ?",
    );
    let check_result = state
        .db
        .query_one(Statement::from_sql_and_values(
            backend,
            check_sql,
            [repo.id.into()],
        ))
        .await
        .map_err(|e| AppError::internal(format!("DB error: {}", e)))?
        .ok_or_else(|| AppError::internal("Failed to check index status".to_string()))?;
    let indexed_count: i64 = check_result
        .try_get_by_index(0)
        .map_err(|e| AppError::internal(format!("DB error: {}", e)))?;

    if indexed_count == 0 {
        return Err(AppError::bad_request(
            "Repository not indexed. Please trigger indexing first by pushing to the repository or calling the index endpoint.".to_string()
        ));
    }

    let (results, total) = indexer
        .search_code(&params.q, Some(repo.id), limit, offset)
        .await
        .map_err(|e| AppError::internal(format!("Search error: {}", e)))?;

    let api_results: Vec<CodeSearchResult> = results
        .into_iter()
        .map(|r| CodeSearchResult {
            repo_id: r.repo_id,
            file_path: r.file_path,
            file_name: r.file_name,
            language: r.language,
            snippet: r.snippet,
        })
        .collect();

    Ok((
        StatusCode::OK,
        [("x-total-count", total.to_string())],
        Json(api_results),
    )
        .into_response())
}

/// Response for index trigger.
#[derive(Serialize, ToSchema)]
pub struct IndexResponse {
    pub indexed_files: usize,
}

/// POST /api/v1/ai/repos/{owner}/{name}/index
#[utoipa::path(
    post,
    path = "/api/v1/ai/repos/{owner}/{name}/index",
    params(
        ("owner" = String, Path, description = "Repository owner"),
        ("name" = String, Path, description = "Repository name"),
    ),
    responses(
        (status = 200, description = "Indexing triggered successfully", body = IndexResponse),
        (status = 404, description = "Repository not found"),
        (status = 500, description = "Indexing error"),
    ),
    tag = "ai",
)]
pub async fn ai_index_repository(
    State(state): State<AppState>,
    Path((owner, name)): Path<(String, String)>,
    headers: axum::http::HeaderMap,
    Json(_body): Json<serde_json::Value>,
) -> impl IntoResponse {
    // Resolve repository + require read access
    let repo = match repo_access::require_read(&state, &headers, &owner, &name).await {
        Ok(r) => r,
        Err(e) => return e.into_response(),
    };

    let repo_path = state.repo_root.join(format!("{}/{}.git", owner, name));
    if !repo_path.exists() {
        return AppError::not_found("repository not found on disk").into_response();
    }

    // Placeholder - will call indexer later
    // Actually call the indexer
    let indexer = rg_core::search::code_indexer::CodeIndexer::new(state.db.clone());
    match indexer
        .index_repository(repo.id, &repo_path, &repo.default_branch)
        .await
    {
        Ok(count) => (
            StatusCode::OK,
            Json(IndexResponse {
                indexed_files: count,
            }),
        )
            .into_response(),
        Err(e) => AppError::internal(format!("Indexing error: {}", e)).into_response(),
    }
}
