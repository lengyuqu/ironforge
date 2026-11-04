//! CI legacy INTEGER compatibility across SQLite, PostgreSQL and MySQL.
//!
//! Without `IRONFORGE_TEST_DATABASE_URL`, each test migrates an isolated
//! in-memory SQLite database. An explicitly supplied URL must name a migrated,
//! disposable database; tests insert uniquely named fixtures without cleanup.

use chrono::Utc;
use rg_db::entities::{ci_environment, pipeline_job, pipeline_stage, repository};
use rg_db::ops::{ci_environment_ops, pipeline_ops, repo_ops, user_ops};
use rg_db::sea_orm::{
    ColumnTrait, ConnectionTrait, DatabaseBackend, EntityTrait, NotSet, PaginatorTrait,
    QueryFilter, Set, Statement,
};
use rg_db::DatabaseConnection;

async fn connect() -> DatabaseConnection {
    if let Ok(url) = std::env::var("IRONFORGE_TEST_DATABASE_URL") {
        return rg_db::connect_with_pool(&url, 20, 60, 4)
            .await
            .expect("connect to disposable test database");
    }
    let mut options = rg_db::sea_orm::ConnectOptions::new("sqlite::memory:");
    options.max_connections(1).min_connections(1);
    let db = rg_db::sea_orm::Database::connect(options)
        .await
        .expect("connect SQLite");
    db.execute(Statement::from_string(
        DatabaseBackend::Sqlite,
        "PRAGMA foreign_keys = ON".to_string(),
    ))
    .await
    .expect("enable foreign keys");
    rg_db::run_migrations(&db)
        .await
        .expect("migrate temporary SQLite");
    db
}

fn unique(prefix: &str) -> String {
    format!("{prefix}{}", uuid::Uuid::new_v4().simple())
}

