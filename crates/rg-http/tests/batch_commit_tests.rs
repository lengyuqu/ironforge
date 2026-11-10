//! `POST /repos/{owner}/{name}/commits` writes many file edits as **one**
//! commit, or as none.
//!
//! The first test is the reason the route exists: a batch whose third edit
//! fails must leave the branch exactly where it was, with the first two edits
//! nowhere on disk. The rest pin the parts that would each be a separate
//! incident if they regressed — the branch protection verdict, the token branch
//! limit, `Idempotency-Key`, the required base commit, and the `move` cycle
//! refusal.

mod common;
use common::{register_user, spawn_test_app};

use reqwest::{Client, StatusCode};
use serde_json::{json, Value};

const PW: &str = "Qz7$wRtm";

struct Fixture {
    base: String,
    jwt: String,
    owner: &'static str,
    client: Client,
}

impl Fixture {
    async fn new(owner: &'static str) -> Fixture {
        let base = spawn_test_app().await;
        let jwt = register_user(&base, owner, &format!("{owner}@example.com"), PW).await;
        let client = Client::new();
        let resp = client
            .post(format!("{base}/api/v1/repos"))
            .bearer_auth(&jwt)
            .json(&json!({"name": "work", "auto_init": true, "is_private": false}))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 201);
        Fixture {
            base,
            jwt,
            owner,
            client,
        }
    }

    fn url(&self, suffix: &str) -> String {
        format!("{}/api/v1/repos/{}/work{suffix}", self.base, self.owner)
    }

    /// A branch tip, read from the server's own git endpoint.
    fn tip(&self, branch: &str) -> Option<String> {
        let remote = format!("{}/git/{}/work", self.base, self.owner);
        let gateway = rg_git::cli_gateway::global_gateway().as_ref().unwrap();
        let out = gateway
            .run(
                &["ls-remote", &remote, &format!("refs/heads/{branch}")],
                None,
            )
            .unwrap();
        out.stdout_str()
            .split_whitespace()
            .next()
            .map(str::to_string)
    }

    /// The blob sha of `path` on `branch`, read through the API.
    async fn blob(&self, branch: &str, path: &str) -> Option<String> {
        let resp = self
            .client
            .get(self.url(&format!("/blob/{path}")))
            .query(&[("ref", branch)])
            .bearer_auth(&self.jwt)
            .send()
            .await
            .ok()?;
        if resp.status() != StatusCode::OK {
            return None;
        }
        let body: Value = resp.json().await.ok()?;
        body["sha"].as_str().map(str::to_string)
    }

    /// Read a file's content on `branch` through the API.
    async fn content(&self, branch: &str, path: &str) -> Option<String> {
        let resp = self
            .client
            .get(self.url(&format!("/blob/{path}")))
            .query(&[("ref", branch)])
            .bearer_auth(&self.jwt)
            .send()
            .await
            .unwrap();
        if resp.status() != StatusCode::OK {
            return None;
        }
        let body: Value = resp.json().await.unwrap();
        body["content"].as_str().map(str::to_string)
    }

    async fn commit(&self, body: Value) -> (StatusCode, Value) {
        self.commit_with(body, None).await
    }

    async fn commit_with(
        &self,
        body: Value,
        header: Option<(&str, String)>,
    ) -> (StatusCode, Value) {
        let mut req = self
            .client
            .post(self.url("/commits"))
            .bearer_auth(&self.jwt)
            .json(&body);
        if let Some((name, value)) = header {
            req = req.header(name, value);
        }
        let resp = req.send().await.unwrap();
        let status = resp.status();
        (status, resp.json().await.unwrap_or(Value::Null))
    }

    /// A single-file commit through the existing route, returning the new tip.
    async fn single_file_commit(&self, branch: &str, path: &str, content: &str) -> String {
        let (status, v) = self
            .client
            .post(self.url(&format!("/contents/{path}")))
            .bearer_auth(&self.jwt)
            .json(&json!({"branch": branch, "message": "seed", "content": content}))
            .send()
            .await
            .unwrap()
            .json::<Value>()
            .await
            .map(|v| (StatusCode::OK, v))
            .unwrap();
        assert_eq!(status, StatusCode::OK, "{v}");
        v["commit_sha"].as_str().unwrap().to_string()
    }
}

