//! `GET /repos/{owner}/{name}/pulls/{number}/export`
//!
//! One self-contained JSON document describing a pull request as it stands: the
//! commits it is anchored at, its metadata, the review history and the CI result
//! for its head commit. It is meant for building a replay or evaluation set from
//! past pull requests (a base commit plus the issue text, a patch to produce, the
//! repository's own CI to judge it).
//!
//! What it does and does not promise:
//! - Anchors: `refs/pull/<n>/head` is written for pull requests whose head branch
//!   is in the same repository, and it survives deleting the branch. A fork pull
//!   request has no such ref in this repository (`head_ref` is `null`); its head
//!   commit is only reachable while the fork keeps it.
//! - The base commit when the pull request was opened is not stored. For a merged
//!   pull request `base_sha_at_merge` (first parent of the merge commit) is the
//!   best available; for an open one `base_tip_sha` is the base branch tip now.
//! - Review and CI data are live reads, not a snapshot. Log text is not included,
//!   only each job's status and exit code.
//! - Text written by users is marked `origin: "user_content"`.

use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::response::IntoResponse;
use axum::Json;
use serde_json::{json, Value};

use crate::api::repo_access;
use crate::error::AppError;
use crate::AppState;

/// One text field is cut to this many characters.
const MAX_TEXT_CHARS: usize = 20_000;
const MAX_COMMENTS: usize = 1_000;
const MAX_REVIEWS: usize = 200;
const MAX_JOBS: usize = 200;

fn clip(text: &str) -> (String, bool) {
    if text.chars().count() <= MAX_TEXT_CHARS {
        return (text.to_string(), false);
    }
    (text.chars().take(MAX_TEXT_CHARS).collect(), true)
}