async fn fixture_repo(db: &DatabaseConnection) -> (i64, i64) {
    let name = unique("ciint");
    let user = user_ops::create_user(
        db,
        &name,
        &format!("{name}@example.invalid"),
        "unused",
        "CI integer test",
    )
    .await
    .expect("create fixture user");
    let now = Utc::now();
    let repo = repo_ops::create(
        db,
        repository::ActiveModel {
            id: NotSet,
            owner_id: Set(user.id),
            name: Set(unique("repo")),
            description: Set(None),
            is_private: Set(false),
            default_branch: Set("main".into()),
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
    .expect("create fixture repo");
    (user.id, repo.id)
}

async fn fixture_pipeline(db: &DatabaseConnection, repo_id: i64) -> i64 {
    pipeline_ops::create_pipeline(db, repo_id, &"a".repeat(40), "main", "manual", None)
        .await
        .expect("create fixture pipeline")
        .id
}

// SQL fixture insertion intentionally avoids decoding the affected models, but
// keeps the stage attached to a real pipeline and repository.
async fn seed_stage_in_repo(db: &DatabaseConnection, repo_id: i64) -> (i64, i64) {
    let pipeline_id = fixture_pipeline(db, repo_id).await;
    let name = unique("stage");
    let backend = db.get_database_backend();
    let marker = if backend == DatabaseBackend::Postgres {
        "$1"
    } else {
        "?"
    };
    db.execute(Statement::from_sql_and_values(backend,
        format!("INSERT INTO pipeline_stages (pipeline_id, name, stage_order, status) VALUES ({pipeline_id}, {marker}, 0, 'pending')"),
        [name.clone().into()])).await.expect("seed stage");
    let cast = match backend {
        DatabaseBackend::Postgres => "CAST(id AS BIGINT)",
        DatabaseBackend::MySql => "CAST(id AS SIGNED)",
        DatabaseBackend::Sqlite => "CAST(id AS INTEGER)",
    };
    let id = db
        .query_one(Statement::from_sql_and_values(
            backend,
            format!("SELECT {cast} AS id FROM pipeline_stages WHERE name = {marker}"),
            [name.into()],
        ))
        .await
        .expect("query stage fixture")
        .expect("stage fixture exists")
        .try_get("", "id")
        .expect("stage fixture id");
    (id, pipeline_id)
}

async fn seed_stage(db: &DatabaseConnection) -> (i64, i64) {
    let (_, repo_id) = fixture_repo(db).await;
    seed_stage_in_repo(db, repo_id).await
}

async fn seed_job(db: &DatabaseConnection, stage_id: i64) -> i64 {
    let name = unique("job");
    let backend = db.get_database_backend();
    let marker = if backend == DatabaseBackend::Postgres {
        "$1"
    } else {
        "?"
    };
    db.execute(Statement::from_sql_and_values(backend,
        format!("INSERT INTO pipeline_jobs (stage_id, name, script, status) VALUES ({stage_id}, {marker}, 'true', 'pending')"),
        [name.clone().into()])).await.expect("seed job");
    let cast = match backend {
        DatabaseBackend::Postgres => "CAST(id AS BIGINT)",
        DatabaseBackend::MySql => "CAST(id AS SIGNED)",
        DatabaseBackend::Sqlite => "CAST(id AS INTEGER)",
    };
    db.query_one(Statement::from_sql_and_values(
        backend,
        format!("SELECT {cast} AS id FROM pipeline_jobs WHERE name = {marker}"),
        [name.into()],
    ))
    .await
    .expect("query job fixture")
    .expect("job fixture exists")
    .try_get("", "id")
    .expect("job fixture id")
}

async fn approval_fixture(db: &DatabaseConnection) -> (i64, i64, i64) {
    let (user_id, repo_id) = fixture_repo(db).await;
    let now = Utc::now();
    let env = ci_environment_ops::create(
        db,
        ci_environment::ActiveModel {
            id: NotSet,
            repo_id: Set(repo_id),
            name: Set(unique("env")),
            protected: Set(true),
            required_approvals: Set(1),
            allowed_approver_ids: Set(None),
            created_at: Set(now),
            updated_at: Set(now),
        },
    )
    .await
    .expect("create fixture environment");
    let (stage_id, _) = seed_stage_in_repo(db, repo_id).await;
    (seed_job(db, stage_id).await, env.id, user_id)
}

#[tokio::test]
async fn stage_create() {
    let db = connect().await;
    let name = unique("create_stage");
    let (_, repo_id) = fixture_repo(&db).await;
    let pipeline_id = fixture_pipeline(&db, repo_id).await;
    let active = pipeline_ops::find_active_pipelines_by_ref(&db, repo_id, "main")
        .await
        .unwrap();
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].id, pipeline_id);
    assert!(
        pipeline_ops::find_active_pipelines_by_ref(&db, repo_id, "missing")
            .await
            .unwrap()
            .is_empty()
    );
    let result = pipeline_ops::create_stage(&db, pipeline_id, &name, 0).await;
    let stage = result.unwrap_or_else(|e| panic!("stage create: {e:#}"));
    assert_eq!(
        (stage.pipeline_id, stage.name.as_str()),
        (pipeline_id, name.as_str())
    );
    let listed = pipeline_ops::list_stages_by_pipeline(&db, pipeline_id)
        .await
        .unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, stage.id);
    let started = Utc::now().naive_utc();
    pipeline_ops::update_stage_status(&db, stage.id, "running", Some(started), None)
        .await
        .unwrap();
    pipeline_ops::update_stage_status(&db, stage.id, "running", None, None).await.unwrap();
    assert!(pipeline_ops::update_stage_status(&db, i64::MAX, "running", None, None).await.is_err());
    let refreshed = pipeline_ops::get_stage_by_id(&db, stage.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(refreshed.status, "running");
    assert!(refreshed.started_at.is_some());
}

#[tokio::test]
async fn stage_read() {
    let db = connect().await;
    let (id, pipeline_id) = seed_stage(&db).await;
    let stage = pipeline_ops::get_stage_by_id(&db, id)
        .await
        .unwrap_or_else(|e| panic!("stage read: {e:#}"))
        .expect("stage exists");
    assert_eq!((stage.id, stage.pipeline_id), (id, pipeline_id));
}