/// The whole point of the route: a batch is all-or-nothing.
///
/// Three edits, the third refuses (it pins a blob sha that is not there). The
/// first two must not land: the branch tip must be byte-identical to what it
/// was, and neither file may exist on the branch.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_batch_that_fails_on_its_third_edit_leaves_the_branch_untouched() {
    let f = Fixture::new("atomic_user").await;
    let tip = f.tip("main").expect("main exists");

    let (status, v) = f
        .commit(json!({
            "branch": "main",
            "expected_sha": tip,
            "message": "three files, one bad",
            "files": [
                {"operation": "create", "path": "a.txt", "content": "a\n"},
                {"operation": "create", "path": "b.txt", "content": "b\n"},
                // Pins a blob that cannot be there for a file that does not
                // exist yet: refused as "missing at HEAD".
                {"operation": "update", "path": "c.txt", "content": "c\n",
                 "sha": "1111111111111111111111111111111111111111"},
            ],
        }))
        .await;

    assert_ne!(
        status,
        StatusCode::OK,
        "a failing batch must not succeed: {v}"
    );
    // The branch did not move — not even to a two-file commit.
    assert_eq!(
        f.tip("main"),
        Some(tip.clone()),
        "the third edit failed, so the first two must not have been committed"
    );
    assert_eq!(
        f.content("main", "a.txt").await,
        None,
        "a.txt must not exist"
    );
    assert_eq!(
        f.content("main", "b.txt").await,
        None,
        "b.txt must not exist"
    );
    assert_eq!(f.content("main", "c.txt").await, None);
    // No ref was created on the side either.
    assert_eq!(f.tip("main"), Some(tip));
}

/// A successful batch is exactly one commit, carrying every edit.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_batch_of_three_edits_is_one_commit() {
    let f = Fixture::new("onecommit_user").await;
    let tip = f.tip("main").expect("main exists");

    let (status, v) = f
        .commit(json!({
            "branch": "main",
            "expected_sha": tip,
            "message": "one commit, three files",
            "files": [
                {"operation": "create", "path": "a.txt", "content": "a\n"},
                {"operation": "create", "path": "dir/b.txt", "content": "b\n"},
                {"operation": "create", "path": "dir/c.txt", "content": "c\n"},
            ],
        }))
        .await;
    assert_eq!(status, StatusCode::OK, "{v}");

    let commit_sha = v["commit_sha"].as_str().expect("a commit sha");
    assert_eq!(v["dry_run"], json!(false));
    assert_eq!(v["tip_before"], json!(tip));
    assert_eq!(v["branch"], "main");
    let files = v["files"].as_array().unwrap();
    assert_eq!(files.len(), 3);
    assert_eq!(files[0]["operation"], "create");
    assert_eq!(files[0]["path"], "a.txt");

    // The branch moved by exactly one commit, and that commit is the parent's
    // only child: `git rev-list --count tip..new` == 1.
    assert_eq!(f.tip("main"), Some(commit_sha.to_string()));
    assert_eq!(f.content("main", "a.txt").await.as_deref(), Some("a\n"));
    assert_eq!(f.content("main", "dir/b.txt").await.as_deref(), Some("b\n"));
    assert_eq!(f.content("main", "dir/c.txt").await.as_deref(), Some("c\n"));
    let log = f
        .client
        .get(f.url("/log"))
        .query(&[("ref", "main"), ("limit", "2")])
        .bearer_auth(&f.jwt)
        .send()
        .await
        .unwrap()
        .json::<Value>()
        .await
        .unwrap();
    let log = log["commits"].as_array().expect("a list of commits");
    assert_eq!(
        log[0]["sha"],
        json!(commit_sha),
        "the batch commit is the tip"
    );
    assert_eq!(log[1]["sha"], json!(tip), "and it sits on the old tip");
    assert_eq!(log[0]["message"], "one commit, three files");
}

