//! `ironforge repair-pull-refs` puts back `refs/pull/<n>/head` for pull requests
//! that were opened before the ref existed.
//!
//! The command is an operator tool, so the tests drive the core function the
//! command calls (`rg_core::pull_request::repair_pull_refs`) against the same
//! database and repository root the HTTP tests use, and check the ref with the
//! git CLI. Runs against PostgreSQL / MySQL too when
//! `IRONFORGE_TEST_DATABASE_URL` is set.

mod common;
use common::{register_user, spawn_test_app_on_env_db_with_root};

use reqwest::{Client, StatusCode};
use rg_core::pull_request::{repair_pull_refs, RepairOptions};
use sea_orm::DatabaseConnection;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

const PW: &str = "Qz7$wRtm";

struct World {
    base: String,
    root: PathBuf,
    db: DatabaseConnection,
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
            root,
            db,
            client,
            alice: alice_jwt,
            bob: bob_jwt,
        }
    }

    fn repo_path(&self, owner: &str) -> PathBuf {
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

    async fn open_fork_pull_request(&self, alice: &str, bob: &str) -> i64 {
        let resp = self
            .client
            .post(format!("{}/api/v1/repos/{alice}/work/pulls", self.base))
            .bearer_auth(&self.bob)
            .json(&json!({"title": "From a fork", "head": format!("{bob}:feat"), "base": "main"}))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let v: Value = resp.json().await.unwrap();
        v["number"].as_i64().unwrap()
    }

    fn rev(&self, owner: &str, spec: &str) -> Option<String> {
        rg_git::ops::try_rev_parse(&self.repo_path(owner), spec)
            .ok()
            .flatten()
    }

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

    /// Repair one repository. Scoped with `--repo` on purpose: the tests of one
    /// process share a database (CLAUDE.md pitfall 23), so a whole-instance walk
    /// would also see the repositories of the other tests in this file.
    async fn repair(
        &self,
        owner: &str,
        dry_run: bool,
        include_closed: bool,
    ) -> rg_core::pull_request::RepairReport {
        repair_pull_refs(
            &self.db,
            &self.root,
            &RepairOptions {
                owner: Some(owner.to_string()),
                repo_name: Some("work".to_string()),
                include_closed,
                dry_run,
            },
        )
        .await
        .expect("repair must succeed")
    }
}

