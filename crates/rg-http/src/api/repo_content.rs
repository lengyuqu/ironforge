//! REST API handlers for repository content browsing (tree, blob, history).

use anyhow::Context;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::error::AppError;
use crate::AppState;
use utoipa::ToSchema;

// ── Request / Response types ──────────────────────────────────────────

#[derive(Deserialize)]
pub struct TreeQuery {
    /// Git ref (branch, tag, commit SHA). Default: HEAD
    #[serde(default)]
    pub r#ref: Option<String>,
    /// Sub-path within the tree. Default: root
    #[serde(default)]
    pub path: Option<String>,
}

#[derive(Deserialize)]
pub struct BlobQuery {
    #[serde(default)]
    pub r#ref: Option<String>,
}

#[derive(Deserialize)]
pub struct LogQuery {
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub limit: Option<i64>,
}

#[derive(Serialize)]
pub struct TreeEntry {
    pub name: String,
    pub path: String,
    pub kind: String, // "tree" | "blob"
    pub size: Option<i64>,
    pub sha: Option<String>,
}

#[derive(Serialize)]
pub struct BlobContent {
    pub path: String,
    pub sha: String,
    pub size: i64,
    pub content: String,
    pub encoding: String, // "utf-8" | "base64"
    pub is_binary: bool,
}

#[derive(Serialize)]
pub struct CommitEntry {
    pub sha: String,
    pub author_name: String,
    pub author_email: String,
    pub author_date: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gpg_signature: Option<GpgSignature>,
}

/// GPG signature information for a commit.
#[derive(Serialize)]
pub struct GpgSignature {
    pub verified: bool,
    pub signer_key: Option<String>,
    pub signer_name: Option<String>,
    pub signer_email: Option<String>,
    pub status: String,
}

// ── Handlers ──────────────────────────────────────────────────────────

/// List tree entries (directory listing) for a repo.
/// GET /api/v1/repos/:owner/:name/tree
#[utoipa::path(
    get,
    path = "/repos/{owner}/{name}/tree",
    tag = "Repository Content",
    params(
        ("owner" = String, Path, description = "owner"),
        ("name" = String, Path, description = "name"),
    ),
    responses(
        (status = 200, description = "Success", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
    ),
)]
pub async fn list_tree(
    State(state): State<AppState>,
    Path((owner, repo)): Path<(String, String)>,
    Query(params): Query<TreeQuery>,
) -> impl IntoResponse {
    // H-02: Validate owner/repo before constructing repository path
    if let Err(e) = rg_core::platform::validate_repo_path(&owner) {
        return AppError::bad_request(e.to_string()).into_response();
    }
    if let Err(e) = rg_core::platform::validate_repo_path(&repo) {
        return AppError::bad_request(e.to_string()).into_response();
    }

    let repo_path = state.repo_root.join(format!("{}/{}.git", owner, repo));
    if !repo_path.exists() {
        return AppError::not_found("repository not found").into_response();
    }

    let git_ref = params.r#ref.unwrap_or_else(|| "HEAD".to_string());
    let sub_path = params.path.unwrap_or_default();

    let result = list_tree_entries(&repo_path, &git_ref, &sub_path);

    match result {
        Ok(entries) => (StatusCode::OK, Json(entries)).into_response(),
        Err(e) => {
            tracing::error!(%e, "list_tree failed");
            AppError::internal(e).into_response()
        }
    }
}

/// Get blob (file) content.
/// GET /api/v1/repos/:owner/:name/blob/:path
#[utoipa::path(
    get,
    path = "/repos/{owner}/{name}/blob/{*path}",
    tag = "Repository Content",
    params(
        ("owner" = String, Path, description = "owner"),
        ("name" = String, Path, description = "name"),
    ),
    responses(
        (status = 200, description = "Success", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
    ),
)]
pub async fn get_blob(
    State(state): State<AppState>,
    Path((owner, repo, path)): Path<(String, String, String)>,
    Query(params): Query<BlobQuery>,
) -> impl IntoResponse {
    // H-02: Validate owner/repo before constructing repository path
    if let Err(e) = rg_core::platform::validate_repo_path(&owner) {
        return AppError::bad_request(e.to_string()).into_response();
    }
    if let Err(e) = rg_core::platform::validate_repo_path(&repo) {
        return AppError::bad_request(e.to_string()).into_response();
    }

    let repo_path = state.repo_root.join(format!("{}/{}.git", owner, repo));
    if !repo_path.exists() {
        return AppError::not_found("repository not found").into_response();
    }

    let git_ref = params.r#ref.unwrap_or_else(|| "HEAD".to_string());

    match get_blob_content(&repo_path, &git_ref, &path) {
        Ok(blob) => (StatusCode::OK, Json(blob)).into_response(),
        Err(e) => AppError::not_found(e).into_response(),
    }
}

