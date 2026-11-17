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
use rg_core::search::map;
use rg_core::search::outline::{extract_outline, Language};
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

// ── Repository map ──────────────────────────────────
//
// `GET /ai/repos/{owner}/{name}/map` walks the tree at `ref`, parses every
// source blob with the T09 outline extractor and hands the result to
// `rg_core::search::map::build`, which trims it to the token budget. Nothing is
// persisted: this is a live parse of the requested commit.

/// Query params for the map endpoint.
#[derive(Deserialize)]
pub struct MapQuery {
    #[serde(default)]
    pub r#ref: Option<String>,
    #[serde(default)]
    pub path: Option<String>,
    /// Parsed by hand so a non-numeric value produces `INVALID_BUDGET` instead
    /// of axum's generic query rejection.
    #[serde(default)]
    pub budget: Option<String>,
}

/// Upper bound on outline parsing threads inside one blocking task. Parsing is
/// CPU-bound; more threads than this only adds contention.
const MAP_PARSE_THREADS: usize = 8;

fn parse_budget(raw: Option<&str>) -> Result<usize, AppError> {
    let Some(raw) = raw else {
        return Ok(map::DEFAULT_BUDGET);
    };
    let invalid = || {
        AppError::coded(
            StatusCode::BAD_REQUEST,
            "INVALID_BUDGET",
            format!(
                "budget must be an integer between {} and {}",
                map::MIN_BUDGET,
                map::MAX_BUDGET
            ),
        )
    };
    let value: usize = raw.trim().parse().map_err(|_| invalid())?;
    if !(map::MIN_BUDGET..=map::MAX_BUDGET).contains(&value) {
        return Err(invalid());
    }
    Ok(value)
}

/// A source blob found while walking the tree.
struct SourceBlob {
    path: String,
    oid: gix::ObjectId,
}

/// Walk `tree_id` under `prefix`, collecting source files and reporting
/// everything else as `unsupported language`.
///
/// Symlinks and submodules are pointers, not source files, so they are left out
/// silently instead of claiming they have an unsupported language.
fn collect_source_blobs(
    repo: &gix::Repository,
    tree_id: gix::ObjectId,
    prefix: &str,
    out: &mut Vec<SourceBlob>,
    skipped: &mut Vec<map::MapSkipped>,
) -> rg_core::error::CoreResult<()> {
    use rg_core::error::CoreError;

    let tree = repo
        .find_tree(tree_id)
        .map_err(|e| CoreError::internal(format!("failed to read tree: {e}")))?;
    for entry in tree.iter() {
        let entry = entry.map_err(|e| CoreError::internal(format!("corrupt tree entry: {e}")))?;
        let name = entry.filename().to_string();
        let path = if prefix.is_empty() {
            name
        } else {
            format!("{prefix}/{name}")
        };
        let oid = entry.oid().to_owned();
        if entry.mode().is_tree() {
            collect_source_blobs(repo, oid, &path, out, skipped)?;
        } else if entry.mode().is_blob() {
            if !Language::from_path(&path).is_supported() {
                skipped.push(map::MapSkipped::new(
                    path,
                    map::SkipReason::UnsupportedLanguage,
                ));
                continue;
            }
            let size = repo.find_header(oid).map(|h| h.size()).unwrap_or(u64::MAX);
            // Rejecting oversized blobs here keeps them out of the parse queue
            // (and out of the MAX_FILES admission) without reading them.
            if size > rg_core::search::outline::MAX_FILE_BYTES as u64 {
                skipped.push(map::MapSkipped::new(path, map::SkipReason::TooLarge));
                continue;
            }
            out.push(SourceBlob { path, oid });
        }
    }
    Ok(())
}

#[derive(Default)]
struct ParseOutput {
    files: Vec<map::MappedFile>,
    skipped: Vec<map::MapSkipped>,
}

fn read_source(repo: &gix::Repository, blob: &SourceBlob) -> Result<String, map::SkipReason> {
    let object = repo
        .find_object(blob.oid)
        .map_err(|_| map::SkipReason::Unreadable)?;
    let blob = object
        .try_into_blob()
        .map_err(|_| map::SkipReason::Unreadable)?;
    if blob.data.len() > rg_core::search::outline::MAX_FILE_BYTES {
        return Err(map::SkipReason::TooLarge);
    }
    if blob.data.contains(&0) {
        return Err(map::SkipReason::Binary);
    }
    std::str::from_utf8(&blob.data)
        .map(|source| source.to_string())
        .map_err(|_| map::SkipReason::Unreadable)
}

