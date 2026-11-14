//! `POST /repos/{owner}/{name}/commits/apply-patch` — commit a caller-supplied
//! unified diff onto a branch.
//!
//! This is the endpoint the MCP "unified diff" input (#48) needed. It shares the
//! contract of the other REST commit paths: `branch` and `expected_sha` are both
//! required and a branch that moved is a 409 `REF_ADVANCED`; a branch that does
//! not exist is never created; a patch that does not apply to the tip is a 409
//! `PATCH_CONFLICT` naming the paths, and nothing is written; a success is
//! authored and committed by [`rg_core::agent::commit_identity`] and announced
//! like a push (pull-request heads, CI, webhooks, the event log).
//!
//! On top of that it is a *write fed by arbitrary text*, so the patch is
//! validated before anything is applied (size, file count, and paths that would
//! escape the repository or land in `.git`) and the blobs it introduces go
//! through the same secret-scanning rules as a push — a blocking hit is a 422
//! `SECRET_DETECTED`.
//!
//! A dry run reports the tree and the file changes without writing a ref, an
//! object, an audit row or a CI/webhook effect.

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};

use rg_core::repo::patch::{ApplyPatchOutcome, ApplyPatchParams};
use rg_git::apply::FileChange;

use crate::api::auth::{ensure_token_may_write_branch, extract_bearer_claims};
use crate::api::repo_content::{dry_run_requested, get_latest_commit_sha, run_blocking};
use crate::error::AppError;
use crate::AppState;

/// Request body for an apply-patch commit.
#[derive(Deserialize, utoipa::ToSchema)]
pub struct ApplyPatchRequest {
    /// Branch the new commit goes on. It must already exist.
    pub branch: String,
    /// The commit the branch must be at; a branch that moved is a 409
    /// `REF_ADVANCED`, and a branch that does not exist is not created.
    pub expected_sha: String,
    /// The unified diff, `git diff`-shaped (`a/…`, `b/…`, `/dev/null`). At most
    /// 1 MiB and 500 paths; paths outside the repository and paths inside
    /// `.git` are refused.
    pub patch: String,
    /// Commit message; omitted, one is generated from the branch and the change.
    #[serde(default)]
    pub message: Option<String>,
    /// Check everything and report what would happen without writing. The
    /// `X-Dry-Run: 1` header means the same.
    #[serde(default)]
    pub dry_run: Option<bool>,
}

/// One path the patch touches.
#[derive(Serialize, utoipa::ToSchema)]
pub struct ApplyPatchFileResult {
    pub path: String,
    /// `added` | `modified` | `deleted` | `type_changed`
    pub status: String,
}

impl From<&FileChange> for ApplyPatchFileResult {
    fn from(change: &FileChange) -> Self {
        Self {
            path: change.path.clone(),
            status: change.status.as_str().to_string(),
        }
    }
}

/// Answer to an apply-patch that was carried out.
#[derive(Serialize, utoipa::ToSchema)]
pub struct ApplyPatchResponse {
    pub dry_run: bool,
    pub branch: String,
    /// The branch tip before the new commit.
    pub tip_before: String,
    /// The new commit.
    pub commit_sha: String,
    /// The tree the commit carries.
    pub tree_sha: String,
    pub files_changed: usize,
    pub changes: Vec<ApplyPatchFileResult>,
}

