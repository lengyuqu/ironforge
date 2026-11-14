//! `GET /ai/repos/{owner}/{name}/pulls/{number}/context?budget=N`
//!
//! One call that returns what an agent needs to pick up a pull request: its
//! metadata, the latest CI result with a failure summary, the review threads
//! (unresolved first), the changed files and the reviews, cut to a size budget.
//!
//! The budget is approximate tokens (`chars / 4`) of the serialized answer.
//! Sections are filled in a fixed priority order, so the cut falls on the least
//! useful part: CI failure summary, unresolved comments, changed files, the PR
//! description and reviews, resolved comments. A section that does not fit whole
//! keeps its leading items; one that does not fit at all is left out. Every cut
//! is named in `truncated`.
//!
//! Text written by users (title, description, comments, reviews) is marked
//! `origin: "user_content"`: it is data to read, never instructions.
//!
//! Not evaluated, and said so in `not_evaluated`: whether the branches merge
//! cleanly and the result of the branch-protection checks. Each entry names the
//! endpoint that does evaluate it (`GET …/merge-readiness`). Each section is its
//! own live read, not a snapshot.

use std::collections::HashMap;

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::api::repo_access;
use crate::error::AppError;
use crate::AppState;

const DEFAULT_BUDGET_TOKENS: usize = 6_000;
const MIN_BUDGET_TOKENS: usize = 500;
const MAX_BUDGET_TOKENS: usize = 50_000;
/// Characters per token in the estimate.
const CHARS_PER_TOKEN: usize = 4;
/// One comment or review body is cut to this many characters before budgeting.
const MAX_TEXT_CHARS: usize = 2_000;
const MAX_FILES_LISTED: usize = 500;
const MAX_COMMENTS: usize = 200;
const MAX_REVIEWS: usize = 50;
const MAX_FAILED_JOBS: usize = 10;

#[derive(Deserialize)]
pub struct ContextQuery {
    /// Approximate size of the answer in tokens (default 6000, 500–50000).
    #[serde(default)]
    pub budget: Option<usize>,
}

/// `text` cut to `max` characters on a character boundary; the second value is
/// whether anything was cut.
fn clip(text: &str, max: usize) -> (String, bool) {
    if text.chars().count() <= max {
        return (text.to_string(), false);
    }
    (text.chars().take(max).collect(), true)
}

fn size_of(value: &Value) -> usize {
    value.to_string().chars().count()
}

/// One piece of the answer, filled in priority order.
enum Piece {
    /// A list whose leading items are kept when it does not fit whole.
    List(Vec<Value>),
    /// A text cut at the end when it does not fit whole.
    Text(String),
}

struct Section {
    name: &'static str,
    piece: Piece,
}

