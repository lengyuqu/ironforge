//! REST API handlers for repository content browsing (tree, blob, history).

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::AppState;

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
pub async fn list_tree(
    State(state): State<AppState>,
    Path((owner, repo)): Path<(String, String)>,
    Query(params): Query<TreeQuery>,
) -> impl IntoResponse {
    let repo_path = state.repo_root.join(format!("{}/{}.git", owner, repo));
    if !repo_path.exists() {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "repository not found"})),
        )
            .into_response();
    }

    let git_ref = params.r#ref.unwrap_or_else(|| "HEAD".to_string());
    let sub_path = params.path.unwrap_or_default();

    let result = list_tree_entries(&repo_path, &git_ref, &sub_path);

    match result {
        Ok(entries) => (StatusCode::OK, Json(entries)).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("{:#}", e)})),
        )
            .into_response(),
    }
}

/// Get blob (file) content.
/// GET /api/v1/repos/:owner/:name/blob/:path
pub async fn get_blob(
    State(state): State<AppState>,
    Path((owner, repo, path)): Path<(String, String, String)>,
    Query(params): Query<BlobQuery>,
) -> impl IntoResponse {
    let repo_path = state.repo_root.join(format!("{}/{}.git", owner, repo));
    if !repo_path.exists() {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "repository not found"})),
        )
            .into_response();
    }

    let git_ref = params.r#ref.unwrap_or_else(|| "HEAD".to_string());

    match get_blob_content(&repo_path, &git_ref, &path) {
        Ok(blob) => (StatusCode::OK, Json(blob)).into_response(),
        Err(e) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": format!("{:#}", e)})),
        )
            .into_response(),
    }
}

/// Get commit log for a repo or a specific file.
/// GET /api/v1/repos/:owner/:name/log
pub async fn get_log(
    State(state): State<AppState>,
    Path((owner, repo)): Path<(String, String)>,
    Query(params): Query<LogQuery>,
) -> impl IntoResponse {
    let repo_path = state.repo_root.join(format!("{}/{}.git", owner, repo));
    if !repo_path.exists() {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "repository not found"})),
        )
            .into_response();
    }

    let limit = params.limit.unwrap_or(50).min(100);
    let file_path = params.path.unwrap_or_default();

    match get_commit_log(&repo_path, &file_path, limit) {
        Ok(log) => (StatusCode::OK, Json(log)).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("{:#}", e)})),
        )
            .into_response(),
    }
}

/// List branches.
/// GET /api/v1/repos/:owner/:name/branches
pub async fn list_branches(
    State(state): State<AppState>,
    Path((owner, repo)): Path<(String, String)>,
) -> impl IntoResponse {
    let repo_path = state.repo_root.join(format!("{}/{}.git", owner, repo));
    if !repo_path.exists() {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "repository not found"})),
        )
            .into_response();
    }

    match list_branch_names(&repo_path) {
        Ok(branches) => (StatusCode::OK, Json(branches)).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("{:#}", e)})),
        )
            .into_response(),
    }
}

/// List tags.
/// GET /api/v1/repos/:owner/:name/tags
pub async fn list_tags(
    State(state): State<AppState>,
    Path((owner, repo)): Path<(String, String)>,
) -> impl IntoResponse {
    let repo_path = state.repo_root.join(format!("{}/{}.git", owner, repo));
    if !repo_path.exists() {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "repository not found"})),
        )
            .into_response();
    }

    match list_tag_names(&repo_path) {
        Ok(tags) => (StatusCode::OK, Json(tags)).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("{:#}", e)})),
        )
            .into_response(),
    }
}

// ── Git CLI helpers ───────────────────────────────────────────────────

