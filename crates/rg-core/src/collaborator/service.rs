//! Collaborator service — repo collaborators + permission management.

use chrono::Utc;
use sea_orm::{DatabaseConnection, Set, SqlErr};

use rg_db::entities::repo_collaborator::{self, Model as RepoCollaborator};
use rg_db::ops::{repo_collaborator_ops, user_ops};

use crate::error::{CoreError, CoreResult};

fn validate_permission(permission: &str) -> CoreResult<()> {
    match permission {
        "read" | "write" | "admin" => Ok(()),
        _ => Err(CoreError::invalid_input(format!(
            "invalid permission: {permission}, must be read/write/admin"
        ))),
    }
}

/// Whether `error` (an `anyhow` chain from the db layer) is a unique-key violation.
fn is_unique_violation(error: &anyhow::Error) -> bool {
    error
        .chain()
        .filter_map(|cause| cause.downcast_ref::<sea_orm::DbErr>())
        .any(|db_error| {
            matches!(
                db_error.sql_err(),
                Some(SqlErr::UniqueConstraintViolation(_))
            )
        })
}

/// Add a collaborator to a repo.
pub async fn add_collaborator(
    db: &DatabaseConnection,
    owner: &str,
    repo_name: &str,
    user_id: i64,
    permission: String,
) -> CoreResult<RepoCollaborator> {
    let repo = resolve_repo(db, owner, repo_name).await?;

    // Validate permission
    validate_permission(&permission)?;

    // The user being added is the subject of the operation; an unknown one is
    // reported as not found instead of surfacing a foreign-key failure.
    if user_ops::find_by_id(db, user_id).await?.is_none() {
        return Err(CoreError::not_found(format!("user {user_id} not found")));
    }

    // Check if already a collaborator
    if let Some(existing) =
        repo_collaborator_ops::find_by_repo_and_user(db, repo.id, user_id).await?
    {
        return Err(CoreError::conflict(format!(
            "user {} is already a collaborator (permission: {})",
            user_id, existing.permission
        )));
    }

    let model = repo_collaborator::ActiveModel {
        id: sea_orm::NotSet,
        repo_id: Set(repo.id),
        user_id: Set(user_id),
        permission: Set(permission),
        created_at: Set(Utc::now()),
    };

    let created = match repo_collaborator_ops::create(db, model).await {
        Ok(created) => created,
        // Lost a race with a concurrent add of the same user.
        Err(error) if is_unique_violation(&error) => {
            return Err(CoreError::conflict(format!(
                "user {user_id} is already a collaborator"
            )));
        }
        Err(error) => return Err(error.into()),
    };
    crate::repo::service::invalidate_perm_cache_user(repo.id, user_id);
    Ok(created)
}

/// List all collaborators for a repo.
pub async fn list_collaborators(
    db: &DatabaseConnection,
    owner: &str,
    repo_name: &str,
) -> CoreResult<Vec<RepoCollaborator>> {
    let repo = resolve_repo(db, owner, repo_name).await?;
    Ok(repo_collaborator_ops::list_by_repo(db, repo.id).await?)
}

/// Update a collaborator's permission.
///
/// `collaborator_id` must belong to `owner/repo_name`; a collaborator row of
/// another repository is reported as not found rather than being modified.
pub async fn update_permission(
    db: &DatabaseConnection,
    owner: &str,
    repo_name: &str,
    collaborator_id: i64,
    permission: String,
) -> CoreResult<RepoCollaborator> {
    validate_permission(&permission)?;

    let repo = resolve_repo(db, owner, repo_name).await?;
    let mut collab = repo_collaborator_ops::find_by_id(db, collaborator_id)
        .await?
        .filter(|collab| collab.repo_id == repo.id)
        .ok_or_else(|| CoreError::not_found("collaborator not found"))?;

    let (repo_id, user_id) = (collab.repo_id, collab.user_id);
    collab.permission = permission;
    let active: repo_collaborator::ActiveModel = collab.into();
    let updated = repo_collaborator_ops::update(db, active).await?;
    crate::repo::service::invalidate_perm_cache_user(repo_id, user_id);
    Ok(updated)
}

/// Remove a collaborator from a repo.
pub async fn remove_collaborator(
    db: &DatabaseConnection,
    owner: &str,
    repo_name: &str,
    user_id: i64,
) -> CoreResult<()> {
    let repo = resolve_repo(db, owner, repo_name).await?;
    repo_collaborator_ops::delete_by_repo_and_user(db, repo.id, user_id).await?;
    crate::repo::service::invalidate_perm_cache_user(repo.id, user_id);
    Ok(())
}

/// Get the effective permission for a user on a repo.
/// Takes into account: repo owner (admin) + collaborator permission.
/// Returns: "admin" | "write" | "read" | None
pub async fn get_effective_permission(
    db: &DatabaseConnection,
    owner: &str,
    repo_name: &str,
    user_id: i64,
) -> CoreResult<Option<String>> {
    let repo = resolve_repo(db, owner, repo_name).await?;

    // Owner always has admin
    if repo.owner_id == user_id {
        return Ok(Some("admin".to_string()));
    }

    // Check collaborator
    Ok(repo_collaborator_ops::get_permission(db, repo.id, user_id).await?)
}

// ── Helpers ───────────────────────────────────────────────────────────

async fn resolve_repo(
    db: &DatabaseConnection,
    owner: &str,
    repo_name: &str,
) -> CoreResult<rg_db::entities::repository::Model> {
    // Resolve through the shared helper so organization repositories
    // (`acme/widget`) work, not only personal ones: this used to look the owner
    // up as a *user*, which made every org URL fail with "owner not found" and
    // left the creator's personal URL as the only way in.
    crate::repo::service::find_repo_by_owner_name(db, owner, repo_name)
        .await?
        .ok_or_else(|| CoreError::not_found("repository not found"))
}
