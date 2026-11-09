//! A fork pull request gets `refs/pull/<n>/head` in the repository it targets, and
//! every pull request remembers the base commit it was opened against.
//!
//! The head branch of a fork pull request lives in the fork. Its objects are
//! copied into the target repository so the ref resolves there and keeps working
//! after the fork's branch moves or goes. The ref belongs to the *target*: it must
//! never be written into the fork under the target's pull-request number.
//! Runs against PostgreSQL / MySQL too when `IRONFORGE_TEST_DATABASE_URL` is set.

mod common;
use common::{register_user, spawn_test_app_on_env_db_with_root};

use reqwest::{Client, StatusCode};
use serde_json::{json, Value};
use std::path::Path;

const PW: &str = "Qz7$wRtm";

struct World {
    base: String,
    root: std::path::PathBuf,
    client: Client,
    alice: String,
    bob: String,
}

impl World {
    async fn new(alice: &str, bob: &str) -> World {
        let (base, _db, root) = spawn_test_app_on_env_db_with_root().await;
        let client = Client::new();
        let alice_jwt = register_user(&base, alice, &format!("{alice}@example.com"), PW).await;
        let bob_jwt = register_user(&base, bob, &format!("{bob}@example.com"), PW).await;
        let resp = client
            .post(format!("{base}/api/v1/repos"))
            .bearer_auth(&alice_jwt)
            .json(&json!({"name": "work", "auto_init": true, "is_private": false}))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 201);
        let resp = client
            .post(format!("{base}/api/v1/repos/{alice}/work/fork"))
            .bearer_auth(&bob_jwt)
            .json(&json!({}))
            .send()
            .await
            .unwrap();
        assert!(resp.status().is_success(), "fork: {}", resp.status());
        World {
            base,
            root,
            client,
            alice: alice_jwt,
            bob: bob_jwt,
        }
    }

    fn repo_path(&self, owner: &str) -> std::path::PathBuf {
        self.root.join(format!("{owner}/work.git"))
    }

    async fn commit(&self, jwt: &str, owner: &str, branch: &str, path: &str) -> String {
        let resp = self
            .client
            .post(format!(
                "{}/api/v1/repos/{owner}/work/contents/{path}",
                self.base
            ))
            .bearer_auth(jwt)
            .json(&json!({"branch": branch, "message": format!("add {path}"), "content": "x\n"}))
            .send()
            .await
            .unwrap();
        let status = resp.status();
        let v: Value = resp.json().await.unwrap_or(Value::Null);
        assert_eq!(status, StatusCode::OK, "{v}");
        v["commit_sha"].as_str().unwrap().to_string()
    }

    fn rev(&self, owner: &str, spec: &str) -> Option<String> {
        rg_git::ops::try_rev_parse(&self.repo_path(owner), spec)
            .ok()
            .flatten()
    }

    /// Wait for a background hook: `refs/pull/<n>/head` of `owner` becomes `expected`.
    async fn pull_ref_becomes(&self, owner: &str, number: i64, expected: &str) -> Option<String> {
        let mut last = None;
        for _ in 0..60 {
            last = self.rev(owner, &format!("refs/pull/{number}/head"));
            if last.as_deref() == Some(expected) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        last
    }
}

fn git(args: &[&str], cwd: &Path) -> String {
    let gateway = rg_git::cli_gateway::global_gateway().as_ref().unwrap();
    let out = gateway.run(args, Some(cwd)).unwrap();
    out.ensure_success().unwrap();
    out.stdout_str().trim().to_string()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_fork_pull_request_gets_its_head_ref_in_the_target_repository() {
    let w = World::new("fp_alice", "fp_bob").await;
    let first = w.commit(&w.bob, "fp_bob", "feat", "one.txt").await;
    // The commit exists only in the fork so far.
    assert!(w.rev("fp_alice", &first).is_none());

    let resp = w
        .client
        .post(format!("{}/api/v1/repos/fp_alice/work/pulls", w.base))
        .bearer_auth(&w.bob)
        .json(&json!({"title": "From a fork", "head": "fp_bob:feat", "base": "main"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);

    // The ref is in the target and resolves there: the objects came with it.
    assert_eq!(
        w.pull_ref_becomes("fp_alice", 1, &first).await.as_deref(),
        Some(first.as_str())
    );
    assert_eq!(
        git(&["cat-file", "-t", &first], &w.repo_path("fp_alice")),
        "commit"
    );
    // ... and not in the fork, under the target's number.
    assert_eq!(w.rev("fp_bob", "refs/pull/1/head"), None);

    // A push to the fork's head branch moves the ref in the target.
    let second = w.commit(&w.bob, "fp_bob", "feat", "two.txt").await;
    assert_eq!(
        w.pull_ref_becomes("fp_alice", 1, &second).await.as_deref(),
        Some(second.as_str())
    );
    assert_eq!(w.rev("fp_bob", "refs/pull/1/head"), None);

    // The export now has an anchor for a fork pull request.
    let resp = w
        .client
        .get(format!(
            "{}/api/v1/repos/fp_alice/work/pulls/1/export",
            w.base
        ))
        .bearer_auth(&w.alice)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let export: Value = resp.json().await.unwrap();
    assert_eq!(
        export["anchors"]["head_ref"], "refs/pull/1/head",
        "{export}"
    );
    assert_eq!(export["anchors"]["head_ref_sha"], second.as_str());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_pull_request_remembers_the_base_it_was_opened_against() {
    let w = World::new("bp_alice", "bp_bob").await;
    let opened_against = w.rev("bp_alice", "refs/heads/main").unwrap();
    w.commit(&w.alice, "bp_alice", "agent/x", "a.txt").await;
    let resp = w
        .client
        .post(format!("{}/api/v1/repos/bp_alice/work/pulls", w.base))
        .bearer_auth(&w.alice)
        .json(&json!({"title": "Add a", "head": "agent/x", "base": "main"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);

    // The base branch moves on.
    let advanced = w.commit(&w.alice, "bp_alice", "main", "later.txt").await;
    assert_ne!(advanced, opened_against);

    let resp = w
        .client
        .get(format!(
            "{}/api/v1/repos/bp_alice/work/pulls/1/export",
            w.base
        ))
        .bearer_auth(&w.alice)
        .send()
        .await
        .unwrap();
    let export: Value = resp.json().await.unwrap();
    let anchors = &export["anchors"];
    assert_eq!(
        anchors["base_sha_when_opened"],
        opened_against.as_str(),
        "{export}"
    );
    assert_eq!(anchors["base_tip_sha"], advanced.as_str());
    assert_eq!(anchors["not_stored"], json!([]));
}
