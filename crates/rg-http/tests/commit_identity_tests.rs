//! Who a commit says it is by, and who pushed it (issue #36, T25).
//!
//! A commit the server writes on someone's behalf — a file commit, a delete, a
//! batch commit — names the account that made the request. For an agent that is
//! `<owner>-agent-<name> <<owner>-agent-<name>@agents.invalid>`, derived from the
//! username when the commit is made (the stored address is a random uuid that
//! says nothing in `git log`). A person's commits are unchanged.
//!
//! A commit that arrives by `git push` keeps the author the client wrote; the
//! server does not rewrite it. What it records instead is who pushed: the
//! `push` repository event carries `pushed_by`, for a git push and for a REST
//! file commit alike.

mod common;
use common::{register_user, spawn_test_app_with_root};

use reqwest::{Client, StatusCode};
use serde_json::{json, Value};

const PASSWORD: &str = "Qz7$wRtm";

/// The permission cache is process-wide and keyed by `(repo id, user id)`; each
/// test has its own database, so run them one at a time from an empty cache.
static ONE_AT_A_TIME: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn git(args: &[&str], cwd: Option<&std::path::Path>) -> String {
    let gateway = rg_git::cli_gateway::global_gateway().as_ref().unwrap();
    let out = gateway.run(args, cwd).unwrap();
    out.ensure_success().unwrap();
    out.stdout_str().trim().to_string()
}

struct World {
    _exclusive: tokio::sync::MutexGuard<'static, ()>,
    base: String,
    db: rg_db::DatabaseConnection,
    bare: std::path::PathBuf,
    alice: String,
    agent: String,
    agent_id: i64,
    agent_token: String,
    client: Client,
}

