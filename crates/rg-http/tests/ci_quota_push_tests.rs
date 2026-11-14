//! The push path must tell CI **who** pushed, so the agent quota (R10, #40) can
//! limit agents without touching people.
//!
//! `rg-http` cannot depend on `rg-ci` (and `Cargo.*` is frozen for T05), so this
//! test proves the wiring with a recording `CiTrigger`: a REST file commit — the
//! same post-push hook a git push uses — reaches the engine with
//! `trigger_type = "push"` and `triggered_by = <pusher>`. What the engine then
//! does with an over-quota agent (409 `CI_QUOTA_CONCURRENCY`) is covered in
//! `rg-ci`'s `quota::tests::push_triggered_pipelines_are_gated_for_agents_only`.

mod common;

use std::sync::{Arc, Mutex};

use reqwest::Client;
use serde_json::{json, Value};

const PASSWORD: &str = "Qz7$wRtm";

#[derive(Debug, Clone)]
struct Call {
    trigger_type: String,
    triggered_by: Option<i64>,
    repo_id: i64,
}

/// Records what `trigger_pipeline` was called with — no pipeline is created.
#[derive(Clone, Default)]
struct RecordingCi {
    calls: Arc<Mutex<Vec<Call>>>,
}

impl RecordingCi {
    fn calls(&self) -> Vec<Call> {
        self.calls.lock().unwrap().clone()
    }
}

impl rg_core::ci::CiTrigger for RecordingCi {
    fn has_ci_config(&self, _repo_path: &std::path::Path, _commit_sha: &str) -> bool {
        true
    }

    fn trigger_pipeline<'a>(
        &'a self,
        params: rg_core::ci::TriggerPipelineParams<'a>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<i64>> + Send + 'a>> {
        self.calls.lock().unwrap().push(Call {
            trigger_type: params.trigger_type.to_string(),
            triggered_by: params.triggered_by,
            repo_id: params.repo_id,
        });
        Box::pin(async { Ok(1) })
    }

    fn resume_pipeline<'a>(
        &'a self,
        _params: rg_core::ci::ResumePipelineParams<'a>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<()>> + Send + 'a>> {
        Box::pin(async { Ok(()) })
    }
}

struct App {
    base: String,
    client: Client,
    engine: RecordingCi,
    db: rg_db::DatabaseConnection,
    dir: tempfile::TempDir,
}

async fn spawn_app_with(engine: RecordingCi) -> App {
    let (db, dir) = common::setup_test_db().await;
    let repo_root = dir.path().join("repos");
    std::fs::create_dir_all(&repo_root).ok();
    let mut state = common::build_test_app_state(db.clone(), repo_root);
    state.ci_engine = Arc::new(engine.clone());
    let app = rg_http::create_router_for_test(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    App {
        base,
        client: Client::new(),
        engine,
        db,
        dir,
    }
}

impl App {
    fn url(&self, path: &str) -> String {
        format!("{}/api/v1{path}", self.base)
    }

    async fn send(&self, method: reqwest::Method, path: &str, auth: &str, body: Value) -> Value {
        let response = self
            .client
            .request(method, self.url(path))
            .bearer_auth(auth)
            .json(&body)
            .send()
            .await
            .unwrap();
        let status = response.status();
        let body: Value = response.json().await.unwrap_or(Value::Null);
        assert!(status.is_success(), "{path}: {status} {body}");
        body
    }

    /// Wait for the background post-push hook to reach the engine.
    async fn next_call_after(&self, already: usize) -> Call {
        for _ in 0..100 {
            let calls = self.engine.calls();
            if calls.len() > already {
                return calls[already].clone();
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        panic!(
            "post-push hook never reached the CI engine (calls: {:?})",
            self.engine.calls()
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_file_commit_reaches_ci_with_the_pusher_as_the_subject() {
    let app = spawn_app_with(RecordingCi::default()).await;
    let _keep = &app.dir; // keep the bare repositories alive for the whole test

    // The first account is the administrator; it owns the repository.
    let owner =
        common::register_user(&app.base, "push_owner", "push_owner@example.com", PASSWORD).await;
    let me: Value = app
        .client
        .get(app.url("/users/me"))
        .bearer_auth(&owner)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let owner_id = me["id"].as_i64().expect("owner id");

    app.send(
        reqwest::Method::POST,
        "/repos",
        &owner,
        json!({ "name": "wired", "auto_init": true, "is_private": true }),
    )
    .await;
    let repo_id = rg_db::ops::repo_ops::find_by_owner_and_name(&app.db, owner_id, "wired")
        .await
        .unwrap()
        .expect("repository row")
        .id;

    // An agent of that owner, with a repo-scoped token and write access.
    let agent: Value = app
        .send(
            reqwest::Method::POST,
            "/agents",
            &owner,
            json!({ "name": "pusher" }),
        )
        .await;
    let agent_name = agent["username"].as_str().unwrap().to_string();
    let agent_id = agent["id"].as_i64().unwrap();
    let token: Value = app
        .send(
            reqwest::Method::POST,
            &format!("/agents/{agent_name}/tokens"),
            &owner,
            json!({ "name": "push" }),
        )
        .await;
    let agent_token = token["token"].as_str().unwrap().to_string();
    app.send(
        reqwest::Method::POST,
        "/repos/push_owner/wired/collaborators",
        &owner,
        json!({ "username": agent_name, "permission": "write" }),
    )
    .await;

    // The agent commits a file: the identical post-push hook a git push runs.
    let before = app.engine.calls().len();
    app.send(
        reqwest::Method::POST,
        "/repos/push_owner/wired/contents/.ironforge-ci.yml",
        &agent_token,
        json!({
            "branch": "main",
            "message": "ci",
            "content": "stages: [build]\nbuild:\n  stage: build\n  script: [echo ok]\n",
        }),
    )
    .await;
    let agent_call = app.next_call_after(before).await;
    assert_eq!(agent_call.trigger_type, "push");
    assert_eq!(
        agent_call.triggered_by,
        Some(agent_id),
        "the agent's push must reach CI with the agent as the subject"
    );
    assert_eq!(agent_call.repo_id, repo_id);

    // The owner committing in the same way reaches CI as the person.
    let before = app.engine.calls().len();
    app.send(
        reqwest::Method::POST,
        "/repos/push_owner/wired/contents/owner-note.md",
        &owner,
        json!({ "branch": "main", "message": "note", "content": "hi\n" }),
    )
    .await;
    let owner_call = app.next_call_after(before).await;
    assert_eq!(owner_call.trigger_type, "push");
    assert_eq!(owner_call.triggered_by, Some(owner_id));
    assert_eq!(owner_call.repo_id, repo_id);
}
