//! Cross-backend checks for the CI usage counters and quota gates (R10, #40).
//!
//! Runs against `IRONFORGE_TEST_DATABASE_URL` (SQLite, PostgreSQL or MySQL);
//! without it a throw-away SQLite file is used. Every run creates its own
//! users/repository with unique names, so a shared server database can be
//! reused:
//!
//! ```text
//! cargo test -p rg-ci --test quota_backends
//! IRONFORGE_TEST_DATABASE_URL=postgres://ironforge:ironforge@127.0.0.1:55432/ironforge \
//!   cargo test -p rg-ci --test quota_backends
//! IRONFORGE_TEST_DATABASE_URL=mysql://root:ironforge@127.0.0.1:53306/ironforge \
//!   cargo test -p rg-ci --test quota_backends
//! ```
//!
//! What the backends disagree about, and what this test therefore pins:
//! the unique index on `(subject_kind, subject_id, repo_id, metric,
//! period_start)`, the `UPDATE`-then-`INSERT` upsert (MySQL has no
//! `ON CONFLICT`), and `DATE`/`DATETIME` round-tripping (MySQL keeps whole
//! seconds).

use std::sync::atomic::{AtomicU64, Ordering};

use rg_ci::quota;
use rg_core::error::CoreError;
use rg_db::sea_orm::{ConnectionTrait, NotSet, Set};
use rg_db::DatabaseConnection;

/// Migrations must not race when several tests open the same server database.
static SETUP_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
static COUNTER: AtomicU64 = AtomicU64::new(0);

async fn connect() -> DatabaseConnection {
    let url = std::env::var("IRONFORGE_TEST_DATABASE_URL").unwrap_or_else(|_| {
        let path = std::env::temp_dir().join(format!(
            "ironforge_quota_backends_{}.db",
            std::process::id()
        ));
        format!("sqlite://{}?mode=rwc", path.display())
    });
    let _guard = SETUP_LOCK.lock().await;
    let db = rg_db::connect_with_pool(&url, 20, 60, 8)
        .await
        .expect("connect");
    rg_db::run_migrations(&db).await.expect("migrate");
    db
}

fn unique(prefix: &str) -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{prefix}{}{}{}", std::process::id() % 100_000, nanos, n)
}

/// `(status, code)` of a coded `CoreError`, panicking on any other shape.
fn coded(error: &anyhow::Error) -> (u16, &'static str) {
    match error.downcast_ref::<CoreError>() {
        Some(CoreError::Coded { status, code, .. }) => (*status, code),
        other => panic!("expected CoreError::Coded, got {other:?}"),
    }
}

fn backend_name(db: &DatabaseConnection) -> String {
    format!("{:?}", db.get_database_backend())
}

async fn user(db: &DatabaseConnection, name: &str) -> i64 {
    rg_db::ops::user_ops::create_user(db, name, &format!("{name}@example.invalid"), "unused", name)
        .await
        .expect("create user")
        .id
}

async fn agent(db: &DatabaseConnection, name: &str, owner_id: i64) -> i64 {
    rg_db::ops::user_ops::create_agent(db, name, &format!("{name}@example.invalid"), owner_id, None)
        .await
        .expect("create agent")
        .id
}

