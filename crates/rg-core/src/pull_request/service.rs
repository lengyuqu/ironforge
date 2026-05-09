//! Pull request service — PR creation, diff, merge strategies.

use anyhow::{bail, Context, Result};
use chrono::Utc;
use sea_orm::{DatabaseConnection, Set};
use std::process::Command;

use rg_db::entities::pull_request::{self, Model as PullRequest};
use rg_db::ops::{pull_request_ops, repo_ops};

// ── PR CRUD ─────────────────────────────────────────────────────────────

/// Create a new pull request.
pub async fn create_pr(
    db: &DatabaseConnection,
    repo_id: i64,
    author_id: i64,
    title: String,
    body: Option<String>,
    head_branch: String,
    base_branch: String,
) -> Result<PullRequest> {
    if title.trim().is_empty() {
        bail!("PR title cannot be empty");
    }
    if head_branch == base_branch {
        bail!("head and base branches cannot be the same");
    }

    let number = pull_request_ops::next_number(db, repo_id).await?;

    // Resolve head SHA
    let head_sha = None; // Will be filled on first push / refresh

    let model = pull_request::ActiveModel {
        id: sea_orm::NotSet,
        repo_id: Set(repo_id),
        number: Set(number),
        title: Set(title),
        body: Set(body),
        state: Set("open".to_string()),
        author_id: Set(author_id),
        reviewer_id: Set(None),
        head_branch: Set(head_branch),
        base_branch: Set(base_branch),
        head_sha: Set(head_sha),
        merge_strategy: Set(None),
        merge_commit_sha: Set(None),
        created_at: Set(Utc::now()),
        updated_at: Set(Utc::now()),
        closed_at: Set(None),
        merged_at: Set(None),
    };

    let pr = pull_request_ops::create(db, model).await?;

    // Trigger pull_request.opened webhook
    let payload = serde_json::json!({
        "id": pr.id,
        "repo_id": pr.repo_id,
        "number": pr.number,
        "title": pr.title,
        "state": pr.state,
        "head_branch": pr.head_branch,
        "base_branch": pr.base_branch,
        "author_id": pr.author_id,
    });
    let _ = crate::webhook::service::trigger_pr_opened(db, repo_id, &payload).await;

    Ok(pr)
}

/// Notify watchers of a PR event.
pub async fn notify_watchers_pr(
    db: &DatabaseConnection,
    repo_id: i64,
    repo_name: &str,
    author_name: &str,
    pr_number: i64,
    pr_title: &str,
    action: &str,
) -> Result<()> {
    crate::notification::notify_watchers_pr(db, repo_id, repo_name, author_name, pr_number, pr_title, action).await
}

/// List PRs for a repo, optionally filtered by state.
pub async fn list_prs(
    db: &DatabaseConnection,
    owner: &str,
    repo_name: &str,
    state: Option<&str>,
) -> Result<Vec<PullRequest>> {
    let repo = resolve_repo(db, owner, repo_name).await?;
    pull_request_ops::list_by_repo(db, repo.id, state).await
}

/// Paginated list of PRs. Returns (prs, total).
pub async fn list_prs_paginated(
    db: &DatabaseConnection,
    owner: &str,
    repo_name: &str,
    state: Option<&str>,
    offset: u64,
    limit: u64,
) -> Result<(Vec<PullRequest>, i64)> {
    let repo = resolve_repo(db, owner, repo_name).await?;
    pull_request_ops::list_by_repo_paginated(db, repo.id, state, offset, limit).await
}

/// Get a single PR.
pub async fn get_pr(
    db: &DatabaseConnection,
    owner: &str,
    repo_name: &str,
    number: i64,
) -> Result<PullRequest> {
    let repo = resolve_repo(db, owner, repo_name).await?;
    pull_request_ops::find_by_repo_and_number(db, repo.id, number)
        .await?
        .context("pull request not found")
}

/// Update PR metadata (title, body, state).
pub async fn update_pr(
    db: &DatabaseConnection,
    owner: &str,
    repo_name: &str,
    number: i64,
    title: Option<String>,
    body: Option<String>,
    state: Option<String>,
) -> Result<PullRequest> {
    let mut pr = get_pr(db, owner, repo_name, number).await?;

    if let Some(t) = title {
        if t.trim().is_empty() {
            bail!("PR title cannot be empty");
        }
        pr.title = t;
    }
    if let Some(b) = body {
        pr.body = Some(b);
    }
    if let Some(s) = &state {
        match s.as_str() {
            "open" | "closed" | "merged" => {
                let was_open = pr.state == "open";
                pr.state = s.clone();
                if s == "closed" && pr.closed_at.is_none() {
                    pr.closed_at = Some(Utc::now());
                }

                // Trigger pull_request.closed webhook when transitioning to closed
                if was_open && s == "closed" {
                    let close_payload = serde_json::json!({
                        "id": pr.id,
                        "repo_id": pr.repo_id,
                        "number": pr.number,
                        "title": pr.title,
                        "state": s,
                    });
                    let _ = crate::webhook::service::trigger_pr_closed(db, pr.repo_id, &close_payload).await;
                }
            }
            _ => bail!("invalid PR state: {}", s),
        }
    }

    pr.updated_at = Utc::now();

    let active: pull_request::ActiveModel = pr.into();
    pull_request_ops::update(db, active).await
}

