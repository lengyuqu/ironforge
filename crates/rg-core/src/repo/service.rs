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
#[allow(dead_code)]
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

    // Create bare git repo on disk
    let git_path = repo_root.join(format!("{}/{}.git", path_prefix, name));
    std::fs::create_dir_all(&git_path)
        .with_context(|| format!("failed to create directory: {:?}", git_path))?;

    let output = std::process::Command::new("git")
        .arg("init")
        .arg("--bare")
        .arg(&git_path)
        .output()
        .context("git init --bare failed")?;

    if !output.status.success() {
        bail!(
            "git init --bare failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

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
