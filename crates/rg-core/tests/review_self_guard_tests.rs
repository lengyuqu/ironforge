//! The "a PR author cannot decide their own PR" rule lives in the core service,
//! not only in the HTTP handler (#19 R19-2).
//!
//! The HTTP layer already refuses a self-approval with `400`, but `submit_review`
//! is the only write path for reviews: any future caller that does not go
//! through that handler (MCP tool, internal service, bulk import) would record a
//! vote on the author's own PR, and those votes are what
//! `count_current_human_approvals` feeds into the protected-branch merge gate.
//!
//! These tests call `rg_core::review::service::submit_review` **directly**, with
//! no HTTP in the loop — testing the handler instead would prove nothing about
//! the guard being moved.

use chrono::Utc;
use rg_core::error::CoreError;
use rg_core::review::service::{submit_review, ReviewAction};
use rg_db::entities::{pull_request, repository};
use rg_db::ops::user_ops;
use sea_orm::{ActiveModelTrait, ConnectionTrait, DatabaseConnection, Set};

// ── database ──────────────────────────────────────────────────────────────

/// A migrated SQLite file, built once per test binary (migrations cost seconds;
/// copying the finished file costs milliseconds).
static TEMPLATE: tokio::sync::OnceCell<std::path::PathBuf> = tokio::sync::OnceCell::const_new();

async fn migrated_template() -> &'static std::path::PathBuf {
    TEMPLATE
        .get_or_init(|| async {
            let path = std::env::temp_dir().join(format!(
                "ironforge-review-self-guard-{}.db",
                std::process::id()
            ));
            let _ = std::fs::remove_file(&path);
            let db = rg_db::connect_with_pool(
                &format!("sqlite://{}?mode=rwc", path.display()),
                5,
                60,
                1,
            )
            .await
            .expect("connect to the template database");
            rg_db::run_migrations(&db).await.expect("migrations");
            db.execute_unprepared("PRAGMA wal_checkpoint(TRUNCATE)")
                .await
                .expect("checkpoint");
            db.close().await.expect("close");
            for suffix in ["-wal", "-shm"] {
                let mut sidecar = path.clone().into_os_string();
                sidecar.push(suffix);
                let _ = std::fs::remove_file(sidecar);
            }
            path
        })
        .await
}

struct Fixture {
    db: DatabaseConnection,
    _dir: tempfile::TempDir,
    repo_id: i64,
    pr_id: i64,
    author_id: i64,
}

/// A migrated database with an open PR (`main` ← `feature`) authored by one
/// user, plus a second user who is not the author.
async fn fixture() -> Fixture {
    let template = migrated_template().await;
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("review-self-guard.db");
    std::fs::copy(template, &path).expect("copy the migrated template");
    let db = rg_db::connect_with_pool(&format!("sqlite://{}?mode=rwc", path.display()), 5, 60, 4)
        .await
        .expect("connect");

    let author = user_ops::create_user(
        &db,
        "guard-author",
        "guard-author@example.com",
        "",
        "Author",
    )
    .await
    .expect("create the author");
    user_ops::create_user(
        &db,
        "guard-reviewer",
        "guard-reviewer@example.com",
        "",
        "Reviewer",
    )
    .await
    .expect("create the reviewer");

    let now = Utc::now();
    let repo = rg_db::ops::repo_ops::create(
        &db,
        repository::ActiveModel {
            id: sea_orm::NotSet,
            owner_id: Set(author.id),
            name: Set("guarded".to_string()),
            description: Set(None),
            is_private: Set(false),
            default_branch: Set("main".to_string()),
            fork_id: Set(None),
            stars_count: Set(0),
            forks_count: Set(0),
            org_id: Set(None),
            created_at: Set(now),
            updated_at: Set(now),
            deleted_at: Set(None),
            origin_repo_id: Set(None),
        },
    )
    .await
    .expect("create the repository");

    let pr = pull_request::ActiveModel {
        id: sea_orm::NotSet,
        repo_id: Set(repo.id),
        number: Set(1),
        title: Set("A change".to_string()),
        body: Set(None),
        state: Set("open".to_string()),
        is_draft: Set(false),
        auto_merge_enabled: Set(false),
        auto_merge_strategy: Set(None),
        auto_merge_enabled_by_id: Set(None),
        auto_merge_enabled_at: Set(None),
        author_id: Set(author.id),
        reviewer_id: Set(None),
        head_branch: Set("feature".to_string()),
        base_branch: Set("main".to_string()),
        head_sha: Set(Some("deadbeef".to_string())),
        merge_strategy: Set(None),
        merge_commit_sha: Set(None),
        head_repo_id: Set(None),
        milestone_id: Set(None),
        labels: Set(None),
        created_at: Set(now),
        updated_at: Set(now),
        closed_at: Set(None),
        merged_at: Set(None),
    }
    .insert(&db)
    .await
    .expect("insert the pull request");

    Fixture {
        db,
        _dir: dir,
        repo_id: repo.id,
        pr_id: pr.id,
        author_id: author.id,
    }
}

