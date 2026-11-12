//! `POST /repos/{owner}/{name}/commits` — many file edits, one commit.
//!
//! The single-file routes under `/contents` write one commit per call, so an
//! agent that changes ten files leaves ten commits and a branch that can only
//! be reasoned about as a whole if it succeeded halfway. This route takes the
//! whole set, checks all of it, and moves the branch once — or not at all.
//!
//! It shares the permission surface of the single-file routes: the same write
//! access check, the same `require_pr` / `require_signed_commits` verdict, the
//! same token branch limit, the same `X-Dry-Run`. What it does *not* share is
//! the `HEAD` fallback: a batch names the branch and the commit it is based on.

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};

use rg_core::error::CoreError;
use rg_core::repo::service::{BatchCommitError, BatchCommitOptions, BatchCommitReport, BatchEdit};

use crate::api::auth::{ensure_token_may_write_branch, extract_bearer_claims};
use crate::api::repo_content::{dry_run_requested, get_latest_commit_sha, run_blocking};
use crate::error::AppError;
use crate::AppState;

/// Two paths in one request touched each other.
const PATH_CONFLICT: &str = "PATH_CONFLICT";

/// One file change inside [`BatchCommitRequest`].
#[derive(Deserialize, utoipa::ToSchema)]
#[serde(tag = "operation", rename_all = "snake_case")]
pub enum BatchCommitFileRequest {
    /// Create a file that does not exist yet.
    Create { path: String, content: String },
    /// Replace a file, pinned to the blob the caller last saw.
    Update {
        path: String,
        content: String,
        sha: String,
    },
    /// Remove a file, pinned to the blob the caller last saw.
    Delete { path: String, sha: String },
    /// Move a file, keeping its content and mode.
    Move {
        from: String,
        to: String,
        sha: String,
    },
}

impl BatchCommitFileRequest {
    fn into_edit(self) -> BatchEdit {
        match self {
            BatchCommitFileRequest::Create { path, content } => BatchEdit::Create { path, content },
            BatchCommitFileRequest::Update { path, content, sha } => BatchEdit::Update {
                path,
                content,
                expected_blob_sha: sha,
            },
            BatchCommitFileRequest::Delete { path, sha } => BatchEdit::Delete {
                path,
                expected_blob_sha: sha,
            },
            BatchCommitFileRequest::Move { from, to, sha } => BatchEdit::Move {
                from,
                to,
                expected_blob_sha: sha,
            },
        }
    }
}

/// Request body for a batch commit.
///
/// `branch` and `expected_sha` are both required: a batch is a plan replayed
/// against a branch the caller has already read, and guessing either would let
/// it land somewhere nobody asked for.
#[derive(Deserialize, utoipa::ToSchema)]
pub struct BatchCommitRequest {
    /// Branch to commit on.
    pub branch: String,
    /// The commit the branch must be at; a branch that moved is a 409
    /// `REF_ADVANCED`, and a branch that does not exist is not created.
    pub expected_sha: String,
    /// Commit message.
    pub message: String,
    /// The file changes, applied as one commit.
    pub files: Vec<BatchCommitFileRequest>,
    /// Check everything and report what would happen without writing. The
    /// `X-Dry-Run: 1` header means the same.
    #[serde(default)]
    pub dry_run: Option<bool>,
}

/// One file change as it was applied (or would be).
#[derive(Serialize, utoipa::ToSchema)]
pub struct BatchCommitFileResult {
    /// `create` | `update` | `delete` | `move`
    pub operation: String,
    /// The source path of a move; absent otherwise.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    /// The path written to.
    pub path: String,
    /// The blob the path held before; `null` when it was free or is deleted.
    pub blob_before: Option<String>,
}

/// Answer to a batch commit.
#[derive(Serialize, utoipa::ToSchema)]
pub struct BatchCommitResponse {
    /// `true` when nothing was written.
    pub dry_run: bool,
    pub branch: String,
    /// The branch tip before the commit.
    pub tip_before: String,
    /// The one commit carrying every edit; `null` for a dry run.
    pub commit_sha: Option<String>,
    pub files: Vec<BatchCommitFileResult>,
}

