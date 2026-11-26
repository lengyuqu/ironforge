//! `POST /repos/{owner}/{name}/commits/{sha}/cherry-pick` and `…/revert` —
//! apply (or undo) one existing commit on a branch, server side.
//!
//! Like the batch commit route, a pick is a plan replayed against a branch the
//! caller has already read: `branch` and `expected_sha` are both required and a
//! branch that moved is a 409 `REF_ADVANCED`. A change that does not apply
//! cleanly is a 409 `MERGE_CONFLICT` naming the paths; the branch does not move
//! and no object is written (see [`rg_git::pick`]).
//!
//! Permission surface, shared with the other REST commit paths: write access to
//! the repository, the `require_pr` / `require_signed_commits` verdict of the
//! target branch, and the token branch limit. The commit is authored and
//! committed by [`rg_core::agent::commit_identity`], and a success is announced
//! like a push (pull-request heads, CI, webhooks, the event log).

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};

use rg_core::repo::pick::PickParams;
use rg_git::pick::{PickKind, PickOutcome};

use crate::api::auth::{ensure_token_may_write_branch, extract_claims};
use crate::api::repo_content::{dry_run_requested, get_latest_commit_sha, run_blocking};
use crate::error::AppError;
use crate::AppState;

/// Request body for a cherry-pick or a revert.
#[derive(Deserialize, utoipa::ToSchema)]
pub struct PickCommitRequest {
    /// Branch the new commit goes on.
    pub branch: String,
    /// The commit the branch must be at; a branch that moved is a 409
    /// `REF_ADVANCED`, and a branch that does not exist is not created.
    pub expected_sha: String,
    /// For a merge commit: the 1-based number of the parent to treat as the
    /// mainline (git's `-m`). Required for a merge commit, refused for any
    /// other commit.
    #[serde(default)]
    pub mainline: Option<u32>,
    /// Replaces the generated subject and body. The standard trailer
    /// (`This reverts commit …` / `(cherry picked from commit …)`) is still
    /// appended.
    #[serde(default)]
    pub message: Option<String>,
    /// Check everything and report what would happen without writing. The
    /// `X-Dry-Run: 1` header means the same.
    #[serde(default)]
    pub dry_run: Option<bool>,
}

/// Answer to a cherry-pick or revert that was carried out.
#[derive(Serialize, utoipa::ToSchema)]
pub struct PickCommitResponse {
    pub dry_run: bool,
    /// `cherry_pick` | `revert`
    pub operation: String,
    pub branch: String,
    /// The branch tip before the new commit.
    pub tip_before: String,
    /// The new commit.
    pub commit_sha: String,
    /// Full id of the commit that was applied.
    pub source_sha: String,
}

/// `POST /api/v1/repos/:owner/:name/commits/:sha/cherry-pick`
///
/// Apply the change commit `sha` made on top of `branch`, as one new commit.
/// The branch moves only if the change applies cleanly.
#[utoipa::path(
    post,
    path = "/repos/{owner}/{name}/commits/{sha}/cherry-pick",
    tag = "Repository Content",
    params(
        ("owner" = String, Path, description = "owner"),
        ("name" = String, Path, description = "name"),
        ("sha" = String, Path, description = "commit to apply (hex id)"),
    ),
    request_body = PickCommitRequest,
    responses(
        (status = 200, description = "Success", body = PickCommitResponse),
        (status = 400, description = "Bad request", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
        (status = 403, description = "Forbidden", body = serde_json::Value),
        (status = 404, description = "Not found", body = serde_json::Value),
        (status = 409, description = "Conflict", body = serde_json::Value),
    ),
)]
pub async fn cherry_pick_commit(
    State(state): State<AppState>,
    Path((owner, repo, sha)): Path<(String, String, String)>,
    headers: HeaderMap,
    Json(req): Json<PickCommitRequest>,
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
    if let Some(claims) = extract_claims(&headers, &state.jwt_secret) {
        if let Err(e) = ensure_token_may_write_branch(&state.db, &claims, &req.branch).await {
            return e.into_response();
        }
    }
    run_pick(
        &state,
        &headers,
        PickKind::CherryPick,
        (owner, repo, sha),
        repo_model,
        user,
        req,
    )
    .await
}

/// `POST /api/v1/repos/:owner/:name/commits/:sha/revert`
///
/// Undo the change commit `sha` made, as one new commit on top of `branch`.
/// The branch moves only if the reversal applies cleanly.
#[utoipa::path(
    post,
    path = "/repos/{owner}/{name}/commits/{sha}/revert",
    tag = "Repository Content",
    params(
        ("owner" = String, Path, description = "owner"),
        ("name" = String, Path, description = "name"),
        ("sha" = String, Path, description = "commit to undo (hex id)"),
    ),
    request_body = PickCommitRequest,
    responses(
        (status = 200, description = "Success", body = PickCommitResponse),
        (status = 400, description = "Bad request", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
        (status = 403, description = "Forbidden", body = serde_json::Value),
        (status = 404, description = "Not found", body = serde_json::Value),
        (status = 409, description = "Conflict", body = serde_json::Value),
    ),
)]
pub async fn revert_commit(
    State(state): State<AppState>,
    Path((owner, repo, sha)): Path<(String, String, String)>,
    headers: HeaderMap,
    Json(req): Json<PickCommitRequest>,
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
    if let Some(claims) = extract_claims(&headers, &state.jwt_secret) {
        if let Err(e) = ensure_token_may_write_branch(&state.db, &claims, &req.branch).await {
            return e.into_response();
        }
    }
    run_pick(
        &state,
        &headers,
        PickKind::Revert,
        (owner, repo, sha),
        repo_model,
        user,
        req,
    )
    .await
}

