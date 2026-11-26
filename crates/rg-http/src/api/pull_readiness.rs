//! Merge readiness and branch update for pull requests.
//!
//! * `GET  /repos/{owner}/{name}/pulls/{number}/merge-readiness` — what stands
//!   between the pull request and a merge: conflicts (files and blocks), every
//!   branch-protection gate with a machine code, the required checks still to
//!   pass. Read-only, repository read access. The rules are the merge's own
//!   (`rg_core::branch_protection::merge_gates`), so the preview cannot drift
//!   from the merge.
//! * `POST /repos/{owner}/{name}/pulls/{number}/update-branch` — merge the base
//!   branch into the head branch, pinned to `expected_head_sha`.
//!
//! # Permission of update-branch
//!
//! It writes the head branch, so it is held to the rules of a push to that
//! branch: repository write access, the token's own branch limit
//! (`ensure_token_may_write_branch`), and the branch's protection rule (a
//! protected head branch that requires a pull request or signed commits
//! refuses). A token limited to some branches is refused by the PAT middleware
//! before it gets here (the route is not declared branch-limit-open), and the
//! handler's own check keeps it refused if that declaration is ever relaxed.
//! A pull request from a fork is refused with `FORK_PR_UNSUPPORTED`.

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;
use serde_json::json;

use rg_core::pull_request::update_branch::{UpdateBranchError, UpdateBranchOutcome};
use rg_core::pull_request::MergeStrategy;

use super::repo_access::require_read;
use crate::api::auth::{ensure_token_may_write_branch, extract_claims};
use crate::api::repo_content::{
    announce_branch_move, dry_run_requested, resolve_and_check_write_access,
};
use crate::error::AppError;
use crate::AppState;

/// Query of the readiness read.
#[derive(Deserialize)]
pub struct ReadinessQuery {
    /// `merge` (default), `squash` or `rebase`.
    #[serde(default)]
    pub strategy: Option<String>,
    /// Start the conflict list after this path: the value a cut answer's
    /// `continue_with` carries.
    #[serde(default)]
    pub conflicts_after: Option<String>,
}

/// `GET /api/v1/repos/:owner/:name/pulls/:number/merge-readiness`
#[utoipa::path(
    get,
    path = "/repos/{owner}/{name}/pulls/{number}/merge-readiness",
    tag = "Pull Requests",
    params(
        ("owner" = String, Path, description = "owner"),
        ("name" = String, Path, description = "name"),
        ("number" = i64, Path, description = "pull request number"),
        ("strategy" = Option<String>, Query, description = "merge (default), squash or rebase"),
        ("conflicts_after" = Option<String>, Query, description = "continue the conflict list after this path"),
    ),
    responses(
        (status = 200, description = "Merge readiness: conflicts, protection gates, required checks", body = serde_json::Value),
        (status = 400, description = "Unknown strategy", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
        (status = 403, description = "Forbidden", body = serde_json::Value),
        (status = 404, description = "Not found", body = serde_json::Value),
    ),
)]
pub async fn get_merge_readiness(
    State(state): State<AppState>,
    Path((owner, repo, number)): Path<(String, String, i64)>,
    headers: HeaderMap,
    Query(query): Query<ReadinessQuery>,
) -> impl IntoResponse {
    if let Err(e) = require_read(&state, &headers, &owner, &repo).await {
        return e.into_response();
    }
    let strategy = match MergeStrategy::parse(query.strategy.as_deref().unwrap_or("merge")) {
        Ok(strategy) => strategy,
        Err(error) => return AppError::from(error).into_response(),
    };
    let pr = match rg_core::pull_request::get_pr(&state.db, &owner, &repo, number).await {
        Ok(pr) => pr,
        Err(e) => return AppError::from(e).into_response(),
    };
    match rg_core::pull_request::readiness::merge_readiness(
        &state.db,
        &state.repo_root,
        &owner,
        &repo,
        &pr,
        strategy,
        query.conflicts_after,
    )
    .await
    {
        Ok(body) => (StatusCode::OK, Json(body)).into_response(),
        Err(e) => AppError::from(e).into_response(),
    }
}