// ── Diff ────────────────────────────────────────────────────────────────

/// Diff result for a PR.
#[derive(Debug, serde::Serialize)]
pub struct PrDiff {
    pub base_branch: String,
    pub head_branch: String,
    pub files_changed: Vec<FileDiff>,
    pub stats: DiffStats,
}

#[derive(Debug, serde::Serialize)]
pub struct FileDiff {
    pub path: String,
    pub status: String, // added / modified / deleted / renamed
    pub additions: i64,
    pub deletions: i64,
    pub patch: Option<String>,
}

#[derive(Debug, serde::Serialize)]
pub struct DiffStats {
    pub total_additions: i64,
    pub total_deletions: i64,
    pub files_changed: i64,
}

/// Compute the diff between base and head branches using `git diff`.
pub async fn compute_diff(
    db: &DatabaseConnection,
    repo_root: &std::path::Path,
    owner: &str,
    repo_name: &str,
    number: i64,
) -> Result<PrDiff> {
    let pr = get_pr(db, owner, repo_name, number).await?;
    let repo_path = repo_root.join(format!("{}/{}.git", owner, repo_name));

    if !repo_path.exists() {
        bail!("repository path does not exist: {:?}", repo_path);
    }

    // Get diff stat
    let stat_output = Command::new("git")
        .arg("-C")
        .arg(&repo_path)
        .arg("diff")
        .arg("--numstat")
        .arg(&format!("{}...{}", pr.base_branch, pr.head_branch))
        .output()?;

    let mut files_changed = Vec::new();
    let mut total_additions = 0i64;
    let mut total_deletions = 0i64;

    for line in String::from_utf8_lossy(&stat_output.stdout).lines() {
        let parts: Vec<&str> = line.splitn(3, '\t').collect();
        if parts.len() >= 3 {
            let additions = parts[0].parse::<i64>().unwrap_or(0);
            let deletions = parts[1].parse::<i64>().unwrap_or(0);
            let path = parts[2].to_string();

            total_additions += additions;
            total_deletions += deletions;

            let status = if additions > 0 && deletions == 0 {
                "added"
            } else if additions == 0 && deletions > 0 {
                "deleted"
            } else {
                "modified"
            };

            files_changed.push(FileDiff {
                path,
                status: status.to_string(),
                additions,
                deletions,
                patch: None,
            });
        }
    }

    // Get diff patch (truncated for safety)
    let patch_output = Command::new("git")
        .arg("-C")
        .arg(&repo_path)
        .arg("diff")
        .arg(&format!("{}...{}", pr.base_branch, pr.head_branch))
        .output()?;

    let patch_text = String::from_utf8_lossy(&patch_output.stdout).to_string();

    // Attach patch to each file (simplified: full patch as first file's patch)
    if let Some(first) = files_changed.first_mut() {
        first.patch = Some(patch_text);
    }

    Ok(PrDiff {
        base_branch: pr.base_branch.clone(),
        head_branch: pr.head_branch.clone(),
        stats: DiffStats {
            total_additions,
            total_deletions,
            files_changed: files_changed.len() as i64,
        },
        files_changed,
    })
}

// ── Merge ───────────────────────────────────────────────────────────────

/// Merge strategy for a PR.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MergeStrategy {
    Merge,
    Squash,
    Rebase,
}

/// Result of a merge operation.
#[derive(Debug, serde::Serialize)]
pub struct MergeResult {
    pub merge_commit_sha: String,
    pub strategy: String,
}