/// Get commit log for a repo or a specific file.
/// GET /api/v1/repos/:owner/:name/log
#[utoipa::path(
    get,
    path = "/repos/{owner}/{name}/log",
    tag = "Repository Content",
    params(
        ("owner" = String, Path, description = "owner"),
        ("name" = String, Path, description = "name"),
    ),
    responses(
        (status = 200, description = "Success", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
    ),
)]
pub async fn get_log(
    State(state): State<AppState>,
    Path((owner, repo)): Path<(String, String)>,
    Query(params): Query<LogQuery>,
) -> impl IntoResponse {
    // H-02: Validate owner/repo before constructing repository path
    if let Err(e) = rg_core::platform::validate_repo_path(&owner) {
        return AppError::bad_request(e.to_string()).into_response();
    }
    if let Err(e) = rg_core::platform::validate_repo_path(&repo) {
        return AppError::bad_request(e.to_string()).into_response();
    }

    let repo_path = state.repo_root.join(format!("{}/{}.git", owner, repo));
    if !repo_path.exists() {
        return AppError::not_found("repository not found").into_response();
    }

    let limit = params.limit.unwrap_or(50).min(100);
    let file_path = params.path.unwrap_or_default();

    match get_commit_log(&repo_path, &file_path, limit) {
        Ok(log) => (StatusCode::OK, Json(log)).into_response(),
        Err(e) => {
            tracing::error!(%e, "get_log failed");
            AppError::internal(e).into_response()
        }
    }
}

/// List branches.
/// GET /api/v1/repos/:owner/:name/branches
#[utoipa::path(
    get,
    path = "/repos/{owner}/{name}/branches",
    tag = "Repository Content",
    params(
        ("owner" = String, Path, description = "owner"),
        ("name" = String, Path, description = "name"),
    ),
    responses(
        (status = 200, description = "Success", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
    ),
)]
pub async fn list_branches(
    State(state): State<AppState>,
    Path((owner, repo)): Path<(String, String)>,
) -> impl IntoResponse {
    // H-02: Validate owner/repo before constructing repository path
    if let Err(e) = rg_core::platform::validate_repo_path(&owner) {
        return AppError::bad_request(e.to_string()).into_response();
    }
    if let Err(e) = rg_core::platform::validate_repo_path(&repo) {
        return AppError::bad_request(e.to_string()).into_response();
    }

    let repo_path = state.repo_root.join(format!("{}/{}.git", owner, repo));
    if !repo_path.exists() {
        return AppError::not_found("repository not found").into_response();
    }

    match list_branch_names(&repo_path) {
        Ok(branches) => (StatusCode::OK, Json(branches)).into_response(),
        Err(e) => {
            tracing::error!(%e, "list_branches failed");
            AppError::internal(e).into_response()
        }
    }
}

/// List tags.
/// GET /api/v1/repos/:owner/:name/tags
#[utoipa::path(
    get,
    path = "/repos/{owner}/{name}/tags",
    tag = "Repository Content",
    params(
        ("owner" = String, Path, description = "owner"),
        ("name" = String, Path, description = "name"),
    ),
    responses(
        (status = 200, description = "Success", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
    ),
)]
pub async fn list_tags(
    State(state): State<AppState>,
    Path((owner, repo)): Path<(String, String)>,
) -> impl IntoResponse {
    // H-02: Validate owner/repo before constructing repository path
    if let Err(e) = rg_core::platform::validate_repo_path(&owner) {
        return AppError::bad_request(e.to_string()).into_response();
    }
    if let Err(e) = rg_core::platform::validate_repo_path(&repo) {
        return AppError::bad_request(e.to_string()).into_response();
    }

    let repo_path = state.repo_root.join(format!("{}/{}.git", owner, repo));
    if !repo_path.exists() {
        return AppError::not_found("repository not found").into_response();
    }

    match list_tag_names(&repo_path) {
        Ok(tags) => (StatusCode::OK, Json(tags)).into_response(),
        Err(e) => {
            tracing::error!(%e, "list_tags failed");
            AppError::internal(e).into_response()
        }
    }
}

