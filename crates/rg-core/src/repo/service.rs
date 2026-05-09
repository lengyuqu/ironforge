//! Repository service — business logic for repo creation and access control.

use anyhow::{bail, Context, Result};
use chrono::Utc;
use sea_orm::{ActiveValue::Set, DatabaseConnection};
use std::path::PathBuf;

use rg_db::{
    entities::repository::ActiveModel as RepoActiveModel,
    ops::{repo_ops, user_ops},
};

/// Resolve an "owner" string to either a user ID or an org ID.
/// Returns (owner_id, org_id, owner_name_for_path).
/// - If owner is a username: returns (user_id, None, username)
/// - If owner is an org name: returns (org_owner_id, Some(org_id), org_name)
async fn resolve_owner(
    db: &DatabaseConnection,
    owner: &str,
) -> Result<(i64, Option<i64>, String)> {
    // Try user first
    if let Some(user) = user_ops::find_by_username(db, owner).await? {
        return Ok((user.id, None, user.username.clone()));
    }

    // Try organization
    if let Some(org) = rg_db::ops::org_ops::get_org_by_name(db, owner).await? {
        return Ok((org.owner_id, Some(org.id), org.name.clone()));
    }

    bail!("owner '{}' not found (neither user nor organization)", owner)
}

/// Find a repository by owner name (user or org) and repo name.
pub async fn find_repo_by_owner_name(
    db: &DatabaseConnection,
    owner: &str,
    repo_name: &str,
) -> Result<Option<rg_db::entities::repository::Model>> {
    // Try as user
    if let Some(user) = user_ops::find_by_username(db, owner).await? {
        return repo_ops::find_by_owner_and_name(db, user.id, repo_name).await;
    }

    // Try as organization
    if let Some(org) = rg_db::ops::org_ops::get_org_by_name(db, owner).await? {
        return repo_ops::find_by_org_and_name(db, org.id, repo_name).await;
    }

    Ok(None)
}

/// Check whether `actor_id` (None = anonymous) can read `owner/repo`.
/// Takes into account: public repos, private repos (owner + collaborators + org members).
pub async fn can_read(
    db: &DatabaseConnection,
    owner: &str,
    repo_name: &str,
    actor_id: Option<i64>,
) -> Result<bool> {
    let repo = find_repo_by_owner_name(db, owner, repo_name)
        .await?
        .ok_or_else(|| anyhow::anyhow!("repository '{}/{}' not found", owner, repo_name))?;

    if !repo.is_private {
        return Ok(true);
    }

    // Private repo: check owner + collaborators + org members
    match actor_id {
        Some(id) => {
            // Owner always has access
            if id == repo.owner_id {
                return Ok(true);
            }

            // Check collaborator permission
            let perm = rg_db::ops::repo_collaborator_ops::get_permission(db, repo.id, id).await?;
            if perm.is_some() {
                return Ok(true);
            }

            // Check org membership (if repo belongs to an org)
            if let Some(org_id) = repo.org_id {
                if rg_db::ops::org_ops::is_org_member(db, org_id, id).await? {
                    return Ok(true);
                }
            }

            Ok(false)
        }
        None => Ok(false),
    }
}

/// Check whether `actor_id` can write to `owner/repo`.
/// Owner always has write. Collaborators with "write" or "admin" can write.
/// Org admins/members with write team permission can write.
pub async fn can_write(
    db: &DatabaseConnection,
    owner: &str,
    repo_name: &str,
    actor_id: Option<i64>,
) -> Result<bool> {
    let repo = find_repo_by_owner_name(db, owner, repo_name)
        .await?
        .ok_or_else(|| anyhow::anyhow!("repository '{}/{}' not found", owner, repo_name))?;

    match actor_id {
        Some(id) => {
            // Owner always has write
            if id == repo.owner_id {
                return Ok(true);
            }

            // Check collaborator permission (write/admin can write)
            let perm = rg_db::ops::repo_collaborator_ops::get_permission(db, repo.id, id).await?;
            match perm.as_deref() {
                Some("write") | Some("admin") => return Ok(true),
                _ => {}
            }

            // Check org membership with write/admin role
            if let Some(org_id) = repo.org_id {
                if let Some(member) = rg_db::ops::org_ops::find_org_member(db, org_id, id).await? {
                    if member.role == "owner" || member.role == "admin" {
                        return Ok(true);
                    }
                }

                // Check team permissions for this repo's org
                let teams = rg_db::ops::org_ops::list_org_teams(db, org_id).await?;
                for team in teams {
                    if team.permission == "write" || team.permission == "admin" {
                        if rg_db::ops::org_ops::is_team_member(db, team.id, id).await? {
                            return Ok(true);
                        }
                    }
                }
            }

            Ok(false)
        }
        None => Ok(false),
    }
}

