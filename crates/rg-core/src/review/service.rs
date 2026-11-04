//! Code review service — submit reviews, add inline comments, approve / request changes.

use chrono::Utc;
use sea_orm::{ActiveModelTrait, DatabaseConnection, EntityTrait, Set, TransactionTrait};
use std::collections::{BTreeMap, HashSet};

use rg_db::entities::pr_review::{self, Model as PrReview};
use rg_db::entities::pull_request;
use rg_db::entities::review_comment::{self, Model as ReviewComment};
use rg_db::ops::{pr_review_ops, pull_request_ops, review_comment_ops};

use crate::error::{CoreError, CoreResult};

// ── Review actions ────────────────────────────────────────────────────

/// Review action types.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewAction {
    /// Submit a comment without explicit approval/rejection
    Comment,
    /// Approve the PR
    Approve,
    /// Request changes before merging
    RequestChanges,
    /// Dismiss a previous review
    Dismiss,
}

impl ReviewAction {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Comment => "comment",
            Self::Approve => "approve",
            Self::RequestChanges => "request_changes",
            Self::Dismiss => "dismiss",
        }
    }

    pub fn parse_action(s: &str) -> CoreResult<Self> {
        match s {
            "comment" => Ok(Self::Comment),
            "approve" => Ok(Self::Approve),
            "request_changes" => Ok(Self::RequestChanges),
            "dismiss" => Ok(Self::Dismiss),
            _ => Err(CoreError::invalid_input(format!(
                "invalid review action: {}",
                s
            ))),
        }
    }
}

// ── Submit a review ───────────────────────────────────────────────────

/// Submit a review on a pull request.
pub async fn submit_review(
    db: &DatabaseConnection,
    repo_id: i64,
    pr_number: i64,
    reviewer_id: i64,
    action: ReviewAction,
    body: Option<String>,
    commit_id: Option<String>,
) -> CoreResult<PrReview> {
    // Validate PR exists
    let pr = pull_request_ops::find_by_repo_and_number(db, repo_id, pr_number)
        .await?
        .ok_or_else(|| CoreError::not_found("pull request not found"))?;

    if pr.state != "open" {
        return Err(CoreError::conflict(format!(
            "cannot review a PR that is not open (current: {})",
            pr.state
        )));
    }

    // For dismiss, body is typically about why the review is dismissed
    let model = pr_review::ActiveModel {
        id: sea_orm::NotSet,
        pr_id: Set(pr.id),
        repo_id: Set(repo_id),
        reviewer_id: Set(reviewer_id),
        action: Set(action.as_str().to_string()),
        body: Set(body),
        commit_id: Set(commit_id.or_else(|| pr.head_sha.clone())),
        created_at: Set(Utc::now()),
    };

    let review = pr_review_ops::create(db, model).await?;
    rg_db::ops::pr_event_ops::record(
        db,
        repo_id,
        pr.id,
        Some(reviewer_id),
        &format!("review_{}", review.action),
        review.body.clone(),
        serde_json::json!({"review_id": review.id, "commit_id": review.commit_id}),
    )
    .await?;
    Ok(review)
}

/// List all reviews for a PR.
pub async fn list_reviews(
    db: &DatabaseConnection,
    owner: &str,
    repo_name: &str,
    pr_number: i64,
) -> CoreResult<Vec<PrReview>> {
    let repo = resolve_repo(db, owner, repo_name).await?;
    let pr = pull_request_ops::find_by_repo_and_number(db, repo.id, pr_number)
        .await?
        .ok_or_else(|| CoreError::not_found("pull request not found"))?;

    Ok(pr_review_ops::list_by_pr(db, pr.id).await?)
}

/// Get a single review by ID.
pub async fn get_review(db: &DatabaseConnection, review_id: i64) -> CoreResult<PrReview> {
    pr_review_ops::find_by_id(db, review_id)
        .await?
        .ok_or_else(|| CoreError::not_found("review not found"))
}

