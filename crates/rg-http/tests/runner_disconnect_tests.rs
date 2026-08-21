//! Q3.2 — Runner heartbeat & disconnect handling.
//!
//! Semantics (decision): when a runner misses heartbeats (or deregisters),
//! its active jobs are marked **failed** — not re-queued — to avoid
//! duplicate execution. Stage/pipeline statuses cascade like a normal
//! job failure.

mod common;

use common::{register_full, spawn_test_app_with_db};
use sea_orm::{ActiveModelTrait, Set};

/// Fixture: repo row + placeholder sha (heartbeat semantics tests only touch
/// DB rows, not git objects; avoids auto_init whose git push is broken by
/// Windows `\\?\` temp paths — pre-existing environment issue).
async fn setup_repo_with_sha(base: &str, token: &str, repo_name: &str) -> (i64, String) {
    let client = reqwest::Client::new();
    let created = client
        .post(format!("{base}/api/v1/repos"))
        .bearer_auth(token)
        .json(&serde_json::json!({"name": repo_name}))
        .send()
        .await
        .unwrap();
    let status = created.status();
    let created_body: serde_json::Value = created.json().await.unwrap();
    assert_eq!(status, 201, "create_repo failed: {created_body}");
    let repo_id = created_body["id"].as_i64().unwrap();
    (repo_id, "0123456789abcdef0123456789abcdef01234567".to_string())
}

#[tokio::test]
async fn offline_runner_jobs_marked_failed_with_cascade() {
    let (base, db) = spawn_test_app_with_db().await;
    let (token, _) = register_full(&base, "disc_owner", "disc_owner@example.com").await;
    let (repo_id, sha) = setup_repo_with_sha(&base, &token, "disc-repo").await;

    // Online runner holding an assigned job
    let runner =
        rg_db::ops::runner_ops::register_runner(&db, "disc-runner", "[]", None, None, None)
            .await
            .unwrap();
    rg_db::ops::runner_ops::update_status(&db, runner.id, "online")
        .await
        .unwrap();

    let pipeline = rg_db::ops::pipeline_ops::create_pipeline(
        &db,
        repo_id,
        &sha,
        "refs/heads/main",
        "manual",
        None,
    )
    .await
    .unwrap();
    let stage = rg_db::ops::pipeline_ops::create_stage(&db, pipeline.id, "build", 0)
        .await
        .unwrap();
    let job = rg_db::ops::pipeline_ops::create_job(
        &db,
        stage.id,
        "compile",
        "echo hi",
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
    rg_db::ops::pipeline_ops::assign_job(&db, job.id, runner.id)
        .await
        .unwrap();

    // Simulate heartbeat loss: back-date last_seen beyond the 90s threshold
    let runner_id = runner.id;
    let mut stale: rg_db::entities::runner::ActiveModel = runner.into();
    stale.last_seen_at = Set(chrono::Utc::now() - chrono::Duration::seconds(300));
    stale.update(&db).await.unwrap();

    // Watchdog detection query finds the runner
    let offline = rg_db::ops::pipeline_ops::find_offline_runners(&db, 90)
        .await
        .unwrap();
    assert!(offline.iter().any(|r| r.id == runner_id));

    // Watchdog action: active jobs are marked failed (not re-queued)
    let failed = rg_db::ops::pipeline_ops::fail_runner_jobs(&db, runner_id)
        .await
        .unwrap();
    assert_eq!(failed.len(), 1);

    let job_after = rg_db::ops::pipeline_ops::get_job(&db, job.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(job_after.status, "error");
    assert_ne!(job_after.status, "pending");
    assert_eq!(job_after.exit_code, Some(-1));
    assert!(job_after.finished_at.is_some());

    // Stage and pipeline cascade to failed
    let stage_after = rg_db::ops::pipeline_ops::get_stage_by_id(&db, stage.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stage_after.status, "failed");

    let pipeline_after = rg_db::ops::pipeline_ops::get_pipeline(&db, pipeline.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(pipeline_after.status, "failed");
}

#[tokio::test]
async fn fresh_heartbeat_runner_not_offline() {
    let (_base, db) = spawn_test_app_with_db().await;
    let runner = rg_db::ops::runner_ops::register_runner(&db, "hb-runner", "[]", None, None, None)
        .await
        .unwrap();
    rg_db::ops::runner_ops::update_status(&db, runner.id, "online")
        .await
        .unwrap();
    rg_db::ops::runner_ops::update_heartbeat(&db, runner.id)
        .await
        .unwrap();

    let offline = rg_db::ops::pipeline_ops::find_offline_runners(&db, 90)
        .await
        .unwrap();
    assert!(!offline.iter().any(|r| r.id == runner.id));

    // No active jobs → nothing to fail
    let failed = rg_db::ops::pipeline_ops::fail_runner_jobs(&db, runner.id)
        .await
        .unwrap();
    assert!(failed.is_empty());
}

#[tokio::test]
async fn deregister_marks_active_jobs_failed() {
    let (base, db) = spawn_test_app_with_db().await;
    let (token, _) = register_full(&base, "dereg_owner", "dereg_owner@example.com").await;
    let (repo_id, sha) = setup_repo_with_sha(&base, &token, "dereg-repo").await;

    let runner =
        rg_db::ops::runner_ops::register_runner(&db, "dereg-runner", "[]", None, None, None)
            .await
            .unwrap();

    let pipeline = rg_db::ops::pipeline_ops::create_pipeline(
        &db,
        repo_id,
        &sha,
        "refs/heads/main",
        "manual",
        None,
    )
    .await
    .unwrap();
    let stage = rg_db::ops::pipeline_ops::create_stage(&db, pipeline.id, "test", 0)
        .await
        .unwrap();
    let job = rg_db::ops::pipeline_ops::create_job(
        &db,
        stage.id,
        "lint",
        "echo lint",
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
    rg_db::ops::pipeline_ops::assign_job(&db, job.id, runner.id)
        .await
        .unwrap();

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{base}/api/v1/runners/{}/deregister", runner.id))
        .bearer_auth(&runner.token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    // Job must be marked failed, never re-queued
    let job_after = rg_db::ops::pipeline_ops::get_job(&db, job.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(job_after.status, "error");
    assert_ne!(job_after.status, "pending");
    assert_eq!(job_after.exit_code, Some(-1));
}