/// Create a new repository (bare git init + DB record).
/// If org_id is Some, the repo belongs to the organization.
pub async fn create_repo(
    db: &DatabaseConnection,
    owner_id: i64,
    name: &str,
    description: Option<&str>,
    is_private: bool,
    repo_root: &PathBuf,
    org_id: Option<i64>,
) -> Result<rg_db::entities::repository::Model> {
    // Check name conflict (per owner)
    if repo_ops::find_by_owner_and_name(db, owner_id, name).await?.is_some() {
        bail!("repository '{}' already exists", name);
    }

    // Determine path prefix: org name or user name
    let path_prefix = if let Some(oid) = org_id {
        let org = rg_db::ops::org_ops::get_org(db, oid)
            .await?
            .ok_or_else(|| anyhow::anyhow!("organization not found"))?;
        org.name
    } else {
        let owner_user = rg_db::ops::user_ops::find_by_id(db, owner_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("owner not found"))?;
        owner_user.username
    };

    // Create bare git repo on disk using gix
    let git_path = repo_root.join(format!("{}/{}.git", path_prefix, name));
    std::fs::create_dir_all(&git_path)
        .with_context(|| format!("failed to create directory: {:?}", git_path))?;

    // Use gix to create bare repository
    gix::create::into(&git_path, gix::create::Kind::Bare, gix::create::Options::default())
        .with_context(|| format!("gix init --bare failed for {:?}", git_path))?;

    // Insert DB record
    let now = Utc::now();
    let model = RepoActiveModel {
        owner_id: Set(owner_id),
        name: Set(name.to_string()),
        description: Set(description.map(str::to_string)),
        is_private: Set(is_private),
        default_branch: Set("main".to_string()),
        stars_count: Set(0),
        forks_count: Set(0),
        org_id: Set(org_id),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    };

    repo_ops::create(db, model).await
}

/// Star a repository. Returns true if newly starred, false if unstarred.
pub async fn toggle_star(db: &DatabaseConnection, user_id: i64, repo_id: i64) -> Result<bool> {
    let starred = rg_db::ops::repo_star_ops::toggle_star(db, user_id, repo_id).await?;
    // Refresh cache count field
    rg_db::ops::repo_ops::update_stars_count(db, repo_id).await?;
    Ok(starred)
}

/// Check if user has starred a repo.
pub async fn is_starred(db: &DatabaseConnection, user_id: i64, repo_id: i64) -> Result<bool> {
    rg_db::ops::repo_star_ops::is_starred(db, user_id, repo_id).await
}

/// List stargazers of a repo.
pub async fn list_stargazers(
    db: &DatabaseConnection,
    repo_id: i64,
    offset: u64,
    limit: u64,
) -> Result<(Vec<rg_db::entities::repo_star::Model>, i64)> {
    rg_db::ops::repo_star_ops::list_stargazers(db, repo_id, offset, limit).await
}

/// Set watch state for a repo. Returns new watch_state.
pub async fn set_watch(
    db: &DatabaseConnection,
    user_id: i64,
    repo_id: i64,
    state: &str,
) -> Result<String> {
    rg_db::ops::repo_watch_ops::set_watch_state(db, user_id, repo_id, state).await
}

/// Get watch state.
pub async fn get_watch(
    db: &DatabaseConnection,
    user_id: i64,
    repo_id: i64,
) -> Result<Option<String>> {
    rg_db::ops::repo_watch_ops::get_watch_state(db, user_id, repo_id).await
}

/// Soft-delete a repository.
pub async fn delete_repo(db: &DatabaseConnection, repo_id: i64) -> Result<()> {
    rg_db::ops::repo_ops::soft_delete(db, repo_id).await
}