/// Dismiss a review.
pub async fn dismiss_review(
    db: &DatabaseConnection,
    review_id: i64,
    dismissor_id: i64,
    message: String,
) -> CoreResult<PrReview> {
    let review = pr_review_ops::find_by_id(db, review_id)
        .await?
        .ok_or_else(|| CoreError::not_found("review not found"))?;

    // Create a dismiss review entry
    let model = pr_review::ActiveModel {
        id: sea_orm::NotSet,
        pr_id: Set(review.pr_id),
        repo_id: Set(review.repo_id),
        reviewer_id: Set(dismissor_id),
        action: Set("dismiss".to_string()),
        body: Set(Some(message)),
        commit_id: Set(review.commit_id.clone()),
        created_at: Set(Utc::now()),
    };

    let dismissal = pr_review_ops::create(db, model).await?;
    rg_db::ops::pr_event_ops::record(
        db,
        dismissal.repo_id,
        dismissal.pr_id,
        Some(dismissor_id),
        "review_dismiss",
        dismissal.body.clone(),
        serde_json::json!({"review_id": review_id, "commit_id": dismissal.commit_id}),
    )
    .await?;
    Ok(dismissal)
}

// ── Inline Review Comments ────────────────────────────────────────────

/// Create an inline review comment on a specific diff line.
#[allow(clippy::too_many_arguments)]
pub async fn create_review_comment(
    db: &DatabaseConnection,
    repo_id: i64,
    pr_number: i64,
    review_id: i64,
    author_id: i64,
    path: String,
    line: Option<i64>,
    start_line: Option<i64>,
    side: Option<String>,
    start_side: Option<String>,
    body: String,
    suggestion: Option<String>,
    commit_id: Option<String>,
    reply_to_id: Option<i64>,
) -> CoreResult<ReviewComment> {
    // Validate PR
    let pr = pull_request_ops::find_by_repo_and_number(db, repo_id, pr_number)
        .await?
        .ok_or_else(|| CoreError::not_found("pull request not found"))?;

    // Validate review exists
    let review = pr_review_ops::find_by_id(db, review_id)
        .await?
        .ok_or_else(|| CoreError::not_found("review not found"))?;
    if review.repo_id != repo_id || review.pr_id != pr.id {
        return Err(CoreError::not_found("review not found"));
    }

    // Validate reply_to if specified. It is a reference carried in the request
    // body (not the addressed resource), so a bad one is invalid input.
    if let Some(rtid) = reply_to_id {
        review_comment_ops::find_by_id(db, rtid)
            .await?
            .filter(|parent| parent.pr_id == pr.id)
            .ok_or_else(|| CoreError::invalid_input("parent comment not found on this PR"))?;
    }

    if body.trim().is_empty() {
        return Err(CoreError::invalid_input("comment body cannot be empty"));
    }
    let suggestion = suggestion.map(|value| value.replace("\r\n", "\n"));
    if suggestion.is_some() {
        if reply_to_id.is_some() || line.is_none() || side.as_deref() != Some("RIGHT") {
            return Err(CoreError::invalid_input(
                "suggestions require a top-level RIGHT-side line comment",
            ));
        }
        let end = line.unwrap();
        let start = start_line.unwrap_or(end);
        if start < 1 || start > end || start_side.as_deref().unwrap_or("RIGHT") != "RIGHT" {
            return Err(CoreError::invalid_input(
                "suggestion range must be an ordered RIGHT-side range",
            ));
        }
    }

    let model = review_comment::ActiveModel {
        id: sea_orm::NotSet,
        review_id: Set(review_id),
        pr_id: Set(pr.id),
        author_id: Set(author_id),
        path: Set(path),
        position: Set(None), // Deprecated, use line instead
        line: Set(line),
        start_line: Set(start_line),
        side: Set(side),
        start_side: Set(start_side),
        body: Set(body),
        suggestion: Set(suggestion),
        suggestion_applied_at: Set(None),
        suggestion_applied_by_id: Set(None),
        suggestion_commit_sha: Set(None),
        commit_id: Set(commit_id.or_else(|| review.commit_id.clone())),
        reply_to_id: Set(reply_to_id),
        resolved_at: Set(None),
        resolved_by_id: Set(None),
        created_at: Set(Utc::now()),
        updated_at: Set(Utc::now()),
    };

    let comment = review_comment_ops::create(db, model).await?;
    let event_type = if comment.reply_to_id.is_some() {
        "review_reply"
    } else if comment.suggestion.is_some() {
        "code_suggestion"
    } else {
        "review_comment"
    };
    rg_db::ops::pr_event_ops::record(
        db,
        repo_id,
        pr.id,
        Some(author_id),
        event_type,
        Some(comment.body.clone()),
        serde_json::json!({
            "comment_id": comment.id,
            "path": comment.path,
            "start_line": comment.start_line,
            "line": comment.line,
            "side": comment.side,
            "reply_to_id": comment.reply_to_id
        }),
    )
    .await?;
    Ok(comment)
}

