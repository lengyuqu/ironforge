//! Pull request service — PR creation, diff, merge strategies.

use anyhow::{bail, Context, Result};
use chrono::Utc;
use sea_orm::{DatabaseConnection, EntityTrait, Set};
use std::process::Command;

use rg_db::entities::pull_request::{self, Model as PullRequest};
use rg_db::entities::repository as repo_entity;
use rg_db::ops::{pull_request_ops, repo_ops, user_ops};

// ── PR CRUD ─────────────────────────────────────────────────────────────

/// Create a new pull request.
///
/// If `head_repo_id` is provided, this is a fork PR (cross-repository).
/// The `head_branch` should contain just the branch name (not `owner:branch` format).
pub async fn create_pr(
    db: &DatabaseConnection,
    repo_id: i64,
    author_id: i64,
    title: String,
    body: Option<String>,
    head_branch: String,
    base_branch: String,
    head_repo_id: Option<i64>,
) -> Result<PullRequest> {
    if title.trim().is_empty() {
        bail!("PR title cannot be empty");
    }
    if head_branch == base_branch {
        bail!("head and base branches cannot be the same");
    }

    let number = pull_request_ops::next_number(db, repo_id).await?;

    // Resolve head SHA (for same-repo PRs, look up branch; for fork PRs, use the head repo)
    let head_sha = if let Some(head_repo_id) = head_repo_id {
        // For fork PRs, resolve from the fork repo's git data
        let head_repo = repo_entity::Entity::find_by_id(head_repo_id)
            .one(db)
            .await?
            .context("head repository not found")?;
        let head_owner = user_ops::find_by_id(db, head_repo.owner_id)
            .await?
            .context("head repo owner not found")?;
        let head_path = crate::platform::path::repo_path(&head_owner.username, &head_repo.name);
        if head_path.exists() {
            get_ref_sha(&head_path, &head_branch).ok()
        } else {
            None
        }
    } else {
        None
    };

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
        head_repo_id: Set(head_repo_id),
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
        "head_repo_id": pr.head_repo_id,
        "author_id": pr.author_id,
    });
    if let Err(e) = crate::webhook::service::trigger_pr_opened(db, repo_id, &payload).await {
        tracing::warn!("Failed to trigger PR opened webhook: {e}");
    }

    Ok(pr)
}

