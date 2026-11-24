//! `GET /ai/repos/{o}/{n}/pulls/{number}/context`: one call with what an agent
//! needs to pick up a pull request, cut to a size budget in a fixed priority.

mod common;
use common::{register_user, spawn_test_app_on_env_db};

use reqwest::{Client, StatusCode};
use serde_json::{json, Value};

const PW: &str = "Qz7$wRtm";

struct Fixture {
    base: String,
    jwt: String,
    owner: &'static str,
    client: Client,
    head_sha: String,
}

impl Fixture {
    fn url(&self, suffix: &str) -> String {
        format!("{}/api/v1/repos/{}/work{suffix}", self.base, self.owner)
    }

    async fn post(&self, suffix: &str, body: Value) -> (StatusCode, Value) {
        let resp = self
            .client
            .post(self.url(suffix))
            .bearer_auth(&self.jwt)
            .json(&body)
            .send()
            .await
            .unwrap();
        let status = resp.status();
        (status, resp.json().await.unwrap_or(Value::Null))
    }

    async fn context(&self, bearer: &str, query: &str) -> (StatusCode, Value) {
        let resp = self
            .client
            .get(format!(
                "{}/api/v1/ai/repos/{}/work/pulls/1/context{query}",
                self.base, self.owner
            ))
            .bearer_auth(bearer)
            .send()
            .await
            .unwrap();
        let status = resp.status();
        (status, resp.json().await.unwrap_or(Value::Null))
    }
}

