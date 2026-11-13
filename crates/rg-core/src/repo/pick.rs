//! Cherry-pick and revert of one commit onto a branch, as a server-side write.
//!
//! The merge itself lives in [`rg_git::pick`]; this module adds what a *write*
//! needs on top: the same `require_pr` / `require_signed_commits` verdict the
//! other REST commit paths use, and the bare-repository lookup. It is a
//! separate file (not part of `service.rs`) so the commit paths stay apart.

use rg_git::pick::{PickKind, PickOutcome, PickRequest};
use sea_orm::DatabaseConnection;

use crate::error::{CoreError, CoreResult};

/// What the caller asked for; the identity is already resolved
/// ([`crate::agent::commit_identity`]).
#[derive(Debug, Clone)]
pub struct PickParams<'a> {
    pub kind: PickKind,
    /// Hex id of the commit to apply.
    pub source: &'a str,
    pub branch: &'a str,
    /// The commit the branch must be at.
    pub expected_sha: &'a str,
    pub mainline: Option<u32>,
    pub message: Option<&'a str>,
    pub dry_run: bool,
    pub identity_name: &'a str,
    pub identity_email: &'a str,
}

/// Apply `params` to `owner/repo_name`.
///
/// Branch protection is checked first (a protected branch refuses a direct
/// commit exactly as `contents` writes do), then the pick runs on a blocking
/// thread. A conflict, a stale `expected_sha` and the other expected outcomes
/// come back as [`PickOutcome`] variants, not errors.
#[allow(clippy::too_many_arguments)]
pub async fn pick_commit(
    db: &DatabaseConnection,
    repo_id: i64,
    owner: &str,
    repo_name: &str,
    repo_root: &std::path::Path,
    actor_id: Option<i64>,
    params: PickParams<'_>,
) -> CoreResult<PickOutcome> {
    if params.branch.is_empty()
        || gix::refs::FullName::try_from(format!("refs/heads/{}", params.branch)).is_err()
    {
        return Err(CoreError::invalid_input(format!(
            "invalid branch name: {}",
            params.branch
        )));
    }
    if params.expected_sha.trim().is_empty() {
        return Err(CoreError::invalid_input("expected_sha is required"));
    }
    if let Some(message) = params.message {
        if message.trim().is_empty() {
            return Err(CoreError::invalid_input("message must not be blank"));
        }
    }

    // Same verdict as the other REST commit paths: a pick must not be the way
    // around `require_pr` / `require_signed_commits`.
    crate::branch_protection::service::check_direct_commit_allowed(
        db,
        repo_id,
        params.branch,
        actor_id,
    )
    .await?;

    let repo_path = repo_root.join(format!("{owner}/{repo_name}.git"));
    if !repo_path.exists() {
        return Err(CoreError::internal(format!(
            "repository storage missing on disk: {}",
            repo_path.display()
        )));
    }

    let owned = (
        params.source.to_string(),
        params.branch.to_string(),
        params.expected_sha.to_string(),
        params.message.map(str::to_string),
        params.identity_name.to_string(),
        params.identity_email.to_string(),
    );
    let (kind, mainline, dry_run) = (params.kind, params.mainline, params.dry_run);
    tokio::task::spawn_blocking(move || {
        let (source, branch, expected, message, name, email) = owned;
        rg_git::pick::pick_commit(
            &repo_path,
            &PickRequest {
                kind,
                source: &source,
                branch: &branch,
                expected_tip: &expected,
                mainline,
                message: message.as_deref(),
                identity_name: &name,
                identity_email: &email,
                dry_run,
            },
        )
    })
    .await
    .map_err(|error| CoreError::internal(format!("pick task failed: {error}")))?
    .map_err(|error| CoreError::internal(format!("{} failed: {error:#}", kind.as_str())))
}
