//! Merging with `expected_head_sha` must merge exactly the commit the caller
//! reviewed, or not merge at all.
//!
//! The REST handler checks the head first and answers quickly, but a push can land
//! after that check. The merge itself is therefore pinned: it is built from the
//! expected commit id and refuses to start when the head is anywhere else. These
//! tests call the merge service directly with a head that has already moved (the
//! window cannot be hit reliably through HTTP) and check, for every strategy and
//! for a fork pull request, that nothing is merged and the pull request stays open.
//! Runs against PostgreSQL / MySQL too when `IRONFORGE_TEST_DATABASE_URL` is set.

mod common;
use common::{register_user, spawn_test_app_on_env_db_with_root};

use reqwest::{Client, StatusCode};
use rg_core::pull_request::{merge_pr_expecting_head, MergeError, MergeStrategy};
use serde_json::{json, Value};

const PW: &str = "Qz7$wRtm";

struct World {
    base: String,
    db: rg_db::DatabaseConnection,
    root: std::path::PathBuf,
    client: Client,
    alice: String,
    bob: String,
}

impl World {
    async fn new(alice: &str, bob: &str) -> World {
        let (base, db, root) = spawn_test_app_on_env_db_with_root().await;
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
            db,
            root,
            client,
            alice: alice_jwt,
            bob: bob_jwt,
        }
    }

    fn rev(&self, owner: &str, spec: &str) -> Option<String> {
        rg_git::ops::try_rev_parse(&self.root.join(format!("{owner}/work.git")), spec)
            .ok()
            .flatten()
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

    async fn open_pr(&self, jwt: &str, owner: &str, head: &str) {
        let resp = self
            .client
            .post(format!("{}/api/v1/repos/{owner}/work/pulls", self.base))
            .bearer_auth(jwt)
            .json(&json!({"title": "Change", "head": head, "base": "main"}))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
    }

    async fn pr_state(&self, owner: &str, number: i64) -> String {
        let v: Value = self
            .client
            .get(format!(
                "{}/api/v1/repos/{owner}/work/pulls/{number}",
                self.base
            ))
            .bearer_auth(&self.alice)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        v["state"].as_str().unwrap().to_string()
    }

    async fn merge(
        &self,
        owner: &str,
        number: i64,
        strategy: MergeStrategy,
        expected: Option<&str>,
    ) -> Result<rg_core::pull_request::MergeResult, MergeError> {
        merge_pr_expecting_head(
            &self.db, &self.root, owner, "work", number, strategy, expected,
        )
        .await
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_merge_pinned_to_a_stale_head_merges_nothing_for_every_strategy() {
    let w = World::new("mp_alice", "mp_bob").await;

    for (index, strategy) in [
        MergeStrategy::Merge,
        MergeStrategy::Squash,
        MergeStrategy::Rebase,
    ]
    .into_iter()
    .enumerate()
    {
        let number = index as i64 + 1;
        // Each round starts from wherever main is now (the earlier rounds merged).
        let main_before = w.rev("mp_alice", "refs/heads/main").unwrap();
        let branch = format!("agent/s{number}");
        let reviewed = w
            .commit(&w.alice, "mp_alice", &branch, &format!("s{number}a.txt"))
            .await;
        w.open_pr(&w.alice, "mp_alice", &branch).await;
        // The branch moves after the caller looked at it.
        let moved = w
            .commit(&w.alice, "mp_alice", &branch, &format!("s{number}b.txt"))
            .await;

        match w.merge("mp_alice", number, strategy, Some(&reviewed)).await {
            Err(MergeError::HeadMoved { current, expected }) => {
                assert_eq!(current.as_deref(), Some(moved.as_str()));
                assert_eq!(expected, reviewed);
            }
            other => panic!("{strategy:?}: expected HeadMoved, got {other:?}"),
        }
        assert_eq!(
            w.rev("mp_alice", "refs/heads/main").unwrap(),
            main_before,
            "{strategy:?}: nothing may be merged"
        );
        assert_eq!(w.pr_state("mp_alice", number).await, "open", "{strategy:?}");

        // Pinned to what is actually there, it merges.
        let result = w
            .merge("mp_alice", number, strategy, Some(&moved))
            .await
            .unwrap_or_else(|e| panic!("{strategy:?}: {e:?}"));
        assert!(!result.merge_commit_sha.is_empty());
        assert_eq!(
            w.pr_state("mp_alice", number).await,
            "merged",
            "{strategy:?}"
        );
        // What landed on main contains the reviewed commit and the one after it.
        let tip = w.rev("mp_alice", "refs/heads/main").unwrap();
        assert_eq!(tip, result.merge_commit_sha, "{strategy:?}");
        let tree_has = |path: &str| w.rev("mp_alice", &format!("{tip}:{path}")).is_some();
        assert!(tree_has(&format!("s{number}a.txt")), "{strategy:?}");
        assert!(tree_has(&format!("s{number}b.txt")), "{strategy:?}");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_merge_pinned_to_a_gone_head_branch_reports_it_gone() {
    let w = World::new("mg_alice", "mg_bob").await;
    let reviewed = w.commit(&w.alice, "mg_alice", "agent/gone", "g.txt").await;
    w.open_pr(&w.alice, "mg_alice", "agent/gone").await;
    rg_git::ops::delete_ref(&w.root.join("mg_alice/work.git"), "refs/heads/agent/gone").unwrap();
    match w
        .merge("mg_alice", 1, MergeStrategy::Merge, Some(&reviewed))
        .await
    {
        Err(MergeError::HeadMoved { current, .. }) => assert_eq!(current, None),
        other => panic!("expected HeadMoved with no current head, got {other:?}"),
    }
    assert_eq!(w.pr_state("mg_alice", 1).await, "open");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_fork_pull_request_is_pinned_the_same_way() {
    let w = World::new("mf_alice", "mf_bob").await;
    let main_before = w.rev("mf_alice", "refs/heads/main").unwrap();
    let reviewed = w.commit(&w.bob, "mf_bob", "feat", "one.txt").await;
    w.open_pr(&w.bob, "mf_alice", "mf_bob:feat").await;
    let moved = w.commit(&w.bob, "mf_bob", "feat", "two.txt").await;

    match w
        .merge("mf_alice", 1, MergeStrategy::Merge, Some(&reviewed))
        .await
    {
        Err(MergeError::HeadMoved { current, expected }) => {
            assert_eq!(current.as_deref(), Some(moved.as_str()));
            assert_eq!(expected, reviewed);
        }
        other => panic!("expected HeadMoved, got {other:?}"),
    }
    assert_eq!(w.rev("mf_alice", "refs/heads/main").unwrap(), main_before);
    assert_eq!(w.pr_state("mf_alice", 1).await, "open");

    let result = w
        .merge("mf_alice", 1, MergeStrategy::Merge, Some(&moved))
        .await
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(
        w.rev("mf_alice", "refs/heads/main").unwrap(),
        result.merge_commit_sha
    );
    assert_eq!(w.pr_state("mf_alice", 1).await, "merged");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_rest_merge_still_answers_a_stale_expected_head_with_ref_advanced() {
    let w = World::new("mr_alice", "mr_bob").await;
    let reviewed = w.commit(&w.alice, "mr_alice", "agent/r", "r1.txt").await;
    w.open_pr(&w.alice, "mr_alice", "agent/r").await;
    let moved = w.commit(&w.alice, "mr_alice", "agent/r", "r2.txt").await;
    let resp = w
        .client
        .post(format!(
            "{}/api/v1/repos/mr_alice/work/pulls/1/merge",
            w.base
        ))
        .bearer_auth(&w.alice)
        .json(&json!({"strategy": "merge", "expected_head_sha": reviewed}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["error"]["reason"], "REF_ADVANCED", "{body}");
    assert_eq!(body["error"]["details"]["current_sha"], moved.as_str());

    // With the right commit the REST merge goes through.
    let resp = w
        .client
        .post(format!(
            "{}/api/v1/repos/mr_alice/work/pulls/1/merge",
            w.base
        ))
        .bearer_auth(&w.alice)
        .json(&json!({"strategy": "squash", "expected_head_sha": moved}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "{:?}", resp.text().await);
    assert_eq!(w.pr_state("mr_alice", 1).await, "merged");
}