/// Body of `POST …/update-branch`.
#[derive(Deserialize, utoipa::ToSchema)]
pub struct UpdateBranchRequest {
    /// The head commit the caller looked at. The update is built from exactly
    /// this commit, or it does not happen (409 `REF_ADVANCED`).
    pub expected_head_sha: String,
    /// Report what would happen and write nothing (also the `X-Dry-Run: 1`
    /// header).
    #[serde(default)]
    pub dry_run: Option<bool>,
}

fn ref_advanced(current: Option<String>, expected: String) -> AppError {
    AppError::conflict_because(
        "REF_ADVANCED",
        true,
        "the pull request's head branch moved; it is no longer at the expected commit",
        json!({ "current_sha": current, "expected_sha": expected }),
        "re-read the pull request, review the new commits, and retry with expected_head_sha set to current_sha",
    )
}

/// `POST /api/v1/repos/:owner/:name/pulls/:number/update-branch`
///
/// Merge the base branch into the head branch (one merge commit), pinned to the
/// head commit the caller looked at.
#[utoipa::path(
    post,
    path = "/repos/{owner}/{name}/pulls/{number}/update-branch",
    tag = "Pull Requests",
    params(
        ("owner" = String, Path, description = "owner"),
        ("name" = String, Path, description = "name"),
        ("number" = i64, Path, description = "pull request number"),
    ),
    request_body = UpdateBranchRequest,
    responses(
        (status = 200, description = "updated, already_up_to_date, or (dry run) what would happen", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
        (status = 403, description = "Forbidden", body = serde_json::Value),
        (status = 404, description = "Not found", body = serde_json::Value),
        (status = 409, description = "REF_ADVANCED, MERGE_CONFLICTS, FORK_PR_UNSUPPORTED, NO_MERGE_BASE, or the pull request is not open", body = serde_json::Value),
    ),
)]
pub async fn update_branch(
    State(state): State<AppState>,
    Path((owner, repo, number)): Path<(String, String, i64)>,
    headers: HeaderMap,
    Json(req): Json<UpdateBranchRequest>,
) -> impl IntoResponse {
    let (repo_model, user) =
        match resolve_and_check_write_access(&state, &headers, &owner, &repo).await {
            Ok(pair) => pair,
            Err(e) => return e.into_response(),
        };
    let pr = match rg_core::pull_request::get_pr(&state.db, &owner, &repo, number).await {
        Ok(pr) => pr,
        Err(e) => return AppError::from(e).into_response(),
    };

    // A token limited to some branches may only write those. The branch written
    // is the pull request's head branch.
    if let Some(claims) = extract_claims(&headers, &state.jwt_secret) {
        if let Err(e) = ensure_token_may_write_branch(&state.db, &claims, &pr.head_branch).await {
            return e.into_response();
        }
    }

    let dry_run = dry_run_requested(&headers, req.dry_run);
    let identity = rg_core::agent::commit_identity(&user);
    let outcome = rg_core::pull_request::update_branch::update_branch(
        &state.db,
        &state.repo_root,
        &owner,
        &repo,
        &pr,
        user.id,
        &req.expected_head_sha,
        dry_run,
        identity,
    )
    .await;

    match outcome {
        Err(UpdateBranchError::HeadMoved { current, expected }) => {
            ref_advanced(current, expected).into_response()
        }
        Err(UpdateBranchError::ForkUnsupported) => AppError::conflict_because(
            "FORK_PR_UNSUPPORTED",
            false,
            "the head branch of this pull request is in a fork; it cannot be updated from here",
            json!({ "head_repo_id": pr.head_repo_id }),
            "merge the base branch into the fork's branch there, or ask the author to update it",
        )
        .into_response(),
        Err(UpdateBranchError::NoMergeBase) => AppError::conflict_because(
            "NO_MERGE_BASE",
            false,
            "the head and base branches share no history",
            json!({}),
            "there is nothing to merge the base into; rebuild the head branch on the base",
        )
        .into_response(),
        Err(UpdateBranchError::Core(e)) => AppError::from(e).into_response(),
        Ok(UpdateBranchOutcome::Conflicts {
            head_sha,
            base_sha,
            report,
        }) => {
            let merge = rg_core::pull_request::readiness::conflicts_json(&report, None);
            AppError::conflict_because(
                "MERGE_CONFLICTS",
                false,
                format!(
                    "the base branch does not merge cleanly into the head branch: {} file(s) conflict",
                    report.total_files
                ),
                json!({
                    "outcome": "conflicts",
                    "head_sha": head_sha,
                    "base_sha": base_sha,
                    "merge": merge,
                    "head_branch": pr.head_branch,
                    "readiness_url": format!(
                        "/api/v1/repos/{owner}/{repo}/pulls/{number}/merge-readiness"
                    ),
                }),
                "resolve the listed conflicts on the head branch (the head and the base differ on the same lines), then push",
            )
            .into_response()
        }
        Ok(UpdateBranchOutcome::AlreadyUpToDate { head_sha, base_sha }) => (
            StatusCode::OK,
            Json(json!({
                "outcome": "already_up_to_date",
                "dry_run": dry_run,
                "head_branch": pr.head_branch,
                "head_sha_before": head_sha,
                "head_sha_after": head_sha,
                "base_sha": base_sha,
                "created_commits": 0,
                "merge_commit_sha": null,
            })),
        )
            .into_response(),
        Ok(UpdateBranchOutcome::WouldUpdate { head_sha, base_sha }) => (
            StatusCode::OK,
            Json(json!({
                "dry_run": true,
                "outcome": "would_update",
                "would": [{
                    "effect": "merge_commit",
                    "ref": format!("refs/heads/{}", pr.head_branch),
                    "old": head_sha,
                    "merges": base_sha,
                    "parents": [head_sha, base_sha],
                }],
                "not_evaluated": ["webhook", "ci_trigger", "notification"],
            })),
        )
            .into_response(),
        Ok(UpdateBranchOutcome::Updated {
            head_sha_before,
            head_sha_after,
            base_sha,
        }) => {
            // The branch moved; give that the effects of a push (pull-request
            // head ref, CI, webhooks, the event log).
            let repo_path = state.repo_root.join(format!("{owner}/{repo}.git"));
            announce_branch_move(
                &state,
                &owner,
                &repo,
                repo_path,
                &pr.head_branch,
                Some(head_sha_before.clone()),
                &head_sha_after,
                user.id,
            );
            crate::api::audit::record_write(
                &state.db,
                user.id,
                "pull_request.update_branch",
                "pull_request",
                Some(pr.id),
                &format!("{owner}/{repo}#{number}"),
                &headers,
                json!({
                    "number": number,
                    "repo_id": repo_model.id,
                    "head_branch": pr.head_branch,
                    "base_branch": pr.base_branch,
                    "base_sha": base_sha,
                    "before": head_sha_before,
                    "after": head_sha_after,
                }),
            )
            .await;
            (
                StatusCode::OK,
                Json(json!({
                    "outcome": "updated",
                    "dry_run": false,
                    "head_branch": pr.head_branch,
                    "head_sha_before": head_sha_before,
                    "head_sha_after": head_sha_after,
                    "base_sha": base_sha,
                    "created_commits": 1,
                    "merge_commit_sha": head_sha_after,
                    // Fired in the background after this answer.
                    "not_evaluated": ["webhook", "ci_trigger", "notification", "merge_readiness"],
                })),
            )
                .into_response()
        }
    }
}
