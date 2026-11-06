//! `refs/pull/<n>/head` anchors a pull request's head commit, and
//! `GET …/pulls/{n}/export` describes the pull request in one document.
//!
//! The anchor is checked with real `git` over HTTP: it can be fetched, it
//! follows a later push to the head branch, and it is still there after the head
//! branch is deleted.

mod common;
use common::{register_user, spawn_test_app_on_env_db_with_root};

use reqwest::{Client, StatusCode};
use serde_json::{json, Value};
use std::path::Path;

const PW: &str = "Qz7$wRtm";

fn git_try(args: &[&str], cwd: Option<&Path>) -> (bool, String) {
    let gateway = rg_git::cli_gateway::global_gateway().as_ref().unwrap();
    let out = gateway.run(args, cwd).unwrap();
    (out.success(), out.stderr_str())
}

fn git(args: &[&str], cwd: Option<&Path>) -> String {
    let gateway = rg_git::cli_gateway::global_gateway().as_ref().unwrap();
    let out = gateway.run(args, cwd).unwrap();
    out.ensure_success().unwrap();
    out.stdout_str().trim().to_string()
}

struct Fixture {
    base: String,
    jwt: String,
    owner: &'static str,
    client: Client,
    db: rg_db::DatabaseConnection,
    repo_id: i64,
    repo_root: std::path::PathBuf,
}

