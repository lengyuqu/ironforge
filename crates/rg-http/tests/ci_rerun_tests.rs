//! CI-201: rerun a single failed/errored/canceled job.
//!
//! Covers the HTTP contract (auth, ownership, state guard) and, more
//! importantly, the pipeline state machine afterwards: a rerun must put the
//! failed job, its stage and the pipeline back into a runnable state, rebuild
//! the downstream stages that were skipped because of the failure, and never
//! let the pipeline report success while those stages have not run.

mod common;

use common::{register_full, spawn_test_app_with_db};
use rg_db::ops::pipeline_ops as ops;
use rg_db::DatabaseConnection;

const SHA: &str = "0123456789012345678901234567890123456789";

struct Seed {
    pipeline_id: i64,
    stages: Vec<i64>,
    /// `jobs[stage_index][job_index]`
    jobs: Vec<Vec<i64>>,
}

async fn create_repo(base: &str, token: &str, name: &str) -> i64 {
    let resp = reqwest::Client::new()
        .post(format!("{base}/api/v1/repos"))
        .bearer_auth(token)
        .json(&serde_json::json!({"name": name, "is_private": true}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201, "create repo failed");
    resp.json::<serde_json::Value>().await.unwrap()["id"]
        .as_i64()
        .unwrap()
}

/// One pipeline with the given stage layout, e.g. `&[&["build"], &["test", "lint"]]`.
async fn seed(db: &DatabaseConnection, repo_id: i64, layout: &[&[&str]]) -> Seed {
    let pipeline = ops::create_pipeline(db, repo_id, SHA, "refs/heads/main", "manual", None)
        .await
        .unwrap();
    let (mut stages, mut jobs) = (Vec::new(), Vec::new());
    for (order, names) in layout.iter().enumerate() {
        let stage = ops::create_stage(db, pipeline.id, &format!("stage-{order}"), order as i32)
            .await
            .unwrap();
        let mut ids = Vec::new();
        for name in *names {
            let job = ops::create_job(
                db, stage.id, name, "echo hi", None, None, None, None, None, false, None, None,
                None,
            )
            .await
            .unwrap();
            ids.push(job.id);
        }
        stages.push(stage.id);
        jobs.push(ids);
    }
    Seed {
        pipeline_id: pipeline.id,
        stages,
        jobs,
    }
}

/// Finish a job the way the runner does, then let the stage/pipeline settle.
async fn finish_job(db: &DatabaseConnection, job_id: i64, status: &str) {
    let now = Some(chrono::Utc::now().naive_utc());
    let exit = Some(if status == "success" { 0 } else { 1 });
    ops::update_job_result(db, job_id, status, exit, Some("log"), now, now)
        .await
        .unwrap();
    let job = ops::get_job(db, job_id).await.unwrap().unwrap();
    let stage_id = job.stage_id;
    if ops::try_update_stage(db, stage_id).await.unwrap().is_some() {
        let stage = ops::get_stage_by_id(db, stage_id).await.unwrap().unwrap();
        ops::try_update_pipeline(db, stage.pipeline_id)
            .await
            .unwrap();
    }
}

async fn job_status(db: &DatabaseConnection, id: i64) -> String {
    ops::get_job(db, id).await.unwrap().unwrap().status
}

async fn stage_status(db: &DatabaseConnection, id: i64) -> String {
    ops::get_stage_by_id(db, id).await.unwrap().unwrap().status
}

async fn pipeline_status(db: &DatabaseConnection, id: i64) -> String {
    ops::get_pipeline(db, id).await.unwrap().unwrap().status
}

async fn rerun(
    base: &str,
    token: Option<&str>,
    owner: &str,
    repo: &str,
    pid: i64,
    jid: i64,
) -> reqwest::Response {
    let mut req = reqwest::Client::new().post(format!(
        "{base}/api/v1/repos/{owner}/{repo}/pipelines/{pid}/jobs/{jid}/rerun"
    ));
    if let Some(t) = token {
        req = req.bearer_auth(t);
    }
    req.send().await.unwrap()
}

// ── HTTP contract ───────────────────────────────────────────────────────

#[tokio::test]
async fn rerun_requires_write_access() {
    let (base, db) = spawn_test_app_with_db().await;
    let (owner, _) = register_full(&base, "rr_owner", "rr_owner@example.com").await;
    let (other, _) = register_full(&base, "rr_other", "rr_other@example.com").await;
    let repo_id = create_repo(&base, &owner, "r").await;
    let s = seed(&db, repo_id, &[&["build"]]).await;
    finish_job(&db, s.jobs[0][0], "failed").await;

    let (pid, jid) = (s.pipeline_id, s.jobs[0][0]);
    assert_eq!(
        rerun(&base, None, "rr_owner", "r", pid, jid).await.status(),
        401
    );
    assert_eq!(
        rerun(&base, Some(&other), "rr_owner", "r", pid, jid)
            .await
            .status(),
        403
    );
    // Neither refused attempt touched the job.
    assert_eq!(job_status(&db, jid).await, "failed");

    assert_eq!(
        rerun(&base, Some(&owner), "rr_owner", "r", pid, jid)
            .await
            .status(),
        200
    );
    assert_eq!(job_status(&db, jid).await, "pending");
}

#[tokio::test]
async fn rerun_rejects_pipelines_and_jobs_from_elsewhere() {
    let (base, db) = spawn_test_app_with_db().await;
    let (owner, _) = register_full(&base, "rr_scope", "rr_scope@example.com").await;
    let repo_a = create_repo(&base, &owner, "a").await;
    let repo_b = create_repo(&base, &owner, "b").await;
    let a = seed(&db, repo_a, &[&["build"]]).await;
    let b = seed(&db, repo_b, &[&["build"]]).await;
    finish_job(&db, a.jobs[0][0], "failed").await;
    finish_job(&db, b.jobs[0][0], "failed").await;

    // Pipeline of another repo.
    let r = rerun(
        &base,
        Some(&owner),
        "rr_scope",
        "a",
        b.pipeline_id,
        b.jobs[0][0],
    )
    .await;
    assert_eq!(r.status(), 404);
    // Job of another pipeline under a valid pipeline id.
    let r = rerun(
        &base,
        Some(&owner),
        "rr_scope",
        "a",
        a.pipeline_id,
        b.jobs[0][0],
    )
    .await;
    assert_eq!(r.status(), 404);
    // Unknown ids.
    assert_eq!(
        rerun(&base, Some(&owner), "rr_scope", "a", 9999, a.jobs[0][0])
            .await
            .status(),
        404
    );
    assert_eq!(
        rerun(&base, Some(&owner), "rr_scope", "a", a.pipeline_id, 9999)
            .await
            .status(),
        404
    );

    assert_eq!(job_status(&db, a.jobs[0][0]).await, "failed");
    assert_eq!(job_status(&db, b.jobs[0][0]).await, "failed");
}

#[tokio::test]
async fn rerun_refuses_jobs_that_did_not_fail() {
    let (base, db) = spawn_test_app_with_db().await;
    let (owner, _) = register_full(&base, "rr_state", "rr_state@example.com").await;
    let repo_id = create_repo(&base, &owner, "r").await;
    let s = seed(&db, repo_id, &[&["ok", "running", "waiting"]]).await;
    ops::update_job_result(&db, s.jobs[0][0], "success", Some(0), None, None, None)
        .await
        .unwrap();
    ops::update_job_result(&db, s.jobs[0][1], "running", None, None, None, None)
        .await
        .unwrap();

    for (idx, expected) in [(0, "success"), (1, "running"), (2, "pending")] {
        let jid = s.jobs[0][idx];
        let r = rerun(&base, Some(&owner), "rr_state", "r", s.pipeline_id, jid).await;
        assert_eq!(
            r.status(),
            400,
            "job in state {expected} must not be rerunnable"
        );
        assert_eq!(
            job_status(&db, jid).await,
            expected,
            "refused rerun must not mutate the job"
        );
    }
}

// ── State machine ───────────────────────────────────────────────────────

#[tokio::test]
async fn rerun_resets_job_stage_and_pipeline_and_clears_run_data() {
    let (base, db) = spawn_test_app_with_db().await;
    let (owner, _) = register_full(&base, "rr_reset", "rr_reset@example.com").await;
    let repo_id = create_repo(&base, &owner, "r").await;
    let s = seed(&db, repo_id, &[&["build"]]).await;
    // Give the job a runner so we can prove the rerun releases it.
    let runner = rg_db::ops::runner_ops::register_runner(&db, "rr-runner", "[]", None, None, None)
        .await
        .unwrap();
    ops::assign_job(&db, s.jobs[0][0], runner.runner.id)
        .await
        .unwrap();
    assert!(ops::get_job(&db, s.jobs[0][0])
        .await
        .unwrap()
        .unwrap()
        .runner_id
        .is_some());
    finish_job(&db, s.jobs[0][0], "failed").await;
    assert_eq!(stage_status(&db, s.stages[0]).await, "failed");
    assert_eq!(pipeline_status(&db, s.pipeline_id).await, "failed");

    let r = rerun(
        &base,
        Some(&owner),
        "rr_reset",
        "r",
        s.pipeline_id,
        s.jobs[0][0],
    )
    .await;
    assert_eq!(r.status(), 200);
    let body: serde_json::Value = r.json().await.unwrap();
    assert_eq!(body["status"], "pending");

    let job = ops::get_job(&db, s.jobs[0][0]).await.unwrap().unwrap();
    assert_eq!(job.status, "pending");
    assert!(
        job.runner_id.is_none(),
        "a rerun job must be claimable by any runner"
    );
    assert!(job.exit_code.is_none());
    assert!(job.finished_at.is_none());
    assert_eq!(stage_status(&db, s.stages[0]).await, "pending");
    assert_eq!(pipeline_status(&db, s.pipeline_id).await, "pending");
    assert!(ops::get_stage_by_id(&db, s.stages[0])
        .await
        .unwrap()
        .unwrap()
        .finished_at
        .is_none());
    assert!(ops::get_pipeline(&db, s.pipeline_id)
        .await
        .unwrap()
        .unwrap()
        .finished_at
        .is_none());

    // And the scheduler can actually pick it up again.
    let next = ops::find_pending_job(&db)
        .await
        .unwrap()
        .expect("rerun job must be schedulable");
    assert_eq!(next.id, s.jobs[0][0]);
}

#[tokio::test]
async fn rerun_only_touches_the_chosen_job() {
    let (base, db) = spawn_test_app_with_db().await;
    let (owner, _) = register_full(&base, "rr_only", "rr_only@example.com").await;
    let repo_id = create_repo(&base, &owner, "r").await;
    let s = seed(&db, repo_id, &[&["lint", "test", "docs"]]).await;
    finish_job(&db, s.jobs[0][0], "success").await;
    finish_job(&db, s.jobs[0][1], "failed").await;
    finish_job(&db, s.jobs[0][2], "success").await;

    let r = rerun(
        &base,
        Some(&owner),
        "rr_only",
        "r",
        s.pipeline_id,
        s.jobs[0][1],
    )
    .await;
    assert_eq!(r.status(), 200);

    assert_eq!(
        job_status(&db, s.jobs[0][0]).await,
        "success",
        "finished sibling must not rerun"
    );
    assert_eq!(job_status(&db, s.jobs[0][1]).await, "pending");
    assert_eq!(
        job_status(&db, s.jobs[0][2]).await,
        "success",
        "finished sibling must not rerun"
    );

    // The rerun succeeds → stage and pipeline become green without re-running siblings.
    finish_job(&db, s.jobs[0][1], "success").await;
    assert_eq!(stage_status(&db, s.stages[0]).await, "success");
    assert_eq!(pipeline_status(&db, s.pipeline_id).await, "success");
}

#[tokio::test]
async fn stage_stays_failed_while_another_job_in_it_still_fails() {
    let (base, db) = spawn_test_app_with_db().await;
    let (owner, _) = register_full(&base, "rr_two", "rr_two@example.com").await;
    let repo_id = create_repo(&base, &owner, "r").await;
    let s = seed(&db, repo_id, &[&["a", "b"], &["deploy"]]).await;
    finish_job(&db, s.jobs[0][0], "failed").await;
    finish_job(&db, s.jobs[0][1], "failed").await;

    assert_eq!(
        rerun(
            &base,
            Some(&owner),
            "rr_two",
            "r",
            s.pipeline_id,
            s.jobs[0][0]
        )
        .await
        .status(),
        200
    );
    finish_job(&db, s.jobs[0][0], "success").await;

    // `b` is still failed, so the stage must fail again and deploy must not run.
    assert_eq!(stage_status(&db, s.stages[0]).await, "failed");
    assert_eq!(pipeline_status(&db, s.pipeline_id).await, "failed");
    assert!(ops::find_pending_job(&db).await.unwrap().is_none());
    assert_eq!(job_status(&db, s.jobs[1][0]).await, "skipped");
}

#[tokio::test]
async fn rerun_rebuilds_downstream_stages_skipped_by_the_failure() {
    let (base, db) = spawn_test_app_with_db().await;
    let (owner, _) = register_full(&base, "rr_down", "rr_down@example.com").await;
    let repo_id = create_repo(&base, &owner, "r").await;
    let s = seed(&db, repo_id, &[&["build"], &["test"], &["deploy"]]).await;
    finish_job(&db, s.jobs[0][0], "failed").await;
    // Precondition: the failure skipped everything downstream.
    assert_eq!(stage_status(&db, s.stages[1]).await, "skipped");
    assert_eq!(job_status(&db, s.jobs[1][0]).await, "skipped");
    assert_eq!(job_status(&db, s.jobs[2][0]).await, "skipped");

    assert_eq!(
        rerun(
            &base,
            Some(&owner),
            "rr_down",
            "r",
            s.pipeline_id,
            s.jobs[0][0]
        )
        .await
        .status(),
        200
    );

    // Downstream work is runnable again, in order.
    assert_eq!(stage_status(&db, s.stages[1]).await, "pending");
    assert_eq!(stage_status(&db, s.stages[2]).await, "pending");
    assert_eq!(job_status(&db, s.jobs[1][0]).await, "pending");
    assert_eq!(job_status(&db, s.jobs[2][0]).await, "pending");

    // Drive the pipeline to completion: each stage only becomes schedulable
    // after the previous one succeeded, and the pipeline is green only at the end.
    for (i, expected_next) in [(0usize, Some(1usize)), (1, Some(2)), (2, None)] {
        let pending = ops::find_pending_job(&db).await.unwrap().map(|j| j.id);
        assert_eq!(
            pending,
            Some(s.jobs[i][0]),
            "stage {i} must be the schedulable one"
        );
        assert_ne!(
            pipeline_status(&db, s.pipeline_id).await,
            "success",
            "green before stage {i} ran"
        );
        finish_job(&db, s.jobs[i][0], "success").await;
        assert_eq!(stage_status(&db, s.stages[i]).await, "success");
        let _ = expected_next;
    }
    assert_eq!(pipeline_status(&db, s.pipeline_id).await, "success");
}

#[tokio::test]
async fn pipeline_is_not_reported_green_when_the_rerun_job_keeps_failing() {
    let (base, db) = spawn_test_app_with_db().await;
    let (owner, _) = register_full(&base, "rr_again", "rr_again@example.com").await;
    let repo_id = create_repo(&base, &owner, "r").await;
    let s = seed(&db, repo_id, &[&["build"], &["test"]]).await;
    finish_job(&db, s.jobs[0][0], "failed").await;

    for _ in 0..2 {
        assert_eq!(
            rerun(
                &base,
                Some(&owner),
                "rr_again",
                "r",
                s.pipeline_id,
                s.jobs[0][0]
            )
            .await
            .status(),
            200
        );
        finish_job(&db, s.jobs[0][0], "failed").await;
        assert_eq!(pipeline_status(&db, s.pipeline_id).await, "failed");
        // Failing again re-skips what the rerun had re-opened.
        assert_eq!(stage_status(&db, s.stages[1]).await, "skipped");
        assert_eq!(job_status(&db, s.jobs[1][0]).await, "skipped");
    }
}

#[tokio::test]
async fn rerun_after_cancel_reopens_the_canceled_stages() {
    let (base, db) = spawn_test_app_with_db().await;
    let (owner, _) = register_full(&base, "rr_cancel", "rr_cancel@example.com").await;
    let repo_id = create_repo(&base, &owner, "r").await;
    let s = seed(&db, repo_id, &[&["build"], &["test"]]).await;
    assert!(ops::cancel_pipeline_chain(&db, s.pipeline_id)
        .await
        .unwrap());
    assert_eq!(job_status(&db, s.jobs[0][0]).await, "canceled");
    assert_eq!(job_status(&db, s.jobs[1][0]).await, "canceled");

    assert_eq!(
        rerun(
            &base,
            Some(&owner),
            "rr_cancel",
            "r",
            s.pipeline_id,
            s.jobs[0][0]
        )
        .await
        .status(),
        200
    );
    assert_eq!(pipeline_status(&db, s.pipeline_id).await, "pending");
    assert_eq!(stage_status(&db, s.stages[1]).await, "pending");
    assert_eq!(job_status(&db, s.jobs[1][0]).await, "pending");

    finish_job(&db, s.jobs[0][0], "success").await;
    assert_ne!(
        pipeline_status(&db, s.pipeline_id).await,
        "success",
        "test stage has not run yet"
    );
    finish_job(&db, s.jobs[1][0], "success").await;
    assert_eq!(pipeline_status(&db, s.pipeline_id).await, "success");
}

#[tokio::test]
async fn rerun_is_refused_when_an_earlier_stage_did_not_succeed() {
    let (base, db) = spawn_test_app_with_db().await;
    let (owner, _) = register_full(&base, "rr_order", "rr_order@example.com").await;
    let repo_id = create_repo(&base, &owner, "r").await;
    let s = seed(&db, repo_id, &[&["build"], &["test"]]).await;
    assert!(ops::cancel_pipeline_chain(&db, s.pipeline_id)
        .await
        .unwrap());

    // `test` can never be scheduled while `build` is canceled; rerunning it
    // would leave the pipeline pending forever.
    let r = rerun(
        &base,
        Some(&owner),
        "rr_order",
        "r",
        s.pipeline_id,
        s.jobs[1][0],
    )
    .await;
    assert_eq!(r.status(), 400);
    assert_eq!(job_status(&db, s.jobs[1][0]).await, "canceled");
    assert_eq!(pipeline_status(&db, s.pipeline_id).await, "canceled");
}

#[tokio::test]
async fn reopened_downstream_jobs_keep_their_gates() {
    let (base, db) = spawn_test_app_with_db().await;
    let (owner, _) = register_full(&base, "rr_gate", "rr_gate@example.com").await;
    let repo_id = create_repo(&base, &owner, "r").await;
    let s = seed(&db, repo_id, &[&["build"], &[]]).await;
    // Downstream stage with a manual job, a conditional job and a plain job.
    let mk = |name: &'static str, when: Option<&'static str>, cond: Option<&'static str>| {
        let db = db.clone();
        let stage = s.stages[1];
        async move {
            ops::create_job(
                &db, stage, name, "echo", None, None, None, None, None, false, None, when, cond,
            )
            .await
            .unwrap()
            .id
        }
    };
    let manual = mk("deploy-prod", Some("manual"), None).await;
    let conditional = mk("only-on-tag", None, Some("ref == 'refs/tags/v1'")).await;
    let plain = mk("test", None, None).await;
    assert_eq!(job_status(&db, manual).await, "manual");

    finish_job(&db, s.jobs[0][0], "failed").await;
    assert_eq!(job_status(&db, manual).await, "skipped");

    assert_eq!(
        rerun(
            &base,
            Some(&owner),
            "rr_gate",
            "r",
            s.pipeline_id,
            s.jobs[0][0]
        )
        .await
        .status(),
        200
    );
    // A manual job must wait for a click again, not start by itself.
    assert_eq!(job_status(&db, manual).await, "manual");
    // The plain job is runnable again.
    assert_eq!(job_status(&db, plain).await, "pending");
    // A skipped job with an `if:` is left alone: we cannot tell whether the
    // condition or the failure skipped it, and running it wrongly is worse.
    assert_eq!(job_status(&db, conditional).await, "skipped");
    assert_eq!(stage_status(&db, s.stages[1]).await, "pending");
}