#[derive(Debug, serde::Serialize)]
pub struct AppliedSuggestion {
    pub comment: ReviewComment,
    pub commit_sha: String,
}

#[derive(Debug, serde::Serialize)]
pub struct AppliedSuggestions {
    pub comments: Vec<ReviewComment>,
    pub commit_sha: String,
}

#[allow(clippy::too_many_arguments)]
pub async fn apply_suggestion(
    db: &DatabaseConnection,
    repo_root: &std::path::Path,
    source_repo: &rg_db::entities::repository::Model,
    source_namespace: &str,
    pr: &rg_db::entities::pull_request::Model,
    comment_id: i64,
    actor: &rg_db::entities::user::Model,
) -> CoreResult<AppliedSuggestion> {
    let mut applied = apply_suggestions(
        db,
        repo_root,
        source_repo,
        source_namespace,
        pr,
        &[comment_id],
        actor,
    )
    .await?;
    let comment = applied.comments.remove(0);
    Ok(AppliedSuggestion {
        comment,
        commit_sha: applied.commit_sha,
    })
}

#[derive(Debug)]
struct ValidatedSuggestion {
    comment: ReviewComment,
    start_line: i64,
    end_line: i64,
    replacement: String,
}

#[allow(clippy::too_many_arguments)]
pub async fn apply_suggestions(
    db: &DatabaseConnection,
    repo_root: &std::path::Path,
    source_repo: &rg_db::entities::repository::Model,
    source_namespace: &str,
    pr: &rg_db::entities::pull_request::Model,
    comment_ids: &[i64],
    actor: &rg_db::entities::user::Model,
) -> CoreResult<AppliedSuggestions> {
    if comment_ids.is_empty() || comment_ids.len() > 100 {
        return Err(CoreError::invalid_input(
            "between 1 and 100 suggestions must be selected",
        ));
    }
    let head_sha = pr
        .head_sha
        .as_deref()
        .ok_or_else(|| CoreError::invalid_input("pull request head SHA is missing"))?;
    let mut unique_ids = HashSet::new();
    let mut suggestions_by_path: BTreeMap<String, Vec<ValidatedSuggestion>> = BTreeMap::new();
    for &comment_id in comment_ids {
        if !unique_ids.insert(comment_id) {
            return Err(CoreError::invalid_input(format!(
                "duplicate suggestion comment #{comment_id}"
            )));
        }
        let comment = review_comment_ops::find_by_id(db, comment_id)
            .await?
            .ok_or_else(|| CoreError::not_found("review comment not found"))?;
        if comment.pr_id != pr.id || comment.reply_to_id.is_some() {
            return Err(CoreError::invalid_input(
                "suggestion does not belong to this pull request",
            ));
        }
        let replacement = comment
            .suggestion
            .clone()
            .ok_or_else(|| CoreError::invalid_input("comment does not contain a suggestion"))?;
        if comment.suggestion_applied_at.is_some() {
            return Err(CoreError::conflict("suggestion has already been applied"));
        }
        if comment.commit_id.as_deref() != Some(head_sha) {
            return Err(CoreError::conflict(
                "suggestion is outdated because the pull request head has changed",
            ));
        }
        let end_line = comment
            .line
            .ok_or_else(|| CoreError::invalid_input("suggestion line is missing"))?;
        let start_line = comment.start_line.unwrap_or(end_line);
        if start_line < 1
            || start_line > end_line
            || comment.side.as_deref() != Some("RIGHT")
            || comment.start_side.as_deref().unwrap_or("RIGHT") != "RIGHT"
        {
            return Err(CoreError::invalid_input(
                "suggestion must target a valid RIGHT-side range",
            ));
        }
        suggestions_by_path
            .entry(comment.path.clone())
            .or_default()
            .push(ValidatedSuggestion {
                comment,
                start_line,
                end_line,
                replacement,
            });
    }

    let repo_path = repo_root.join(format!("{source_namespace}/{}.git", source_repo.name));
    let mut file_updates = Vec::with_capacity(suggestions_by_path.len());
    for (path, suggestions) in &mut suggestions_by_path {
        suggestions.sort_by_key(|suggestion| suggestion.start_line);
        for pair in suggestions.windows(2) {
            if pair[0].end_line >= pair[1].start_line {
                return Err(CoreError::invalid_input(format!(
                    "suggestion ranges overlap in {path}"
                )));
            }
        }

        let Some((blob_sha, blob_bytes)) = rg_git::ops::blob_at(&repo_path, head_sha, path)
            .map_err(|error| CoreError::internal(format!("failed to read '{path}' at {head_sha}: {error:#}")))?
        else {
            return Err(CoreError::not_found(format!(
                "'{path}' does not exist at {head_sha}"
            )));
        };
        let content = String::from_utf8(blob_bytes).map_err(|_| {
            CoreError::invalid_input("suggestions cannot be applied to a non-UTF-8 file")
        })?;
        let had_trailing_newline = content.ends_with('\n');
        let mut lines = content.lines().map(str::to_string).collect::<Vec<_>>();

        for suggestion in suggestions.iter().rev() {
            let start_index = (suggestion.start_line - 1) as usize;
            let end_index = suggestion.end_line as usize;
            if start_index >= lines.len() || end_index > lines.len() {
                return Err(CoreError::invalid_input(format!(
                    "suggestion range {}-{} is outside {}",
                    suggestion.start_line, suggestion.end_line, path
                )));
            }
            let replacement = suggestion
                .replacement
                .lines()
                .map(str::to_string)
                .collect::<Vec<_>>();
            if lines[start_index..end_index] == replacement {
                return Err(CoreError::invalid_input(format!(
                    "suggestion #{} does not change {path}",
                    suggestion.comment.id
                )));
            }
            lines.splice(start_index..end_index, replacement);
        }
        let mut updated_content = lines.join("\n");
        if had_trailing_newline {
            updated_content.push('\n');
        }
        file_updates.push(crate::repo::service::FileUpdate {
            path: path.clone(),
            content: updated_content,
            expected_blob_sha: blob_sha,
        });
    }

    // Applying a suggestion creates a server-side commit on the PR head
    // branch. That branch may be protected, so the same check the contents API
    // uses must run here — otherwise `require_pr` / `require_signed_commits`
    // can be bypassed by applying a suggestion instead of pushing.
    crate::branch_protection::service::check_direct_commit_allowed(
        db,
        source_repo.id,
        &pr.head_branch,
        Some(actor.id),
    )
    .await?;

    let commit_sha = crate::repo::service::update_files_in_commit(
        source_namespace,
        &source_repo.name,
        &pr.head_branch,
        head_sha,
        &file_updates,
        &format!("Apply {} review suggestion(s)", comment_ids.len()),
        &actor.username,
        &actor.email,
        repo_root,
    )?;
    pull_request_ops::advance_open_head_sha(
        db,
        source_repo.id,
        &pr.head_branch,
        head_sha,
        &commit_sha,
    )
    .await?;

    let now = Utc::now();
    let transaction = db.begin().await?;
    let mut comments = Vec::with_capacity(comment_ids.len());
    for suggestions in suggestions_by_path.values() {
        for suggestion in suggestions {
            let mut active: review_comment::ActiveModel = suggestion.comment.clone().into();
            active.suggestion_applied_at = Set(Some(now));
            active.suggestion_applied_by_id = Set(Some(actor.id));
            active.suggestion_commit_sha = Set(Some(commit_sha.clone()));
            active.updated_at = Set(now);
            comments.push(active.update(&transaction).await?);
        }
    }
    transaction.commit().await?;
    for comment in &comments {
        rg_db::ops::pr_event_ops::record(
            db,
            pr.repo_id,
            pr.id,
            Some(actor.id),
            "suggestion_applied",
            None,
            serde_json::json!({
                "comment_id": comment.id,
                "commit_sha": commit_sha
            }),
        )
        .await?;
    }
    comments.sort_by_key(|comment| {
        comment_ids
            .iter()
            .position(|id| *id == comment.id)
            .unwrap_or(usize::MAX)
    });
    Ok(AppliedSuggestions {
        comments,
        commit_sha,
    })
}

