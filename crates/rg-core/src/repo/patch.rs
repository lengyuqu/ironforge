//! A caller-supplied unified diff, applied to a branch as one server-side
//! commit.
//!
//! The mechanics (temporary index, `git apply --cached`, `commit-tree`, the ref
//! CAS) live in [`rg_git::apply`]. This module adds what a *write* needs on top:
//! the same `require_pr` / `require_signed_commits` verdict as the other REST
//! commit paths, the secret scan of the blobs the patch introduces, and the
//! two-phase order that keeps a refused patch out of the repository — preview
//! in a scratch object database, scan, and only then commit.
//!
//! A dry run stops after the preview (and the scan), so it reports what would
//! happen without writing a blob, a ref or an audit row.

use rg_git::apply::{FileChange, PatchPreview, PatchRequest, PreviewOutcome};
use sea_orm::DatabaseConnection;

use crate::error::{CoreError, CoreResult};

/// What the caller asked for; the identity is already resolved
/// ([`crate::agent::commit_identity`]).
#[derive(Debug, Clone)]
pub struct ApplyPatchParams<'a> {
    /// Branch the commit goes on. It must already exist.
    pub branch: &'a str,
    /// The commit the branch must be at.
    pub expected_sha: &'a str,
    /// The unified diff.
    pub patch: &'a str,
    /// Commit message; `None` generates one.
    pub message: Option<&'a str>,
    pub dry_run: bool,
    pub identity_name: &'a str,
    pub identity_email: &'a str,
}

/// How an apply ended. Everything the caller can act on is a variant; only a
/// fault in the repository (or the database) is an `Err`.
#[derive(Debug, Clone)]
pub enum ApplyPatchOutcome {
    /// Dry run: the patch applies cleanly and would produce `preview.tree` on
    /// top of `preview.tip_before`. Nothing was written.
    Planned(PatchPreview),
    /// The commit was written and the branch moved to it.
    Committed {
        commit: String,
        tree: String,
        tip_before: String,
        files_changed: usize,
        changes: Vec<FileChange>,
    },
    /// The patch does not apply to the tip; `paths` are the ones git named.
    /// Nothing was written and the branch did not move.
    Conflict { paths: Vec<String>, tip: String },
    /// The patch applies but produces the tip's own tree.
    Empty { tip: String },
    /// The branch is not at `expected_sha` (`None`: the branch does not exist).
    TipAdvanced { actual: Option<String> },
}