fn parse_chunk(repo_path: &std::path::Path, chunk: Vec<SourceBlob>) -> ParseOutput {
    let mut out = ParseOutput::default();
    let repo = match crate::api::repo_content::open_repo(repo_path) {
        Ok(repo) => repo,
        Err(_) => {
            out.skipped.extend(
                chunk
                    .into_iter()
                    .map(|blob| map::MapSkipped::new(blob.path, map::SkipReason::Unreadable)),
            );
            return out;
        }
    };
    for blob in chunk {
        match read_source(&repo, &blob) {
            Ok(source) => out.files.push(map::MappedFile {
                path: blob.path.clone(),
                outline: extract_outline(&blob.path, &source),
            }),
            Err(reason) => out.skipped.push(map::MapSkipped::new(blob.path, reason)),
        }
    }
    out
}

/// Parse every blob, with at most [`MAP_PARSE_THREADS`] workers. Each worker
/// opens the repository once; results keep the input order.
fn parse_blobs(repo_path: &std::path::Path, blobs: Vec<SourceBlob>) -> ParseOutput {
    if blobs.is_empty() {
        return ParseOutput::default();
    }
    let threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .min(MAP_PARSE_THREADS)
        .min(blobs.len());
    let chunk_size = blobs.len().div_ceil(threads).max(1);

    let mut chunks: Vec<Vec<SourceBlob>> = Vec::new();
    let mut rest = blobs;
    while !rest.is_empty() {
        let tail = rest.split_off(chunk_size.min(rest.len()));
        chunks.push(rest);
        rest = tail;
    }

    let mut out = ParseOutput::default();
    let results: Vec<ParseOutput> = std::thread::scope(|scope| {
        let handles: Vec<_> = chunks
            .into_iter()
            .map(|chunk| scope.spawn(move || parse_chunk(repo_path, chunk)))
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().expect("outline parse worker panicked"))
            .collect()
    });
    for part in results {
        out.files.extend(part.files);
        out.skipped.extend(part.skipped);
    }
    out
}