/// Resolve or reopen the top-level thread containing a review comment.
pub async fn set_thread_resolved(
    db: &DatabaseConnection,
    pr_id: i64,
    comment_id: i64,
    actor_id: i64,
    resolved: bool,
) -> CoreResult<ReviewComment> {
    let comment = get_thread_root(db, pr_id, comment_id).await?;

    let mut active: review_comment::ActiveModel = comment.into();
    active.resolved_at = Set(resolved.then(Utc::now));
    active.resolved_by_id = Set(resolved.then_some(actor_id));
    active.updated_at = Set(Utc::now());
    let updated = review_comment_ops::update(db, active).await?;
    rg_db::ops::pr_event_ops::record(
        db,
        // The repository is recovered from the PR to keep the event scoped.
        pull_request::Entity::find_by_id(pr_id)
            .one(db)
            .await?
            .ok_or_else(|| CoreError::not_found("pull request not found"))?
            .repo_id,
        pr_id,
        Some(actor_id),
        if resolved {
            "thread_resolved"
        } else {
            "thread_reopened"
        },
        None,
        serde_json::json!({"comment_id": updated.id}),
    )
    .await?;
    Ok(updated)
}

/// Find and validate the top-level comment for a review thread.
pub async fn get_thread_root(
    db: &DatabaseConnection,
    pr_id: i64,
    comment_id: i64,
) -> CoreResult<ReviewComment> {
    let mut comment = review_comment_ops::find_by_id(db, comment_id)
        .await?
        .ok_or_else(|| CoreError::not_found("review comment not found"))?;
    if comment.pr_id != pr_id {
        // Comments of other PRs are not addressable through this PR.
        return Err(CoreError::not_found("review comment not found"));
    }

    // Resolution is stored on the root. Replies currently form a shallow
    // tree, but walking makes this safe if clients reply to another reply.
    let mut hops = 0;
    while let Some(parent_id) = comment.reply_to_id {
        comment = review_comment_ops::find_by_id(db, parent_id)
            .await?
            .ok_or_else(|| CoreError::not_found("review thread root not found"))?;
        if comment.pr_id != pr_id {
            return Err(CoreError::not_found("review thread root not found"));
        }
        hops += 1;
        if hops > 100 {
            // Stored data is malformed; nothing the caller can fix.
            return Err(CoreError::internal(format!(
                "review comment {comment_id} has an invalid thread nesting"
            )));
        }
    }

    Ok(comment)
}