/// `POST /api/v1/repos/:owner/:name/commits/apply-patch`
///
/// Apply a unified diff on top of `expected_sha` as one commit on `branch`.
/// Either the whole patch applies or the branch does not move.
#[utoipa::path(
    post,
    path = "/repos/{owner}/{name}/commits/apply-patch",
    tag = "Repository Content",
    params(
        ("owner" = String, Path, description = "owner"),
        ("name" = String, Path, description = "name"),
    ),
    request_body = ApplyPatchRequest,
    responses(
        (status = 200, description = "Success (or, with dry_run, what would happen)", body = ApplyPatchResponse),
        (status = 400, description = "Bad request (malformed, oversized or unsafe patch)", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
        (status = 403, description = "Forbidden", body = serde_json::Value),
        (status = 404, description = "Not found", body = serde_json::Value),
        (status = 409, description = "Conflict (does not apply, or the branch moved)", body = serde_json::Value),
        (status = 422, description = "Secret detected in the patch", body = serde_json::Value),
    ),
)]
pub async fn apply_patch(
    State(state): State<AppState>,
    Path((owner, repo)): Path<(String, String)>,
    headers: HeaderMap,
    Json(req): Json<ApplyPatchRequest>,
) -> impl IntoResponse {
    if let Err(e) = super::repo_content::validate_owner_repo(&owner, &repo) {
        return e.into_response();
    }
    let (repo_model, user) =
        match super::repo_content::resolve_and_check_write_access(&state, &headers, &owner, &repo)
            .await
        {
            Ok(pair) => pair,
            Err(e) => return e.into_response(),
        };
    // A token limited to some branches may only commit to those.
    if let Some(claims) = extract_bearer_claims(&headers, &state.jwt_secret) {
        if let Err(e) = ensure_token_may_write_branch(&state.db, &claims, &req.branch).await {
            return e.into_response();
        }
    }

    let dry_run = dry_run_requested(&headers, req.dry_run);
    let (identity_name, identity_email) = rg_core::agent::commit_identity(&user);

    let outcome = rg_core::repo::patch::apply_patch(
        &state.db,
        repo_model.id,
        &owner,
        &repo,
        &state.repo_root,
        Some(user.id),
        ApplyPatchParams {
            branch: &req.branch,
            expected_sha: &req.expected_sha,
            patch: &req.patch,
            message: req.message.as_deref(),
            dry_run,
            identity_name: &identity_name,
            identity_email: &identity_email,
        },
    )
    .await;

    match outcome {
        Err(e) => AppError::from(e).into_response(),
        Ok(ApplyPatchOutcome::Planned(preview)) => {
            let changes: Vec<ApplyPatchFileResult> =
                preview.changes.iter().map(ApplyPatchFileResult::from).collect();
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "dry_run": true,
                    "branch": req.branch,
                    "tip_before": preview.tip_before,
                    "tree_sha": preview.tree,
                    "files_changed": preview.files_changed,
                    "changes": changes,
                    "would": [
                        {
                            "effect": "ref_update",
                            "ref": format!("refs/heads/{}", req.branch),
                            "old": preview.tip_before,
                            "creates_branch": false,
                        },
                        {
                            "effect": "commit",
                            // Computed in a scratch object database: it names no
                            // stored object.
                            "tree": preview.tree,
                            "files_changed": preview.files_changed,
                        },
                    ],
                    "not_evaluated": ["webhook", "ci_trigger", "notification"],
                })),
            )
                .into_response()
        }
        Ok(ApplyPatchOutcome::Committed {
            commit,
            tree,
            tip_before,
            files_changed,
            changes,
        }) => {
            let repo_path = state.repo_root.join(format!("{owner}/{repo}.git"));
            let hook_repo_path = repo_path.clone();
            let hook_branch = req.branch.clone();
            // The commit already landed; failing to re-read the tip must not
            // turn that into an error the client would retry.
            let new_sha = run_blocking(move || get_latest_commit_sha(&repo_path, &hook_branch))
                .await
                .unwrap_or_else(|e| {
                    tracing::warn!(error = %e, "could not resolve new commit sha after write");
                    String::new()
                });
            let new_sha = if new_sha.is_empty() {
                commit.clone()
            } else {
                new_sha
            };
            crate::api::repo_content::announce_branch_move(
                &state,
                &owner,
                &repo,
                hook_repo_path,
                &req.branch,
                Some(tip_before.clone()),
                &new_sha,
                user.id,
            );
            crate::api::audit::record_write(
                &state.db,
                user.id,
                "repo.apply_patch",
                "repo",
                Some(repo_model.id),
                &format!("{owner}/{repo}"),
                &headers,
                serde_json::json!({
                    "branch": req.branch,
                    "commit_sha": new_sha,
                    "before": tip_before,
                    "tree": tree,
                    "files_changed": files_changed,
                    "paths": changes.iter().map(|c| c.path.clone()).collect::<Vec<_>>(),
                }),
            )
            .await;
            (
                StatusCode::OK,
                Json(ApplyPatchResponse {
                    dry_run: false,
                    branch: req.branch,
                    tip_before,
                    commit_sha: commit,
                    tree_sha: tree,
                    files_changed,
                    changes: changes.iter().map(ApplyPatchFileResult::from).collect(),
                }),
            )
                .into_response()
        }
        Ok(ApplyPatchOutcome::Conflict { paths, tip }) => AppError::conflict_because(
            "PATCH_CONFLICT",
            false,
            format!(
                "the patch does not apply to '{}': {} conflicting path(s)",
                req.branch,
                paths.len()
            ),
            serde_json::json!({ "tip_sha": tip, "paths": paths }),
            "the branch was not changed; rebase the change onto the current tip and retry",
        )
        .into_response(),
        Ok(ApplyPatchOutcome::Empty { tip }) => AppError::conflict_because(
            "NOTHING_TO_APPLY",
            false,
            "applying this patch would not change the branch",
            serde_json::json!({ "tip_sha": tip }),
            "nothing to commit; the branch already holds this content",
        )
        .into_response(),
        Ok(ApplyPatchOutcome::TipAdvanced { actual }) => AppError::conflict_because(
            "REF_ADVANCED",
            true,
            "the branch moved; it is no longer at the expected commit",
            serde_json::json!({ "current_sha": actual, "expected_sha": req.expected_sha }),
            "re-read the branch, check the new commits, and retry with expected_sha set to current_sha",
        )
        .into_response(),
    }
}
