//! Branch protection service — protected branches + required status checks.

use anyhow::{bail, Context, Result};
use chrono::Utc;
use sea_orm::{ActiveModelTrait, DatabaseConnection, Set};

use rg_db::entities::protected_branch::{self, Model as ProtectedBranch};
use rg_db::ops::{protected_branch_ops, repo_ops, pull_request_ops, pr_review_ops};

/// Create a branch protection rule.
pub async fn create_protection(
    db: &DatabaseConnection,
    owner: &str,
    repo_name: &str,
    branch_name: String,
    require_pr: bool,
    require_status_check: bool,
    required_status_checks: Option<Vec<String>>,
    require_approval: bool,
    required_approvals: Option<i64>,
    allow_force_push: bool,
    allowed_push_user_ids: Option<Vec<i64>>,
) -> Result<ProtectedBranch> {
    let repo = resolve_repo(db, owner, repo_name).await?;

    // Check if protection already exists
    if protected_branch_ops::find_by_repo_and_branch(db, repo.id, &branch_name)
        .await?
        .is_some()
    {
        bail!("branch '{}' is already protected", branch_name);
    }

    let model = protected_branch::ActiveModel {
        id: sea_orm::NotSet,
        repo_id: Set(repo.id),
        branch_name: Set(branch_name),
        require_pr: Set(require_pr),
        require_status_check: Set(require_status_check),
        required_status_checks: Set(required_status_checks.map(|v| serde_json::to_string(&v).unwrap_or_default())),
        require_approval: Set(require_approval),
        required_approvals: Set(required_approvals),
        allow_force_push: Set(allow_force_push),
        allowed_push_user_ids: Set(allowed_push_user_ids.map(|v| serde_json::to_string(&v).unwrap_or_default())),
        created_at: Set(Utc::now()),
        updated_at: Set(Utc::now()),
    };

    protected_branch_ops::create(db, model).await
}

/// List all branch protection rules for a repo.
pub async fn list_protections(
    db: &DatabaseConnection,
    owner: &str,
    repo_name: &str,
) -> Result<Vec<ProtectedBranch>> {
    let repo = resolve_repo(db, owner, repo_name).await?;
    protected_branch_ops::list_by_repo(db, repo.id).await
}

/// Get a branch protection rule by ID.
pub async fn get_protection(
    db: &DatabaseConnection,
    protection_id: i64,
) -> Result<ProtectedBranch> {
    protected_branch_ops::find_by_id(db, protection_id)
        .await?
        .context("protection rule not found")
}

/// Update a branch protection rule.
pub async fn update_protection(
    db: &DatabaseConnection,
    protection_id: i64,
    require_pr: Option<bool>,
    require_status_check: Option<bool>,
    required_status_checks: Option<Vec<String>>,
    require_approval: Option<bool>,
    required_approvals: Option<i64>,
    allow_force_push: Option<bool>,
    allowed_push_user_ids: Option<Vec<i64>>,
) -> Result<ProtectedBranch> {
    let mut protection = protected_branch_ops::find_by_id(db, protection_id)
        .await?
        .context("protection rule not found")?;

    if let Some(v) = require_pr {
        protection.require_pr = v;
    }
    if let Some(v) = require_status_check {
        protection.require_status_check = v;
    }
    if let Some(v) = required_status_checks {
        protection.required_status_checks = Some(serde_json::to_string(&v).unwrap_or_default());
    }
    if let Some(v) = require_approval {
        protection.require_approval = v;
    }
    if let Some(v) = required_approvals {
        protection.required_approvals = Some(v);
    }
    if let Some(v) = allow_force_push {
        protection.allow_force_push = v;
    }
    if let Some(v) = allowed_push_user_ids {
        protection.allowed_push_user_ids = Some(serde_json::to_string(&v).unwrap_or_default());
    }
    protection.updated_at = Utc::now();

    let active: protected_branch::ActiveModel = protection.into();
    protected_branch_ops::update(db, active).await
}

/// Delete a branch protection rule.
pub async fn delete_protection(
    db: &DatabaseConnection,
    protection_id: i64,
) -> Result<()> {
    protected_branch_ops::delete_by_id(db, protection_id).await
}

/// Check if a push to a branch is allowed.
/// Returns Ok(()) if allowed, or Err with the reason if blocked.
pub async fn check_push_allowed(
    db: &DatabaseConnection,
    repo_id: i64,
    branch_name: &str,
    user_id: Option<i64>,
) -> Result<()> {
    let protection = protected_branch_ops::find_by_repo_and_branch(db, repo_id, branch_name)
        .await?;

    let Some(protection) = protection else {
        // Not protected, push is allowed
        return Ok(());
    };

    // Check if user is in the allowed list
    if let Some(uid) = user_id {
        if let Some(allowed_json) = &protection.allowed_push_user_ids {
            if let Ok(allowed_ids) = serde_json::from_str::<Vec<i64>>(allowed_json) {
                if allowed_ids.contains(&uid) {
                    return Ok(());
                }
            }
        }
    }

    if protection.require_pr {
        bail!("push to protected branch '{}' is not allowed; open a pull request instead", branch_name);
    }

    if !protection.allow_force_push {
        bail!("force push to protected branch '{}' is not allowed", branch_name);
    }

    Ok(())
}

/// Check if a PR merge is allowed under branch protection rules.
pub async fn check_merge_allowed(
    db: &DatabaseConnection,
    repo_id: i64,
    target_branch: &str,
    pr_id: i64,
) -> Result<()> {
    let protection = protected_branch_ops::find_by_repo_and_branch(db, repo_id, target_branch)
        .await?;

    let Some(protection) = protection else {
        return Ok(());
    };

    // Check required approvals
    if protection.require_approval {
        let required = protection.required_approvals.unwrap_or(1);
        let approval_count = pr_review_ops::count_approvals(db, pr_id).await?;
        if approval_count < required {
            bail!(
                "merging into protected branch '{}' requires at least {} approval(s), got {}",
                target_branch,
                required,
                approval_count
            );
        }
    }

    // Check required status checks
    if protection.require_status_check {
        if let Some(checks_json) = &protection.required_status_checks {
            if let Ok(required_checks) = serde_json::from_str::<Vec<String>>(checks_json) {
                // TODO: Check pipeline job statuses against required_checks
                // For now, just log that checks are required
                tracing::info!(
                    branch = %target_branch,
                    checks = ?required_checks,
                    "Branch protection: status checks required (not yet enforced)"
                );
            }
        }
    }

    Ok(())
}

// ── Helpers ───────────────────────────────────────────────────────────

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