async fn review_count(db: &DatabaseConnection, pr_id: i64) -> usize {
    rg_db::ops::pr_review_ops::list_by_pr(db, pr_id)
        .await
        .expect("list reviews")
        .len()
}

// ── R19-2: the guard at the core boundary ────────────────────────────────

/// The author approving their own PR is refused by the service itself, with no
/// HTTP handler in the path. This is the whole point of the move.
#[tokio::test]
async fn the_author_cannot_approve_their_own_pr_through_the_service() {
    let f = fixture().await;
    let err = submit_review(
        &f.db,
        f.repo_id,
        1,
        f.author_id,
        ReviewAction::Approve,
        Some("ship it".to_string()),
        None,
    )
    .await
    .expect_err("a self-approval must fail in the service, not only over HTTP");

    assert!(
        matches!(err, CoreError::InvalidInput(_)),
        "expected an invalid-input error, got: {err:?}"
    );
    assert!(
        err.to_string()
            .contains("cannot approve or request changes on their own PR"),
        "the message must stay recognisable to callers: {err}"
    );
    assert_eq!(
        review_count(&f.db, f.pr_id).await,
        0,
        "a refused self-approval must not be recorded"
    );
}

/// Same rule for `request_changes`.
#[tokio::test]
async fn the_author_cannot_request_changes_on_their_own_pr_through_the_service() {
    let f = fixture().await;
    let err = submit_review(
        &f.db,
        f.repo_id,
        1,
        f.author_id,
        ReviewAction::RequestChanges,
        None,
        None,
    )
    .await
    .expect_err("a self request-changes must fail in the service");
    assert!(matches!(err, CoreError::InvalidInput(_)), "{err:?}");
    assert_eq!(review_count(&f.db, f.pr_id).await, 0);
}

/// A comment is not a decision: the author may still discuss their own PR.
#[tokio::test]
async fn the_author_may_comment_on_their_own_pr_through_the_service() {
    let f = fixture().await;
    let review = submit_review(
        &f.db,
        f.repo_id,
        1,
        f.author_id,
        ReviewAction::Comment,
        Some("note to self".to_string()),
        None,
    )
    .await
    .expect("a comment review is not a decision and must be allowed");
    assert_eq!(review.action, "comment");
    assert_eq!(review.reviewer_id, f.author_id);
    assert_eq!(review_count(&f.db, f.pr_id).await, 1);
}

/// The guard is about self-review, not about reviews in general: someone else
/// can still approve the PR through the same service.
#[tokio::test]
async fn a_different_reviewer_may_approve_through_the_service() {
    let f = fixture().await;
    let reviewer = user_ops::find_by_username(&f.db, "guard-reviewer")
        .await
        .expect("query the reviewer")
        .expect("the reviewer exists");
    let review = submit_review(
        &f.db,
        f.repo_id,
        1,
        reviewer.id,
        ReviewAction::Approve,
        None,
        None,
    )
    .await
    .expect("a non-author review must pass the guard");
    assert_eq!(review.action, "approve");
    assert_ne!(review.reviewer_id, f.author_id);
}