#[tokio::test]
async fn job_create() {
    let db = connect().await;
    let (stage_id, pipeline_id) = seed_stage(&db).await;
    let job = pipeline_ops::create_job(
        &db,
        stage_id,
        &unique("create_job"),
        "true",
        Some("registry.invalid/ci:latest"),
        Some("[\"linux\"]"),
        Some("{\"MODE\":\"test\"}"),
        Some("cache-key"),
        Some("[\"target\"]"),
        true,
        Some(60),
        Some("on_success"),
        Some("branch == main"),
    )
    .await
    .unwrap_or_else(|e| panic!("job create: {e:#}"));
    assert_eq!((job.stage_id, job.timeout_seconds), (stage_id, Some(60)));
    assert_eq!(job.image.as_deref(), Some("registry.invalid/ci:latest"));
    assert_eq!(job.tags.as_deref(), Some("[\"linux\"]"));
    assert_eq!(job.variables.as_deref(), Some("{\"MODE\":\"test\"}"));
    assert_eq!(job.cache_key.as_deref(), Some("cache-key"));
    assert_eq!(job.cache_paths.as_deref(), Some("[\"target\"]"));
    assert!(job.allow_failure);
    assert_eq!(job.if_condition.as_deref(), Some("branch == main"));
    assert_eq!(
        pipeline_ops::list_jobs_by_stage(&db, stage_id)
            .await
            .unwrap()[0]
            .id,
        job.id
    );
    assert_eq!(
        pipeline_ops::list_jobs_by_pipeline(&db, pipeline_id)
            .await
            .unwrap()[0]
            .id,
        job.id
    );
    pipeline_ops::update_job_log(&db, job.id, "hello")
        .await
        .unwrap();
    pipeline_ops::update_job_log(&db, job.id, "hello").await.unwrap();
    assert!(pipeline_ops::update_job_log(&db, i64::MAX, "hello").await.is_err());
    let read = pipeline_ops::get_job(&db, job.id).await.unwrap().unwrap();
    assert_eq!(read.log.as_deref(), Some("hello"));
    assert!(pipeline_ops::stage_has_job_status(&db, stage_id, "pending")
        .await
        .unwrap());
    let runner_id = 4_294_967_300_i64 + job.id;
    pipeline_ops::assign_job(&db, job.id, runner_id)
        .await
        .unwrap();
    pipeline_ops::assign_job(&db, job.id, runner_id).await.unwrap();
    assert!(pipeline_ops::assign_job(&db, i64::MAX, runner_id).await.is_err());
    let assigned = pipeline_ops::get_job(&db, job.id).await.unwrap().unwrap();
    assert_eq!(
        (assigned.status.as_str(), assigned.runner_id),
        ("assigned", Some(runner_id))
    );
    assert!(
        assigned.updated_at.is_some(),
        "non-null updated_at must decode"
    );
    pipeline_ops::update_job_result(
        &db,
        job.id,
        "success",
        Some(0),
        None,
        None,
        Some(Utc::now().naive_utc()),
    )
    .await
    .unwrap();
    let done = pipeline_ops::get_job(&db, job.id).await.unwrap().unwrap();
    assert_eq!(
        (done.status.as_str(), done.exit_code, done.log.as_deref()),
        ("success", Some(0), Some("hello"))
    );
}

#[tokio::test]
async fn job_read() {
    let db = connect().await;
    let (stage_id, pipeline_id) = seed_stage(&db).await;
    let id = seed_job(&db, stage_id).await;
    let job = pipeline_ops::get_job(&db, id)
        .await
        .unwrap_or_else(|e| panic!("job read: {e:#}"))
        .expect("job exists");
    assert_eq!((job.id, job.stage_id), (id, stage_id));
    assert!(pipeline_ops::list_jobs_by_pipeline(&db, pipeline_id)
        .await
        .unwrap()
        .iter()
        .any(|j| j.id == id));
    assert!(pipeline_ops::find_pending_job(&db).await.unwrap().is_some());
    assert!(pipeline_ops::find_pending_job_matching_labels(&db, &[])
        .await
        .unwrap()
        .is_some());
}

#[tokio::test]
async fn manual_job_can_be_released_once_and_read_back() {
    let db = connect().await;
    let (stage_id, _) = seed_stage(&db).await;
    let job = pipeline_ops::create_job(
        &db,
        stage_id,
        &unique("manual"),
        "true",
        None,
        None,
        None,
        None,
        None,
        false,
        None,
        Some("manual"),
        None,
    )
    .await
    .unwrap();
    assert_eq!(job.status, "manual");
    assert!(pipeline_ops::play_manual_job(&db, job.id).await.unwrap());
    assert!(!pipeline_ops::play_manual_job(&db, job.id).await.unwrap());
    let released = pipeline_ops::get_job(&db, job.id).await.unwrap().unwrap();
    assert_eq!(released.status, "pending");
    assert!(released.updated_at.is_some());
}