/// `POST /api/v1/repos/:owner/:name/commits`
///
/// Commit many file edits at once. Either every edit lands in one commit or the
/// branch does not move.
#[utoipa::path(
    post,
    path = "/repos/{owner}/{name}/commits",
    tag = "Repository Content",
    params(
        ("owner" = String, Path, description = "owner"),
        ("name" = String, Path, description = "name"),
    ),
    request_body = BatchCommitRequest,
    responses(
        (status = 200, description = "Success", body = BatchCommitResponse),
        (status = 400, description = "Bad request", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
        (status = 403, description = "Forbidden", body = serde_json::Value),
        (status = 404, description = "Not found", body = serde_json::Value),
        (status = 409, description = "Conflict", body = serde_json::Value),
    ),
)]
pub async fn create_commit(
    State(state): State<AppState>,
    Path((owner, repo)): Path<(String, String)>,
    headers: HeaderMap,
    Json(req): Json<BatchCommitRequest>,
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

    // A token limited to some branches may only commit to those. The whole batch
    // is on one branch, so one check covers it.
    if let Some(claims) = extract_bearer_claims(&headers, &state.jwt_secret) {
        if let Err(e) = ensure_token_may_write_branch(&state.db, &claims, &req.branch).await {
            return e.into_response();
        }
    }

    let edits: Vec<BatchEdit> = req.files.into_iter().map(|f| f.into_edit()).collect();
    let dry_run = dry_run_requested(&headers, req.dry_run);

    let (commit_name, commit_email) = rg_core::agent::commit_identity(&user);

    match rg_core::repo::service::commit_batch(
        &state.db,
        repo_model.id,
        &owner,
        &repo,
        &req.branch,
        &edits,
        &req.message,
        &commit_name,
        &commit_email,
        &state.repo_root,
        Some(user.id),
        BatchCommitOptions {
            expected_tip: &req.expected_sha,
            dry_run,
        },
    )
    .await
    {
        Ok(report) if report.dry_run => {
            (StatusCode::OK, Json(batch_dry_run_body(&report))).into_response()
        }
        Ok(report) => {
            let commit_sha = report.commit_sha.clone().unwrap_or_default();
            let repo_path = state.repo_root.join(format!("{}/{}.git", owner, repo));
            let hook_repo_path = repo_path.clone();
            let hook_branch = report.branch.clone();
            // The commit already landed; a failure to re-read the tip must not
            // turn that into an error the client would retry.
            let new_sha = run_blocking(move || get_latest_commit_sha(&repo_path, &hook_branch))
                .await
                .unwrap_or_else(|e| {
                    tracing::warn!(error = %e, "could not resolve new commit sha after write");
                    String::new()
                });
            let new_sha = if new_sha.is_empty() {
                commit_sha.clone()
            } else {
                new_sha
            };
            crate::api::repo_content::announce_branch_move(
                &state,
                &owner,
                &repo,
                hook_repo_path,
                &report.branch,
                Some(report.tip_before.clone()),
                &new_sha,
                user.id,
            );
            crate::api::audit::record_write(
                &state.db,
                user.id,
                "repo.batch_commit",
                "repo",
                Some(repo_model.id),
                &format!("{owner}/{repo}"),
                &headers,
                serde_json::json!({
                    "branch": report.branch,
                    "files": report.files.len(),
                    "paths": report.files.iter().map(|f| f.path.clone()).collect::<Vec<_>>(),
                    "commit_sha": new_sha,
                    "before": report.tip_before,
                }),
            )
            .await;

            (
                StatusCode::OK,
                Json(BatchCommitResponse {
                    dry_run: false,
                    branch: report.branch,
                    tip_before: report.tip_before,
                    commit_sha: Some(commit_sha),
                    files: report.files.into_iter().map(Into::into).collect(),
                }),
            )
                .into_response()
        }
        Err(e) => batch_commit_error(e).into_response(),
    }
}

impl From<rg_core::repo::service::BatchCommitFile> for BatchCommitFileResult {
    fn from(file: rg_core::repo::service::BatchCommitFile) -> Self {
        Self {
            operation: file.operation.to_string(),
            from: file.from,
            path: file.path,
            blob_before: file.blob_before,
        }
    }
}

/// The body of a dry-run answer, shaped like [`file_commit_dry_run_body`]: the
/// same `would[]` / `not_evaluated[]` pair, with one `file` entry per edit.
fn batch_dry_run_body(report: &BatchCommitReport) -> serde_json::Value {
    let mut would = vec![serde_json::json!({
        "effect": "ref_update",
        "ref": format!("refs/heads/{}", report.branch),
        "old": report.tip_before,
        // `expected_sha` is required and a missing branch is refused, so a
        // batch never creates one.
        "creates_branch": false,
    })];
    for file in &report.files {
        let mut entry = serde_json::json!({
            "effect": "file",
            "operation": file.operation,
            "path": file.path,
            "blob_before": file.blob_before,
        });
        if let Some(from) = &file.from {
            entry["from"] = serde_json::json!(from);
        }
        would.push(entry);
    }
    serde_json::json!({
        "dry_run": true,
        "would": would,
        "not_evaluated": ["webhook", "ci_trigger", "notification"],
    })
}

/// Map a batch failure onto the REST contract: a moved branch or a batch that
/// contradicts itself is a 409 with a stable `reason`.
fn batch_commit_error(error: BatchCommitError) -> AppError {
    match error {
        BatchCommitError::RefAdvanced { current, expected } => {
            AppError::conflict_because(
                "REF_ADVANCED",
                true,
                "the branch moved; it is no longer at the expected commit",
                serde_json::json!({ "current_sha": current, "expected_sha": expected }),
                "re-read the branch, check the new commits, and retry with expected_sha set to current_sha",
            )
        }
        BatchCommitError::PathConflict { reason } => AppError::conflict_because(
            PATH_CONFLICT,
            false,
            reason,
            serde_json::json!({}),
            "the edits in one request must not touch the same path, and a move's source cannot also be written; express a rename chain as its end state",
        ),
        BatchCommitError::Core(CoreError::Conflict(message)) => AppError::conflict_because(
            PATH_CONFLICT,
            false,
            message,
            serde_json::json!({}),
            "the edits in one request must not touch the same path, and a move's source cannot also be written; express a rename chain as its end state",
        ),
        BatchCommitError::Core(core) => AppError::from(core),
    }
}