/// Find repo by owner/name (skip soft-deleted).
pub async fn find_active_repo_by_owner_name(
    db: &DatabaseConnection,
    owner: &str,
    repo_name: &str,
) -> Result<Option<rg_db::entities::repository::Model>> {
    // Reuse find_repo_by_owner_name logic but add deleted_at IS NULL filter
    // Actually existing find_repo_by_owner_name doesn't check deleted_at,
    // so we need to query via rg_db::ops and filter
    let repo = find_repo_by_owner_name(db, owner, repo_name).await?;
    Ok(repo.filter(|r| r.deleted_at.is_none()))
}

/// Fork a repository. Returns the forked repo.
pub async fn fork_repo(
    db: &DatabaseConnection,
    user_id: i64,
    owner: &str,
    repo_name: &str,
    repo_root: &PathBuf,
) -> Result<rg_db::entities::repository::Model> {
    let source_repo = find_repo_by_owner_name(db, owner, repo_name).await?
        .ok_or_else(|| anyhow::anyhow!("source repository not found"))?;

    if source_repo.is_private {
        can_read(db, owner, repo_name, Some(user_id)).await?;
    }

    let forker = user_ops::find_by_id(db, user_id).await?
        .ok_or_else(|| anyhow::anyhow!("user not found"))?;

    if repo_ops::find_by_owner_and_name(db, user_id, repo_name).await?.is_some() {
        bail!("repository '{}' already exists in your account", repo_name);
    }

    let source_path = repo_root.join(format!("{}/{}.git", owner, repo_name));
    let target_path = repo_root.join(format!("{}/{}.git", forker.username, repo_name));
    std::fs::create_dir_all(target_path.parent().unwrap())
        .with_context(|| format!("failed to create directory: {:?}", target_path.parent()))?;

    // TODO(gix): Local bare clone - gix doesn't support local bare clone via prepare_clone_bare
    // For now, use git CLI for local fork operations
    let output = std::process::Command::new("git")
        .arg("clone")
        .arg("--bare")
        .arg(&source_path)
        .arg(&target_path)
        .output()
        .context("git clone --bare failed")?;

    if !output.status.success() {
        anyhow::bail!("git clone --bare failed: {}", String::from_utf8_lossy(&output.stderr));
    }

    let now = Utc::now();
    let model = RepoActiveModel {
        owner_id: Set(user_id),
        name: Set(repo_name.to_string()),
        description: Set(source_repo.description.clone()),
        is_private: Set(source_repo.is_private),
        default_branch: Set(source_repo.default_branch.clone()),
        fork_id: Set(None),
        stars_count: Set(0),
        forks_count: Set(0),
        org_id: Set(None),
        origin_repo_id: Set(Some(source_repo.id)),
        created_at: Set(now),
        updated_at: Set(now),
        deleted_at: Set(None),
        ..Default::default()
    };

    let forked = repo_ops::create(db, model).await?;
    repo_ops::update_forks_count(db, source_repo.id).await?;

    Ok(forked)
}

/// List forks of a repository.
pub async fn list_forks(
    db: &DatabaseConnection,
    owner: &str,
    repo_name: &str,
    offset: u64,
    limit: u64,
) -> Result<(Vec<rg_db::entities::repository::Model>, i64)> {
    let repo = find_repo_by_owner_name(db, owner, repo_name).await?
        .ok_or_else(|| anyhow::anyhow!("repository not found"))?;
    repo_ops::list_forks(db, repo.id, offset, limit).await
}

/// Transfer a repository to a new owner.
pub async fn transfer_repo(
    db: &DatabaseConnection,
    user_id: i64,
    owner: &str,
    repo_name: &str,
    new_owner: &str,
    repo_root: &PathBuf,
) -> Result<rg_db::entities::repository::Model> {
    let repo = find_repo_by_owner_name(db, owner, repo_name).await?
        .ok_or_else(|| anyhow::anyhow!("repository not found"))?;

    if repo.owner_id != user_id {
        bail!("only repository owner can transfer");
    }

    let (new_owner_id, new_org_id, new_owner_name) = resolve_owner(db, new_owner).await?;

    if repo_ops::find_by_owner_and_name(db, new_owner_id, repo_name).await?.is_some() {
        bail!("repository '{}' already exists at destination", repo_name);
    }

    let old_path = repo_root.join(format!("{}/{}.git", owner, repo_name));
    let new_path = repo_root.join(format!("{}/{}.git", new_owner_name, repo_name));
    std::fs::create_dir_all(new_path.parent().unwrap())
        .with_context(|| format!("failed to create directory: {:?}", new_path.parent()))?;
    std::fs::rename(&old_path, &new_path)
        .with_context(|| format!("failed to move repository from {:?} to {:?}", old_path, new_path))?;

    repo_ops::update_owner(db, repo.id, new_owner_id, new_org_id).await?;

    repo_ops::find_by_owner_and_name(db, new_owner_id, repo_name).await?
        .ok_or_else(|| anyhow::anyhow!("repository not found after transfer"))
}