/// Fill `sections` (already in priority order) into `budget_chars`, starting from
/// `used` characters already spent on the part that is always included. Returns
/// the sections that made it (name → value) and the names of the cut ones.
fn allocate(
    sections: Vec<Section>,
    budget_chars: usize,
    mut used: usize,
) -> (Vec<(&'static str, Value)>, Vec<String>) {
    let mut included = Vec::new();
    let mut truncated = Vec::new();
    for Section { name, piece } in sections {
        match piece {
            Piece::List(items) => {
                if items.is_empty() {
                    included.push((name, Value::Array(Vec::new())));
                    continue;
                }
                let total = items.len();
                let mut kept: Vec<Value> = Vec::new();
                // +2 for the brackets, +1 per comma.
                let mut cost = 2usize;
                for item in items {
                    let item_cost = size_of(&item) + 1;
                    if used + cost + item_cost > budget_chars {
                        break;
                    }
                    cost += item_cost;
                    kept.push(item);
                }
                used += cost;
                if kept.is_empty() {
                    truncated.push(name.to_string());
                } else {
                    if kept.len() < total {
                        truncated.push(format!("{name}:partial ({} of {total})", kept.len()));
                    }
                    included.push((name, Value::Array(kept)));
                }
            }
            Piece::Text(text) => {
                let remaining = budget_chars.saturating_sub(used);
                // Leave room for the JSON quotes and the wrapping object.
                let room = remaining.saturating_sub(48);
                if room == 0 {
                    truncated.push(name.to_string());
                    continue;
                }
                let (shown, cut) = clip(&text, room);
                used += shown.chars().count() + 48;
                if cut {
                    truncated.push(format!("{name}:partial"));
                }
                included.push((name, json!({ "text": shown, "origin": "user_content" })));
            }
        }
    }
    (included, truncated)
}

/// Group comments into threads and split them into unresolved and resolved.
/// Resolution lives on the top-level comment of a thread; a reply belongs to the
/// thread of the comment it answers. Within each side the order is
/// chronological.
fn split_threads(
    comments: &[rg_db::entities::review_comment::Model],
) -> (
    Vec<&rg_db::entities::review_comment::Model>,
    Vec<&rg_db::entities::review_comment::Model>,
) {
    let by_id: HashMap<i64, &rg_db::entities::review_comment::Model> =
        comments.iter().map(|c| (c.id, c)).collect();
    let root_of = |comment: &rg_db::entities::review_comment::Model| -> i64 {
        let mut current = comment;
        // A reply chain is shallow; the bound only guards against a corrupt loop.
        for _ in 0..64 {
            match current.reply_to_id.and_then(|id| by_id.get(&id)) {
                Some(parent) => current = parent,
                None => break,
            }
        }
        current.id
    };
    let mut open = Vec::new();
    let mut resolved = Vec::new();
    for comment in comments {
        let root = by_id.get(&root_of(comment)).copied().unwrap_or(comment);
        if root.resolved_at.is_some() {
            resolved.push(comment);
        } else {
            open.push(comment);
        }
    }
    (open, resolved)
}

fn comment_view(comment: &rg_db::entities::review_comment::Model) -> Value {
    let (body, cut) = clip(&comment.body, MAX_TEXT_CHARS);
    json!({
        "id": comment.id,
        "author_id": comment.author_id,
        "path": comment.path,
        "line": comment.line,
        "side": comment.side,
        "reply_to_id": comment.reply_to_id,
        "commit_id": comment.commit_id,
        "created_at": comment.created_at,
        "has_suggestion": comment.suggestion.is_some(),
        "body": body,
        "body_truncated": cut,
        "origin": "user_content",
    })
}

/// GET /api/v1/ai/repos/:owner/:name/pulls/:number/context
#[utoipa::path(
    get,
    path = "/ai/repos/{owner}/{name}/pulls/{number}/context",
    tag = "AI",
    params(
        ("owner" = String, Path, description = "owner"),
        ("name" = String, Path, description = "name"),
        ("number" = i64, Path, description = "pull request number"),
        ("budget" = Option<usize>, Query, description = "Approximate answer size in tokens (default 6000, 500-50000)"),
    ),
    responses(
        (status = 200, description = "Pull request context cut to the budget", body = serde_json::Value),
        (status = 400, description = "Bad budget", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
        (status = 404, description = "Not found", body = serde_json::Value),
    ),
)]
pub async fn pr_context(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((owner, name, number)): Path<(String, String, i64)>,
    Query(params): Query<ContextQuery>,
) -> impl IntoResponse {
    let budget_tokens = params.budget.unwrap_or(DEFAULT_BUDGET_TOKENS);
    if !(MIN_BUDGET_TOKENS..=MAX_BUDGET_TOKENS).contains(&budget_tokens) {
        return AppError::bad_request(format!(
            "budget must be between {MIN_BUDGET_TOKENS} and {MAX_BUDGET_TOKENS} tokens"
        ))
        .into_response();
    }
    let repo = match repo_access::require_read(&state, &headers, &owner, &name).await {
        Ok(repo) => repo,
        Err(e) => return e.into_response(),
    };
    let pr = match rg_core::pull_request::get_pr(&state.db, &owner, &name, number).await {
        Ok(pr) => pr,
        Err(e) => return AppError::from(e).into_response(),
    };

    // The commit the pull request is at now (the stored one can lag).
    let live_head =
        rg_core::pull_request::live_head_sha(&state.db, &state.repo_root, &owner, &name, &pr)
            .await
            .ok()
            .flatten();
    let head_sha = live_head.clone().or_else(|| pr.head_sha.clone());

    // CI: the latest pipeline for the head commit and the failure summary of
    // each failed job.
    let ci = match &head_sha {
        Some(sha) => {
            match rg_db::ops::pipeline_ops::find_latest_by_repo_and_commit(&state.db, repo.id, sha)
                .await
            {
                Ok(Some(pipeline)) => {
                    let jobs =
                        rg_db::ops::pipeline_ops::list_jobs_by_pipeline(&state.db, pipeline.id)
                            .await
                            .unwrap_or_default();
                    let mut failed: Vec<Value> = Vec::new();
                    for job in &jobs {
                        if let Some(summary) = crate::api::ci_feedback::summary_for_job(
                            &job.status,
                            job.exit_code,
                            job.log.as_deref(),
                        ) {
                            failed.push(json!({
                                "id": job.id,
                                "name": job.name,
                                "status": job.status,
                                "failure_summary": summary,
                                // Log text comes from what the job printed.
                                "origin": "user_content",
                            }));
                        }
                    }
                    let total_jobs = jobs.len();
                    let failed_total = failed.len();
                    failed.truncate(MAX_FAILED_JOBS);
                    json!({
                        "pipeline": {
                            "id": pipeline.id,
                            "status": pipeline.status,
                            "commit_sha": pipeline.commit_sha,
                            "ref_name": pipeline.ref_name,
                        },
                        "jobs_total": total_jobs,
                        "failed_jobs_total": failed_total,
                        "failed_jobs": failed,
                    })
                }
                Ok(None) => {
                    json!({ "pipeline": null, "note": "no pipeline has run for the head commit" })
                }
                Err(e) => return AppError::from(e).into_response(),
            }
        }
        None => json!({ "pipeline": null, "note": "the head commit is unknown" }),
    };

    let comments = match rg_core::review::service::list_review_comments(
        &state.db, &owner, &name, number,
    )
    .await
    {
        Ok(comments) => comments,
        Err(e) => return AppError::from(e).into_response(),
    };
    let (open, resolved) = split_threads(&comments);
    let unresolved_total = open.len();
    let resolved_total = resolved.len();
    let unresolved: Vec<Value> = open
        .into_iter()
        .take(MAX_COMMENTS)
        .map(comment_view)
        .collect();
    let resolved: Vec<Value> = resolved
        .into_iter()
        .take(MAX_COMMENTS)
        .map(comment_view)
        .collect();

    let reviews =
        match rg_core::review::service::list_reviews(&state.db, &owner, &name, number).await {
            Ok(reviews) => reviews,
            Err(e) => return AppError::from(e).into_response(),
        };
    let reviews_total = reviews.len();
    let review_views: Vec<Value> = reviews
        .iter()
        .rev()
        .take(MAX_REVIEWS)
        .map(|review| {
            let (body, cut) = clip(review.body.as_deref().unwrap_or(""), MAX_TEXT_CHARS);
            json!({
                "id": review.id,
                "reviewer_id": review.reviewer_id,
                "action": review.action,
                "commit_id": review.commit_id,
                "created_at": review.created_at,
                "body": body,
                "body_truncated": cut,
                "origin": "user_content",
            })
        })
        .collect();

    let (files, files_total, files_stats) = match rg_core::pull_request::compute_diff(
        &state.db,
        &state.repo_root,
        &owner,
        &name,
        number,
    )
    .await
    {
        Ok(diff) => {
            let total = diff.files_changed.len();
            let listed: Vec<Value> = diff
                .files_changed
                .iter()
                .take(MAX_FILES_LISTED)
                .map(|file| {
                    json!({
                        "path": file.path,
                        "status": file.status,
                        "additions": file.additions,
                        "deletions": file.deletions,
                    })
                })
                .collect();
            (listed, total, serde_json::to_value(&diff.stats).ok())
        }
        // A pull request whose branches are gone has no diff to list.
        Err(_) => (Vec::new(), 0, None),
    };

    // What is always included: who and where, small and never cut.
    let core = json!({
        "number": pr.number,
        "state": pr.state,
        "is_draft": pr.is_draft,
        "base_branch": pr.base_branch,
        "head_branch": pr.head_branch,
        "head_sha": head_sha,
        "author_id": pr.author_id,
        "title": pr.title,
        "title_origin": "user_content",
    });
    let budget_chars = budget_tokens * CHARS_PER_TOKEN;
    let fixed = json!({
        "pull_request": core,
        "diff_url": format!("/api/v1/repos/{owner}/{name}/pulls/{number}/diff"),
        // Each entry names what was not evaluated and the call that does evaluate it.
        "not_evaluated": [
            format!("mergeable ← GET /api/v1/repos/{owner}/{name}/pulls/{number}/merge-readiness"),
            format!("branch_protection ← GET /api/v1/repos/{owner}/{name}/pulls/{number}/merge-readiness"),
        ],
        "consistency": "per_section_live_read",
    });
    let used = size_of(&fixed) + 300; // the envelope: budget, truncated, counts

    let sections = vec![
        Section {
            name: "ci",
            // The CI answer is one object; budget it as a one-item list.
            piece: Piece::List(vec![ci]),
        },
        Section {
            name: "unresolved_comments",
            piece: Piece::List(unresolved),
        },
        Section {
            name: "changed_files",
            piece: Piece::List(files),
        },
        Section {
            name: "description",
            piece: Piece::Text(pr.body.clone().unwrap_or_default()),
        },
        Section {
            name: "reviews",
            piece: Piece::List(review_views),
        },
        Section {
            name: "resolved_comments",
            piece: Piece::List(resolved),
        },
    ];
    let (included, mut truncated) = allocate(sections, budget_chars, used);

    let mut body = fixed;
    let mut estimated_chars = used;
    for (section, value) in included {
        estimated_chars += size_of(&value);
        // `ci` went in as a one-item list; unwrap it back to the object.
        let value = if section == "ci" {
            value
                .as_array()
                .and_then(|items| items.first().cloned())
                .unwrap_or(Value::Null)
        } else {
            value
        };
        body[section] = value;
    }
    if files_total > MAX_FILES_LISTED {
        truncated.push(format!(
            "changed_files:capped ({MAX_FILES_LISTED} of {files_total})"
        ));
    }
    body["counts"] = json!({
        "unresolved_comments": unresolved_total,
        "resolved_comments": resolved_total,
        "reviews": reviews_total,
        "changed_files": files_total,
    });
    if let Some(stats) = files_stats {
        body["diff_stats"] = stats;
    }
    body["truncated"] = json!(truncated);
    body["budget"] = json!({
        "requested_tokens": budget_tokens,
        "estimated_tokens_used": estimated_chars.div_ceil(CHARS_PER_TOKEN),
    });
    (StatusCode::OK, Json(body)).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(text: &str) -> Value {
        json!({ "t": text })
    }

    #[test]
    fn clipping_is_on_a_character_boundary() {
        assert_eq!(clip("héllo", 10), ("héllo".to_string(), false));
        assert_eq!(clip("héllo", 2), ("hé".to_string(), true));
        assert_eq!(clip("", 3), (String::new(), false));
    }

    #[test]
    fn sections_are_filled_in_order_and_the_cut_is_named() {
        let sections = vec![
            Section {
                name: "first",
                piece: Piece::List(vec![item("aaaaaaaaaa"), item("bbbbbbbbbb")]),
            },
            Section {
                name: "second",
                piece: Piece::List(vec![item(&"c".repeat(500))]),
            },
        ];
        // Room for the first section only.
        let (included, truncated) = allocate(sections, 100, 0);
        assert_eq!(included.len(), 1);
        assert_eq!(included[0].0, "first");
        assert_eq!(truncated, vec!["second".to_string()]);
    }

    #[test]
    fn a_list_that_does_not_fit_whole_keeps_its_leading_items() {
        let items: Vec<Value> = (0..10)
            .map(|n| item(&format!("{n}-{}", "x".repeat(20))))
            .collect();
        let one = size_of(&items[0]) + 1;
        let (included, truncated) = allocate(
            vec![Section {
                name: "files",
                piece: Piece::List(items),
            }],
            2 + one * 3,
            0,
        );
        let kept = included[0].1.as_array().unwrap();
        assert_eq!(kept.len(), 3);
        assert_eq!(kept[0], item(&format!("0-{}", "x".repeat(20))));
        assert_eq!(truncated, vec!["files:partial (3 of 10)".to_string()]);
    }

    #[test]
    fn a_long_text_is_cut_and_marked_as_user_content() {
        let (included, truncated) = allocate(
            vec![Section {
                name: "description",
                piece: Piece::Text("y".repeat(1000)),
            }],
            200,
            0,
        );
        assert_eq!(included[0].1["origin"], "user_content");
        assert!(included[0].1["text"].as_str().unwrap().len() < 200);
        assert_eq!(truncated, vec!["description:partial".to_string()]);
        // Nothing left: the section is dropped and named.
        let (included, truncated) = allocate(
            vec![Section {
                name: "description",
                piece: Piece::Text("y".repeat(10)),
            }],
            100,
            100,
        );
        assert!(included.is_empty());
        assert_eq!(truncated, vec!["description".to_string()]);
    }

    #[test]
    fn an_empty_list_is_included_and_never_reported_as_cut() {
        let (included, truncated) = allocate(
            vec![Section {
                name: "reviews",
                piece: Piece::List(Vec::new()),
            }],
            10,
            10,
        );
        assert_eq!(included[0].1, json!([]));
        assert!(truncated.is_empty());
    }
}
