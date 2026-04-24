//! Code review service — submit reviews, add inline comments, approve / request changes.

use anyhow::{bail, Context, Result};
use chrono::Utc;
use sea_orm::{ActiveModelTrait, DatabaseConnection, Set};

use rg_db::entities::pr_review::{self, Model as PrReview};
use rg_db::entities::review_comment::{self, Model as ReviewComment};
use rg_db::ops::{pull_request_ops, pr_review_ops, review_comment_ops, repo_ops};

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

    pub fn from_str(s: &str) -> Result<Self> {
        match s {
            "comment" => Ok(Self::Comment),
            "approve" => Ok(Self::Approve),
            "request_changes" => Ok(Self::RequestChanges),
            "dismiss" => Ok(Self::Dismiss),
            _ => bail!("invalid review action: {}", s),
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
) -> Result<PrReview> {
    // Validate PR exists
    let pr = pull_request_ops::find_by_repo_and_number(db, repo_id, pr_number)
        .await?
        .context("pull request not found")?;

    if pr.state != "open" {
        bail!("cannot review a PR that is not open (current: {})", pr.state);
    }

    // For dismiss, body is typically about why the review is dismissed
    let model = pr_review::ActiveModel {
        id: sea_orm::NotSet,
        pr_id: Set(pr.id),
        repo_id: Set(repo_id),
        reviewer_id: Set(reviewer_id),
        action: Set(action.as_str().to_string()),
        body: Set(body),
        commit_id: Set(commit_id),
        created_at: Set(Utc::now()),
    };

    pr_review_ops::create(db, model).await
}

/// List all reviews for a PR.
pub async fn list_reviews(
    db: &DatabaseConnection,
    owner: &str,
    repo_name: &str,
    pr_number: i64,
) -> Result<Vec<PrReview>> {
    let repo = resolve_repo(db, owner, repo_name).await?;
    let pr = pull_request_ops::find_by_repo_and_number(db, repo.id, pr_number)
        .await?
        .context("pull request not found")?;

    pr_review_ops::list_by_pr(db, pr.id).await
}

/// Get a single review by ID.
pub async fn get_review(
    db: &DatabaseConnection,
    review_id: i64,
) -> Result<PrReview> {
    pr_review_ops::find_by_id(db, review_id)
        .await?
        .context("review not found")
}

/// Dismiss a review.
pub async fn dismiss_review(
    db: &DatabaseConnection,
    review_id: i64,
    dismissor_id: i64,
    message: String,
) -> Result<PrReview> {
    let review = pr_review_ops::find_by_id(db, review_id)
        .await?
        .context("review not found")?;

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

    pr_review_ops::create(db, model).await
}

// ── Inline Review Comments ────────────────────────────────────────────

/// Create an inline review comment on a specific diff line.
pub async fn create_review_comment(
    db: &DatabaseConnection,
    repo_id: i64,
    pr_number: i64,
    review_id: i64,
    author_id: i64,
    path: String,
    line: Option<i64>,
    side: Option<String>,
    body: String,
    commit_id: Option<String>,
    reply_to_id: Option<i64>,
) -> Result<ReviewComment> {
    // Validate PR
    let pr = pull_request_ops::find_by_repo_and_number(db, repo_id, pr_number)
        .await?
        .context("pull request not found")?;

    // Validate review exists
    let _review = pr_review_ops::find_by_id(db, review_id)
        .await?
        .context("review not found")?;

    // Validate reply_to if specified
    if let Some(rtid) = reply_to_id {
        let parent = review_comment_ops::find_by_id(db, rtid)
            .await?
            .context("parent comment not found")?;
        if parent.pr_id != pr.id {
            bail!("parent comment does not belong to this PR");
        }
    }

    if body.trim().is_empty() {
        bail!("comment body cannot be empty");
    }

    let model = review_comment::ActiveModel {
        id: sea_orm::NotSet,
        review_id: Set(review_id),
        pr_id: Set(pr.id),
        author_id: Set(author_id),
        path: Set(path),
        position: Set(None), // Deprecated, use line instead
        line: Set(line),
        side: Set(side),
        body: Set(body),
        commit_id: Set(commit_id),
        reply_to_id: Set(reply_to_id),
        created_at: Set(Utc::now()),
        updated_at: Set(Utc::now()),
    };

    review_comment_ops::create(db, model).await
}

/// List all review comments for a PR.
pub async fn list_review_comments(
    db: &DatabaseConnection,
    owner: &str,
    repo_name: &str,
    pr_number: i64,
) -> Result<Vec<ReviewComment>> {
    let repo = resolve_repo(db, owner, repo_name).await?;
    let pr = pull_request_ops::find_by_repo_and_number(db, repo.id, pr_number)
        .await?
        .context("pull request not found")?;

    review_comment_ops::list_by_pr(db, pr.id).await
}

/// List comments for a specific review.
pub async fn list_comments_for_review(
    db: &DatabaseConnection,
    review_id: i64,
) -> Result<Vec<ReviewComment>> {
    review_comment_ops::list_by_review(db, review_id).await
}

/// Check if a PR has enough approvals.
pub async fn check_approval_status(
    db: &DatabaseConnection,
    pr_id: i64,
    required_approvals: i64,
) -> Result<bool> {
    let count = pr_review_ops::count_approvals(db, pr_id).await?;
    Ok(count >= required_approvals)
}

// ── Helpers ───────────────────────────────────────────────────────────

async fn resolve_repo(
    db: &DatabaseConnection,
    owner: &str,
    repo_name: &str,
) -> Result<rg_db::entities::repository::Model> {
    let user = rg_db::ops::user_ops::find_by_username(db, owner)
        .await?
        .context("owner not found")?;
    repo_ops::find_by_owner_and_name(db, user.id, repo_name)
        .await?
        .context("repository not found")
}