// ── Git CLI helpers ───────────────────────────────────────────────────

fn list_tree_entries(
    repo_path: &std::path::Path,
    git_ref: &str,
    sub_path: &str,
) -> anyhow::Result<Vec<TreeEntry>> {
    let repo = gix::open(repo_path)
        .with_context(|| format!("failed to open repository: {:?}", repo_path))?;

    // Resolve ref to commit
    let commit_id = repo.rev_parse_single(git_ref)
        .map_err(|e| anyhow::anyhow!("failed to resolve ref '{}': {}", git_ref, e))?;

    let commit = repo.find_commit(commit_id)
        .map_err(|e| anyhow::anyhow!("failed to find commit: {}", e))?;

    let decoded = commit.decode()
        .map_err(|e| anyhow::anyhow!("failed to decode commit: {}", e))?;

    let tree_oid = decoded.tree();
    let mut tree = repo.find_tree(tree_oid)
        .map_err(|e| anyhow::anyhow!("failed to get tree: {}", e))?;

    // Traverse into sub_path if specified
    if !sub_path.is_empty() {
        for component in sub_path.split('/') {
            let entry = tree.iter()
                .filter_map(|e| e.ok())
                .find(|e| e.filename() == component);
            let entry = entry.ok_or_else(|| anyhow::anyhow!("path not found: {}", sub_path))?;
            tree = repo.find_tree(entry.oid())
                .map_err(|e| anyhow::anyhow!("failed to find sub-tree: {}", e))?;
        }
    }

    let mut entries = Vec::new();
    for entry in tree.iter() {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let oid = entry.oid();
        let name = entry.filename().to_string();
        let kind = if entry.mode().is_tree() {
            "tree".to_string()
        } else {
            "blob".to_string()
        };

        let size = if kind == "blob" {
            get_blob_size(repo_path, &oid.to_string()).ok()
        } else {
            None
        };

        let full_path = if sub_path.is_empty() {
            name.clone()
        } else {
            format!("{}/{}", sub_path, name)
        };

        entries.push(TreeEntry {
            name,
            path: full_path,
            kind,
            size,
            sha: Some(oid.to_string()),
        });
    }

    Ok(entries)
}

fn get_blob_content(
    repo_path: &std::path::Path,
    git_ref: &str,
    path: &str,
) -> anyhow::Result<BlobContent> {
    let repo = gix::open(repo_path)
        .with_context(|| format!("failed to open repository: {:?}", repo_path))?;

    let target = format!("{}:{}", git_ref, path);

    // Resolve ref:path to object ID
    let object_id = repo.rev_parse_single(target.as_str())
        .map_err(|e| anyhow::anyhow!("path '{}' not found at ref '{}': {}", path, git_ref, e))?;

    // Find and decode the blob
    let object = repo.find_object(object_id)
        .map_err(|e| anyhow::anyhow!("failed to find object: {}", e))?;

    let blob = object.try_into_blob()
        .map_err(|e| anyhow::anyhow!("path '{}' is not a file: {}", path, e))?;

    let data = &blob.data;
    let size = data.len() as i64;

    // Check if binary by looking for null bytes
    let is_binary = data.contains(&0);

    let (content, encoding) = if is_binary {
        use std::fmt::Write;
        let mut s = String::with_capacity(data.len() * 4 / 3 + 4);
        // Simple base64 encoding
        const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let chunks = data.chunks(3);
        for chunk in chunks {
            let b0 = chunk[0] as u32;
            let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
            let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
            let triple = (b0 << 16) | (b1 << 8) | b2;
            let _ = write!(s, "{}", ALPHABET[((triple >> 18) & 0x3F) as usize] as char);
            let _ = write!(s, "{}", ALPHABET[((triple >> 12) & 0x3F) as usize] as char);
            if chunk.len() > 1 {
                let _ = write!(s, "{}", ALPHABET[((triple >> 6) & 0x3F) as usize] as char);
            } else {
                let _ = write!(s, "=");
            }
            if chunk.len() > 2 {
                let _ = write!(s, "{}", ALPHABET[(triple & 0x3F) as usize] as char);
            } else {
                let _ = write!(s, "=");
            }
        }
        (s, "base64".to_string())
    } else {
        (String::from_utf8_lossy(&data).to_string(), "utf-8".to_string())
    };

    Ok(BlobContent {
        path: path.to_string(),
        sha: object_id.to_string(),
        size,
        content,
        encoding,
        is_binary,
    })
}

