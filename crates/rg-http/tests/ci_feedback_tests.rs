//! CI feedback endpoints for scripted and agent clients: incremental job logs,
//! change-aware `wait`, and the failure summary on failed jobs.

mod common;

use common::{create_repo, register_bootstrap_admin, register_full, spawn_test_app_with_db};
use std::time::{Duration, Instant};

struct Fixture {
    base: String,
    db: rg_db::DatabaseConnection,
    token: String,
    pipeline_id: i64,
    job_id: i64,
}

impl Fixture {
    fn job_url(&self, suffix: &str) -> String {
        format!(
            "{}/api/v1/repos/feedback_owner/feedback-repo/pipelines/{}/jobs/{}{}",
            self.base, self.pipeline_id, self.job_id, suffix
        )
    }

    fn wait_url(&self, query: &str) -> String {
        format!(
            "{}/api/v1/repos/feedback_owner/feedback-repo/pipelines/{}/wait{}",
            self.base, self.pipeline_id, query
        )
    }

    async fn get(&self, url: &str) -> serde_json::Value {
        let resp = reqwest::Client::new()
            .get(url)
            .bearer_auth(&self.token)
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200, "GET {url}");
        resp.json().await.unwrap()
    }
}

async fn fixture(job_status: &str, exit_code: Option<i32>, log: Option<&str>) -> Fixture {
    let (base, db) = spawn_test_app_with_db().await;
    register_bootstrap_admin(&base).await;
    let (token, _) = register_full(&base, "feedback_owner", "feedback_owner@example.com").await;
    let repo_id = create_repo(&base, &token, "feedback-repo").await;

    let pipeline = rg_db::ops::pipeline_ops::create_pipeline(
        &db,
        repo_id,
        "0123456789012345678901234567890123456789",
        "refs/heads/main",
        "push",
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
        "unit",
        "cargo test",
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
    rg_db::ops::pipeline_ops::update_job_result(
        &db, job.id, job_status, exit_code, log, None, None,
    )
    .await
    .unwrap();

    Fixture {
        base,
        db,
        token,
        pipeline_id: pipeline.id,
        job_id: job.id,
    }
}

#[tokio::test]
async fn failed_job_carries_a_bounded_failure_summary() {
    let mut log = String::from("\u{1b}[31merror[E0308]\u{1b}[0m: mismatched types\n");
    for n in 0..120 {
        log.push_str(&format!("compiling step {n}\n"));
    }
    let f = fixture("failed", Some(101), Some(&log)).await;

    let job = f.get(&f.job_url("")).await;
    let summary = &job["failure_summary"];
    assert_eq!(summary["exit_code"], 101);
    assert_eq!(summary["tail"].as_array().unwrap().len(), 50);
    assert_eq!(summary["tail"][49], "compiling step 119");
    assert_eq!(summary["truncated"], true);
    // The error is far above the tail, so only the error-line scan finds it.
    assert_eq!(summary["error_lines"][0], "error[E0308]: mismatched types");

    // The same summary is on the pipeline detail response.
    let detail = f
        .get(&format!(
            "{}/api/v1/repos/feedback_owner/feedback-repo/pipelines/{}",
            f.base, f.pipeline_id
        ))
        .await;
    let detail_job = &detail["stages"][0]["jobs"][0];
    assert_eq!(detail_job["failure_summary"]["exit_code"], 101);
}

#[tokio::test]
async fn successful_job_has_no_failure_summary() {
    let f = fixture("success", Some(0), Some("all good\n")).await;
    let job = f.get(&f.job_url("")).await;
    assert!(job.get("failure_summary").is_none(), "{job}");
}

#[tokio::test]
async fn log_can_be_followed_by_offset_and_reassembles_exactly() {
    let log = "first line\nsecond line\n日本語 third\nfourth\n";
    let f = fixture("success", Some(0), Some(log)).await;

    let mut assembled = String::new();
    let mut offset = 0u64;
    for _ in 0..64 {
        let page = f
            .get(&f.job_url(&format!("/log?offset={offset}&limit=5")))
            .await;
        assert_eq!(page["offset"], offset);
        assembled.push_str(page["text"].as_str().unwrap());
        offset = page["next_offset"].as_u64().unwrap();
        if page["eof"] == true {
            break;
        }
    }
    assert_eq!(assembled, log);
    assert_eq!(offset as usize, log.len());
}

#[tokio::test]
async fn log_eof_is_false_while_the_job_is_still_running() {
    let f = fixture("running", None, Some("partial output\n")).await;
    let page = f.get(&f.job_url("/log")).await;
    assert_eq!(page["text"], "partial output\n");
    assert_eq!(page["next_offset"], page["total_bytes"]);
    assert_eq!(page["eof"], false, "a running job may still append");

    rg_db::ops::pipeline_ops::update_job_result(
        &f.db,
        f.job_id,
        "success",
        Some(0),
        Some("partial output\nmore output\n"),
        None,
        None,
    )
    .await
    .unwrap();
    let resumed = f
        .get(&f.job_url(&format!("/log?offset={}", page["next_offset"])))
        .await;
    assert_eq!(resumed["text"], "more output\n");
    assert_eq!(resumed["eof"], true);
}

#[tokio::test]
async fn log_offset_past_the_end_signals_a_reset() {
    // A rerun replaces the log with a shorter one; a stale offset must not
    // silently return nothing.
    let f = fixture("success", Some(0), Some("short\n")).await;
    let page = f.get(&f.job_url("/log?offset=9999")).await;
    assert_eq!(page["reset"], true);
    assert_eq!(page["offset"], 0);
    assert_eq!(page["text"], "short\n");
}

#[tokio::test]
async fn log_tail_returns_the_last_lines_and_can_strip_ansi() {
    let f = fixture("failed", Some(1), Some("a\nb\n\u{1b}[31mc\u{1b}[0m\nd\n")).await;

    let tail = f.get(&f.job_url("/log?tail=2&strip_ansi=true")).await;
    assert_eq!(tail["text"], "c\nd\n");
    assert_eq!(tail["next_offset"], tail["total_bytes"]);

    let raw = f.get(&f.job_url("/log?tail=2")).await;
    assert!(raw["text"].as_str().unwrap().contains('\u{1b}'));
}

#[tokio::test]
async fn log_rejects_offset_combined_with_tail() {
    let f = fixture("success", Some(0), Some("x\n")).await;
    let resp = reqwest::Client::new()
        .get(f.job_url("/log?offset=1&tail=1"))
        .bearer_auth(&f.token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 400);
}

#[tokio::test]
async fn log_and_wait_enforce_repository_read_access() {
    let (base, db) = spawn_test_app_with_db().await;
    register_bootstrap_admin(&base).await;
    let (owner, _) = register_full(&base, "feedback_owner", "feedback_owner@example.com").await;
    let (other, _) = register_full(&base, "feedback_other", "feedback_other@example.com").await;
    let client = reqwest::Client::new();
    let created = client
        .post(format!("{base}/api/v1/repos"))
        .bearer_auth(&owner)
        .json(&serde_json::json!({"name": "feedback-repo", "is_private": true}))
        .send()
        .await
        .unwrap();
    assert_eq!(created.status(), 201);
    let repo_id = created.json::<serde_json::Value>().await.unwrap()["id"]
        .as_i64()
        .unwrap();
    let pipeline = rg_db::ops::pipeline_ops::create_pipeline(
        &db,
        repo_id,
        "0123456789012345678901234567890123456789",
        "refs/heads/main",
        "push",
        None,
    )
    .await
    .unwrap();
    let stage = rg_db::ops::pipeline_ops::create_stage(&db, pipeline.id, "test", 0)
        .await
        .unwrap();
    let job = rg_db::ops::pipeline_ops::create_job(
        &db, stage.id, "unit", "x", None, None, None, None, None, false, None, None, None,
    )
    .await
    .unwrap();

    let prefix = format!(
        "{base}/api/v1/repos/feedback_owner/feedback-repo/pipelines/{}",
        pipeline.id
    );
    for url in [
        format!("{prefix}/jobs/{}/log", job.id),
        format!("{prefix}/wait"),
    ] {
        let anon = client.get(&url).send().await.unwrap();
        assert_eq!(anon.status(), 401, "{url}");
        let stranger = client.get(&url).bearer_auth(&other).send().await.unwrap();
        assert_eq!(stranger.status(), 403, "{url}");
        let allowed = client.get(&url).bearer_auth(&owner).send().await.unwrap();
        assert_eq!(allowed.status(), 200, "{url}");
    }
}

#[tokio::test]
async fn log_of_a_job_in_another_pipeline_is_not_found() {
    let f = fixture("success", Some(0), Some("x\n")).await;
    let other_pipeline = rg_db::ops::pipeline_ops::create_pipeline(
        &f.db,
        // Same repository: the first fixture's repo id is the only one.
        rg_db::ops::pipeline_ops::get_pipeline(&f.db, f.pipeline_id)
            .await
            .unwrap()
            .unwrap()
            .repo_id,
        "abcdefabcdefabcdefabcdefabcdefabcdefabcd",
        "refs/heads/main",
        "push",
        None,
    )
    .await
    .unwrap();
    let resp = reqwest::Client::new()
        .get(format!(
            "{}/api/v1/repos/feedback_owner/feedback-repo/pipelines/{}/jobs/{}/log",
            f.base, other_pipeline.id, f.job_id
        ))
        .bearer_auth(&f.token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn wait_without_an_etag_returns_the_current_state_immediately() {
    let f = fixture("running", None, None).await;
    let started = Instant::now();
    let state = f.get(&f.wait_url("")).await;
    assert!(started.elapsed() < Duration::from_millis(900));
    assert_eq!(state["changed"], true);
    assert_eq!(state["timed_out"], false);
    assert_eq!(state["jobs"][0]["status"], "running");
    assert!(state["etag"].as_str().unwrap().len() >= 8);
}

#[tokio::test]
async fn wait_times_out_when_nothing_changes() {
    let f = fixture("running", None, None).await;
    let etag = f.get(&f.wait_url("")).await["etag"]
        .as_str()
        .unwrap()
        .to_string();

    let started = Instant::now();
    let state = f.get(&f.wait_url(&format!("?etag={etag}&timeout=1"))).await;
    assert!(started.elapsed() >= Duration::from_millis(900));
    assert_eq!(state["changed"], false);
    assert_eq!(state["timed_out"], true);
    assert_eq!(state["etag"], etag);
}

#[tokio::test]
async fn wait_returns_early_when_a_job_status_changes() {
    let f = fixture("running", None, None).await;
    let etag = f.get(&f.wait_url("")).await["etag"]
        .as_str()
        .unwrap()
        .to_string();

    let db = f.db.clone();
    let job_id = f.job_id;
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(300)).await;
        rg_db::ops::pipeline_ops::update_job_result(
            &db,
            job_id,
            "failed",
            Some(1),
            Some("error: boom\n"),
            None,
            None,
        )
        .await
        .unwrap();
    });

    let started = Instant::now();
    let state = f
        .get(&f.wait_url(&format!("?etag={etag}&timeout=20")))
        .await;
    assert!(
        started.elapsed() < Duration::from_secs(8),
        "must wake on the change, not run out the timeout"
    );
    assert_eq!(state["changed"], true);
    assert_eq!(state["timed_out"], false);
    assert_eq!(state["jobs"][0]["status"], "failed");
    assert_eq!(
        state["jobs"][0]["failure_summary"]["error_lines"][0],
        "error: boom"
    );
    assert_ne!(state["etag"], etag);
}

