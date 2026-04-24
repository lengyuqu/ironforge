//! Repository service — business logic for repo creation and access control.

use anyhow::{bail, Context, Result};
use chrono::Utc;
use sea_orm::{ActiveValue::Set, DatabaseConnection};
use std::path::PathBuf;

use rg_db::{
    entities::repository::ActiveModel as RepoActiveModel,
    ops::{repo_ops, user_ops},
};

/// Check whether `actor_id` (None = anonymous) can read `owner/repo`.
pub async fn can_read(
    db: &DatabaseConnection,
    owner: &str,
    repo_name: &str,
    actor_id: Option<i64>,
) -> Result<bool> {
    let owner_user = user_ops::find_by_username(db, owner)
        .await?
        .ok_or_else(|| anyhow::anyhow!("user '{}' not found", owner))?;

    let repo = repo_ops::find_by_owner_and_name(db, owner_user.id, repo_name)
        .await?
        .ok_or_else(|| anyhow::anyhow!("repository '{}/{}' not found", owner, repo_name))?;

    if !repo.is_private {
        return Ok(true);
    }

    // Private repo: actor must be the owner (or an admin — TODO: collaborators Phase 4)
    match actor_id {
        Some(id) => Ok(id == owner_user.id),
        None => Ok(false),
    }
}

/// Check whether `actor_id` can write to `owner/repo`.
/// Currently only the owner can write.
pub async fn can_write(
    db: &DatabaseConnection,
    owner: &str,
    repo_name: &str,
    actor_id: Option<i64>,
) -> Result<bool> {
    let owner_user = user_ops::find_by_username(db, owner)
        .await?
        .ok_or_else(|| anyhow::anyhow!("user '{}' not found", owner))?;

    match actor_id {
        Some(id) => Ok(id == owner_user.id),
        None => Ok(false),
    }
}

/// Create a new repository (bare git init + DB record).
pub async fn create_repo(
    db: &DatabaseConnection,
    owner_id: i64,
    name: &str,
    description: Option<&str>,
    is_private: bool,
    repo_root: &PathBuf,
) -> Result<rg_db::entities::repository::Model> {
    // Check name conflict
    if repo_ops::find_by_owner_and_name(db, owner_id, name).await?.is_some() {
        bail!("repository '{}' already exists", name);
    }

    // Create bare git repo on disk
    let owner_user = rg_db::ops::user_ops::find_by_id(db, owner_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("owner not found"))?;

    let git_path = repo_root.join(format!("{}/{}.git", owner_user.username, name));
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
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    };

    repo_ops::create(db, model).await
}