/// A repository with a pull request from `agent/x` (two changed files), one
/// failed pipeline for its head commit, two review threads (one resolved) and a
/// review.
async fn fixture(owner: &'static str) -> Fixture {
    let (base, db) = spawn_test_app_on_env_db().await;
    let jwt = register_user(&base, owner, &format!("{owner}@example.com"), PW).await;
    let client = Client::new();
    let mut f = Fixture {
        base,
        jwt,
        owner,
        client,
        head_sha: String::new(),
    };
    let resp = f
        .client
        .post(format!("{}/api/v1/repos", f.base))
        .bearer_auth(&f.jwt)
        .json(&json!({"name": "work", "auto_init": true, "is_private": false}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);
    let repo_id = resp.json::<Value>().await.unwrap()["id"].as_i64().unwrap();

    for (path, content) in [("one.txt", "1\n"), ("two.txt", "2\n")] {
        let (status, v) = f
            .post(
                &format!("/contents/{path}"),
                json!({"branch": "agent/x", "message": format!("add {path}"), "content": content}),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{v}");
        f.head_sha = v["commit_sha"].as_str().unwrap().to_string();
    }
    let (status, v) = f
        .post(
            "/pulls",
            json!({"title": "Add two files", "body": "Please look at the details.", "head": "agent/x", "base": "main"}),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{v}");

    // CI: one failed job for the head commit.
    let pipeline = rg_db::ops::pipeline_ops::create_pipeline(
        &db,
        repo_id,
        &f.head_sha,
        "refs/heads/agent/x",
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
        None,
    )
    .await
    .unwrap();
    rg_db::ops::pipeline_ops::update_job_result(
        &db,
        job.id,
        "failed",
        Some(101),
        Some("compiling\nerror[E0308]: mismatched types\n"),
        None,
        None,
    )
    .await
    .unwrap();
    rg_db::ops::pipeline_ops::update_pipeline_status(&db, pipeline.id, "failed", None, None)
        .await
        .unwrap();

    // Two review threads; the first is resolved.
    let mut ids = Vec::new();
    for (path, text) in [("one.txt", "first thread"), ("two.txt", "second thread")] {
        let (status, v) = f
            .post(
                "/pulls/1/comments",
                json!({"path": path, "line": 1, "body": text}),
            )
            .await;
        assert!(status.is_success(), "{status} {v}");
        ids.push(v["id"].as_i64().unwrap());
    }
    let resp = f
        .client
        .patch(f.url(&format!("/pulls/1/comments/{}/resolution", ids[0])))
        .bearer_auth(&f.jwt)
        .json(&json!({"resolved": true}))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success(), "{}", resp.status());
    f
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_context_has_ci_threads_files_and_marks_user_text() {
    let f = fixture("ctx_a").await;
    let (status, v) = f.context(&f.jwt, "").await;
    assert_eq!(status, StatusCode::OK, "{v}");

    assert_eq!(v["pull_request"]["number"], 1);
    assert_eq!(v["pull_request"]["head_branch"], "agent/x");
    assert_eq!(v["pull_request"]["head_sha"], json!(f.head_sha));
    assert_eq!(v["pull_request"]["title_origin"], "user_content");
    assert_eq!(v["description"]["origin"], "user_content");
    assert_eq!(v["description"]["text"], "Please look at the details.");

    // CI: the failure summary is in, with what the log said.
    assert_eq!(v["ci"]["pipeline"]["status"], "failed");
    assert_eq!(v["ci"]["failed_jobs"][0]["name"], "unit");
    assert_eq!(
        v["ci"]["failed_jobs"][0]["failure_summary"]["error_lines"][0],
        "error[E0308]: mismatched types"
    );
    assert_eq!(v["ci"]["failed_jobs"][0]["origin"], "user_content");

    // Threads: the open one first-class, the resolved one separate.
    let open = v["unresolved_comments"].as_array().unwrap();
    assert_eq!(open.len(), 1);
    assert_eq!(open[0]["body"], "second thread");
    assert_eq!(open[0]["origin"], "user_content");
    let done = v["resolved_comments"].as_array().unwrap();
    assert_eq!(done.len(), 1);
    assert_eq!(done[0]["body"], "first thread");

    let files = v["changed_files"].as_array().unwrap();
    let paths: Vec<&str> = files.iter().map(|f| f["path"].as_str().unwrap()).collect();
    assert!(
        paths.contains(&"one.txt") && paths.contains(&"two.txt"),
        "{paths:?}"
    );
    assert_eq!(v["counts"]["changed_files"], 2);
    assert_eq!(v["counts"]["unresolved_comments"], 1);
    assert_eq!(v["counts"]["resolved_comments"], 1);

    // Each not-evaluated item names the endpoint that does evaluate it.
    let not_evaluated: Vec<&str> = v["not_evaluated"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item.as_str().unwrap())
        .collect();
    for what in ["mergeable", "branch_protection"] {
        assert!(
            not_evaluated
                .iter()
                .any(|item| item.starts_with(&format!("{what} ← GET "))
                    && item.ends_with("/pulls/1/merge-readiness")),
            "{what} must point at the merge-readiness endpoint: {not_evaluated:?}"
        );
    }
    assert!(v["diff_url"].as_str().unwrap().ends_with("/pulls/1/diff"));
    assert_eq!(v["truncated"], json!([]));
    assert_eq!(v["budget"]["requested_tokens"], 6000);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_small_budget_cuts_the_least_important_sections_first() {
    let f = fixture("ctx_b").await;
    // A budget so small that only the always-included part and the CI failure fit.
    let (status, v) = f.context(&f.jwt, "?budget=500").await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(v["pull_request"]["number"], 1, "the core is never cut");
    let truncated: Vec<&str> = v["truncated"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t.as_str().unwrap())
        .collect();
    // The CI failure outranks everything else, so if anything survived it is that;
    // the resolved comments are the first to go.
    assert!(
        truncated.iter().any(|t| t.starts_with("resolved_comments")),
        "{truncated:?}"
    );
    if v.get("ci").is_none() {
        // Too small even for CI: then nothing below it fit either.
        assert!(
            truncated.iter().any(|t| t.starts_with("ci")),
            "{truncated:?}"
        );
        assert!(v.get("unresolved_comments").is_none() || v["unresolved_comments"] == json!([]));
    }
    // The estimate stays within what was asked (the always-included part counts).
    let used = v["budget"]["estimated_tokens_used"].as_u64().unwrap();
    assert!(
        used <= 600,
        "estimated {used} tokens for a 500-token budget"
    );

    // Counts always tell the whole story, whatever was cut.
    assert_eq!(v["counts"]["resolved_comments"], 1);
    assert_eq!(v["counts"]["changed_files"], 2);

    // A bigger budget has the same sections whole and nothing cut.
    let (_, full) = f.context(&f.jwt, "?budget=20000").await;
    assert_eq!(full["truncated"], json!([]));
    assert_eq!(full["resolved_comments"].as_array().unwrap().len(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bad_budgets_and_unknown_pull_requests_are_refused() {
    let f = fixture("ctx_c").await;
    for q in ["?budget=0", "?budget=499", "?budget=50001"] {
        let (status, v) = f.context(&f.jwt, q).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{q}: {v}");
    }
    let resp = f
        .client
        .get(format!(
            "{}/api/v1/ai/repos/{}/work/pulls/99/context",
            f.base, f.owner
        ))
        .bearer_auth(&f.jwt)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_private_repository_keeps_its_pull_requests_to_those_who_can_read() {
    let (base, _db) = spawn_test_app_on_env_db().await;
    let owner = "ctx_d";
    let jwt = register_user(&base, owner, "ctx_d@example.com", PW).await;
    let client = Client::new();
    let resp = client
        .post(format!("{base}/api/v1/repos"))
        .bearer_auth(&jwt)
        .json(&json!({"name": "work", "auto_init": true, "is_private": true}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);
    let stranger = register_user(&base, "ctx_stranger", "ctx_stranger@example.com", PW).await;
    let status = client
        .get(format!(
            "{base}/api/v1/ai/repos/{owner}/work/pulls/1/context"
        ))
        .bearer_auth(&stranger)
        .send()
        .await
        .unwrap()
        .status();
    assert!(
        status == StatusCode::NOT_FOUND || status == StatusCode::FORBIDDEN,
        "{status}"
    );
}