/// The batch route is held to the same branch protection as the single-file
/// route: `require_pr` cannot be walked around by sending many edits at once.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_batch_respects_require_pr() {
    let f = Fixture::new("batch_perm_user").await;
    let tip = f.tip("main").unwrap();

    let resp = f
        .client
        .post(f.url("/branches/protection"))
        .bearer_auth(&f.jwt)
        .json(&json!({"branch_name": "main", "require_pr": true}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);

    let (status, v) = f
        .commit(json!({
            "branch": "main",
            "expected_sha": tip,
            "message": "bypass attempt",
            "files": [{"operation": "create", "path": "x.txt", "content": "x\n"}],
        }))
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{v}");
    assert_eq!(f.tip("main"), Some(tip.clone()));
    assert_eq!(f.content("main", "x.txt").await, None);

    // The verdict is the same as the single-file route's, not a different one.
    let single = f
        .client
        .post(f.url("/contents/x.txt"))
        .bearer_auth(&f.jwt)
        .json(&json!({"branch": "main", "message": "m", "content": "x\n"}))
        .send()
        .await
        .unwrap();
    assert_eq!(single.status(), StatusCode::FORBIDDEN);

    // A dry run is refused too: it reports what would happen, and nothing would.
    let (status, _) = f
        .commit(json!({
            "branch": "main",
            "expected_sha": tip,
            "message": "m",
            "dry_run": true,
            "files": [{"operation": "create", "path": "x.txt", "content": "x\n"}],
        }))
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

/// `branch` is required and `expected_sha` is not defaulted to HEAD: a batch is
/// a plan replayed against a branch the caller already read.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_batch_requires_a_branch_and_a_base_commit() {
    let f = Fixture::new("batch_base_user").await;
    let tip = f.tip("main").unwrap();

    // No `branch`.
    let (status, v) = f
        .commit(json!({
            "expected_sha": tip,
            "message": "m",
            "files": [{"operation": "create", "path": "x.txt", "content": "x\n"}],
        }))
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{v}");

    // No `expected_sha`: not defaulted to HEAD.
    let (status, v) = f
        .commit(json!({
            "branch": "main",
            "message": "m",
            "files": [{"operation": "create", "path": "x.txt", "content": "x\n"}],
        }))
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{v}");

    // A branch that does not exist is a stale plan, not "create it from HEAD".
    let (status, v) = f
        .commit(json!({
            "branch": "agent/missing",
            "expected_sha": tip,
            "message": "m",
            "files": [{"operation": "create", "path": "x.txt", "content": "x\n"}],
        }))
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{v}");
    assert_eq!(v["error"]["reason"], "REF_ADVANCED");
    assert_eq!(v["error"]["details"]["current_sha"], Value::Null);
    assert_eq!(f.tip("agent/missing"), None, "no branch is created");

    // A base commit that is no longer the tip: 409 REF_ADVANCED, nothing written.
    let moved = f.single_file_commit("main", "seed.txt", "s\n").await;
    let (status, v) = f
        .commit(json!({
            "branch": "main",
            "expected_sha": tip,
            "message": "m",
            "files": [{"operation": "create", "path": "x.txt", "content": "x\n"}],
        }))
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{v}");
    assert_eq!(v["error"]["reason"], "REF_ADVANCED");
    assert_eq!(v["error"]["details"]["current_sha"], json!(moved));
    assert_eq!(v["error"]["details"]["expected_sha"], json!(tip));
    assert_eq!(f.tip("main"), Some(moved));
    assert_eq!(f.content("main", "x.txt").await, None);
}

/// A move chain that closes a cycle is refused instead of ordered: there is no
/// order the caller could rely on, so guessing one would silently lose a file.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_move_cycle_is_refused() {
    let f = Fixture::new("batch_cycle_user").await;
    f.single_file_commit("main", "a.txt", "a\n").await;
    let tip = f.single_file_commit("main", "b.txt", "b\n").await;
    let a_sha = f.blob("main", "a.txt").await.unwrap();
    let b_sha = f.blob("main", "b.txt").await.unwrap();

    // a→b and b→a: the destination set intersects the source set.
    let (status, v) = f
        .commit(json!({
            "branch": "main",
            "expected_sha": tip,
            "message": "swap",
            "files": [
                {"operation": "move", "from": "a.txt", "to": "b.txt", "sha": a_sha},
                {"operation": "move", "from": "b.txt", "to": "a.txt", "sha": b_sha},
            ],
        }))
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{v}");
    assert_eq!(v["error"]["reason"], "PATH_CONFLICT");
    assert_eq!(f.tip("main"), Some(tip.clone()));
    assert_eq!(f.content("main", "a.txt").await.as_deref(), Some("a\n"));
    assert_eq!(f.content("main", "b.txt").await.as_deref(), Some("b\n"));

    // A cycle through three files is refused the same way.
    let tip = f.single_file_commit("main", "c.txt", "c\n").await;
    let c_sha = f.blob("main", "c.txt").await.unwrap();
    let (status, v) = f
        .commit(json!({
            "branch": "main",
            "expected_sha": tip,
            "message": "rotate",
            "files": [
                {"operation": "move", "from": "a.txt", "to": "b.txt", "sha": a_sha},
                {"operation": "move", "from": "b.txt", "to": "c.txt", "sha": b_sha},
                {"operation": "move", "from": "c.txt", "to": "a.txt", "sha": c_sha},
            ],
        }))
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{v}");
    assert_eq!(v["error"]["reason"], "PATH_CONFLICT");
    assert_eq!(f.tip("main"), Some(tip));
}

