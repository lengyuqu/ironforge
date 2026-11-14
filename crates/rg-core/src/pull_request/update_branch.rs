//! Bring a pull request's head branch up to date with its base: merge the base
//! branch into the head branch, as `git merge <base>` on the head would.
//!
//! The head branch gets one new merge commit (first parent: the old head,
//! second parent: the base tip) and moves to it with a compare-and-swap, so the
//! update either lands on exactly the commit the caller looked at or does not
//! happen. A merge that conflicts changes nothing: no commit is made and no ref
//! moves; the conflicting files come back instead.
//!
//! # Who may do it
//!
//! Updating the head branch is writing to it, so it follows the rules of a
//! `git push` to that branch: the caller needs write access to the repository
//! (the HTTP layer checks that, and the token's own branch limit), and the
//! branch's protection rule must allow a direct server-side commit
//! ([`crate::branch_protection::service::check_direct_commit_allowed`], the same
//! verdict the REST contents API uses). A protected head branch that requires a
//! pull request or signed commits therefore refuses, as it refuses a push.
//!
//! # What it does not do
//!
//! * A pull request from a fork is refused ([`UpdateBranchError::ForkUnsupported`]):
//!   its head branch lives in another repository, and writing there is not this
//!   repository's call.
//! * It never touches the base branch.
//! * Webhooks, CI and the pull-request head ref are the caller's job after a
//!   successful update (the HTTP layer announces the branch move the way a push
//!   is announced).

use std::path::Path;

use rg_git::merge_analysis::{self, ConflictReport, MergeVerdict};
use sea_orm::DatabaseConnection;

use rg_db::entities::pull_request::Model as PullRequest;

use crate::branch_protection::merge_gates::state_gate;
use crate::error::CoreError;

/// What an update-branch call did (or, for a dry run, would do).
#[derive(Debug)]
pub enum UpdateBranchOutcome {
    /// The head already contains the base. Nothing was written.
    AlreadyUpToDate { head_sha: String, base_sha: String },
    /// The head moved from `head_sha_before` to `head_sha_after`, a merge
    /// commit; `dry_run` outcomes carry no commit.
    Updated {
        head_sha_before: String,
        head_sha_after: String,
        base_sha: String,
    },
    /// Dry run: the update would succeed.
    WouldUpdate { head_sha: String, base_sha: String },
    /// The base does not merge cleanly into the head. Nothing was written.
    Conflicts {
        head_sha: String,
        base_sha: String,
        report: ConflictReport,
    },
}

/// Why an update-branch call did not run.
#[derive(Debug)]
pub enum UpdateBranchError {
    /// The head branch is at `current` (`None`: gone), not at the commit the
    /// caller named. Nothing was written.
    HeadMoved {
        current: Option<String>,
        expected: String,
    },
    /// The pull request's head branch is in a fork.
    ForkUnsupported,
    /// The two branches share no history.
    NoMergeBase,
    Core(CoreError),
}

impl From<CoreError> for UpdateBranchError {
    fn from(error: CoreError) -> Self {
        Self::Core(error)
    }
}

impl From<anyhow::Error> for UpdateBranchError {
    fn from(error: anyhow::Error) -> Self {
        Self::Core(CoreError::from(error))
    }
}

/// Merge the base branch of `pr` into its head branch.
///
/// `expected_head_sha` is required: the update is built from exactly that
/// commit. `identity` is the author and committer of the merge commit.
#[allow(clippy::too_many_arguments)]
pub async fn update_branch(
    db: &DatabaseConnection,
    repo_root: &Path,
    owner: &str,
    repo_name: &str,
    pr: &PullRequest,
    actor_id: i64,
    expected_head_sha: &str,
    dry_run: bool,
    identity: (String, String),
) -> Result<UpdateBranchOutcome, UpdateBranchError> {
    if pr.head_repo_id.is_some() {
        return Err(UpdateBranchError::ForkUnsupported);
    }
    if let Some(refusal) = state_gate(pr).state_refusal() {
        return Err(refusal.into());
    }

    // Writing the head branch is held to the branch's own protection rule, the
    // same verdict as a direct server-side commit.
    crate::branch_protection::service::check_direct_commit_allowed(
        db,
        pr.repo_id,
        &pr.head_branch,
        Some(actor_id),
    )
    .await?;

    let repo_path = repo_root.join(format!("{owner}/{repo_name}.git"));
    if !repo_path.exists() {
        return Err(CoreError::internal("repository storage missing on disk").into());
    }

    let head_branch = pr.head_branch.clone();
    let base_branch = pr.base_branch.clone();
    let expected = expected_head_sha.to_string();

    tokio::task::spawn_blocking(move || -> Result<UpdateBranchOutcome, UpdateBranchError> {
        let head_ref = format!("refs/heads/{head_branch}");
        let current = rg_git::ops::try_rev_parse(&repo_path, &head_ref).map_err(CoreError::from)?;
        let Some(head_sha) = current.clone() else {
            return Err(UpdateBranchError::HeadMoved {
                current: None,
                expected,
            });
        };
        if head_sha != expected {
            return Err(UpdateBranchError::HeadMoved { current, expected });
        }
        let base_sha = rg_git::ops::try_rev_parse(&repo_path, &format!("refs/heads/{base_branch}"))
            .map_err(CoreError::from)?
            .ok_or_else(|| {
                CoreError::conflict(format!("base branch '{base_branch}' no longer exists"))
            })?;

        if merge_analysis::is_ancestor(&repo_path, &base_sha, &head_sha)? {
            return Ok(UpdateBranchOutcome::AlreadyUpToDate { head_sha, base_sha });
        }

        let analysis =
            merge_analysis::analyze_merge(&repo_path, &base_sha, &head_sha, !dry_run, None)?;
        match analysis.verdict {
            MergeVerdict::NoMergeBase => Err(UpdateBranchError::NoMergeBase),
            MergeVerdict::Conflicts(report) => Ok(UpdateBranchOutcome::Conflicts {
                head_sha,
                base_sha,
                report,
            }),
            MergeVerdict::Clean { .. } if dry_run => {
                Ok(UpdateBranchOutcome::WouldUpdate { head_sha, base_sha })
            }
            MergeVerdict::Clean { tree } => {
                let tree =
                    tree.ok_or_else(|| CoreError::internal("merged tree was not written"))?;
                let message = format!("Merge branch '{base_branch}' into {head_branch}");
                let commit = merge_analysis::create_commit_object(
                    &repo_path,
                    &tree,
                    &[&head_sha, &base_sha],
                    &message,
                    &identity.0,
                    &identity.1,
                )?;
                // The swap is what makes "exactly the commit the caller saw" true:
                // a push that lands between the read above and this write loses.
                if let Err(error) = merge_analysis::update_ref_expecting(
                    &repo_path,
                    &head_ref,
                    &commit,
                    &head_sha,
                    "update branch",
                ) {
                    let now = rg_git::ops::try_rev_parse(&repo_path, &head_ref)
                        .ok()
                        .flatten();
                    return Err(if now.as_deref() != Some(head_sha.as_str()) {
                        UpdateBranchError::HeadMoved {
                            current: now,
                            expected: head_sha,
                        }
                    } else {
                        UpdateBranchError::Core(CoreError::internal(format!(
                            "failed to move the head branch: {error}"
                        )))
                    });
                }
                Ok(UpdateBranchOutcome::Updated {
                    head_sha_before: head_sha,
                    head_sha_after: commit,
                    base_sha,
                })
            }
        }
    })
    .await
    .map_err(|e| CoreError::internal(format!("update-branch task failed: {e}")))?
}