fn build_repo_map_blocking(
    repo_path: &std::path::Path,
    git_ref: &str,
    sub_path: &str,
    budget: usize,
) -> rg_core::error::CoreResult<map::RepoMap> {
    use rg_core::error::CoreError;

    let repo = crate::api::repo_content::open_repo(repo_path)?;
    let commit_id = match repo.rev_parse_single(git_ref) {
        Ok(id) => id.detach(),
        // A repository with no commits has an unborn HEAD: a valid, empty map.
        Err(_) if crate::api::repo_content::is_empty_repo(repo_path) => {
            return Ok(map::build(git_ref, None, Vec::new(), Vec::new(), budget));
        }
        Err(e) => {
            tracing::debug!(error = %e, git_ref, "map ref did not resolve");
            return Err(CoreError::coded(
                404,
                "REF_NOT_FOUND",
                format!("ref '{git_ref}' not found"),
            ));
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
        tree_id = tree
            .iter()
            .filter_map(|entry| entry.ok())
            .find(|entry| entry.filename() == component && entry.mode().is_tree())
            .map(|entry| entry.oid().to_owned())
            .ok_or_else(|| CoreError::not_found(format!("path not found: {sub_path}")))?;
    }

    let mut blobs = Vec::new();
    let mut skipped = Vec::new();
    collect_source_blobs(
        &repo,
        tree_id,
        sub_path.trim_matches('/'),
        &mut blobs,
        &mut skipped,
    )?;

    // Deterministic admission: the first MAX_FILES paths sorted by path.
    blobs.sort_by(|a, b| a.path.cmp(&b.path));
    if blobs.len() > map::MAX_FILES {
        for blob in blobs.split_off(map::MAX_FILES) {
            skipped.push(map::MapSkipped::new(
                blob.path,
                map::SkipReason::TooManyFiles,
            ));
        }
    }

    let parsed = parse_blobs(repo_path, blobs);
    skipped.extend(parsed.skipped);

    Ok(map::build(
        git_ref,
        Some(commit_id.to_string()),
        parsed.files,
        skipped,
        budget,
    ))
}

/// GET /api/v1/ai/repos/{owner}/{name}/map
#[utoipa::path(
    get,
    path = "/api/v1/ai/repos/{owner}/{name}/map",
    params(
        ("owner" = String, Path, description = "Repository owner"),
        ("name" = String, Path, description = "Repository name"),
        ("ref" = Option<String>, Query, description = "Branch/tag/SHA (default: default branch)"),
        ("path" = Option<String>, Query, description = "Subdirectory to map (default: repository root)"),
        ("budget" = Option<usize>, Query, description = "Token budget (default 4000, 64..=32000); non-numeric or out of range is 400 INVALID_BUDGET"),
    ),
    responses(
        (status = 200, description = "Budget-trimmed repository map. `directories` rolls up source files per directory; `files` lists every mapped file ranked by structural importance, with `items_omitted: true` when its items did not fit; `skipped` names each excluded path and why (`unsupported language`, `too many files`, `file too large`, `binary content`, `unreadable`, `parse error`, `budget`). `estimated_tokens` is ceil(json_chars / 4) over this body and never exceeds `budget`; `truncated` is true only when content was dropped for budget reasons. Svelte files carry a `notice` because only their <script> blocks are parsed.", body = serde_json::Value),
        (status = 400, description = "INVALID_BUDGET"),
        (status = 404, description = "Repository, ref (REF_NOT_FOUND) or path not found"),
    ),
    tag = "ai",
)]
pub async fn ai_repo_map(
    State(state): State<AppState>,
    Path((owner, name)): Path<(String, String)>,
    headers: HeaderMap,
    Query(params): Query<MapQuery>,
) -> Result<(StatusCode, Json<map::RepoMap>), AppError> {
    let repo = repo_access::require_read(&state, &headers, &owner, &name).await?;
    let budget = parse_budget(params.budget.as_deref())?;
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

    let map = crate::api::repo_content::run_blocking(move || {
        build_repo_map_blocking(&repo_path, &git_ref, &sub_path, budget)
    })
    .await?;
    Ok((StatusCode::OK, Json(map)))
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

/// Body of `POST /ai/repos/{owner}/{name}/index`.
#[derive(Deserialize, ToSchema)]
pub struct IndexRequest {
    /// Commit (or ref) to index. Defaults to the repository's default branch.
    #[serde(default)]
    pub commit_sha: Option<String>,
}

/// Response for index trigger.
#[derive(Serialize, ToSchema)]
pub struct IndexResponse {
    /// Files written to `code_fts` by the full-text pass.
    pub indexed_files: usize,
    /// Commit both passes were built from.
    pub commit_sha: String,
    /// `symbol_commits.id` of that commit.
    pub commit_id: i32,
    /// Rows written to `symbol_definitions`.
    pub definitions_written: usize,
    /// `symbol_commits` rows removed by the retention prune.
    pub pruned_commits: usize,
}

/// POST /api/v1/ai/repos/{owner}/{name}/index
#[utoipa::path(
    post,
    path = "/api/v1/ai/repos/{owner}/{name}/index",
    params(
        ("owner" = String, Path, description = "Repository owner"),
        ("name" = String, Path, description = "Repository name"),
    ),
    request_body = IndexRequest,
    responses(
        (status = 200, description = "Full-text and symbol index refreshed; the report covers both", body = IndexResponse),
        (status = 403, description = "Write access to the repository is required"),
        (status = 404, description = "Repository, or the requested commit, not found"),
        (status = 500, description = "Indexing error"),
    ),
    tag = "ai",
)]
pub async fn ai_index_repository(
    State(state): State<AppState>,
    Path((owner, name)): Path<(String, String)>,
    headers: axum::http::HeaderMap,
    Json(body): Json<IndexRequest>,
) -> Result<(StatusCode, Json<IndexResponse>), AppError> {
    // Indexing is an expensive write: reading a public repository is not enough.
    let (repo, _actor_id) = repo_access::require_write(&state, &headers, &owner, &name).await?;

    let repo_path = state.repo_root.join(format!("{}/{}.git", owner, name));
    if !repo_path.exists() {
        return Err(AppError::not_found("repository not found on disk"));
    }

    // `commit_sha` is any revision the symbol indexer can resolve; the resolved
    // 40-hex SHA comes back in the report and is what both passes use.
    let commit_ref = body
        .commit_sha
        .filter(|sha| !sha.trim().is_empty())
        .unwrap_or_else(|| repo.default_branch.clone());

    let report = rg_core::search::symbol_indexer::index_commit(
        &state.db,
        &repo_path,
        repo.id,
        &commit_ref,
        &repo.default_branch,
    )
    .await
    .map_err(AppError::from)?;

    let indexer = rg_core::search::code_indexer::CodeIndexer::new(state.db.clone());
    let indexed_files = indexer
        .index_repository(repo.id, &repo_path, &report.commit_sha)
        .await
        .map_err(|error| AppError::internal(format!("Indexing error: {error}")))?;

    Ok((
        StatusCode::OK,
        Json(IndexResponse {
            indexed_files,
            commit_sha: report.commit_sha,
            commit_id: report.commit_id,
            definitions_written: report.definitions_written,
            pruned_commits: report.pruned_commits,
        }),
    ))
}