fn git(args: &[&str], cwd: &Path) -> String {
    let gateway = rg_git::cli_gateway::global_gateway().as_ref().unwrap();
    let out = gateway.run(args, Some(cwd)).unwrap();
    out.ensure_success().unwrap();
    out.stdout_str().trim().to_string()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_repair_command_restores_a_missing_ref_and_leaves_the_rest_alone() {
    let w = World::new("rp_alice", "rp_bob").await;
    let first = w.commit(&w.bob, "rp_bob", "feat", "one.txt").await;
    let number = w.open_fork_pull_request("rp_alice", "rp_bob").await;
    assert_eq!(
        w.pull_ref_becomes("rp_alice", number, &first)
            .await
            .as_deref(),
        Some(first.as_str()),
        "the ref is published when the pull request is opened"
    );

    // Strip the ref: this is what an old fork pull request looks like.
    git(
        &["update-ref", "-d", &format!("refs/pull/{number}/head")],
        &w.repo_path("rp_alice"),
    );
    assert_eq!(w.rev("rp_alice", &format!("refs/pull/{number}/head")), None);

    // A dry run reports it and writes nothing.
    let dry = w.repair("rp_alice", true, false).await;
    assert_eq!(dry.scanned, 1, "one pull request in scope");
    assert_eq!(dry.planned.len(), 1, "{dry:?}");
    assert!(dry.applied.is_empty(), "dry run must not write");
    assert_eq!(dry.planned[0].kind(), "missing");
    assert!(dry.planned[0].is_fork);
    assert_eq!(
        w.rev("rp_alice", &format!("refs/pull/{number}/head")),
        None,
        "dry run left the ref missing"
    );

    // The real run puts it back, pointing at the fork's tip.
    let done = w.repair("rp_alice", false, false).await;
    assert_eq!(done.applied.len(), 1, "{done:?}");
    assert_eq!(done.planned.len(), 0);
    assert_eq!(
        w.rev("rp_alice", &format!("refs/pull/{number}/head")),
        Some(first.clone())
    );
    // The commit came along too, so the ref resolves in the target.
    assert_eq!(
        git(&["cat-file", "-t", &first], &w.repo_path("rp_alice")),
        "commit"
    );

    // Re-running changes nothing: the command is idempotent.
    let again = w.repair("rp_alice", false, false).await;
    assert_eq!(again.current, 1, "{again:?}");
    assert!(again.applied.is_empty(), "{again:?}");
    assert_eq!(
        w.rev("rp_alice", &format!("refs/pull/{number}/head")),
        Some(first)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_ref_that_points_at_an_older_commit_is_moved_to_the_live_head() {
    let w = World::new("rs_alice", "rs_bob").await;
    let first = w.commit(&w.bob, "rs_bob", "feat", "one.txt").await;
    let number = w.open_fork_pull_request("rs_alice", "rs_bob").await;
    assert_eq!(
        w.pull_ref_becomes("rs_alice", number, &first)
            .await
            .as_deref(),
        Some(first.as_str())
    );

    // The fork branch moves; the push hook follows it.
    let second = w.commit(&w.bob, "rs_bob", "feat", "two.txt").await;
    assert_eq!(
        w.pull_ref_becomes("rs_alice", number, &second)
            .await
            .as_deref(),
        Some(second.as_str())
    );

    // Force the ref back to the older commit: the state a repair has to fix.
    git(
        &["update-ref", &format!("refs/pull/{number}/head"), &first],
        &w.repo_path("rs_alice"),
    );

    let report = w.repair("rs_alice", false, false).await;
    assert_eq!(report.applied.len(), 1, "{report:?}");
    assert_eq!(report.applied[0].kind(), "stale");
    assert_eq!(report.applied[0].previous.as_deref(), Some(first.as_str()));
    assert_eq!(
        w.rev("rs_alice", &format!("refs/pull/{number}/head")),
        Some(second)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_pull_request_whose_head_branch_is_gone_is_reported_not_repaired() {
    let w = World::new("rg_alice", "rg_bob").await;
    let first = w.commit(&w.bob, "rg_bob", "feat", "one.txt").await;
    let number = w.open_fork_pull_request("rg_alice", "rg_bob").await;
    assert_eq!(
        w.pull_ref_becomes("rg_alice", number, &first)
            .await
            .as_deref(),
        Some(first.as_str())
    );

    // The ref disappears and the fork's branch goes with it.
    git(
        &["update-ref", "-d", &format!("refs/pull/{number}/head")],
        &w.repo_path("rg_alice"),
    );
    git(
        &["update-ref", "-d", "refs/heads/feat"],
        &w.repo_path("rg_bob"),
    );

    let report = w.repair("rg_alice", false, false).await;
    assert_eq!(report.scanned, 1, "{report:?}");
    assert!(report.applied.is_empty(), "nothing to point at: {report:?}");
    assert_eq!(report.skipped.len(), 1, "{report:?}");
    assert_eq!(report.skipped[0].number, Some(number));
    assert!(
        report.skipped[0]
            .reason
            .contains("head branch no longer exists"),
        "{report:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn closed_pull_requests_are_out_of_scope_unless_asked_for() {
    let w = World::new("rc_alice", "rc_bob").await;
    let first = w.commit(&w.bob, "rc_bob", "feat", "one.txt").await;
    let number = w.open_fork_pull_request("rc_alice", "rc_bob").await;
    assert_eq!(
        w.pull_ref_becomes("rc_alice", number, &first)
            .await
            .as_deref(),
        Some(first.as_str())
    );
    let resp = w
        .client
        .patch(format!(
            "{}/api/v1/repos/rc_alice/work/pulls/{number}",
            w.base
        ))
        .bearer_auth(&w.alice)
        .json(&json!({"state": "closed"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    git(
        &["update-ref", "-d", &format!("refs/pull/{number}/head")],
        &w.repo_path("rc_alice"),
    );

    let default = w.repair("rc_alice", false, false).await;
    assert_eq!(
        default.scanned, 0,
        "closed PRs are skipped by default: {default:?}"
    );
    assert_eq!(w.rev("rc_alice", &format!("refs/pull/{number}/head")), None);

    let widened = w.repair("rc_alice", false, true).await;
    assert_eq!(
        widened.scanned, 1,
        "--include-closed brings it in: {widened:?}"
    );
    assert_eq!(widened.applied.len(), 1, "{widened:?}");
    assert_eq!(
        w.rev("rc_alice", &format!("refs/pull/{number}/head")),
        Some(first)
    );
}
