//! Storage-level tests for the authorization primitive (#19, step 1).
//!
//! Runs against the database named by `IRONFORGE_TEST_DATABASE_URL` (SQLite,
//! PostgreSQL or MySQL); without it a throw-away SQLite file is used. Every test
//! creates its own user and repository with random names, so a shared server
//! database can be reused. To cover all backends:
//!
//! ```text
//! cargo test -p rg-db --test authorization_store                                     # SQLite
//! IRONFORGE_TEST_DATABASE_URL=postgres://... cargo test -p rg-db --test authorization_store
//! IRONFORGE_TEST_DATABASE_URL=mysql://...    cargo test -p rg-db --test authorization_store
//! ```
//!
//! What matters on every backend: the idempotency key is unique, a duplicate
//! vote answers `false` instead of inserting a second ticket, the guarded
//! `UPDATE ... WHERE state = 'pending'` is what makes consumption one-shot, and
//! the `head_sha` sweep only touches rows bound to a different sha.

use chrono::{Duration, Utc};
use rg_db::entities::authorization_request::{
    REASON_HEAD_SHA_CHANGED, STATE_CONSUMED, STATE_EXPIRED, STATE_PENDING,
};
use rg_db::ops::authorization_ops::{self, NewRequest, RequestOutcome};
use rg_db::ops::{repo_ops, user_ops};
use rg_db::sea_orm::{NotSet, Set, TransactionTrait};
use rg_db::DatabaseConnection;

static SETUP_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

const SHA_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const SHA_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

async fn connect() -> DatabaseConnection {
    let url = std::env::var("IRONFORGE_TEST_DATABASE_URL").unwrap_or_else(|_| {
        let path = std::env::temp_dir().join(format!(
            "ironforge_authorization_store_{}.db",
            std::process::id()
        ));
        format!("sqlite://{}?mode=rwc", path.display())
    });
    let _guard = SETUP_LOCK.lock().await;
    let db = rg_db::connect_with_pool(&url, 20, 60, 16)
        .await
        .expect("connect");
    rg_db::run_migrations(&db).await.expect("migrate");
    db
}

async fn fixture() -> (DatabaseConnection, i64, i64) {
    let db = connect().await;
    let suffix = uuid::Uuid::new_v4().simple().to_string();
    let suffix = &suffix[..10];
    let username = format!("authz{suffix}");
    let user = user_ops::create_user(
        &db,
        &username,
        &format!("{username}@example.invalid"),
        "unused",
        "Authorization Test",
    )
    .await
    .expect("create user");
    let now = Utc::now();
    let repo = repo_ops::create(
        &db,
        rg_db::entities::repository::ActiveModel {
            id: NotSet,
            owner_id: Set(user.id),
            name: Set(format!("repo{suffix}")),
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

            is_template: sea_orm::ActiveValue::Set(false),
        },
    )
    .await
    .expect("create repo");
    (db, user.id, repo.id)
}

fn new_request(repo_id: i64, applicant: i64, key: &str) -> NewRequest {
    NewRequest {
        repo_id,
        action: "protected_branch_merge".to_string(),
        subject: "1".to_string(),
        head_sha: Some(SHA_A.to_string()),
        requested_by_kind: "human".to_string(),
        requested_by_id: applicant,
        authorization_set_id: 11,
        required_approvals: 1,
        client_request_id: key.to_string(),
        payload_hash: "a".repeat(64),
        expires_at: Utc::now() + Duration::hours(1),
    }
}

async fn file(
    db: &DatabaseConnection,
    repo_id: i64,
    applicant: i64,
    key: &str,
) -> authorization_request::Model {
    match authorization_ops::create(db, new_request(repo_id, applicant, key), Utc::now())
        .await
        .expect("create request")
    {
        RequestOutcome::Created(row) | RequestOutcome::Existing(row) => row,
    }
}

use rg_db::entities::authorization_request;