/// Resolve a head reference in `owner:branch` format to (head_branch, head_repo_id).
/// Returns (branch_name, Some(head_repo_id)) if owner differs from the target repo owner,
/// or (branch_name, None) if same owner (same-repo PR).
pub async fn resolve_head_ref(
    db: &DatabaseConnection,
    target_repo_id: i64,
    head_ref: &str,
) -> Result<(String, Option<i64>)> {
    if let Some((head_owner, head_branch)) = head_ref.split_once(':') {
        // Cross-repo (fork) PR: "owner:branch"
        let head_branch = head_branch.to_string();
        let head_owner_user = user_ops::find_by_username(db, head_owner)
            .await?
            .with_context(|| format!("head owner '{}' not found", head_owner))?;

        // Find the target repo to compare
        let target_repo = repo_entity::Entity::find_by_id(target_repo_id)
            .one(db)
            .await?
            .context("target repository not found")?;

        if head_owner_user.id != target_repo.owner_id {
            // Different owner — this is a fork PR
            // Find the fork repo by the head owner (user may have forked the same repo)
            let fork_repo = repo_ops::find_by_owner_and_name(db, head_owner_user.id, &target_repo.name)
                .await?
                .with_context(|| {
                    format!("no repository '{}/{}' found for head owner", head_owner, target_repo.name)
                })?;

            // Verify it's actually a fork of the target
            if fork_repo.origin_repo_id != Some(target_repo_id) && fork_repo.id != target_repo_id {
                bail!("'{}/{}' is not a fork of the target repository", head_owner, target_repo.name);
            }

            return Ok((head_branch, Some(fork_repo.id)));
        }

        // Same owner — not a fork, just a branch reference with owner prefix
        Ok((head_branch, None))
    } else {
        // Simple branch name — same-repo PR
        Ok((head_ref.to_string(), None))
    }
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
    crate::notification::notify_watchers(
        db,
        repo_id,
        author_name,
        &format!("PR #{} {} in {}", pr_number, action, repo_name),
        "pull_request",
        Some(format!("{} {}: {}", author_name, action, pr_title)),
    )
    .await
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
                    if let Err(e) = crate::webhook::service::trigger_pr_closed(db, pr.repo_id, &close_payload).await {
                        tracing::warn!("Failed to trigger PR closed webhook: {e}");
                    }
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
/// Supports cross-repository (fork) PRs.
pub async fn compute_diff(
    db: &DatabaseConnection,
    repo_root: &std::path::Path,
    owner: &str,
    repo_name: &str,
    number: i64,
) -> Result<PrDiff> {
    let pr = get_pr(db, owner, repo_name, number).await?;
    let base_repo_path = repo_root.join(format!("{}/{}.git", owner, repo_name));

    if !base_repo_path.exists() {
        bail!("repository path does not exist: {:?}", base_repo_path);
    }

    // For fork PRs, fetch the head branch into the target repo first
    if let Some(head_repo_id) = pr.head_repo_id {
        let head_repo = repo_entity::Entity::find_by_id(head_repo_id)
            .one(db)
            .await?
            .context("head repository not found")?;
        let head_owner = user_ops::find_by_id(db, head_repo.owner_id)
            .await?
            .context("head repo owner not found")?;
        let head_repo_path = repo_root.join(format!("{}/{}.git", head_owner.username, head_repo.name));

        if head_repo_path.exists() {
            // Fetch head branch into target repo under refs/forks/
            let fetch_ref = format!("refs/heads/{}", pr.head_branch);
            let local_ref = format!("refs/forks/{}/{}", head_owner.username, pr.head_branch);

            let fetch_output = Command::new("git")
                .arg("-C")
                .arg(&base_repo_path)
                .arg("fetch")
                .arg(&head_repo_path)
                .arg(format!("{}:{}", fetch_ref, local_ref))
                .output()?;

            if !fetch_output.status.success() {
                // Log but don't fail — branch may not exist yet
                tracing::warn!(
                    "fetch of fork branch failed (non-fatal): {}",
                    String::from_utf8_lossy(&fetch_output.stderr)
                );
            }

            // Compute diff between base and fetched fork ref
            return compute_cross_repo_diff(
                &base_repo_path,
                &pr.base_branch,
                &local_ref,
                &pr,
            );
        }
    }

    // Same-repo diff
    compute_same_repo_diff(&base_repo_path, &pr)
}

/// Compute diff for same-repo PR.
fn compute_same_repo_diff(repo_path: &std::path::Path, pr: &PullRequest) -> Result<PrDiff> {
    let range = format!("{}...{}", pr.base_branch, pr.head_branch);

    let stat_output = Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .arg("diff")
        .arg("--numstat")
        .arg(&range)
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

    // Get diff patch
    let patch_output = Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .arg("diff")
        .arg(&range)
        .output()?;

    let patch_text = String::from_utf8_lossy(&patch_output.stdout).to_string();

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

/// Compute diff for cross-repo (fork) PR using a fetched ref.
fn compute_cross_repo_diff(
    repo_path: &std::path::Path,
    base_branch: &str,
    fork_ref: &str,
    pr: &PullRequest,
) -> Result<PrDiff> {
    let range = format!("{}...{}", base_branch, fork_ref);

    let stat_output = Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .arg("diff")
        .arg("--numstat")
        .arg(&range)
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

    let patch_output = Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .arg("diff")
        .arg(&range)
        .output()?;

    let patch_text = String::from_utf8_lossy(&patch_output.stdout).to_string();

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
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
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
/// Supports cross-repository (fork) PRs by fetching the head branch first.
pub async fn merge_pr(
    db: &DatabaseConnection,
    repo_root: &std::path::Path,
    owner: &str,
    repo_name: &str,
    number: i64,
    strategy: MergeStrategy,
) -> Result<MergeResult> {
    let pr = get_pr(db, owner, repo_name, number).await?;

    if pr.state != "open" {
        bail!("cannot merge a PR that is not in 'open' state (current: {})", pr.state);
    }

    let repo_path = repo_root.join(format!("{}/{}.git", owner, repo_name));
    if !repo_path.exists() {
        bail!("repository path does not exist: {:?}", repo_path);
    }

    // For fork PRs, fetch head branch into target repo
    if let Some(head_repo_id) = pr.head_repo_id {
        let head_repo = repo_entity::Entity::find_by_id(head_repo_id)
            .one(db)
            .await?
            .context("head repository not found")?;
        let head_owner = user_ops::find_by_id(db, head_repo.owner_id)
            .await?
            .context("head repo owner not found")?;
        let head_repo_path = repo_root.join(format!("{}/{}.git", head_owner.username, head_repo.name));

        if head_repo_path.exists() {
            let fetch_ref = format!("refs/heads/{}", pr.head_branch);
            let local_ref = format!("refs/forks/{}/{}", head_owner.username, pr.head_branch);

            let fetch_output = Command::new("git")
                .arg("-C")
                .arg(&repo_path)
                .arg("fetch")
                .arg(&head_repo_path)
                .arg(format!("{}:{}", fetch_ref, local_ref))
                .output()?;

            if !fetch_output.status.success() {
                bail!(
                    "failed to fetch fork branch: {}",
                    String::from_utf8_lossy(&fetch_output.stderr)
                );
            }

            // Merge using the fetched ref
            let merge_ref = format!("refs/forks/{}/{}", head_owner.username, pr.head_branch);
            let merge_commit_sha = merge_from_ref(&repo_path, &pr, &merge_ref, strategy)?;

            // Clean up fetched ref via gix (non-fatal on failure — just log)
            if let Err(e) = gix_delete_ref(&repo_path, &merge_ref) {
                tracing::warn!("failed to clean up fork ref '{}': {}", merge_ref, e);
            }

            return update_pr_merged(db, pr, merge_commit_sha, strategy).await;
        }
    }

    // Same-repo merge
    let merge_commit_sha = match strategy {
        MergeStrategy::Merge => do_merge_commit(&repo_path, &pr)?,
        MergeStrategy::Squash => do_squash_merge(&repo_path, &pr)?,
        MergeStrategy::Rebase => do_rebase_merge(&repo_path, &pr)?,
    };

    update_pr_merged(db, pr, merge_commit_sha, strategy).await
}

/// Merge from an arbitrary ref (used for fork PRs).
/// Uses gix merge APIs for Merge and Squash strategies; Rebase still uses git CLI.
fn merge_from_ref(
    repo_path: &std::path::Path,
    pr: &PullRequest,
    merge_ref: &str,
    strategy: MergeStrategy,
) -> Result<String> {
    match strategy {
        MergeStrategy::Merge => {
            let merge_msg = format!(
                "Merge pull request #{} from {}",
                pr.number, pr.head_branch
            );
            gix_merge_no_ff(repo_path, merge_ref, &merge_msg)
        }
        MergeStrategy::Squash => {
            let squash_msg = format!(
                "Squash merge pull request #{} from {}",
                pr.number, pr.head_branch
            );
            gix_squash_merge(repo_path, merge_ref, &squash_msg)
        }
        MergeStrategy::Rebase => {
            gix_set_head_to_branch(repo_path, &pr.base_branch)
                .with_context(|| format!("failed to checkout base branch: {}", pr.base_branch))?;

            // Rebase still uses git CLI — gix-rebase crate is in "idea" stage
            let rebase = Command::new("git")
                .arg("-C")
                .arg(repo_path)
                .arg("rebase")
                .arg(&pr.base_branch)
                .arg(merge_ref)
                .output()?;

            if !rebase.status.success() {
                if let Err(e) = Command::new("git")
                    .arg("-C")
                    .arg(repo_path)
                    .arg("rebase")
                    .arg("--abort")
                    .output()
                {
                    tracing::warn!("failed to abort rebase: {}", e);
                }
                bail!(
                    "rebase merge failed: {}",
                    String::from_utf8_lossy(&rebase.stderr)
                );
            }

            gix_set_head_to_branch(repo_path, &pr.base_branch)
                .with_context(|| "failed to checkout base branch for fast-forward")?;

            get_head_sha(repo_path)
        }
    }
}

/// Update PR state after successful merge.
async fn update_pr_merged(
    db: &DatabaseConnection,
    mut pr: PullRequest,
    merge_commit_sha: String,
    strategy: MergeStrategy,
) -> Result<MergeResult> {
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
    if let Err(e) = crate::webhook::service::trigger_pr_merged(db, merged_pr.repo_id, &merge_payload).await {
        tracing::warn!("Failed to trigger PR merged webhook: {e}");
    }

    Ok(MergeResult {
        merge_commit_sha,
        strategy: format!("{:?}", strategy).to_lowercase(),
    })
}

fn do_merge_commit(repo_path: &std::path::Path, pr: &PullRequest) -> Result<String> {
    let merge_msg = format!("Merge pull request #{} from {}", pr.number, pr.head_branch);
    gix_merge_no_ff(repo_path, &pr.head_branch, &merge_msg)
}

fn do_squash_merge(repo_path: &std::path::Path, pr: &PullRequest) -> Result<String> {
    let squash_msg = format!(
        "Squash merge pull request #{} from {}",
        pr.number, pr.head_branch
    );
    gix_squash_merge(repo_path, &pr.head_branch, &squash_msg)
}

fn do_rebase_merge(repo_path: &std::path::Path, pr: &PullRequest) -> Result<String> {
    // TODO(gix): Replace rebase with gix rebase API (complex operation)
    // Step 1: Checkout base branch (set HEAD symbolic ref via gix)
    gix_set_head_to_branch(repo_path, &pr.base_branch)
        .with_context(|| format!("failed to checkout base branch: {}", pr.base_branch))?;

    // Step 2: Rebase head onto base (keep CLI — gix doesn't support rebase)
    let rebase = Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .arg("rebase")
        .arg(&pr.base_branch)
        .arg(&pr.head_branch)
        .output()?;

    if !rebase.status.success() {
        // Abort the rebase on failure — non-fatal, just log if abort itself fails
        if let Err(e) = Command::new("git")
            .arg("-C")
            .arg(repo_path)
            .arg("rebase")
            .arg("--abort")
            .output()
        {
            tracing::warn!("failed to abort rebase: {}", e);
        }
        bail!(
            "rebase merge failed: {}",
            String::from_utf8_lossy(&rebase.stderr)
        );
    }

    // Step 3: Checkout base again (set HEAD symbolic ref via gix)
    gix_set_head_to_branch(repo_path, &pr.base_branch)
        .with_context(|| "failed to checkout base branch for fast-forward")?;

    // Step 4: Fast-forward base to head (update branch ref via gix)
    gix_fast_forward(repo_path, &pr.base_branch, &pr.head_branch)?;

    get_head_sha(repo_path)
}

/// Set HEAD to point to a branch (equivalent to `git checkout <branch>` in a bare repo).
/// Uses gix to update the HEAD symbolic reference.
fn gix_set_head_to_branch(repo_path: &std::path::Path, branch: &str) -> Result<()> {
    use gix::refs::transaction::{Change, LogChange, PreviousValue, RefEdit, RefLog};
    use gix::refs::{FullName, Target};

    let repo = gix::open(repo_path)
        .with_context(|| format!("failed to open repository: {:?}", repo_path))?;

    let branch_ref: FullName = format!("refs/heads/{}", branch)
        .try_into()
        .map_err(|e| anyhow::anyhow!("invalid branch reference: {}", e))?;
    let head_name: FullName = "HEAD"
        .try_into()
        .map_err(|e| anyhow::anyhow!("invalid HEAD reference: {}", e))?;

    repo.edit_reference(RefEdit {
        change: Change::Update {
            log: LogChange {
                mode: RefLog::AndReference,
                force_create_reflog: false,
                message: "checkout".into(),
            },
            expected: PreviousValue::Any,
            new: Target::Symbolic(branch_ref),
        },
        name: head_name,
        deref: false,
    })
    .map_err(|e| anyhow::anyhow!("failed to set HEAD to refs/heads/{}: {}", branch, e))?;

    Ok(())
}

/// Fast-forward a branch to point to another branch's commit (equivalent to `git merge --ff-only`).
/// Uses gix to update the base branch reference.
fn gix_fast_forward(repo_path: &std::path::Path, base_branch: &str, head_branch: &str) -> Result<()> {
    let repo = gix::open(repo_path)
        .with_context(|| format!("failed to open repository: {:?}", repo_path))?;

    let head_ref_str = format!("refs/heads/{}", head_branch);
    let base_ref_str = format!("refs/heads/{}", base_branch);

    // Resolve head branch commit
    let head_id = repo
        .rev_parse_single(head_ref_str.as_str())
        .map_err(|e| anyhow::anyhow!("failed to resolve {}: {}", head_ref_str, e))?;

    // Update base branch to point to head's commit
    repo.reference(
        base_ref_str.as_str(),
        head_id.detach(),
        gix::refs::transaction::PreviousValue::Any,
        "fast-forward merge",
    )
    .map_err(|e| anyhow::anyhow!("fast-forward failed: {}", e))?;

    Ok(())
}

fn get_head_sha(repo_path: &std::path::Path) -> Result<String> {
    let repo = gix::open(repo_path)
        .with_context(|| format!("failed to open repository: {:?}", repo_path))?;
    let head_id = repo.rev_parse_single("HEAD")
        .map_err(|e| anyhow::anyhow!("failed to parse HEAD: {}", e))?;
    Ok(head_id.to_string())
}

/// Resolve a branch reference to its SHA using gix.
fn get_ref_sha(repo_path: &std::path::Path, branch: &str) -> Result<String> {
    let repo = gix::open(repo_path)
        .with_context(|| format!("failed to open repository: {:?}", repo_path))?;
    let ref_str = format!("refs/heads/{}", branch);
    let id = repo.rev_parse_single(ref_str.as_str())
        .map_err(|e| anyhow::anyhow!("failed to resolve {}: {}", ref_str, e))?;
    Ok(id.to_string())
}

// ── Gix merge helpers ───────────────────────────────────────────────────

/// Delete a reference using gix (replaces `git update-ref -d <ref>`).
fn gix_delete_ref(repo_path: &std::path::Path, ref_name: &str) -> Result<()> {
    let repo = gix::open(repo_path)
        .with_context(|| format!("failed to open repository: {:?}", repo_path))?;

    use gix::refs::transaction::{Change, PreviousValue, RefEdit, RefLog};
    use gix::refs::FullName;

    let full_name: FullName = ref_name
        .try_into()
        .map_err(|e| anyhow::anyhow!("invalid ref name '{}': {}", ref_name, e))?;

    repo.edit_reference(RefEdit {
        change: Change::Delete {
            expected: PreviousValue::Any,
            log: RefLog::AndReference,
        },
        name: full_name,
        deref: false,
    })
    .map_err(|e| anyhow::anyhow!("failed to delete ref '{}': {}", ref_name, e))?;

    Ok(())
}

/// Perform a `--no-ff` merge using gix merge_commits API.
/// Creates a merge commit with two parents (current HEAD + `head_ref`).
fn gix_merge_no_ff(repo_path: &std::path::Path, head_ref: &str, message: &str) -> Result<String> {
    let repo = gix::open(repo_path)
        .with_context(|| format!("failed to open repository: {:?}", repo_path))?;

    let our_commit = repo.rev_parse_single("HEAD")
        .map_err(|e| anyhow::anyhow!("failed to resolve HEAD: {}", e))?;
    let their_commit = repo.rev_parse_single(head_ref)
        .with_context(|| format!("failed to resolve merge ref '{}'", head_ref))?;

    let (merged_tree_id, _conflicts) = gix_merge_commits_to_tree(&repo, our_commit, their_commit, head_ref)?;

    // Create merge commit (two parents)
    let commit_id = repo.commit(
        "HEAD",
        message,
        merged_tree_id.detach(),
        [our_commit.detach(), their_commit.detach()],
    )
    .map_err(|e| anyhow::anyhow!("failed to create merge commit: {}", e))?;

    Ok(commit_id.detach().to_string())
}

/// Perform a squash merge: merge commits, then create a single-parent commit.
fn gix_squash_merge(repo_path: &std::path::Path, head_ref: &str, message: &str) -> Result<String> {
    let repo = gix::open(repo_path)
        .with_context(|| format!("failed to open repository: {:?}", repo_path))?;

    let our_commit = repo.rev_parse_single("HEAD")
        .map_err(|e| anyhow::anyhow!("failed to resolve HEAD: {}", e))?;
    let their_commit = repo.rev_parse_single(head_ref)
        .with_context(|| format!("failed to resolve merge ref '{}'", head_ref))?;

    let (merged_tree_id, _conflicts) = gix_merge_commits_to_tree(&repo, our_commit, their_commit, head_ref)?;

    // Squash merge: single-parent commit
    let commit_id = repo.commit(
        "HEAD",
        message,
        merged_tree_id.detach(),
        [our_commit.detach()],
    )
    .map_err(|e| anyhow::anyhow!("failed to create squash commit: {}", e))?;

    Ok(commit_id.detach().to_string())
}

/// Core merge logic: merge two commits and return the merged tree id + conflicts.
fn gix_merge_commits_to_tree<'repo>(
    repo: &'repo gix::Repository,
    our_commit: gix::Id<'repo>,
    their_commit: gix::Id<'repo>,
    their_label: &str,
) -> Result<(gix::Id<'repo>, Vec<gix::merge::tree::Conflict>)> {
    use gix::merge::blob::builtin_driver::text::Labels;

    let labels = Labels {
        current: Some("HEAD".into()),
        other: Some(their_label.into()),
        ancestor: None, // auto-determined from merge-base
    };

    let options: gix::merge::commit::Options = repo
        .tree_merge_options()
        .map_err(|e| anyhow::anyhow!("failed to get tree merge options: {}", e))?
        .into();

    let mut outcome = repo
        .merge_commits(our_commit, their_commit, labels, options)
        .map_err(|e| anyhow::anyhow!("merge failed: {}", e))?;

    // Check for unresolved conflicts
    let conflicts = outcome.tree_merge.conflicts;
    if !conflicts.is_empty() {
        tracing::warn!("merge has {} conflict(s)", conflicts.len());
        bail!(
            "merge conflict detected: {} files with conflicts",
            conflicts.len()
        );
    }

    // Write the merged tree to the object database
    let tree_id = outcome
        .tree_merge
        .tree
        .write()
        .map_err(|e| anyhow::anyhow!("failed to write merged tree: {}", e))?;

    Ok((tree_id, conflicts))
}

// ── Helpers ─────────────────────────────────────────────────────────────

async fn resolve_repo(
    db: &DatabaseConnection,
    owner: &str,
    repo_name: &str,
) -> Result<rg_db::entities::repository::Model> {
    let user = user_ops::find_by_username(db, owner)
        .await?
        .context("owner not found")?;
    repo_ops::find_by_owner_and_name(db, user.id, repo_name)
        .await?
        .context("repository not found")
}