#[tokio::test]
async fn approval_create() {
    let db = connect().await;
    let (job_id, environment_id, user_id) = approval_fixture(&db).await;
    let inserted = ci_environment_ops::add_approval(&db, job_id, environment_id, user_id)
        .await
        .unwrap_or_else(|e| panic!("approval create: {e:#}"));
    assert!(inserted);
    assert!(
        !ci_environment_ops::add_approval(&db, job_id, environment_id, user_id)
            .await
            .unwrap(),
        "duplicate must be idempotent"
    );
    assert_eq!(
        ci_environment_ops::count_approvals(&db, job_id)
            .await
            .unwrap(),
        1
    );
    let approvals = ci_environment_ops::list_approvals(&db, job_id)
        .await
        .unwrap();
    assert_eq!(approvals.len(), 1);
    assert_eq!(
        (
            approvals[0].job_id,
            approvals[0].environment_id,
            approvals[0].approved_by
        ),
        (job_id, environment_id, user_id)
    );
    assert!(
        ci_environment_ops::add_approval(&db, i64::MAX, environment_id, user_id)
            .await
            .is_err(),
        "invalid FK or out-of-range job must fail"
    );
}

#[tokio::test]
async fn approval_read() {
    let db = connect().await;
    let (job_id, environment_id, user_id) = approval_fixture(&db).await;
    let backend = db.get_database_backend();
    let sql = format!("INSERT INTO ci_environment_approvals (job_id, environment_id, approved_by, created_at) VALUES ({job_id}, {environment_id}, {user_id}, CURRENT_TIMESTAMP)");
    db.execute(Statement::from_string(backend, sql))
        .await
        .expect("seed approval");
    let approvals = ci_environment_ops::list_approvals(&db, job_id)
        .await
        .unwrap_or_else(|e| panic!("approval read: {e:#}"));
    assert_eq!(approvals.len(), 1);
    assert_eq!(
        (
            approvals[0].job_id,
            approvals[0].environment_id,
            approvals[0].approved_by
        ),
        (job_id, environment_id, user_id)
    );
}