// ── Symbol definitions ──────────────────────────────
//
// `GET /ai/repos/{owner}/{name}/symbols` reads the definitions T28 indexed
// (#63 step 3). The query itself lives in
// [`rg_core::search::symbol_query::find_symbols`] so the MCP tool and this
// endpoint cannot drift apart.

/// Query params for the symbol endpoint.
#[derive(Deserialize)]
pub struct SymbolQuery {
    /// Symbol name: exact matches first, then prefix matches.
    #[serde(default)]
    pub name: Option<String>,
    /// Repository-relative path; lists that file's declarations.
    #[serde(default)]
    pub path: Option<String>,
    /// Branch/tag/SHA; defaults to the repository's default branch.
    #[serde(default)]
    pub r#ref: Option<String>,
    /// Explicit commit. Takes precedence over `ref` because it is the more
    /// specific request; both are resolved to a full 40-hex SHA before use.
    #[serde(default)]
    pub commit_sha: Option<String>,
    #[serde(default)]
    pub limit: Option<usize>,
}

/// One definition row.
#[derive(Serialize, ToSchema)]
pub struct AiSymbolItem {
    pub name: String,
    /// `function` | `method` | `struct` | `enum` | `trait` | `class` |
    /// `interface` | `module` | `const` | `macro` | `type` | `other`.
    /// `impl` rows are never returned: their name is a first line, not an
    /// identifier.
    pub kind: String,
    pub path: String,
    pub start_line: i32,
    pub end_line: i32,
    /// First source line, whitespace-collapsed; `null` when the extractor had
    /// none.
    pub signature: Option<String>,
    /// `exact` — the name equals the query verbatim (or the query was by
    /// `path`, and this row is that file's declaration). `name_match` — only
    /// the prefix matched, so this row is a candidate, not a confirmed answer.
    pub confidence: String,
}

/// A page of definitions for one commit.
#[derive(Serialize, ToSchema)]
pub struct AiSymbolsResponse {
    /// The full 40-hex commit the request resolved to.
    pub commit_sha: String,
    /// The index for this commit is known to be incomplete; absence of a row is
    /// not proof the symbol does not exist.
    pub partial: bool,
    /// What static parsing cannot see. Never empty.
    pub limitations: Vec<String>,
    pub items: Vec<AiSymbolItem>,
}

/// Resolve `git_ref` to a full commit SHA.
///
/// Deliberately not "resolve, and if it fails use the default branch": the
/// caller asked about a specific revision.
fn resolve_symbol_commit(
    repo_path: &std::path::Path,
    git_ref: &str,
) -> rg_core::error::CoreResult<String> {
    use rg_core::error::CoreError;

    let repo = crate::api::repo_content::open_repo(repo_path)?;
    let id = repo
        .rev_parse_single(git_ref)
        .map_err(|error| {
            tracing::debug!(error = %error, git_ref, "symbol query ref did not resolve");
            CoreError::coded(404, "REF_NOT_FOUND", format!("ref '{git_ref}' not found"))
        })?
        .detach();
    // A tag object or a tree resolves too; only a commit has definitions.
    repo.find_commit(id).map_err(|_| {
        CoreError::coded(
            404,
            "REF_NOT_FOUND",
            format!("ref '{git_ref}' does not name a commit"),
        )
    })?;
    Ok(id.to_string())
}