/// List all review comments for a PR.
pub async fn list_review_comments(
    db: &DatabaseConnection,
    owner: &str,
    repo_name: &str,
    pr_number: i64,
) -> CoreResult<Vec<ReviewComment>> {
    let repo = resolve_repo(db, owner, repo_name).await?;
    let pr = pull_request_ops::find_by_repo_and_number(db, repo.id, pr_number)
        .await?
        .ok_or_else(|| CoreError::not_found("pull request not found"))?;

    Ok(review_comment_ops::list_by_pr(db, pr.id).await?)
}

/// List comments for a specific review.
pub async fn list_comments_for_review(
    db: &DatabaseConnection,
    review_id: i64,
) -> CoreResult<Vec<ReviewComment>> {
    Ok(review_comment_ops::list_by_review(db, review_id).await?)
}

/// Check if a PR has enough approvals.
pub async fn check_approval_status(
    db: &DatabaseConnection,
    pr_id: i64,
    required_approvals: i64,
) -> CoreResult<bool> {
    let pr = pull_request::Entity::find_by_id(pr_id)
        .one(db)
        .await?
        .ok_or_else(|| CoreError::not_found("pull request not found"))?;
    let count = pr_review_ops::count_current_approvals(db, pr_id, pr.head_sha.as_deref()).await?;
    Ok(count >= required_approvals)
}

// ── Helpers ───────────────────────────────────────────────────────────

async fn resolve_repo(
    db: &DatabaseConnection,
    owner: &str,
    repo_name: &str,
) -> CoreResult<rg_db::entities::repository::Model> {
    // Resolve through the shared helper so organization repositories
    // (`acme/widget`) work, not only personal ones: this used to look the owner
    // up as a *user*, which made every org URL fail with "owner not found" and
    // left the creator's personal URL as the only way in.
    crate::repo::service::find_repo_by_owner_name(db, owner, repo_name)
        .await?
        .ok_or_else(|| CoreError::not_found("repository not found"))
}