#[tokio::test]
async fn concurrent_duplicate_approvals_create_one_record() {
    let db = connect().await;
    let (job_id, environment_id, user_id) = approval_fixture(&db).await;
    let barrier = std::sync::Arc::new(tokio::sync::Barrier::new(2));
    let a = barrier.clone();
    let b = barrier.clone();
    let (first, second) = tokio::join!(
        async {
            a.wait().await;
            ci_environment_ops::add_approval(&db, job_id, environment_id, user_id).await
        },
        async {
            b.wait().await;
            ci_environment_ops::add_approval(&db, job_id, environment_id, user_id).await
        },
    );
    let mut outcomes = [first.unwrap(), second.unwrap()];
    outcomes.sort();
    assert_eq!(outcomes, [false, true]);
    assert_eq!(
        ci_environment_ops::count_approvals(&db, job_id)
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        ci_environment_ops::list_approvals(&db, job_id)
            .await
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn rerun_and_cancel_restore_then_transition_the_whole_chain() {
    let db = connect().await;
    let (_, repo_id) = fixture_repo(&db).await;
    let pipeline_id = fixture_pipeline(&db, repo_id).await;
    let stage = pipeline_ops::create_stage(&db, pipeline_id, &unique("rerun_stage"), 0)
        .await
        .unwrap();
    let job = pipeline_ops::create_job(
        &db,
        stage.id,
        &unique("rerun_job"),
        "true",
        None,
        None,
        None,
        None,
        None,
        false,
        None,
        None,
        None,
    )
    .await
    .unwrap();
    pipeline_ops::update_job_result(
        &db,
        job.id,
        "failed",
        Some(1),
        Some("failed log"),
        None,
        Some(Utc::now().naive_utc()),
    )
    .await
    .unwrap();
    pipeline_ops::update_stage_status(&db, stage.id, "failed", None, None)
        .await
        .unwrap();
    pipeline_ops::update_pipeline_status(&db, pipeline_id, "failed", None, None)
        .await
        .unwrap();

    pipeline_ops::rerun_failed_job(&db, job.id).await.unwrap();
    let retried = pipeline_ops::get_job(&db, job.id).await.unwrap().unwrap();
    assert_eq!(
        (retried.status.as_str(), retried.exit_code),
        ("pending", None)
    );
    assert_eq!(
        pipeline_ops::get_stage_by_id(&db, stage.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        "pending"
    );
    assert_eq!(
        pipeline_ops::get_pipeline(&db, pipeline_id)
            .await
            .unwrap()
            .unwrap()
            .status,
        "pending"
    );
    assert!(pipeline_ops::cancel_pipeline_chain(&db, pipeline_id)
        .await
        .unwrap());
    assert_eq!(
        pipeline_ops::get_job(&db, job.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        "canceled"
    );
    assert_eq!(
        pipeline_ops::get_stage_by_id(&db, stage.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        "canceled"
    );
    assert_eq!(
        pipeline_ops::get_pipeline(&db, pipeline_id)
            .await
            .unwrap()
            .unwrap()
            .status,
        "canceled"
    );
    assert!(!pipeline_ops::cancel_pipeline_chain(&db, pipeline_id)
        .await
        .unwrap());
}

#[tokio::test]
async fn disconnected_runner_fails_job_and_cascades_to_stage_and_pipeline() {
    let db = connect().await;
    let (_, repo_id) = fixture_repo(&db).await;
    let pipeline_id = fixture_pipeline(&db, repo_id).await;
    let stage = pipeline_ops::create_stage(&db, pipeline_id, &unique("offline_stage"), 0)
        .await
        .unwrap();
    let job = pipeline_ops::create_job(
        &db,
        stage.id,
        &unique("offline_job"),
        "true",
        None,
        None,
        None,
        None,
        None,
        false,
        None,
        None,
        None,
    )
    .await
    .unwrap();
    let runner_id = 5_000_000_000_i64 + job.id;
    pipeline_ops::assign_job(&db, job.id, runner_id)
        .await
        .unwrap();
    let failed = pipeline_ops::fail_runner_jobs(&db, runner_id)
        .await
        .unwrap();
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0].id, job.id);
    let result = pipeline_ops::get_job(&db, job.id).await.unwrap().unwrap();
    assert_eq!(
        (result.status.as_str(), result.exit_code),
        ("error", Some(-1))
    );
    assert_eq!(
        pipeline_ops::get_stage_by_id(&db, stage.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        "failed"
    );
    assert_eq!(
        pipeline_ops::get_pipeline(&db, pipeline_id)
            .await
            .unwrap()
            .unwrap()
            .status,
        "failed"
    );
    assert!(pipeline_ops::fail_runner_jobs(&db, runner_id)
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn legacy_integer_overflow_does_not_insert_partial_stage_or_job() {
    let db = connect().await;
    if db.get_database_backend() == DatabaseBackend::Sqlite {
        // SQLite's INTEGER is 64-bit, unlike these legacy 32-bit SQL columns.
        return;
    }
    let stage_name = unique("overflow_stage");
    assert!(pipeline_ops::create_stage(&db, i64::MAX, &stage_name, 0)
        .await
        .is_err());
    assert_eq!(
        pipeline_stage::Entity::find()
            .filter(pipeline_stage::Column::Name.eq(&stage_name))
            .count(&db)
            .await
            .unwrap(),
        0
    );

    let job_name = unique("overflow_job");
    assert!(pipeline_ops::create_job(
        &db,
        i64::MAX,
        &job_name,
        "true",
        None,
        None,
        None,
        None,
        None,
        false,
        None,
        None,
        None
    )
    .await
    .is_err());
    assert_eq!(
        pipeline_job::Entity::find()
            .filter(pipeline_job::Column::Name.eq(&job_name))
            .count(&db)
            .await
            .unwrap(),
        0
    );
}

/// PostgreSQL `updated_at` is TIMESTAMPTZ while the public model represents
/// UTC as a NaiveDateTime. Use one connection so SET TIME ZONE applies to all
/// operations, then verify write/read independently of the server session zone.
#[tokio::test]
async fn postgres_assignment_preserves_utc_in_non_utc_session() {
    let Ok(url) = std::env::var("IRONFORGE_TEST_DATABASE_URL") else {
        return;
    };
    if !url.starts_with("postgres:") && !url.starts_with("postgresql:") {
        return;
    }
    let db = rg_db::connect_with_pool(&url, 20, 60, 1).await.unwrap();
    db.execute(Statement::from_string(
        DatabaseBackend::Postgres,
        "SET TIME ZONE 'Asia/Shanghai'".to_string(),
    ))
    .await
    .unwrap();
    let (stage_id, _) = seed_stage(&db).await;
    let id = seed_job(&db, stage_id).await;
    let before = Utc::now().naive_utc();
    pipeline_ops::assign_job(&db, id, 4_294_967_300)
        .await
        .unwrap();
    let after = Utc::now().naive_utc();
    let stored = pipeline_ops::get_job(&db, id)
        .await
        .unwrap()
        .unwrap()
        .updated_at
        .unwrap();
    assert!(
        stored >= before - chrono::Duration::seconds(1)
            && stored <= after + chrono::Duration::seconds(1),
        "not UTC: before={before} stored={stored} after={after}"
    );
}