fn list_tree_entries(
    repo_path: &std::path::Path,
    git_ref: &str,
    sub_path: &str,
) -> anyhow::Result<Vec<TreeEntry>> {
    let target = if sub_path.is_empty() {
        git_ref.to_string()
    } else {
        format!("{}:{}", git_ref, sub_path)
    };

    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .arg("ls-tree")
        .arg(&target)
        .output()?;

    if !output.status.success() {
        anyhow::bail!(
            "git ls-tree failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let mut entries = Vec::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        // Format: <mode> <type> <sha>\t<name>
        let parts: Vec<&str> = line.splitn(2, '\t').collect();
        if parts.len() != 2 {
            continue;
        }
        let meta: Vec<&str> = parts[0].split_whitespace().collect();
        if meta.len() < 3 {
            continue;
        }

        let kind = meta[1].to_string();
        let sha = meta[2].to_string();
        let name = parts[1].to_string();
        let full_path = if sub_path.is_empty() {
            name.clone()
        } else {
            format!("{}/{}", sub_path, name)
        };

        let size = if kind == "blob" {
            get_blob_size(repo_path, &sha).ok()
        } else {
            None
        };

        entries.push(TreeEntry {
            name,
            path: full_path,
            kind,
            size,
            sha: Some(sha),
        });
    }

    Ok(entries)
}

fn get_blob_content(
    repo_path: &std::path::Path,
    git_ref: &str,
    path: &str,
) -> anyhow::Result<BlobContent> {
    let target = format!("{}:{}", git_ref, path);

    // Get SHA
    let sha_output = std::process::Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .arg("rev-parse")
        .arg(&target)
        .output()?;

    if !sha_output.status.success() {
        anyhow::bail!("path '{}' not found at ref '{}'", path, git_ref);
    }
    let sha = String::from_utf8(sha_output.stdout)?.trim().to_string();

    // Get size
    let size = get_blob_size(repo_path, &sha).unwrap_or(0);

    // Try to get content as UTF-8
    let content_output = std::process::Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .arg("show")
        .arg(&target)
        .output()?;

    if !content_output.status.success() {
        anyhow::bail!("failed to read blob content");
    }

    // Check if binary by looking for null bytes
    let raw = content_output.stdout;
    let is_binary = raw.contains(&0);

    let (content, encoding) = if is_binary {
        use std::fmt::Write;
        let mut s = String::with_capacity(raw.len() * 4 / 3 + 4);
        // Simple base64 encoding
        const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let chunks = raw.chunks(3);
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
        (String::from_utf8_lossy(&raw).to_string(), "utf-8".to_string())
    };

    Ok(BlobContent {
        path: path.to_string(),
        sha,
        size,
        content,
        encoding,
        is_binary,
    })
}

fn get_blob_size(repo_path: &std::path::Path, sha: &str) -> anyhow::Result<i64> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .arg("cat-file")
        .arg("-s")
        .arg(sha)
        .output()?;

    if !output.status.success() {
        anyhow::bail!("failed to get blob size");
    }
    let size_str = String::from_utf8(output.stdout)?.trim().to_string();
    Ok(size_str.parse::<i64>().unwrap_or(0))
}

fn get_commit_log(
    repo_path: &std::path::Path,
    path: &str,
    limit: i64,
) -> anyhow::Result<Vec<CommitEntry>> {
    let mut cmd = std::process::Command::new("git");
    cmd.arg("-C")
        .arg(repo_path)
        .arg("log")
        .arg(format!("-{}", limit))
        .arg("--format=%H%n%an%n%ae%n%aI%n%s%n---");

    if !path.is_empty() {
        cmd.arg("--").arg(path);
    }

    let output = cmd.output()?;
    if !output.status.success() {
        anyhow::bail!(
            "git log failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let mut entries = Vec::new();
    let text = String::from_utf8_lossy(&output.stdout);

    for block in text.split("---\n") {
        let lines: Vec<&str> = block.lines().collect();
        if lines.len() >= 5 {
            entries.push(CommitEntry {
                sha: lines[0].to_string(),
                author_name: lines[1].to_string(),
                author_email: lines[2].to_string(),
                author_date: lines[3].to_string(),
                message: lines[4].to_string(),
                gpg_signature: None,
            });
        }
    }

    Ok(entries)
}

fn list_branch_names(repo_path: &std::path::Path) -> anyhow::Result<Vec<String>> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .arg("branch")
        .arg("--format=%(refname:short)")
        .output()?;

    if !output.status.success() {
        anyhow::bail!("git branch failed");
    }

    let branches: Vec<String> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();

    Ok(branches)
}

fn list_tag_names(repo_path: &std::path::Path) -> anyhow::Result<Vec<String>> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .arg("tag")
        .output()?;

    if !output.status.success() {
        anyhow::bail!("git tag failed");
    }

    let tags: Vec<String> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();

    Ok(tags)
}

/// GET /api/v1/repos/:owner/:name/commits/:sha/signature
/// Get GPG signature verification status for a commit.
pub async fn get_commit_signature(
    State(state): State<AppState>,
    Path((owner, repo, sha)): Path<(String, String, String)>,
) -> impl IntoResponse {
    let repo_path = state.repo_root.join(format!("{}/{}.git", owner, repo));
    if !repo_path.exists() {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "repository not found"})),
        )
            .into_response();
    }

    // Validate SHA format
    if sha.len() < 7 || !sha.chars().all(|c| c.is_ascii_hexdigit()) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "invalid commit SHA format"})),
        )
            .into_response();
    }

    match verify_commit_signature(&repo_path, &sha) {
        Ok(sig) => (StatusCode::OK, Json(sig)).into_response(),
        Err(e) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": format!("{:#}", e)})),
        )
            .into_response(),
    }
}

/// Verify a commit's GPG signature using `git log --show-signature`.
fn verify_commit_signature(
    repo_path: &std::path::Path,
    sha: &str,
) -> anyhow::Result<GpgSignature> {
    // First check if the commit exists
    let rev_output = std::process::Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .args(["rev-parse", "--verify", sha])
        .output()?;

    if !rev_output.status.success() {
        anyhow::bail!("commit {} not found", sha);
    }

    let full_sha = String::from_utf8(rev_output.stdout)?.trim().to_string();

    // Check if the commit has a signature
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

    // Verify the signature
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