/// Apply `params` to `owner/repo_name`.
///
/// Branch protection is checked first (a protected branch refuses a direct
/// commit exactly as `contents` writes do), then the patch is previewed on a
/// blocking thread, scanned, and — only if both pass — committed.
#[allow(clippy::too_many_arguments)]
pub async fn apply_patch(
    db: &DatabaseConnection,
    repo_id: i64,
    owner: &str,
    repo_name: &str,
    repo_root: &std::path::Path,
    actor_id: Option<i64>,
    params: ApplyPatchParams<'_>,
) -> CoreResult<ApplyPatchOutcome> {
    validate(&params)?;

    // Same verdict as the other REST commit paths: an apply must not be the way
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

    let owned = OwnedPatch {
        branch: params.branch.to_string(),
        expected_tip: params.expected_sha.to_string(),
        patch: params.patch.to_string(),
        message: params.message.map(str::to_string),
        identity_name: params.identity_name.to_string(),
        identity_email: params.identity_email.to_string(),
    };

    let preview_repo = repo_path.clone();
    let preview_owned = owned.clone();
    let preview = tokio::task::spawn_blocking(move || {
        rg_git::apply::preview_patch(&preview_repo, &preview_owned.request())
    })
    .await
    .map_err(|error| CoreError::internal(format!("apply-patch preview task failed: {error}")))?
    .map_err(|error| CoreError::internal(format!("apply-patch preview failed: {error:#}")))?;

    let preview = match preview {
        PreviewOutcome::Invalid { reason } => return Err(CoreError::invalid_input(reason)),
        PreviewOutcome::TipAdvanced { actual } => {
            return Ok(ApplyPatchOutcome::TipAdvanced { actual })
        }
        PreviewOutcome::Conflict { paths, tip } => {
            return Ok(ApplyPatchOutcome::Conflict { paths, tip })
        }
        PreviewOutcome::Empty { tip } => return Ok(ApplyPatchOutcome::Empty { tip }),
        PreviewOutcome::Previewed(preview) => preview,
    };

    let blobs: Vec<(&str, &[u8])> = preview
        .new_blobs
        .iter()
        .map(|(path, bytes)| (path.as_str(), bytes.as_slice()))
        .collect();

    if params.dry_run {
        // A dry run must not write an audit row, so it scans directly instead of
        // going through the recording helper. A blocking hit still refuses the
        // call: the point of a dry run is to say what the real one would do.
        let report = rg_git::secret_scan::scan_files(
            &blobs,
            &rg_git::secret_scan::AllowList::default_rules(),
        );
        if let Some(message) = report.rejection_message() {
            return Err(CoreError::secret_detected(message));
        }
        return Ok(ApplyPatchOutcome::Planned(preview));
    }

    if let Some(message) = crate::repo::service::scan_commit_content(
        db,
        repo_id,
        &format!("{owner}/{repo_name}"),
        actor_id,
        &blobs,
    )
    .await
    {
        return Err(CoreError::secret_detected(message));
    }

    let commit_repo = repo_path;
    let commit_owned = owned;
    let outcome = tokio::task::spawn_blocking(move || {
        rg_git::apply::commit_patch(&commit_repo, &commit_owned.request())
    })
    .await
    .map_err(|error| CoreError::internal(format!("apply-patch commit task failed: {error}")))?
    .map_err(|error| CoreError::internal(format!("apply-patch commit failed: {error:#}")))?;

    match outcome {
        rg_git::apply::CommitOutcome::Invalid { reason } => Err(CoreError::invalid_input(reason)),
        rg_git::apply::CommitOutcome::Conflict { paths, tip } => {
            Ok(ApplyPatchOutcome::Conflict { paths, tip })
        }
        rg_git::apply::CommitOutcome::Empty { tip } => Ok(ApplyPatchOutcome::Empty { tip }),
        rg_git::apply::CommitOutcome::TipAdvanced { actual } => {
            Ok(ApplyPatchOutcome::TipAdvanced { actual })
        }
        rg_git::apply::CommitOutcome::Committed {
            commit,
            tree,
            tip_before,
            files_changed,
            changes,
        } => {
            // The preview and the commit run the same sequence; a different tree
            // means the repository changed under us in a way the ref guard did
            // not catch, which must not be papered over.
            if tree != preview.tree {
                return Err(CoreError::internal(format!(
                    "apply-patch produced different trees in preview ({}) and on disk ({tree})",
                    preview.tree
                )));
            }
            Ok(ApplyPatchOutcome::Committed {
                commit,
                tree,
                tip_before,
                files_changed,
                changes,
            })
        }
    }
}

fn validate(params: &ApplyPatchParams<'_>) -> CoreResult<()> {
    if params.branch.is_empty()
        || gix::refs::FullName::try_from(format!("refs/heads/{}", params.branch)).is_err()
    {
        return Err(CoreError::invalid_input(format!(
            "invalid branch name: {}",
            params.branch
        )));
    }
    let expected = params.expected_sha.trim();
    if gix::ObjectId::from_hex(expected.as_bytes()).is_err() {
        return Err(CoreError::invalid_input(
            "expected_sha must be a full commit id (40 or 64 hex characters)",
        ));
    }
    if let Some(message) = params.message {
        if message.trim().is_empty() {
            return Err(CoreError::invalid_input("message must not be blank"));
        }
    }
    if params.patch.trim().is_empty() {
        return Err(CoreError::invalid_input("patch must not be empty"));
    }
    Ok(())
}

/// The request with owned strings, so it can cross into a blocking thread.
#[derive(Clone)]
struct OwnedPatch {
    branch: String,
    expected_tip: String,
    patch: String,
    message: Option<String>,
    identity_name: String,
    identity_email: String,
}

impl OwnedPatch {
    fn request(&self) -> PatchRequest<'_> {
        PatchRequest {
            branch: &self.branch,
            expected_tip: &self.expected_tip,
            patch: &self.patch,
            message: self.message.as_deref(),
            identity_name: &self.identity_name,
            identity_email: &self.identity_email,
        }
    }
}