#[tokio::test]
async fn the_client_key_is_unique_per_principal() {
    let (db, user, repo) = fixture().await;
    let first = file(&db, repo, user, "k1").await;
    let second = file(&db, repo, user, "k1").await;
    assert_eq!(first.id, second.id, "same key must reuse the row");

    // Another principal may use the same key: the triple includes who asked.
    let other = format!("authz{}", &uuid::Uuid::new_v4().simple().to_string()[..10]);
    let other_user = user_ops::create_user(
        &db,
        &other,
        &format!("{other}@example.invalid"),
        "unused",
        "Other",
    )
    .await
    .expect("second user");
    let theirs = file(&db, repo, other_user.id, "k1").await;
    assert_ne!(first.id, theirs.id);
}

#[tokio::test]
async fn a_duplicate_vote_answers_false() {
    let (db, user, repo) = fixture().await;
    let request = file(&db, repo, user, "k1").await;
    assert!(
        authorization_ops::add_decision(&db, request.id, user, "approve", "h", Utc::now())
            .await
            .expect("first vote")
    );
    assert!(
        !authorization_ops::add_decision(&db, request.id, user, "approve", "h", Utc::now())
            .await
            .expect("duplicate vote"),
        "the unique key must reject the second vote without an error"
    );
    assert_eq!(
        authorization_ops::count_approvals(&db, request.id)
            .await
            .expect("count"),
        1
    );
}

#[tokio::test]
async fn consumption_is_one_shot_across_a_rollback() {
    let (db, user, repo) = fixture().await;
    let request = file(&db, repo, user, "k1").await;

    let txn = db.begin().await.expect("begin");
    assert!(
        authorization_ops::consume_guarded(&txn, request.id, user, "effect", Utc::now())
            .await
            .expect("consume"),
        "the first guarded update must win"
    );
    txn.rollback().await.expect("rollback");

    let row = authorization_ops::find_by_id(&db, request.id)
        .await
        .expect("find")
        .expect("row");
    assert_eq!(
        row.state, STATE_PENDING,
        "a rollback must restore the ticket"
    );

    assert!(
        authorization_ops::consume_guarded(&db, request.id, user, "effect", Utc::now())
            .await
            .expect("consume again")
    );
    assert!(
        !authorization_ops::consume_guarded(&db, request.id, user, "effect", Utc::now())
            .await
            .expect("second consume"),
        "the guard must make the second consume a no-op"
    );
    let row = authorization_ops::find_by_id(&db, request.id)
        .await
        .expect("find")
        .expect("row");
    assert_eq!(row.state, STATE_CONSUMED);
    assert_eq!(row.consumed_effect_hash.as_deref(), Some("effect"));
}

#[tokio::test]
async fn the_head_sha_sweep_only_touches_other_heads() {
    let (db, user, repo) = fixture().await;
    let request = file(&db, repo, user, "k1").await;

    let untouched = authorization_ops::expire_on_head_sha_change(
        &db,
        repo,
        "protected_branch_merge",
        "1",
        SHA_A,
        Utc::now(),
    )
    .await
    .expect("sweep with the same head");
    assert_eq!(untouched, 0);
    assert_eq!(
        authorization_ops::find_by_id(&db, request.id)
            .await
            .expect("find")
            .expect("row")
            .state,
        STATE_PENDING
    );

    let expired = authorization_ops::expire_on_head_sha_change(
        &db,
        repo,
        "protected_branch_merge",
        "1",
        SHA_B,
        Utc::now(),
    )
    .await
    .expect("sweep with a new head");
    assert_eq!(expired, 1);
    let row = authorization_ops::find_by_id(&db, request.id)
        .await
        .expect("find")
        .expect("row must be kept");
    assert_eq!(row.state, STATE_EXPIRED);
    assert_eq!(row.state_reason.as_deref(), Some(REASON_HEAD_SHA_CHANGED));
}

#[tokio::test]
async fn overdue_requests_expire_once() {
    let (db, user, repo) = fixture().await;
    let mut new = new_request(repo, user, "k1");
    new.expires_at = Utc::now() - Duration::seconds(1);
    let request = match authorization_ops::create(&db, new, Utc::now())
        .await
        .expect("create")
    {
        RequestOutcome::Created(row) | RequestOutcome::Existing(row) => row,
    };
    assert!(
        authorization_ops::expire_if_overdue(&db, request.id, Utc::now())
            .await
            .expect("expire")
    );
    assert!(
        !authorization_ops::expire_if_overdue(&db, request.id, Utc::now())
            .await
            .expect("second expire"),
        "an already expired request must not move again"
    );
}