/// The part the two routes share, after each has checked write access and the
/// token's branch limit itself.
async fn run_pick(
    state: &AppState,
    headers: &HeaderMap,
    kind: PickKind,
    (owner, repo, sha): (String, String, String),
    repo_model: rg_db::entities::repository::Model,
    user: rg_db::entities::user::Model,
    req: PickCommitRequest,
) -> axum::response::Response {
    let dry_run = dry_run_requested(headers, req.dry_run);
    let (identity_name, identity_email) = rg_core::agent::commit_identity(&user);

    let outcome = rg_core::repo::pick::pick_commit(
        &state.db,
        repo_model.id,
        &owner,
        &repo,
        &state.repo_root,
        Some(user.id),
        PickParams {
            kind,
            source: &sha,
            branch: &req.branch,
            expected_sha: &req.expected_sha,
            mainline: req.mainline,
            message: req.message.as_deref(),
            dry_run,
            identity_name: &identity_name,
            identity_email: &identity_email,
        },
    )
    .await;

    let operation = kind.as_str();
    match outcome {
        Err(e) => AppError::from(e).into_response(),
        Ok(PickOutcome::Planned {
            tree,
            tip_before,
            source,
        }) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "dry_run": true,
                "operation": operation,
                "would": [
                    {
                        "effect": "ref_update",
                        "ref": format!("refs/heads/{}", req.branch),
                        "old": tip_before,
                        "creates_branch": false,
                    },
                    {
                        "effect": "commit",
                        "operation": operation,
                        "source_sha": source,
                        // Computed in memory: it names no stored object.
                        "tree": tree,
                    },
                ],
                "not_evaluated": ["webhook", "ci_trigger", "notification"],
            })),
        )
            .into_response(),
        Ok(PickOutcome::Committed {
            commit,
            tip_before,
            source,
            ..
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
                state,
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
                if kind == PickKind::CherryPick {
                    "repo.cherry_pick"
                } else {
                    "repo.revert"
                },
                "repo",
                Some(repo_model.id),
                &format!("{owner}/{repo}"),
                headers,
                serde_json::json!({
                    "branch": req.branch,
                    "source_sha": source,
                    "commit_sha": new_sha,
                    "before": tip_before,
                    "mainline": req.mainline,
                }),
            )
            .await;
            (
                StatusCode::OK,
                Json(PickCommitResponse {
                    dry_run: false,
                    operation: operation.to_string(),
                    branch: req.branch,
                    tip_before,
                    commit_sha: commit,
                    source_sha: source,
                }),
            )
                .into_response()
        }
        Ok(PickOutcome::Conflict { paths, tip, source }) => AppError::conflict_because(
            "MERGE_CONFLICT",
            false,
            format!(
                "the change does not apply cleanly to '{}': {} conflicting path(s)",
                req.branch,
                paths.len()
            ),
            serde_json::json!({
                "operation": operation,
                "source_sha": source,
                "tip_sha": tip,
                "paths": paths,
            }),
            "the branch was not changed; resolve the paths on a branch of your own (or pick a different commit) and retry",
        )
        .into_response(),
        Ok(PickOutcome::Empty { tip }) => AppError::conflict_because(
            "NOTHING_TO_APPLY",
            false,
            "applying this commit would not change the branch: its change is already there (or already gone)",
            serde_json::json!({ "operation": operation, "tip_sha": tip }),
            "nothing to commit; the branch already has this change",
        )
        .into_response(),
        Ok(PickOutcome::TipAdvanced { actual }) => AppError::conflict_because(
            "REF_ADVANCED",
            true,
            "the branch moved; it is no longer at the expected commit",
            serde_json::json!({ "current_sha": actual, "expected_sha": req.expected_sha }),
            "re-read the branch, check the new commits, and retry with expected_sha set to current_sha",
        )
        .into_response(),
        Ok(PickOutcome::SourceNotFound) => {
            AppError::not_found("commit not found in this repository").into_response()
        }
        Ok(PickOutcome::MergeNeedsMainline { parents }) => AppError::coded(
            StatusCode::BAD_REQUEST,
            "MAINLINE_REQUIRED",
            "this is a merge commit; say which parent is the mainline",
        )
        .with_details(serde_json::json!({ "parents": parents }))
        .into_response(),
        Ok(PickOutcome::MainlineInvalid { parents, mainline }) => AppError::coded(
            StatusCode::BAD_REQUEST,
            "MAINLINE_INVALID",
            if parents < 2 {
                "mainline only applies to a merge commit".to_string()
            } else {
                format!("mainline must be between 1 and {parents}")
            },
        )
        .with_details(serde_json::json!({ "parents": parents, "mainline": mainline }))
        .into_response(),
    }
}