// ── Commit Status ──────────────────────────────────────────────────────

/// Create a commit status. Validates that state is one of: pending, success, failure, error.
pub async fn create_commit_status(
    db: &DatabaseConnection,
    repo_id: i64,
    sha: &str,
    state: &str,
    context: &str,
    description: Option<&str>,
    target_url: Option<&str>,
    creator_id: i64,
) -> Result<rg_db::entities::commit_status::Model> {
    let valid_states = ["pending", "success", "failure", "error"];
    if !valid_states.contains(&state) {
        bail!("invalid commit status state: '{}', must be one of: {:?}", state, valid_states);
    }

    let now = Utc::now();
    let model = rg_db::entities::commit_status::ActiveModel {
        repo_id: sea_orm::Set(repo_id),
        sha: sea_orm::Set(sha.to_string()),
        state: sea_orm::Set(state.to_string()),
        context: sea_orm::Set(context.to_string()),
        description: sea_orm::Set(description.map(str::to_string)),
        target_url: sea_orm::Set(target_url.map(str::to_string)),
        creator_id: sea_orm::Set(creator_id),
        created_at: sea_orm::Set(now),
        updated_at: sea_orm::Set(now),
        ..Default::default()
    };

    rg_db::ops::commit_status_ops::create_or_update(db, repo_id, sha, context, model).await
}

/// List all statuses for a commit SHA in a repository.
pub async fn list_commit_statuses(
    db: &DatabaseConnection,
    owner: &str,
    repo_name: &str,
    sha: &str,
) -> Result<Vec<rg_db::entities::commit_status::Model>> {
    let repo = find_repo_by_owner_name(db, owner, repo_name).await?
        .ok_or_else(|| anyhow::anyhow!("repository not found"))?;
    rg_db::ops::commit_status_ops::list_by_sha(db, repo.id, sha).await
}

/// Get the combined status for a commit SHA.
/// Returns "failure" if any failure, "pending" if any pending, "success" otherwise.
pub async fn get_combined_status(
    db: &DatabaseConnection,
    owner: &str,
    repo_name: &str,
    sha: &str,
) -> Result<serde_json::Value> {
    let repo = find_repo_by_owner_name(db, owner, repo_name).await?
        .ok_or_else(|| anyhow::anyhow!("repository not found"))?;

    let counts = rg_db::ops::commit_status_ops::get_combined_status(db, repo.id, sha).await?;
    let total: i64 = counts.iter().map(|(_, c)| c).sum();

    if total == 0 {
        return Ok(serde_json::json!({
            "state": "pending",
            "sha": sha,
            "total_count": 0,
            "statuses": []
        }));
    }

    let state_map: std::collections::HashMap<&str, i64> = counts.iter().map(|(k, v)| (k.as_str(), *v)).collect();
    let combined = if state_map.get("failure").map_or(false, |&c| c > 0) {
        "failure"
    } else if state_map.get("error").map_or(false, |&c| c > 0) {
        "failure"
    } else if state_map.get("pending").map_or(false, |&c| c > 0) {
        "pending"
    } else {
        "success"
    };

    let statuses = rg_db::ops::commit_status_ops::list_by_sha(db, repo.id, sha).await?;

    Ok(serde_json::json!({
        "state": combined,
        "sha": sha,
        "total_count": total,
        "statuses": statuses
    }))
}

// ── Watch Notifications ────────────────────────────────────────────────

/// Notify watchers of a push event to a repository.
/// This should be called from the push handler after a successful push.
pub async fn notify_watchers_push(
    db: &DatabaseConnection,
    repo_id: i64,
    repo_name: &str,
    pusher_name: &str,
    ref_name: &str,
) -> Result<()> {
    crate::notification::notify_watchers_push(db, repo_id, repo_name, pusher_name, ref_name).await
}