async fn repo(db: &DatabaseConnection, owner_id: i64, name: &str) -> i64 {
    let now = chrono::Utc::now();
    rg_db::ops::repo_ops::create(
        db,
        rg_db::entities::repository::ActiveModel {
            id: NotSet,
            owner_id: Set(owner_id),
            name: Set(name.into()),
            description: Set(None),
            is_private: Set(true),
            default_branch: Set("main".into()),
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
    .expect("create repo")
    .id
}

async fn finished_pipeline(
    db: &DatabaseConnection,
    repo_id: i64,
    subject_id: i64,
    minutes: i64,
) -> i64 {
    let pipeline = rg_db::ops::pipeline_ops::create_pipeline(
        db,
        repo_id,
        "deadbeef",
        "refs/heads/main",
        "push",
        Some(subject_id),
    )
    .await
    .expect("create pipeline");
    let stage = rg_db::ops::pipeline_ops::create_stage(db, pipeline.id, "build", 0)
        .await
        .expect("create stage");
    let job = rg_db::ops::pipeline_ops::create_job(
        db, stage.id, "build", "true", None, None, None, None, None, false, None, None, None,
    )
    .await
    .expect("create job");
    let finished = chrono::Utc::now().naive_utc();
    let started = finished - chrono::Duration::minutes(minutes);
    rg_db::ops::pipeline_ops::update_job_result(
        db,
        job.id,
        "success",
        Some(0),
        None,
        Some(started),
        Some(finished),
    )
    .await
    .expect("finish job");
    rg_db::ops::pipeline_ops::update_pipeline_status(
        db,
        pipeline.id,
        "success",
        Some(started),
        Some(finished),
    )
    .await
    .expect("finish pipeline");
    pipeline.id
}

#[tokio::test]
async fn counters_and_gates_agree_on_every_backend() {
    let db = connect().await;
    let backend = backend_name(&db);
    let suffix = unique("q");
    let owner = user(&db, &format!("o{suffix}")).await;
    let subject = agent(&db, &format!("o{suffix}-agent-a"), owner).await;
    let repo_id = repo(&db, owner, &format!("r{suffix}")).await;
    let today = quota::utc_day(chrono::Utc::now().naive_utc());

    // 3 × 40 min = 120 min, exactly the daily limit. Recording each pipeline
    // twice must still yield one row per metric and the single-run total.
    for _ in 0..3 {
        let pipeline_id = finished_pipeline(&db, repo_id, subject, 40).await;
        quota::record_finished_pipeline(&db, pipeline_id)
            .await
            .unwrap_or_else(|e| panic!("[{backend}] record: {e:#}"));
        quota::record_finished_pipeline(&db, pipeline_id)
            .await
            .unwrap_or_else(|e| panic!("[{backend}] re-record: {e:#}"));
    }
    assert_eq!(
        quota::counter_value(&db, subject, repo_id, quota::METRIC_CI_MINUTES, today)
            .await
            .unwrap(),
        Some(quota::DAILY_MINUTES_PER_REPO),
        "[{backend}] minutes must be recomputed, not accumulated"
    );

    let error = quota::enforce(&db, repo_id, Some(subject))
        .await
        .expect_err("the daily limit must reject the next trigger");
    assert_eq!(
        coded(&error),
        (409, quota::CODE_MINUTES),
        "[{backend}] a quota rejection must carry its own code and status: {error}"
    );

    // Concurrency gate on a second agent of the same repository.
    let concurrent = agent(&db, &format!("o{suffix}-agent-b"), owner).await;
    for _ in 0..quota::MAX_CONCURRENT_PIPELINES_PER_REPO {
        rg_db::ops::pipeline_ops::create_pipeline(
            &db,
            repo_id,
            "deadbeef",
            "refs/heads/main",
            "push",
            Some(concurrent),
        )
        .await
        .expect("create active pipeline");
    }
    let error = quota::enforce(&db, repo_id, Some(concurrent))
        .await
        .expect_err("the concurrency limit must reject the next trigger");
    assert_eq!(
        coded(&error),
        (409, quota::CODE_CONCURRENCY),
        "[{backend}] a quota rejection must carry its own code and status: {error}"
    );
    assert_eq!(
        quota::counter_value(
            &db,
            concurrent,
            repo_id,
            quota::METRIC_CI_CONCURRENT_PIPELINES,
            today
        )
        .await
        .unwrap(),
        Some(quota::MAX_CONCURRENT_PIPELINES_PER_REPO),
        "[{backend}] the gauge must be the live count"
    );

    // A human owner is never gated, even with the same shape of usage.
    let human_pipelines = finished_pipeline(&db, repo_id, owner, 40).await;
    quota::record_finished_pipeline(&db, human_pipelines)
        .await
        .unwrap();
    quota::enforce(&db, repo_id, Some(owner))
        .await
        .unwrap_or_else(|e| panic!("[{backend}] a human must not be gated: {e:#}"));
    assert!(
        quota::counter_value(&db, owner, repo_id, quota::METRIC_CI_MINUTES, today)
            .await
            .unwrap()
            .is_none(),
        "[{backend}] human usage must not be recorded"
    );

    println!("quota counters verified on {backend}");
}