/// GET /api/v1/repos/:owner/:name/pulls/:number/export
#[utoipa::path(
    get,
    path = "/repos/{owner}/{name}/pulls/{number}/export",
    tag = "Pull Requests",
    params(
        ("owner" = String, Path, description = "owner"),
        ("name" = String, Path, description = "name"),
        ("number" = i64, Path, description = "pull request number"),
    ),
    responses(
        (status = 200, description = "Pull request export", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
        (status = 404, description = "Not found", body = serde_json::Value),
    ),
)]
pub async fn export_pr(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((owner, name, number)): Path<(String, String, i64)>,
) -> impl IntoResponse {
    let repo = match repo_access::require_read(&state, &headers, &owner, &name).await {
        Ok(repo) => repo,
        Err(e) => return e.into_response(),
    };
    let pr = match rg_core::pull_request::get_pr(&state.db, &owner, &name, number).await {
        Ok(pr) => pr,
        Err(e) => return AppError::from(e).into_response(),
    };
    let repo_path = state.repo_root.join(format!("{owner}/{name}.git"));

    // Anchors, read from this repository's git.
    let head_ref_name = format!("refs/pull/{}/head", pr.number);
    let head_ref_sha = rg_git::ops::try_rev_parse(&repo_path, &head_ref_name)
        .ok()
        .flatten();
    let live_head =
        rg_core::pull_request::live_head_sha(&state.db, &state.repo_root, &owner, &name, &pr)
            .await
            .ok()
            .flatten();
    let base_tip_sha =
        rg_git::ops::try_rev_parse(&repo_path, &format!("refs/heads/{}", pr.base_branch))
            .ok()
            .flatten();
    let base_sha_at_merge = pr.merge_commit_sha.as_deref().and_then(|merge| {
        rg_git::ops::try_rev_parse(&repo_path, &format!("{merge}^1"))
            .ok()
            .flatten()
    });

    let (body, body_cut) = clip(pr.body.as_deref().unwrap_or(""));
    let (title, _) = clip(&pr.title);
    let pull_request = json!({
        "number": pr.number,
        "state": pr.state,
        "is_draft": pr.is_draft,
        "author_id": pr.author_id,
        "base_branch": pr.base_branch,
        "head_branch": pr.head_branch,
        "head_repo_id": pr.head_repo_id,
        "head_sha": pr.head_sha,
        "merge_commit_sha": pr.merge_commit_sha,
        "merge_strategy": pr.merge_strategy,
        "created_at": pr.created_at,
        "updated_at": pr.updated_at,
        "closed_at": pr.closed_at,
        "merged_at": pr.merged_at,
        "title": title,
        "body": body,
        "body_truncated": body_cut,
        "origin": "user_content",
    });
    let anchors = json!({
        "head_ref": if head_ref_sha.is_some() { Value::String(head_ref_name) } else { Value::Null },
        "head_ref_sha": head_ref_sha,
        "head_branch_sha": live_head,
        "base_tip_sha": base_tip_sha,
        "base_sha_at_merge": base_sha_at_merge,
        "not_stored": ["base_sha_when_opened"],
    });

    let reviews =
        match rg_core::review::service::list_reviews(&state.db, &owner, &name, number).await {
            Ok(reviews) => reviews,
            Err(e) => return AppError::from(e).into_response(),
        };
    let reviews_total = reviews.len();
    let review_views: Vec<Value> = reviews
        .iter()
        .take(MAX_REVIEWS)
        .map(|review| {
            let (text, cut) = clip(review.body.as_deref().unwrap_or(""));
            json!({
                "id": review.id,
                "reviewer_id": review.reviewer_id,
                "action": review.action,
                "commit_id": review.commit_id,
                "created_at": review.created_at,
                "body": text,
                "body_truncated": cut,
                "origin": "user_content",
            })
        })
        .collect();

    let comments = match rg_core::review::service::list_review_comments(
        &state.db, &owner, &name, number,
    )
    .await
    {
        Ok(comments) => comments,
        Err(e) => return AppError::from(e).into_response(),
    };
    let comments_total = comments.len();
    let comment_views: Vec<Value> = comments
        .iter()
        .take(MAX_COMMENTS)
        .map(|comment| {
            let (text, cut) = clip(&comment.body);
            json!({
                "id": comment.id,
                "author_id": comment.author_id,
                "path": comment.path,
                "line": comment.line,
                "side": comment.side,
                "reply_to_id": comment.reply_to_id,
                "commit_id": comment.commit_id,
                "resolved": comment.resolved_at.is_some(),
                "created_at": comment.created_at,
                "has_suggestion": comment.suggestion.is_some(),
                "body": text,
                "body_truncated": cut,
                "origin": "user_content",
            })
        })
        .collect();

    // CI for the commit the pull request ended at: the live head, else the
    // stored one. Status and exit codes only; no log text.
    let ci_commit = live_head.clone().or_else(|| pr.head_sha.clone());
    let ci = match &ci_commit {
        Some(sha) => {
            match rg_db::ops::pipeline_ops::find_latest_by_repo_and_commit(&state.db, repo.id, sha)
                .await
            {
                Ok(Some(pipeline)) => {
                    let jobs =
                        rg_db::ops::pipeline_ops::list_jobs_by_pipeline(&state.db, pipeline.id)
                            .await
                            .unwrap_or_default();
                    let jobs_total = jobs.len();
                    let job_views: Vec<Value> = jobs
                        .iter()
                        .take(MAX_JOBS)
                        .map(|job| {
                            json!({
                                "id": job.id,
                                "name": job.name,
                                "status": job.status,
                                "exit_code": job.exit_code,
                            })
                        })
                        .collect();
                    json!({
                        "commit_sha": sha,
                        "pipeline": {
                            "id": pipeline.id,
                            "status": pipeline.status,
                            "ref_name": pipeline.ref_name,
                        },
                        "jobs_total": jobs_total,
                        "jobs": job_views,
                    })
                }
                Ok(None) => json!({ "commit_sha": sha, "pipeline": null }),
                Err(e) => return AppError::from(e).into_response(),
            }
        }
        None => json!({ "commit_sha": null, "pipeline": null }),
    };

    Json(json!({
        "format": "ironforge.pull_request_export",
        "format_version": 1,
        "repository": format!("{owner}/{name}"),
        "pull_request": pull_request,
        "anchors": anchors,
        "reviews": review_views,
        "review_comments": comment_views,
        "ci": ci,
        "counts": {
            "reviews": reviews_total,
            "review_comments": comments_total,
        },
        "truncated": {
            "reviews": reviews_total > MAX_REVIEWS,
            "review_comments": comments_total > MAX_COMMENTS,
        },
        "consistency": "per_section_live_read",
    }))
    .into_response()
}