/// `alice` (a person) with a repository `work`, and her agent `bot` with write
/// access to it.
async fn world() -> World {
    let exclusive = ONE_AT_A_TIME.lock().await;
    rg_core::repo::service::invalidate_perm_cache_all();
    let (base, db, root) = spawn_test_app_with_root().await;
    // The first account is the administrator.
    register_user(&base, "root_admin", "root_admin@example.com", PASSWORD).await;
    let alice = register_user(&base, "alice", "alice@example.com", PASSWORD).await;
    let client = Client::new();

    let resp = client
        .post(format!("{base}/api/v1/repos"))
        .bearer_auth(&alice)
        .json(&json!({"name": "work", "auto_init": true, "is_private": false}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);

    let resp = client
        .post(format!("{base}/api/v1/agents"))
        .bearer_auth(&alice)
        .json(&json!({"name": "bot"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);
    let body: Value = resp.json().await.unwrap();
    let agent = body["username"].as_str().unwrap().to_string();
    let agent_id = body["id"].as_i64().unwrap();

    let resp = client
        .post(format!("{base}/api/v1/repos/alice/work/collaborators"))
        .bearer_auth(&alice)
        .json(&json!({"username": agent, "permission": "write"}))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success(), "grant write: {}", resp.status());

    let resp = client
        .post(format!("{base}/api/v1/agents/{agent}/tokens"))
        .bearer_auth(&alice)
        .json(&json!({"name": "t"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);
    let body: Value = resp.json().await.unwrap();
    let agent_token = body["token"].as_str().unwrap().to_string();

    World {
        _exclusive: exclusive,
        base,
        db,
        bare: root.join("alice/work.git"),
        alice,
        agent,
        agent_id,
        agent_token,
        client,
    }
}

impl World {
    fn url(&self, suffix: &str) -> String {
        format!("{}/api/v1/repos/alice/work{suffix}", self.base)
    }

    /// `git log` of `branch`, newest first: `author | committer | subject`.
    fn log(&self, branch: &str) -> Vec<String> {
        git(
            &[
                "--git-dir",
                self.bare.to_string_lossy().as_ref(),
                "log",
                "--format=%an <%ae> | %cn <%ce> | %s",
                branch,
            ],
            None,
        )
        .lines()
        .map(str::to_string)
        .collect()
    }

    async fn commit_file(&self, auth: &str, path: &str, message: &str) -> Value {
        let resp = self
            .client
            .post(self.url(&format!("/contents/{path}")))
            .bearer_auth(auth)
            .json(&json!({"branch": "main", "message": message, "content": "x\n"}))
            .send()
            .await
            .unwrap();
        let status = resp.status();
        let body: Value = resp.json().await.unwrap();
        assert_eq!(status, StatusCode::OK, "{body}");
        body
    }

    async fn blob_sha(&self, path: &str) -> String {
        let body: Value = self
            .client
            .get(self.url(&format!("/blob/{path}")))
            .query(&[("ref", "main")])
            .bearer_auth(&self.alice)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        body["sha"].as_str().unwrap().to_string()
    }

    fn tip(&self) -> String {
        git(
            &[
                "--git-dir",
                self.bare.to_string_lossy().as_ref(),
                "rev-parse",
                "main",
            ],
            None,
        )
    }

    /// The `push` events of the repository, oldest first, once there are at
    /// least `at_least` of them (the hooks run in a background task).
    async fn push_events(&self, at_least: usize) -> Vec<Value> {
        for _ in 0..100 {
            let body: Value = self
                .client
                .get(self.url("/events?since=0&types=push&limit=100"))
                .bearer_auth(&self.alice)
                .send()
                .await
                .unwrap()
                .json()
                .await
                .unwrap();
            let events = body["events"].as_array().cloned().unwrap_or_default();
            if events.len() >= at_least {
                return events;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        panic!("fewer than {at_least} push events were recorded");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_agent_file_commit_is_authored_by_the_agent_with_a_derived_address() {
    let w = world().await;
    assert_eq!(w.agent, "alice-agent-bot");
    w.commit_file(&w.agent_token, "from-agent.txt", "agent add")
        .await;

    let log = w.log("main");
    assert_eq!(
        log[0],
        "alice-agent-bot <alice-agent-bot@agents.invalid> | \
         alice-agent-bot <alice-agent-bot@agents.invalid> | agent add"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_persons_file_commit_keeps_the_stored_address() {
    let w = world().await;
    w.commit_file(&w.alice, "from-alice.txt", "alice add").await;

    assert_eq!(
        w.log("main")[0],
        "alice <alice@example.com> | alice <alice@example.com> | alice add"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_agent_delete_and_batch_commit_use_the_same_identity() {
    let w = world().await;
    w.commit_file(&w.alice, "gone.txt", "seed").await;
    let blob = w.blob_sha("gone.txt").await;

    // Delete.
    let resp = w
        .client
        .delete(w.url(&format!(
            "/contents/gone.txt?branch=main&message=agent%20rm&sha={blob}"
        )))
        .bearer_auth(&w.agent_token)
        .send()
        .await
        .unwrap();
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or(Value::Null);
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        w.log("main")[0],
        "alice-agent-bot <alice-agent-bot@agents.invalid> | \
         alice-agent-bot <alice-agent-bot@agents.invalid> | agent rm"
    );

    // Batch commit.
    let resp = w
        .client
        .post(w.url("/commits"))
        .bearer_auth(&w.agent_token)
        .json(&json!({
            "branch": "main",
            "expected_sha": w.tip(),
            "message": "agent batch",
            "files": [
                {"operation": "create", "path": "b1.txt", "content": "1\n"},
                {"operation": "create", "path": "b2.txt", "content": "2\n"},
            ],
        }))
        .send()
        .await
        .unwrap();
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or(Value::Null);
    assert!(status.is_success(), "{status} {body}");
    assert_eq!(
        w.log("main")[0],
        "alice-agent-bot <alice-agent-bot@agents.invalid> | \
         alice-agent-bot <alice-agent-bot@agents.invalid> | agent batch"
    );

    // The same batch route for a person is unchanged.
    let resp = w
        .client
        .post(w.url("/commits"))
        .bearer_auth(&w.alice)
        .json(&json!({
            "branch": "main",
            "expected_sha": w.tip(),
            "message": "alice batch",
            "files": [{"operation": "create", "path": "b3.txt", "content": "3\n"}],
        }))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());
    assert_eq!(
        w.log("main")[0],
        "alice <alice@example.com> | alice <alice@example.com> | alice batch"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_stored_address_of_an_agent_is_not_rewritten() {
    // Nothing is stored or backfilled: after committing, the row still has the
    // random address it was created with.
    let w = world().await;
    w.commit_file(&w.agent_token, "x.txt", "add").await;
    let row = rg_db::ops::user_ops::find_by_username(&w.db, &w.agent)
        .await
        .unwrap()
        .unwrap();
    assert!(
        row.email.starts_with("agent-") && row.email.ends_with("@agents.invalid"),
        "stored address changed: {}",
        row.email
    );
    assert_ne!(row.email, "alice-agent-bot@agents.invalid");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_rest_commit_and_a_git_push_both_record_who_pushed() {
    let w = world().await;

    // REST file commit by the agent.
    w.commit_file(&w.agent_token, "rest.txt", "by rest").await;
    let events = w.push_events(1).await;
    assert_eq!(events[0]["payload"]["pushed_by"], json!(w.agent_id));

    // A real `git push` by the agent, with an author the client chose.
    let url = w.base.replacen(
        "http://",
        &format!("http://{}:{}@", w.agent, w.agent_token),
        1,
    );
    let remote = format!("{url}/alice/work.git");
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path();
    git(&["clone", &remote, path.to_string_lossy().as_ref()], None);
    git(&["config", "user.name", "Client Chosen"], Some(path));
    git(&["config", "user.email", "client@example.org"], Some(path));
    git(&["config", "commit.gpgsign", "false"], Some(path));
    std::fs::write(path.join("pushed.txt"), "p\n").unwrap();
    git(&["add", "."], Some(path));
    git(&["commit", "-m", "by push"], Some(path));
    git(&["push", "origin", "HEAD:main"], Some(path));

    let events = w.push_events(2).await;
    let last = events.last().unwrap();
    assert_eq!(last["payload"]["pushed_by"], json!(w.agent_id), "{last}");
    assert_eq!(last["payload"]["ref"], "refs/heads/main");
    // The server did not touch the author the client wrote.
    assert_eq!(
        w.log("main")[0],
        "Client Chosen <client@example.org> | Client Chosen <client@example.org> | by push"
    );
}