impl Fixture {
    async fn new(owner: &'static str, is_private: bool) -> Fixture {
        let (base, db, repo_root) = spawn_test_app_on_env_db_with_root().await;
        let jwt = register_user(&base, owner, &format!("{owner}@example.com"), PW).await;
        let client = Client::new();
        let resp = client
            .post(format!("{base}/api/v1/repos"))
            .bearer_auth(&jwt)
            .json(&json!({"name": "work", "auto_init": true, "is_private": is_private}))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 201);
        let repo_id = resp.json::<Value>().await.unwrap()["id"].as_i64().unwrap();
        Fixture {
            base,
            jwt,
            owner,
            client,
            db,
            repo_id,
            repo_root,
        }
    }

    fn api(&self, suffix: &str) -> String {
        format!("{}/api/v1/repos/{}/work{suffix}", self.base, self.owner)
    }

    async fn post(&self, suffix: &str, body: Value) -> (StatusCode, Value) {
        let resp = self
            .client
            .post(self.api(suffix))
            .bearer_auth(&self.jwt)
            .json(&body)
            .send()
            .await
            .unwrap();
        let status = resp.status();
        (status, resp.json().await.unwrap_or(Value::Null))
    }

    /// Commit a file to `branch` through the API; returns the new commit.
    async fn commit(&self, branch: &str, path: &str) -> String {
        let (status, v) = self
            .post(
                &format!("/contents/{path}"),
                json!({"branch": branch, "message": format!("add {path}"), "content": "x\n"}),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{v}");
        v["commit_sha"].as_str().unwrap().to_string()
    }

    async fn open_pr(&self, head: &str) -> Value {
        let (status, v) = self
            .post(
                "/pulls",
                json!({"title": "Add files", "body": "Details here.", "head": head, "base": "main"}),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{v}");
        v
    }

    /// A remote URL carrying a write token for the owner.
    async fn remote(&self) -> String {
        let resp = self
            .client
            .post(format!("{}/api/v1/users/tokens", self.base))
            .bearer_auth(&self.jwt)
            .json(&json!({"name": "writer", "scopes": "repo:write"}))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 201);
        let token = resp.json::<Value>().await.unwrap()["token"]
            .as_str()
            .unwrap()
            .to_string();
        self.base
            .replacen("http://", &format!("http://{}:{token}@", self.owner), 1)
            + &format!("/{}/work.git", self.owner)
    }

    /// What `refs/pull/<n>/head` points at, by asking the server over git.
    async fn pull_ref(&self, number: i64) -> Option<String> {
        let remote = self.remote().await;
        git(
            &["ls-remote", &remote, &format!("refs/pull/{number}/head")],
            None,
        )
        .split_whitespace()
        .next()
        .map(str::to_string)
    }

    /// Wait (the hooks run in a background task) until `refs/pull/<n>/head` is
    /// `expected`; returns what it last was.
    async fn pull_ref_becomes(&self, number: i64, expected: &str) -> Option<String> {
        let mut last = None;
        for _ in 0..50 {
            last = self.pull_ref(number).await;
            if last.as_deref() == Some(expected) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        last
    }

    async fn export(&self, bearer: Option<&str>, number: i64) -> (StatusCode, Value) {
        let mut req = self
            .client
            .get(self.api(&format!("/pulls/{number}/export")));
        if let Some(token) = bearer {
            req = req.bearer_auth(token);
        }
        let resp = req.send().await.unwrap();
        let status = resp.status();
        (status, resp.json().await.unwrap_or(Value::Null))
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_pull_ref_follows_the_branch_and_outlives_it() {
    let f = Fixture::new("pullref_owner", false).await;
    let first = f.commit("agent/x", "one.txt").await;
    f.open_pr("agent/x").await;
    assert_eq!(f.pull_ref(1).await.as_deref(), Some(first.as_str()));

    // A push to the head branch moves the anchor with it.
    let remote = f.remote().await;
    let wt = tempfile::tempdir().unwrap();
    let path = wt.path();
    git(&["clone", &remote, &path.to_string_lossy()], None);
    git(&["config", "user.name", "Agent"], Some(path));
    git(&["config", "user.email", "agent@example.com"], Some(path));
    git(&["checkout", "-b", "agent/x", "origin/agent/x"], Some(path));
    std::fs::write(path.join("two.txt"), "2\n").unwrap();
    git(&["add", "two.txt"], Some(path));
    git(&["commit", "-m", "more"], Some(path));
    let pushed = git(&["rev-parse", "HEAD"], Some(path));
    git(&["push", "origin", "HEAD:refs/heads/agent/x"], Some(path));
    assert_eq!(
        f.pull_ref_becomes(1, &pushed).await.as_deref(),
        Some(pushed.as_str())
    );

    // The head branch goes away (the server has no branch-delete endpoint and
    // refuses a delete push, so remove the ref directly): the anchor stays and
    // the commit is still fetchable by it.
    let repo_path = f.repo_root.join(format!("{}/work.git", f.owner));
    rg_git::ops::delete_ref(&repo_path, "refs/heads/agent/x").unwrap();
    let heads = git(&["ls-remote", &remote, "refs/heads/agent/x"], None);
    assert!(heads.is_empty(), "branch should be gone: {heads}");
    assert_eq!(f.pull_ref(1).await.as_deref(), Some(pushed.as_str()));
    let fresh = tempfile::tempdir().unwrap();
    git(&["init", "-q", &fresh.path().to_string_lossy()], None);
    let (ok, err) = git_try(
        &["fetch", &remote, "refs/pull/1/head:refs/replay/1"],
        Some(fresh.path()),
    );
    assert!(ok, "fetching the anchor: {err}");
    assert_eq!(
        git(&["rev-parse", "refs/replay/1"], Some(fresh.path())),
        pushed
    );

    // The export still names the anchor and its commit.
    let (status, v) = f.export(Some(&f.jwt), 1).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(v["anchors"]["head_ref_sha"], pushed.as_str());
    assert!(v["anchors"]["head_branch_sha"].is_null());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_api_commit_to_the_head_branch_moves_the_anchor() {
    let f = Fixture::new("pullref_api", false).await;
    f.commit("agent/y", "one.txt").await;
    f.open_pr("agent/y").await;
    let second = f.commit("agent/y", "two.txt").await;
    assert_eq!(
        f.pull_ref_becomes(1, &second).await.as_deref(),
        Some(second.as_str()),
        "a commit made through the contents API must move refs/pull/1/head too"
    );
    // The stored head of the pull request follows as well.
    let resp = f
        .client
        .get(f.api("/pulls/1"))
        .bearer_auth(&f.jwt)
        .send()
        .await
        .unwrap();
    let v: Value = resp.json().await.unwrap();
    assert_eq!(v["head_sha"], second.as_str(), "{v}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn export_describes_the_pull_request_and_its_history() {
    let f = Fixture::new("export_owner", false).await;
    let head = f.commit("agent/z", "one.txt").await;
    f.open_pr("agent/z").await;

    // One review comment, one review and a failed job for the head commit.
    let (status, v) = f
        .post(
            "/pulls/1/comments",
            json!({"path": "one.txt", "line": 1, "body": "why this?", "commit_id": head}),
        )
        .await;
    assert!(status.is_success(), "{status} {v}");
    let pipeline = rg_db::ops::pipeline_ops::create_pipeline(
        &f.db,
        f.repo_id,
        &head,
        "refs/heads/agent/z",
        "push",
        None,
    )
    .await
    .unwrap();
    let stage = rg_db::ops::pipeline_ops::create_stage(&f.db, pipeline.id, "test", 0)
        .await
        .unwrap();
    rg_db::ops::pipeline_ops::create_job(
        &f.db,
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

    let (status, v) = f.export(Some(&f.jwt), 1).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(v["format_version"], 1);
    assert_eq!(v["repository"], "export_owner/work");
    assert_eq!(v["pull_request"]["head_branch"], "agent/z");
    assert_eq!(v["pull_request"]["origin"], "user_content");
    assert_eq!(v["anchors"]["head_ref"], "refs/pull/1/head");
    assert_eq!(v["anchors"]["head_ref_sha"], head.as_str());
    assert!(v["anchors"]["base_tip_sha"].is_string());
    assert!(v["anchors"]["base_sha_at_merge"].is_null());
    assert_eq!(v["counts"]["review_comments"], 1);
    assert_eq!(v["review_comments"][0]["body"], "why this?");
    assert_eq!(v["review_comments"][0]["origin"], "user_content");
    assert_eq!(v["ci"]["commit_sha"], head.as_str());
    assert_eq!(v["ci"]["jobs_total"], 1);
    assert_eq!(v["ci"]["jobs"][0]["name"], "unit");
    assert!(
        v["ci"]["jobs"][0].get("log").is_none(),
        "no log text in an export"
    );

    let (status, v) = f.export(Some(&f.jwt), 99).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{v}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn export_of_a_private_repository_needs_read_access() {
    let f = Fixture::new("export_private", true).await;
    f.commit("agent/p", "one.txt").await;
    f.open_pr("agent/p").await;
    let (status, _) = f.export(None, 1).await;
    assert!(
        status == StatusCode::UNAUTHORIZED || status == StatusCode::NOT_FOUND,
        "anonymous read of a private repository: {status}"
    );
    let other = register_user(&f.base, "export_other", "export-other@example.com", PW).await;
    let (status, _) = f.export(Some(&other), 1).await;
    assert!(
        status == StatusCode::FORBIDDEN || status == StatusCode::NOT_FOUND,
        "a stranger must not read it: {status}"
    );
    let (status, _) = f.export(Some(&f.jwt), 1).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_file_commit_through_the_api_reaches_the_event_log() {
    let f = Fixture::new("pullref_events", false).await;
    let sha = f.commit("agent/e", "one.txt").await;
    // The hooks run in a background task: ask from the beginning until the push
    // event shows up.
    let mut v = Value::Null;
    let mut pushed = false;
    for _ in 0..50 {
        let resp = f
            .client
            .get(f.api("/events?since=0&types=push"))
            .bearer_auth(&f.jwt)
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        v = resp.json().await.unwrap();
        pushed = v["events"].as_array().unwrap().iter().any(|event| {
            event["payload"]["after"] == sha.as_str()
                && event["payload"]["ref"] == "refs/heads/agent/e"
        });
        if pushed {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert!(pushed, "no push event for {sha}: {v}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn opening_a_pull_request_can_be_checked_first() {
    let f = Fixture::new("pr_dry", false).await;
    let head = f.commit("agent/d", "one.txt").await;

    // A dry run says what would be created and writes nothing.
    let (status, v) = f
        .post(
            "/pulls",
            json!({"title": "Add one", "head": "agent/d", "base": "main", "dry_run": true}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(v["dry_run"], true);
    assert_eq!(v["would"][0]["effect"], "pull_request_create");
    assert_eq!(v["would"][0]["number"], 1);
    assert_eq!(v["would"][0]["head_sha"], head.as_str());
    assert_eq!(v["would"][0]["publishes_pull_ref"], true);
    assert_eq!(v["head_branch_found"], true);
    assert_eq!(v["base_branch_found"], true);
    assert_eq!(v["existing_open_pull_requests"], json!([]));
    let list = f
        .client
        .get(f.api("/pulls"))
        .bearer_auth(&f.jwt)
        .send()
        .await
        .unwrap();
    let listed: Value = list.json().await.unwrap();
    let count = listed
        .get("data")
        .and_then(Value::as_array)
        .or_else(|| listed.as_array())
        .map(Vec::len)
        .unwrap();
    assert_eq!(
        count, 0,
        "a dry run must not create a pull request: {listed}"
    );
    assert!(
        f.pull_ref(1).await.is_none(),
        "a dry run must not publish refs/pull"
    );

    // The same check through the header.
    let resp = f
        .client
        .post(f.api("/pulls"))
        .bearer_auth(&f.jwt)
        .header("X-Dry-Run", "1")
        .json(&json!({"title": "Add one", "head": "agent/d", "base": "main"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // The same refusals as a real call.
    let (status, _) = f
        .post(
            "/pulls",
            json!({"title": "x", "head": "main", "base": "main", "dry_run": true}),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = f
        .post(
            "/pulls",
            json!({"title": "  ", "head": "agent/d", "base": "main", "dry_run": true}),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // A missing head branch is reported rather than hidden.
    let (status, v) = f
        .post(
            "/pulls",
            json!({"title": "x", "head": "agent/none", "base": "main", "dry_run": true}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(v["head_branch_found"], false);
    assert!(v["would"][0]["head_sha"].is_null());
    assert_eq!(v["would"][0]["publishes_pull_ref"], false);

    // After a real one, the next dry run names the open duplicate and the next number.
    f.open_pr("agent/d").await;
    let (status, v) = f
        .post(
            "/pulls",
            json!({"title": "Again", "head": "agent/d", "base": "main", "dry_run": true}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(v["existing_open_pull_requests"], json!([1]));
    assert_eq!(v["would"][0]["number"], 2);
    // The listing does see a real pull request (so the empty listing above meant
    // something) and the dry runs added none.
    let list = f
        .client
        .get(f.api("/pulls"))
        .bearer_auth(&f.jwt)
        .send()
        .await
        .unwrap();
    let listed: Value = list.json().await.unwrap();
    let count = listed
        .get("data")
        .and_then(Value::as_array)
        .or_else(|| listed.as_array())
        .map(Vec::len)
        .unwrap();
    assert_eq!(count, 1, "{listed}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn agent_writes_are_audited_with_the_token_that_made_them() {
    use rg_db::sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
    let f = Fixture::new("audit_agent", false).await;
    let resp = f
        .client
        .post(format!("{}/api/v1/users/tokens", f.base))
        .bearer_auth(&f.jwt)
        .json(&json!({"name": "agent", "scopes": "repo:write"}))
        .send()
        .await
        .unwrap();
    let created: Value = resp.json().await.unwrap();
    let token_id = created["id"].as_i64().unwrap();
    let pat = created["token"].as_str().unwrap().to_string();

    // Commit, open a pull request and merge it, all with the token.
    let resp = f
        .client
        .post(f.api("/contents/a.txt"))
        .bearer_auth(&pat)
        .json(&json!({"branch": "agent/a", "message": "add a", "content": "a\n"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let commit = resp.json::<Value>().await.unwrap()["commit_sha"]
        .as_str()
        .unwrap()
        .to_string();
    let resp = f
        .client
        .post(f.api("/pulls"))
        .bearer_auth(&pat)
        .json(&json!({"title": "Add a", "head": "agent/a", "base": "main"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let resp = f
        .client
        .post(f.api("/pulls/1/merge"))
        .bearer_auth(&pat)
        .json(&json!({"strategy": "merge"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "{:?}", resp.text().await);

    let entry = |action: &'static str| {
        let db = f.db.clone();
        async move {
            let row = rg_db::entities::audit_log::Entity::find()
                .filter(rg_db::entities::audit_log::Column::Action.eq(action))
                .filter(rg_db::entities::audit_log::Column::Username.eq("audit_agent"))
                .one(&db)
                .await
                .unwrap()
                .unwrap_or_else(|| panic!("no {action} entry"));
            serde_json::from_str::<Value>(&row.details.unwrap()).unwrap()
        }
    };
    let file = entry("repo.file_commit").await;
    assert_eq!(file["access_token_id"], json!(token_id));
    assert_eq!(file["branch"], "agent/a");
    assert_eq!(file["commit_sha"], commit.as_str());
    let created = entry("pull_request.create").await;
    assert_eq!(created["access_token_id"], json!(token_id));
    assert_eq!(created["number"], 1);
    let merged = entry("pull_request.merge").await;
    assert_eq!(merged["access_token_id"], json!(token_id));
    assert!(merged["merge_commit_sha"].is_string());

    // A login (no token) writes entries without a token id.
    let resp = f
        .client
        .post(f.api("/contents/b.txt"))
        .bearer_auth(&f.jwt)
        .json(&json!({"branch": "agent/b", "message": "add b", "content": "b\n"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let rows = rg_db::entities::audit_log::Entity::find()
        .filter(rg_db::entities::audit_log::Column::Action.eq("repo.file_commit"))
        .all(&f.db)
        .await
        .unwrap();
    let untokened = rows
        .iter()
        .filter(|row| {
            row.details
                .as_deref()
                .and_then(|d| serde_json::from_str::<Value>(d).ok())
                .is_some_and(|d| d["branch"] == "agent/b" && d.get("access_token_id").is_none())
        })
        .count();
    assert_eq!(untokened, 1);
}