#[tokio::test]
async fn wait_until_terminal_ignores_intermediate_changes() {
    let f = fixture("pending", None, None).await;
    rg_db::ops::pipeline_ops::update_pipeline_status(&f.db, f.pipeline_id, "running", None, None)
        .await
        .unwrap();
    let etag = f.get(&f.wait_url("")).await["etag"]
        .as_str()
        .unwrap()
        .to_string();

    let db = f.db.clone();
    let (pipeline_id, job_id) = (f.pipeline_id, f.job_id);
    tokio::spawn(async move {
        // An intermediate change first: must not release the waiter.
        tokio::time::sleep(Duration::from_millis(300)).await;
        rg_db::ops::pipeline_ops::update_job_result(&db, job_id, "running", None, None, None, None)
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(1500)).await;
        rg_db::ops::pipeline_ops::update_job_result(
            &db,
            job_id,
            "success",
            Some(0),
            None,
            None,
            None,
        )
        .await
        .unwrap();
        rg_db::ops::pipeline_ops::update_pipeline_status(&db, pipeline_id, "success", None, None)
            .await
            .unwrap();
    });

    let state = f
        .get(&f.wait_url(&format!("?etag={etag}&until=terminal&timeout=20")))
        .await;
    assert_eq!(state["terminal"], true);
    assert_eq!(state["timed_out"], false);
    assert_eq!(state["pipeline"]["status"], "success");
}

#[tokio::test]
async fn wait_rejects_an_unknown_until_value() {
    let f = fixture("running", None, None).await;
    let resp = reqwest::Client::new()
        .get(f.wait_url("?until=never"))
        .bearer_auth(&f.token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 400);
}