/// GET /api/v1/ai/repos/{owner}/{name}/symbols
#[utoipa::path(
    get,
    path = "/api/v1/ai/repos/{owner}/{name}/symbols",
    params(
        ("owner" = String, Path, description = "Repository owner"),
        ("name" = String, Path, description = "Repository name"),
        ("name" = Option<String>, Query, description = "Symbol name: exact matches first, then prefix matches. Give this, `path`, or both."),
        ("path" = Option<String>, Query, description = "Repository-relative file path: list that file's declarations. Give this, `name`, or both."),
        ("ref" = Option<String>, Query, description = "Branch/tag/SHA (default: default branch)"),
        ("commit_sha" = Option<String>, Query, description = "Explicit commit; takes precedence over `ref`"),
        ("limit" = Option<usize>, Query, description = "Max rows (default 50, max 200); more than 200 is 400 INVALID_LIMIT, never a silent clamp"),
    ),
    responses(
        (status = 200, description = "Definitions of that commit. Every row carries `confidence`: `exact` when the name equals the query (or the query was by path), `name_match` when only the prefix matched. `partial` is true and `limitations` names what static parsing cannot see (svelte only inside <script>, Rust macros and dyn Trait, TS obj[key]/any, Python getattr, Go interface calls, unrecorded parse failures), so a missing row is not proof the symbol is absent. `impl` rows are excluded. An unindexed commit is 404 SYMBOLS_NOT_INDEXED and is never answered from another commit.", body = AiSymbolsResponse),
        (status = 400, description = "Neither `name` nor `path` given, or INVALID_LIMIT"),
        (status = 404, description = "Repository, ref (REF_NOT_FOUND), or the commit's symbol index (SYMBOLS_NOT_INDEXED) not found"),
    ),
    tag = "ai",
)]
pub async fn ai_repo_symbols(
    State(state): State<AppState>,
    Path((owner, name)): Path<(String, String)>,
    headers: HeaderMap,
    Query(params): Query<SymbolQuery>,
) -> Result<(StatusCode, Json<AiSymbolsResponse>), AppError> {
    let repo = repo_access::require_read(&state, &headers, &owner, &name).await?;

    let symbol_name = params
        .name
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty());
    let symbol_path = params
        .path
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty());
    if symbol_name.is_none() && symbol_path.is_none() {
        return Err(AppError::bad_request("one of 'name' or 'path' is required"));
    }

    // Above the maximum is refused rather than clamped: a clamped answer looks
    // complete and is not.
    let limit = params.limit.unwrap_or(DEFAULT_SYMBOL_LIMIT);
    if limit == 0 || limit > MAX_SYMBOL_LIMIT {
        return Err(AppError::coded(
            StatusCode::BAD_REQUEST,
            "INVALID_LIMIT",
            format!("limit must be between 1 and {MAX_SYMBOL_LIMIT}"),
        ));
    }

    let repo_path = state.repo_root.join(format!("{}/{}.git", owner, name));
    if !repo_path.exists() {
        return Err(AppError::not_found("repository not found on disk"));
    }

    let requested = params
        .commit_sha
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .or_else(|| {
            params
                .r#ref
                .as_deref()
                .map(str::trim)
                .filter(|v| !v.is_empty())
        })
        .unwrap_or(&repo.default_branch)
        .to_string();

    let commit_sha = crate::api::repo_content::run_blocking(move || {
        resolve_symbol_commit(&repo_path, &requested)
    })
    .await?;

    let page = rg_core::search::symbol_query::find_symbols(
        &state.db,
        repo.id,
        &commit_sha,
        symbol_name,
        symbol_path,
        limit,
    )
    .await
    .map_err(|error| symbol_error(error, &owner, &name, &commit_sha))?;

    let items = page
        .items
        .into_iter()
        .map(|item| AiSymbolItem {
            name: item.name,
            kind: item.kind,
            path: item.path,
            start_line: item.start_line,
            end_line: item.end_line,
            signature: item.signature,
            confidence: item.confidence.as_str().to_string(),
        })
        .collect();

    Ok((
        StatusCode::OK,
        Json(AiSymbolsResponse {
            commit_sha: page.commit_sha,
            partial: page.partial,
            limitations: page.limitations,
            items,
        }),
    ))
}

const DEFAULT_SYMBOL_LIMIT: usize = rg_core::search::symbol_query::DEFAULT_LIMIT;
const MAX_SYMBOL_LIMIT: usize = rg_core::search::symbol_query::MAX_LIMIT;

/// Map a core query error to the REST contract, adding the "index it first"
/// hint to `SYMBOLS_NOT_INDEXED`.
fn symbol_error(
    error: rg_core::error::CoreError,
    owner: &str,
    name: &str,
    commit_sha: &str,
) -> AppError {
    match error {
        rg_core::error::CoreError::Coded {
            status: 404,
            code: rg_core::search::symbol_query::NOT_INDEXED_CODE,
            message,
        } => AppError::coded(StatusCode::NOT_FOUND, "SYMBOLS_NOT_INDEXED", message).with_reason(
            "symbols_not_indexed",
            true,
            format!(
                "POST /api/v1/ai/repos/{owner}/{name}/index with {{\"commit_sha\": \"{commit_sha}\"}} \
                 to index this commit, then retry"
            ),
        ),
        other => AppError::from(other),
    }
}