/// A move that does not close a cycle works, keeps the content, and never
/// overwrites the destination.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_move_keeps_content_and_will_not_overwrite() {
    let f = Fixture::new("batch_move_user").await;
    f.single_file_commit("main", "src/old.txt", "payload\n")
        .await;
    let tip = f.single_file_commit("main", "taken.txt", "keep me\n").await;
    let old_sha = f.blob("main", "src/old.txt").await.unwrap();

    // Destination occupied: refused, nothing moved.
    let (status, v) = f
        .commit(json!({
            "branch": "main",
            "expected_sha": tip,
            "message": "move onto a file",
            "files": [{"operation": "move", "from": "src/old.txt", "to": "taken.txt", "sha": old_sha}],
        }))
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{v}");
    assert_eq!(f.tip("main"), Some(tip.clone()));

    // The real move, together with a create in the same commit.
    let (status, v) = f
        .commit(json!({
            "branch": "main",
            "expected_sha": tip,
            "message": "rename",
            "files": [
                {"operation": "move", "from": "src/old.txt", "to": "src/new.txt", "sha": old_sha},
                {"operation": "create", "path": "notes.md", "content": "n\n"},
            ],
        }))
        .await;
    assert_eq!(status, StatusCode::OK, "{v}");
    let files = v["files"].as_array().unwrap();
    assert_eq!(files[0]["operation"], "move");
    assert_eq!(files[0]["from"], "src/old.txt");
    assert_eq!(files[0]["path"], "src/new.txt");
    assert_eq!(files[0]["blob_before"], json!(old_sha));

    assert_eq!(
        f.content("main", "src/new.txt").await.as_deref(),
        Some("payload\n")
    );
    assert_eq!(
        f.content("main", "src/old.txt").await,
        None,
        "source is gone"
    );
    assert_eq!(
        f.content("main", "taken.txt").await.as_deref(),
        Some("keep me\n")
    );
    assert_eq!(f.content("main", "notes.md").await.as_deref(), Some("n\n"));
    // The move carried the object, it did not re-hash it.
    assert_eq!(
        f.blob("main", "src/new.txt").await.as_deref(),
        Some(old_sha.as_str())
    );
}

/// A dry run reports the same shape as the single-file route and writes nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_dry_run_reports_and_writes_nothing() {
    let f = Fixture::new("batch_dry_user").await;
    f.single_file_commit("main", "live.txt", "before\n").await;
    let tip = f.tip("main").unwrap();
    let live_sha = f.blob("main", "live.txt").await.unwrap();

    // Body flag and header both mean dry run, and both report the same shape.
    let cases = [(None, true), (Some(("x-dry-run", "1")), false)];
    for (header, via_body) in cases {
        let mut body = json!({
            "branch": "main",
            "expected_sha": tip,
            "message": "many",
            "files": [
                {"operation": "create", "path": "new.txt", "content": "n\n"},
                {"operation": "update", "path": "live.txt", "content": "after\n", "sha": live_sha},
            ],
        });
        if via_body {
            body["dry_run"] = json!(true);
        }
        let (status, v) = f
            .commit_with(body, header.map(|(n, v)| (n, v.to_string())))
            .await;
        assert_eq!(status, StatusCode::OK, "{v}");
        assert_eq!(v["dry_run"], json!(true));
        let would = v["would"].as_array().unwrap();
        assert_eq!(would[0]["effect"], "ref_update");
        assert_eq!(would[0]["ref"], "refs/heads/main");
        assert_eq!(would[0]["old"], json!(tip));
        assert_eq!(would[0]["creates_branch"], json!(false));
        assert_eq!(would[1]["effect"], "file");
        assert_eq!(would[1]["operation"], "create");
        assert_eq!(would[1]["path"], "new.txt");
        assert_eq!(would[2]["operation"], "update");
        assert_eq!(would[2]["path"], "live.txt");
        assert_eq!(would[2]["blob_before"], json!(live_sha));
        assert!(v["not_evaluated"].as_array().unwrap().len() >= 3);
        assert_eq!(v["commit_sha"], Value::Null);

        // `tip()` is unchanged: no commit, no ref move, no file.
        assert_eq!(f.tip("main"), Some(tip.clone()));
    }
    assert_eq!(f.content("main", "new.txt").await, None);
    assert_eq!(
        f.content("main", "live.txt").await.as_deref(),
        Some("before\n")
    );

    // The checks are real: a dry run fails where the real call would.
    let (status, v) = f
        .commit(json!({
            "branch": "main",
            "expected_sha": tip,
            "message": "m",
            "dry_run": true,
            "files": [
                {"operation": "create", "path": "live.txt", "content": "x\n"},
            ],
        }))
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{v}");
    assert_eq!(f.tip("main"), Some(tip.clone()));

    // And for real the same batch does write, in one commit.
    let (status, v) = f
        .commit(json!({
            "branch": "main",
            "expected_sha": tip,
            "message": "m",
            "files": [
                {"operation": "create", "path": "new.txt", "content": "n\n"},
                {"operation": "update", "path": "live.txt", "content": "after\n", "sha": live_sha},
            ],
        }))
        .await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_ne!(f.tip("main"), Some(tip));
    assert_eq!(
        f.content("main", "live.txt").await.as_deref(),
        Some("after\n")
    );
}