fn get_blob_size(repo_path: &std::path::Path, sha: &str) -> anyhow::Result<i64> {
    let repo = gix::open(repo_path)
        .with_context(|| format!("failed to open repository: {:?}", repo_path))?;

    let oid = gix::ObjectId::from_hex(sha.as_bytes())
        .map_err(|e| anyhow::anyhow!("invalid SHA: {}", e))?;

    let object = repo.find_object(oid)
        .map_err(|e| anyhow::anyhow!("object not found: {}", e))?;

    let blob = object.try_into_blob()
        .map_err(|e| anyhow::anyhow!("not a blob: {}", e))?;

    Ok(blob.data.len() as i64)
}

fn get_commit_log(
    repo_path: &std::path::Path,
    _path: &str,
    limit: i64,
) -> anyhow::Result<Vec<CommitEntry>> {
    let repo = gix::open(repo_path)
        .with_context(|| format!("failed to open repository: {:?}", repo_path))?;

    let mut entries = Vec::new();

    // Use rev_walk to traverse commit history
    let head_id = match repo.rev_parse_single("HEAD") {
        Ok(id) => id,
        Err(_) => return Ok(entries), // No commits yet
    };

    let walk = repo.rev_walk([head_id]);

    let mut count = 0;
    // Call all() to get the iterator
    if let Ok(walk_iter) = walk.all() {
        for info in walk_iter {
            if count >= limit {
                break;
            }

            let info = match info {
                Ok(i) => i,
                Err(_) => continue,
            };

            let commit_id = info.id;

            let object = match repo.find_object(commit_id) {
                Ok(obj) => obj,
                Err(_) => continue,
            };

            let commit = match object.try_into_commit() {
                Ok(c) => c,
                Err(_) => continue,
            };

            // Get commit message
            let message = commit.message_raw().unwrap_or_default().to_string();
            let first_line = message.lines().next().unwrap_or("").to_string();

            // Get author info - access fields directly, not methods
            let author = commit.author().unwrap_or_default();
            let author_name = String::from_utf8_lossy(&author.name).to_string();
            let author_email = String::from_utf8_lossy(&author.email).to_string();
            let author_date = String::new(); // TODO: extract commit time

            entries.push(CommitEntry {
                sha: commit_id.to_string(),
                author_name,
                author_email,
                author_date,
                message: first_line,
                gpg_signature: None,
            });

            count += 1;
        }
    }

    Ok(entries)
}

fn list_branch_names(repo_path: &std::path::Path) -> anyhow::Result<Vec<String>> {
    let repo = gix::open(repo_path)
        .with_context(|| format!("failed to open repository: {:?}", repo_path))?;

    let references = repo.references()?;
    let branches: Vec<String> = references.all()?
        .filter_map(|r| r.ok())
        .filter_map(|r| {
            let name = r.name().as_bstr();
            // Filter to only local branches (refs/heads/)
            if name.starts_with(b"refs/heads/") {
                let stripped = &name["refs/heads/".len()..];
                Some(String::from_utf8_lossy(stripped).to_string())
            } else {
                None
            }
        })
        .collect();

    Ok(branches)
}

fn list_tag_names(repo_path: &std::path::Path) -> anyhow::Result<Vec<String>> {
    let repo = gix::open(repo_path)
        .with_context(|| format!("failed to open repository: {:?}", repo_path))?;

    let references = repo.references()?;
    let tags: Vec<String> = references.all()?
        .filter_map(|r| r.ok())
        .filter_map(|r| {
            let name = r.name().as_bstr();
            // Filter to only tags (refs/tags/)
            if name.starts_with(b"refs/tags/") {
                let stripped = &name["refs/tags/".len()..];
                Some(String::from_utf8_lossy(stripped).to_string())
            } else {
                None
            }
        })
        .collect();

    Ok(tags)
}

