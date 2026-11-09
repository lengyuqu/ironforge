//! Automatic merges and the merge queue merge the commit they evaluated.
//!
//! Both used to decide "this head satisfies the rules" and then merge by branch
//! name, so a push in between merged a commit nobody looked at. They now pass the
//! evaluated commit to `merge_pr_expecting_head`; a moved head is not a failure,
//! it just is not this commit -- auto-merge stays on and the queue entry goes back
//! to the queue for the next push.
//!
//! The window cannot be hit reliably through HTTP, so these tests make the stored
//! head stale on purpose (the branch has moved, the record has not) and call the
//! services directly.
//! Runs against PostgreSQL / MySQL too when `IRONFORGE_TEST_DATABASE_URL` is set.

mod common;
use common::{register_user, spawn_test_app_on_env_db_with_root};

use reqwest::{Client, StatusCode};
use rg_core::pull_request::merge_queue::{enqueue, process_for_head_commit};
use rg_core::pull_request::{try_auto_merge, MergeStrategy};
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};
use serde_json::{json, Value};

const PW: &str = "Qz7$wRtm";

struct World {
    base: String,
    db: rg_db::DatabaseConnection,
    root: std::path::PathBuf,
    client: Client,
    alice: String,
    alice_id: i64,
}

impl World {
    async fn new(alice: &str) -> World {
        let (base, db, root) = spawn_test_app_on_env_db_with_root().await;
        let client = Client::new();
        let alice_jwt = register_user(&base, alice, &format!("{alice}@example.com"), PW).await;
        let me: Value = client
            .get(format!("{base}/api/v1/users/me"))
            .bearer_auth(&alice_jwt)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let resp = client
            .post(format!("{base}/api/v1/repos"))
            .bearer_auth(&alice_jwt)
            .json(&json!({"name": "work", "auto_init": true, "is_private": false}))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 201);
        World {
            base,
            db,
            root,
            client,
            alice: alice_jwt,
            alice_id: me["id"].as_i64().expect("user id"),
        }
    }

    fn rev(&self, owner: &str, spec: &str) -> Option<String> {
        rg_git::ops::try_rev_parse(&self.root.join(format!("{owner}/work.git")), spec)
            .ok()
            .flatten()
    }

    async fn commit(&self, owner: &str, branch: &str, path: &str) -> String {
        let resp = self
            .client
            .post(format!(
                "{}/api/v1/repos/{owner}/work/contents/{path}",
                self.base
            ))
            .bearer_auth(&self.alice)
            .json(&json!({"branch": branch, "message": format!("add {path}"), "content": "x\n"}))
            .send()
            .await
            .unwrap();
        let status = resp.status();
        let v: Value = resp.json().await.unwrap_or(Value::Null);
        assert_eq!(status, StatusCode::OK, "{v}");
        v["commit_sha"].as_str().unwrap().to_string()
    }

    async fn open_pr(&self, owner: &str, head: &str) -> i64 {
        let resp = self
            .client
            .post(format!("{}/api/v1/repos/{owner}/work/pulls", self.base))
            .bearer_auth(&self.alice)
            .json(&json!({"title": "Change", "head": head, "base": "main"}))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let v: Value = resp.json().await.unwrap();
        v["number"].as_i64().unwrap()
    }

    /// The pull request of *this* test. Scoped by repository on purpose: the
    /// tests of one process share a database (CLAUDE.md pitfall 23), so the
    /// number alone finds another test's pull request.
    async fn pr(&self, number: i64) -> rg_db::entities::pull_request::Model {
        let repo = self.repo().await;
        rg_db::entities::pull_request::Entity::find()
            .filter(rg_db::entities::pull_request::Column::RepoId.eq(repo.id))
            .filter(rg_db::entities::pull_request::Column::Number.eq(number))
            .one(&self.db)
            .await
            .unwrap()
            .expect("pull request row")
    }

    /// Make the recorded head lag behind the branch: the state a merge finds
    /// when a push landed after the evaluation but before the merge.
    async fn record_head(&self, number: i64, sha: &str) {
        let mut active: rg_db::entities::pull_request::ActiveModel = self.pr(number).await.into();
        active.head_sha = Set(Some(sha.to_string()));
        active.update(&self.db).await.unwrap();
    }

    async fn repo(&self) -> rg_db::entities::repository::Model {
        rg_db::entities::repository::Entity::find()
            .filter(rg_db::entities::repository::Column::OwnerId.eq(self.alice_id))
            .filter(rg_db::entities::repository::Column::Name.eq("work"))
            .one(&self.db)
            .await
            .unwrap()
            .expect("repository row")
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_auto_merge_whose_head_moved_waits_for_the_next_evaluation() {
    let w = World::new("am_alice").await;
    let branch = "agent/am";
    let reviewed = w.commit("am_alice", branch, "a.txt").await;
    let number = w.open_pr("am_alice", branch).await;
    rg_core::pull_request::enable_auto_merge(
        &w.db,
        "am_alice",
        "work",
        number,
        MergeStrategy::Squash,
        w.alice_id,
    )
    .await
    .expect("enable auto-merge");

    // The branch moves; the record still says `reviewed`.
    let moved = w.commit("am_alice", branch, "b.txt").await;
    w.record_head(number, &reviewed).await;
    let main_before = w.rev("am_alice", "refs/heads/main").unwrap();

    let outcome = try_auto_merge(&w.db, &w.root, "am_alice", "work", number)
        .await
        .expect("auto-merge runs");
    assert_eq!(outcome.status, "pending", "{outcome:?}");
    assert!(
        outcome.merge.is_none(),
        "nothing may be merged: {outcome:?}"
    );
    assert_eq!(
        w.rev("am_alice", "refs/heads/main").unwrap(),
        main_before,
        "main must not move"
    );
    assert_eq!(w.pr(number).await.state, "open");
    assert!(w.pr(number).await.auto_merge_enabled, "auto-merge stays on");

    // A fresh evaluation pins the commit that is actually there and merges.
    w.record_head(number, &moved).await;
    let outcome = try_auto_merge(&w.db, &w.root, "am_alice", "work", number)
        .await
        .expect("auto-merge runs");
    assert_eq!(outcome.status, "merged", "{outcome:?}");
    assert_eq!(w.pr(number).await.state, "merged");
    assert_ne!(w.rev("am_alice", "refs/heads/main").unwrap(), main_before);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_merge_queue_puts_a_moved_head_back_in_the_queue() {
    let w = World::new("mq_alice").await;
    let branch = "agent/mq";
    let reviewed = w.commit("mq_alice", branch, "a.txt").await;
    let number = w.open_pr("mq_alice", branch).await;
    let repo = w.repo().await;
    let pr = w.pr(number).await;
    enqueue(&w.db, &repo, &pr, w.alice_id, MergeStrategy::Merge)
        .await
        .expect("enqueue");

    // The branch moves after the entry was evaluated.
    let moved = w.commit("mq_alice", branch, "b.txt").await;
    w.record_head(number, &reviewed).await;
    let main_before = w.rev("mq_alice", "refs/heads/main").unwrap();

    let results = process_for_head_commit(&w.db, &w.root, repo.id, &reviewed)
        .await
        .expect("queue runs");
    assert!(
        results.iter().all(|r| r.merged.is_empty()),
        "nothing may be merged: {results:?}"
    );
    assert_eq!(
        w.rev("mq_alice", "refs/heads/main").unwrap(),
        main_before,
        "main must not move"
    );
    let entry = rg_db::ops::merge_queue_ops::find_by_pr(&w.db, pr.id)
        .await
        .unwrap()
        .expect("queue entry");
    assert_eq!(
        entry.status, "queued",
        "a moved head sends the entry back to the queue: {entry:?}"
    );

    // The next push re-evaluates and merges.
    w.record_head(number, &moved).await;
    let results = process_for_head_commit(&w.db, &w.root, repo.id, &moved)
        .await
        .expect("queue runs");
    assert!(
        results.iter().any(|r| !r.merged.is_empty()),
        "the re-evaluated entry merges: {results:?}"
    );
    let entry = rg_db::ops::merge_queue_ops::find_by_pr(&w.db, pr.id)
        .await
        .unwrap()
        .expect("queue entry");
    assert_eq!(entry.status, "merged", "{entry:?}");
}