/// An agent that lost the connection retries with the same `Idempotency-Key`;
/// it must get the first answer back, not a second commit.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_batch_is_idempotent_under_a_retried_key() {
    let f = Fixture::new("batch_idem_user").await;
    let tip = f.tip("main").unwrap();
    let body = json!({
        "branch": "main",
        "expected_sha": tip,
        "message": "retried",
        "files": [
            {"operation": "create", "path": "one.txt", "content": "1\n"},
            {"operation": "create", "path": "two.txt", "content": "2\n"},
        ],
    });

    let (status, first) = f
        .commit_with(
            body.clone(),
            Some(("idempotency-key", "batch-key-1".to_string())),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{first}");
    let commit_sha = first["commit_sha"].as_str().unwrap().to_string();
    let moved = f.tip("main").unwrap();
    assert_eq!(moved, commit_sha);

    // Same key, same body: the first answer, not a second commit.
    let resp = f
        .client
        .post(f.url("/commits"))
        .bearer_auth(&f.jwt)
        .header("idempotency-key", "batch-key-1")
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let replay_header = resp
        .headers()
        .get("idempotent-replay")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let replayed: Value = resp.json().await.unwrap();
    assert_eq!(
        replay_header.as_deref(),
        Some("true"),
        "the answer is replayed"
    );
    assert_eq!(replayed["commit_sha"], json!(commit_sha));
    assert_eq!(
        f.tip("main"),
        Some(moved.clone()),
        "the branch did not move again"
    );

    // A different body under the same key is refused, not silently replayed.
    let (status, v) = f
        .commit_with(
            json!({
                "branch": "main",
                "expected_sha": moved,
                "message": "different",
                "files": [{"operation": "create", "path": "three.txt", "content": "3\n"}],
            }),
            Some(("idempotency-key", "batch-key-1".to_string())),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{v}");
    assert_eq!(v["error"]["reason"], "IDEMPOTENCY_KEY_REUSED");
    assert_eq!(f.tip("main"), Some(moved));
}

/// The batch route is on the same permission surface as `/contents`: a read-only
/// collaborator cannot use it, and an anonymous caller is not authenticated.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_batch_needs_write_access() {
    let f = Fixture::new("batch_acl_owner").await;
    let other = register_user(
        &f.base,
        "batch_acl_reader",
        "batch_acl_reader@example.com",
        PW,
    )
    .await;
    let tip = f.tip("main").unwrap();
    let body = json!({
        "branch": "main",
        "expected_sha": tip,
        "message": "m",
        "files": [{"operation": "create", "path": "x.txt", "content": "x\n"}],
    });

    // Anonymous.
    let resp = f
        .client
        .post(f.url("/commits"))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    // Authenticated, but not a collaborator.
    let resp = f
        .client
        .post(f.url("/commits"))
        .bearer_auth(&other)
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    assert_eq!(f.tip("main"), Some(tip));
}

/// A batch that contradicts itself is refused before anything is written.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_self_contradicting_batch_is_refused() {
    let f = Fixture::new("batch_conflict_user").await;
    let tip = f.tip("main").unwrap();

    // Two edits write the same path.
    let (status, v) = f
        .commit(json!({
            "branch": "main",
            "expected_sha": tip,
            "message": "m",
            "files": [
                {"operation": "create", "path": "same.txt", "content": "1\n"},
                {"operation": "create", "path": "same.txt", "content": "2\n"},
            ],
        }))
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{v}");
    assert_eq!(v["error"]["reason"], "PATH_CONFLICT");
    assert_eq!(f.tip("main"), Some(tip.clone()));

    // An empty batch says nothing.
    let (status, v) = f
        .commit(json!({
            "branch": "main",
            "expected_sha": tip,
            "message": "m",
            "files": [],
        }))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{v}");

    // A path that escapes the repository is refused.
    let (status, v) = f
        .commit(json!({
            "branch": "main",
            "expected_sha": tip,
            "message": "m",
            "files": [{"operation": "create", "path": "../escape.txt", "content": "x\n"}],
        }))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{v}");
    assert_eq!(f.tip("main"), Some(tip));
}