/// GET /api/v1/repos/:owner/:name/commits/:sha/signature
/// Get GPG signature verification status for a commit.
#[utoipa::path(
    get,
    path = "/repos/{owner}/{name}/commits/{sha}/signature",
    tag = "Repository Content",
    params(
        ("owner" = String, Path, description = "owner"),
        ("name" = String, Path, description = "name"),
        ("sha" = String, Path, description = "sha"),
    ),
    responses(
        (status = 200, description = "Success", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
    ),
)]
pub async fn get_commit_signature(
    State(state): State<AppState>,
    Path((owner, repo, sha)): Path<(String, String, String)>,
) -> impl IntoResponse {
    // H-02: Validate owner/repo before constructing repository path
    if let Err(e) = rg_core::platform::validate_repo_path(&owner) {
        return AppError::bad_request(e.to_string()).into_response();
    }
    if let Err(e) = rg_core::platform::validate_repo_path(&repo) {
        return AppError::bad_request(e.to_string()).into_response();
    }

    let repo_path = state.repo_root.join(format!("{}/{}.git", owner, repo));
    if !repo_path.exists() {
        return AppError::not_found("repository not found").into_response();
    }

    // Validate SHA format
    if sha.len() < 7 || !sha.chars().all(|c| c.is_ascii_hexdigit()) {
        return AppError::bad_request("invalid commit SHA format").into_response();
    }

    match verify_commit_signature(&repo_path, &sha) {
        Ok(sig) => (StatusCode::OK, Json(sig)).into_response(),
        Err(e) => AppError::not_found(e).into_response(),
    }
}

/// Verify a commit's GPG signature using `git log --show-signature`.
fn verify_commit_signature(
    repo_path: &std::path::Path,
    sha: &str,
) -> anyhow::Result<GpgSignature> {
    let repo = gix::open(repo_path)
        .with_context(|| format!("failed to open repository: {:?}", repo_path))?;

    // Resolve the commit SHA using gix
    let commit_id = match repo.rev_parse_single(sha) {
        Ok(id) => id,
        Err(_) => anyhow::bail!("commit {} not found", sha),
    };

    let full_sha = commit_id.to_string();

    // Read commit object to check for gpgsig header
    let _commit_object = repo.find_object(commit_id)?;
    let _commit = _commit_object.try_into_commit()
        .map_err(|_| anyhow::anyhow!("not a commit object"))?;

    // Check if commit has GPG signature by looking at the raw commit data
    // gix doesn't easily expose raw commit headers, so use git CLI for this check
    let gpgsig_output = std::process::Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .args(["cat-file", "commit", &full_sha])
        .output()?;

    if !gpgsig_output.status.success() {
        anyhow::bail!("failed to read commit object");
    }

    let commit_content = String::from_utf8_lossy(&gpgsig_output.stdout);
    let has_gpgsig = commit_content.lines().any(|l| l.starts_with("gpgsig "));

    if !has_gpgsig {
        return Ok(GpgSignature {
            verified: false,
            signer_key: None,
            signer_name: None,
            signer_email: None,
            status: "no_signature".to_string(),
        });
    }

    // Verify the signature using git CLI (gix GPG support is incomplete)
    let verify_output = std::process::Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .args(["log", "--format=%G?%n%GK%n%GN%n%GE", "-1", &full_sha])
        .output()?;

    if !verify_output.status.success() {
        return Ok(GpgSignature {
            verified: false,
            signer_key: None,
            signer_name: None,
            signer_email: None,
            status: "verification_failed".to_string(),
        });
    }

    let verify_text = String::from_utf8_lossy(&verify_output.stdout);
    let lines: Vec<&str> = verify_text.lines().collect();

    let status_code: &str = lines.first().map(|l: &&str| l.trim()).unwrap_or("N");
    let signer_key = lines.get(1).map(|l: &&str| l.trim().to_string()).filter(|s| !s.is_empty());
    let signer_name = lines.get(2).map(|l: &&str| l.trim().to_string()).filter(|s| !s.is_empty());
    let signer_email = lines.get(3).map(|l: &&str| l.trim().to_string()).filter(|s| !s.is_empty());

    let (verified, status): (bool, String) = match status_code {
        "G" => (true, "valid".to_string()),
        "E" => (false, "expired".to_string()),
        "X" => (false, "expired_key".to_string()),
        "Y" => (false, "expired_key".to_string()),
        "R" => (false, "revoked_key".to_string()),
        "B" => (false, "bad_signature".to_string()),
        "U" => (false, "untrusted".to_string()),
        "N" => (false, "no_signature".to_string()),
        _ => (false, format!("unknown_{}", status_code)),
    };

    Ok(GpgSignature {
        verified,
        signer_key,
        signer_name,
        signer_email,
        status,
    })
}