/// Merge a pull request using the specified strategy.
pub async fn merge_pr(
    db: &DatabaseConnection,
    repo_root: &std::path::Path,
    owner: &str,
    repo_name: &str,
    number: i64,
    strategy: MergeStrategy,
) -> Result<MergeResult> {
    let mut pr = get_pr(db, owner, repo_name, number).await?;

    if pr.state != "open" {
        bail!("cannot merge a PR that is not in 'open' state (current: {})", pr.state);
    }

    let repo_path = repo_root.join(format!("{}/{}.git", owner, repo_name));
    if !repo_path.exists() {
        bail!("repository path does not exist: {:?}", repo_path);
    }

    let merge_commit_sha = match strategy {
        MergeStrategy::Merge => do_merge_commit(&repo_path, &pr)?,
        MergeStrategy::Squash => do_squash_merge(&repo_path, &pr)?,
        MergeStrategy::Rebase => do_rebase_merge(&repo_path, &pr)?,
    };

    // Update PR state
    pr.state = "merged".to_string();
    pr.merge_strategy = Some(format!("{:?}", strategy).to_lowercase());
    pr.merge_commit_sha = Some(merge_commit_sha.clone());
    pr.merged_at = Some(Utc::now());
    pr.closed_at = Some(Utc::now());
    pr.updated_at = Utc::now();

    let active: pull_request::ActiveModel = pr.into();
    let merged_pr = pull_request_ops::update(db, active).await?;

    // Trigger pull_request.merged webhook
    let merge_payload = serde_json::json!({
        "id": merged_pr.id,
        "repo_id": merged_pr.repo_id,
        "number": merged_pr.number,
        "title": merged_pr.title,
        "merge_commit_sha": merge_commit_sha,
        "strategy": format!("{:?}", strategy).to_lowercase(),
    });
    let _ = crate::webhook::service::trigger_pr_merged(db, merged_pr.repo_id, &merge_payload).await;

    Ok(MergeResult {
        merge_commit_sha,
        strategy: format!("{:?}", strategy).to_lowercase(),
    })
}

fn do_merge_commit(repo_path: &std::path::Path, pr: &PullRequest) -> Result<String> {
    // TODO(gix): Replace with gix merge API
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .arg("merge")
        .arg("--no-ff")
        .arg(&pr.head_branch)
        .arg("-m")
        .arg(format!("Merge pull request #{} from {}", pr.number, pr.head_branch))
        .output()?;

    if !output.status.success() {
        bail!(
            "merge failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    get_head_sha(repo_path)
}

fn do_squash_merge(repo_path: &std::path::Path, pr: &PullRequest) -> Result<String> {
    // TODO(gix): Replace with gix merge --squash API
    // Squash all commits from head_branch into one
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .arg("merge")
        .arg("--squash")
        .arg(&pr.head_branch)
        .output()?;

    if !output.status.success() {
        bail!(
            "squash merge failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    // Commit the squashed changes
    let commit_output = Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .arg("commit")
        .arg("-m")
        .arg(format!("Squash merge pull request #{} from {}", pr.number, pr.head_branch))
        .output()?;

    if !commit_output.status.success() {
        bail!(
            "squash commit failed: {}",
            String::from_utf8_lossy(&commit_output.stderr)
        );
    }

    get_head_sha(repo_path)
}

fn do_rebase_merge(repo_path: &std::path::Path, pr: &PullRequest) -> Result<String> {
    // TODO(gix): Replace with gix rebase API (complex operation)
    // Checkout base branch, rebase head onto it
    let checkout = std::process::Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .arg("checkout")
        .arg(&pr.base_branch)
        .output()?;

    if !checkout.status.success() {
        bail!(
            "checkout base branch failed: {}",
            String::from_utf8_lossy(&checkout.stderr)
        );
    }

    let rebase = Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .arg("rebase")
        .arg(&pr.base_branch)
        .arg(&pr.head_branch)
        .output()?;

    if !rebase.status.success() {
        // Abort the rebase on failure
        let _ = Command::new("git")
            .arg("-C")
            .arg(repo_path)
            .arg("rebase")
            .arg("--abort")
            .output();
        bail!(
            "rebase merge failed: {}",
            String::from_utf8_lossy(&rebase.stderr)
        );
    }

    // Fast-forward base to head
    let ff = Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .arg("checkout")
        .arg(&pr.base_branch)
        .output()?;

    if !ff.status.success() {
        bail!("checkout base for fast-forward failed");
    }

    let merge_ff = Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .arg("merge")
        .arg("--ff-only")
        .arg(&pr.head_branch)
        .output()?;

    if !merge_ff.status.success() {
        bail!(
            "fast-forward merge failed: {}",
            String::from_utf8_lossy(&merge_ff.stderr)
        );
    }

    get_head_sha(repo_path)
}

fn get_head_sha(repo_path: &std::path::Path) -> Result<String> {
    let repo = gix::open(repo_path)
        .with_context(|| format!("failed to open repository: {:?}", repo_path))?;
    let head_id = repo.rev_parse_single("HEAD")
        .map_err(|e| anyhow::anyhow!("failed to parse HEAD: {}", e))?;
    Ok(head_id.to_string())
}

// ── Helpers ─────────────────────────────────────────────────────────────

async fn resolve_repo(
    db: &DatabaseConnection,
    owner: &str,
    repo_name: &str,
) -> Result<rg_db::entities::repository::Model> {
    let user = rg_db::ops::user_ops::find_by_username(db, owner)
        .await?
        .context("owner not found")?;
    repo_ops::find_by_owner_and_name(db, user.id, repo_name)
        .await?
        .context("repository not found")
}
