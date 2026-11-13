//! `POST /repos/{o}/{n}/commits/{sha}/cherry-pick` and `…/revert`.
//!
//! What each test pins is something that would be a separate incident if it
//! regressed: a conflict must leave the branch exactly where it was; a stale
//! `expected_sha` must not be applied; `require_pr` and the token branch limit
//! must hold for a pick as they do for a file commit; a merge commit must not be
//! picked by guesswork; a success must look like any other REST commit (push
//! event, audit row, identity). Runs against PostgreSQL / MySQL too when
//! `IRONFORGE_TEST_DATABASE_URL` is set.

mod common;
use common::{register_user, spawn_test_app_on_env_db_with_root};

use reqwest::{Client, Method, StatusCode};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

const PW: &str = "Qz7$wRtm";

fn git(args: &[&str], cwd: &Path) -> String {
    let gateway = rg_git::cli_gateway::global_gateway().as_ref().unwrap();
    let out = gateway.run(args, Some(cwd)).unwrap();
    out.ensure_success().unwrap();
    out.stdout_str().trim().to_string()
}

struct Fixture {
    base: String,
    jwt: String,
    owner: &'static str,
    client: Client,
    db: rg_db::DatabaseConnection,
    repo_dir: PathBuf,
}

impl Fixture {
    async fn new(owner: &'static str) -> Fixture {
        let (base, db, root) = spawn_test_app_on_env_db_with_root().await;
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
        let repo_dir = root.join(format!("{owner}/work.git"));
        Fixture {
            base,
            jwt,
            owner,
            client,
            db,
            repo_dir,
        }
    }

    fn url(&self, suffix: &str) -> String {
        format!("{}/api/v1/repos/{}/work{suffix}", self.base, self.owner)
    }

    fn tip(&self, branch: &str) -> String {
        git(
            &["rev-parse", &format!("refs/heads/{branch}")],
            &self.repo_dir,
        )
    }

    /// Start `branch` at the tip of `from` (what a client would do with `git branch`).
    fn branch(&self, branch: &str, from: &str) {
        git(&["branch", branch, from], &self.repo_dir);
    }

    /// One batch commit on `branch`; returns the new tip.
    async fn write(&self, bearer: &str, branch: &str, files: Value) -> String {
        let (status, v) = self
            .post_with(
                "/commits",
                Some(bearer),
                json!({
                    "branch": branch,
                    "expected_sha": self.tip(branch),
                    "message": "seed",
                    "files": files,
                }),
                None,
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{v}");
        v["commit_sha"].as_str().unwrap().to_string()
    }

    async fn create_file(&self, branch: &str, path: &str, content: &str) -> String {
        self.write(
            &self.jwt,
            branch,
            json!([{"operation": "create", "path": path, "content": content}]),
        )
        .await
    }

    async fn update_file(&self, branch: &str, path: &str, content: &str) -> String {
        let sha = git(
            &["rev-parse", &format!("refs/heads/{branch}:{path}")],
            &self.repo_dir,
        );
        self.write(
            &self.jwt,
            branch,
            json!([{"operation": "update", "path": path, "content": content, "sha": sha}]),
        )
        .await
    }

    async fn post_with(
        &self,
        suffix: &str,
        bearer: Option<&str>,
        body: Value,
        header: Option<(&str, &str)>,
    ) -> (StatusCode, Value) {
        let mut req = self.client.request(Method::POST, self.url(suffix));
        if let Some(token) = bearer {
            req = req.bearer_auth(token);
        }
        if let Some((name, value)) = header {
            req = req.header(name, value);
        }
        let resp = req.json(&body).send().await.unwrap();
        let status = resp.status();
        (status, resp.json().await.unwrap_or(Value::Null))
    }

    /// `op` is `cherry-pick` or `revert`.
    async fn pick(&self, op: &str, sha: &str, body: Value) -> (StatusCode, Value) {
        self.pick_as(op, sha, Some(&self.jwt.clone()), body).await
    }

    async fn pick_as(
        &self,
        op: &str,
        sha: &str,
        bearer: Option<&str>,
        body: Value,
    ) -> (StatusCode, Value) {
        self.post_with(&format!("/commits/{sha}/{op}"), bearer, body, None)
            .await
    }

    /// A write token for the owner; `limited` restricts it to this repository's
    /// `agent/*` branches.
    async fn token(&self, limited: bool) -> (i64, String) {
        let mut body = json!({"name": "agent", "scopes": "repo:write"});
        if limited {
            body["repos"] = json!([format!("{}/work", self.owner)]);
            body["refs"] = json!(["agent/*"]);
        }
        let resp = self
            .client
            .post(format!("{}/api/v1/users/tokens", self.base))
            .bearer_auth(&self.jwt)
            .json(&body)
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 201);
        let v: Value = resp.json().await.unwrap();
        (
            v["id"].as_i64().unwrap(),
            v["token"].as_str().unwrap().to_string(),
        )
    }

    async fn audit_row(&self, action: &str) -> Option<Value> {
        use rg_db::sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
        let row = rg_db::entities::audit_log::Entity::find()
            .filter(rg_db::entities::audit_log::Column::Action.eq(action))
            // PostgreSQL / MySQL runs share one database between the tests of a file.
            .filter(rg_db::entities::audit_log::Column::Username.eq(self.owner))
            .one(&self.db)
            .await
            .unwrap()?;
        serde_json::from_str(&row.details.unwrap()).ok()
    }
}

/// A branch `feature` off main with one commit adding `x.txt`; returns that
/// commit and leaves main one commit further on (so a pick is not a fast-forward).
async fn feature_commit(f: &Fixture) -> String {
    f.branch("feature", "main");
    let x = f.create_file("feature", "x.txt", "x\n").await;
    f.create_file("main", "d.txt", "d\n").await;
    x
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_cherry_pick_lands_as_one_commit_and_is_announced_and_audited() {
    let f = Fixture::new("cp_ok").await;
    let x = feature_commit(&f).await;
    let before = f.tip("main");

    let (status, v) = f
        .pick(
            "cherry-pick",
            &x,
            json!({"branch": "main", "expected_sha": before}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{v}");
    let new = v["commit_sha"].as_str().unwrap().to_string();
    assert_eq!(v["dry_run"], json!(false));
    assert_eq!(v["operation"], "cherry_pick");
    assert_eq!(v["tip_before"], json!(before));
    assert_eq!(v["source_sha"], json!(x));

    // The branch moved by exactly this commit, on top of the old tip.
    assert_eq!(f.tip("main"), new);
    assert_eq!(git(&["rev-parse", &format!("{new}^")], &f.repo_dir), before);
    assert_eq!(git(&["show", &format!("{new}:x.txt")], &f.repo_dir), "x");
    assert_eq!(git(&["show", &format!("{new}:d.txt")], &f.repo_dir), "d");
    // The source branch is untouched.
    assert_eq!(f.tip("feature"), x);
    // Message keeps the original and says where it came from; the actor is the
    // author and committer, like every other REST commit.
    assert_eq!(
        git(&["log", "-1", "--format=%B", &new], &f.repo_dir),
        format!("seed\n\n(cherry picked from commit {x})")
    );
    assert_eq!(
        git(
            &["log", "-1", "--format=%an <%ae>|%cn <%ce>", &new],
            &f.repo_dir
        ),
        "cp_ok <cp_ok@example.com>|cp_ok <cp_ok@example.com>"
    );

    // It is announced like a push: the event log gets a `push` for main.
    let mut seen = false;
    for _ in 0..50 {
        let resp = f
            .client
            .get(f.url("/events?since=0&types=push&limit=100"))
            .bearer_auth(&f.jwt)
            .send()
            .await
            .unwrap();
        let body: Value = resp.json().await.unwrap();
        seen = body["events"].as_array().unwrap().iter().any(|e| {
            e["payload"]["after"] == json!(new) && e["payload"]["ref"] == "refs/heads/main"
        });
        if seen {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
    assert!(seen, "no push event for the cherry-picked commit");

    let audit = f.audit_row("repo.cherry_pick").await.expect("audited");
    assert_eq!(audit["branch"], "main");
    assert_eq!(audit["source_sha"], json!(x));
    assert_eq!(audit["commit_sha"], json!(new));
    assert_eq!(audit["before"], json!(before));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_revert_undoes_the_change_in_one_commit_and_is_audited() {
    let f = Fixture::new("rv_ok").await;
    f.create_file("main", "keep.txt", "k\n").await;
    let y = f.create_file("main", "y.txt", "y\n").await;
    f.create_file("main", "later.txt", "l\n").await;
    let before = f.tip("main");

    let (status, v) = f
        .pick(
            "revert",
            &y,
            json!({"branch": "main", "expected_sha": before}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{v}");
    let new = v["commit_sha"].as_str().unwrap().to_string();
    assert_eq!(v["operation"], "revert");
    assert_eq!(f.tip("main"), new);
    assert_eq!(git(&["rev-parse", &format!("{new}^")], &f.repo_dir), before);
    let files = git(&["ls-tree", "--name-only", &new], &f.repo_dir);
    assert!(!files.contains("y.txt"), "{files}");
    assert!(
        files.contains("keep.txt") && files.contains("later.txt"),
        "{files}"
    );
    assert_eq!(
        git(&["log", "-1", "--format=%B", &new], &f.repo_dir),
        format!("Revert \"seed\"\n\nThis reverts commit {y}.")
    );

    let audit = f.audit_row("repo.revert").await.expect("audited");
    assert_eq!(audit["source_sha"], json!(y));
    assert_eq!(audit["commit_sha"], json!(new));

    // A custom message replaces the generated subject, not the trailer.
    let z = f.create_file("main", "z.txt", "z\n").await;
    let (status, v) = f
        .pick(
            "revert",
            &z,
            json!({"branch": "main", "expected_sha": f.tip("main"), "message": "Back out z"}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(
        git(
            &[
                "log",
                "-1",
                "--format=%B",
                v["commit_sha"].as_str().unwrap()
            ],
            &f.repo_dir
        ),
        format!("Back out z\n\nThis reverts commit {z}.")
    );
}

/// A change that does not apply names the conflicting paths and moves nothing —
/// not on a real call, not on a dry run — and writes no object.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_cherry_pick_conflict_is_a_409_with_paths_and_the_branch_stays() {
    let f = Fixture::new("cp_conflict").await;
    f.create_file("main", "c.txt", "base\n").await;
    f.branch("feature", "main");
    let c = f.update_file("feature", "c.txt", "feature\n").await;
    f.update_file("main", "c.txt", "main\n").await;
    let before = f.tip("main");
    let objects = git(&["count-objects", "-v"], &f.repo_dir);

    for dry_run in [false, true] {
        let (status, v) = f
            .pick(
                "cherry-pick",
                &c,
                json!({"branch": "main", "expected_sha": before, "dry_run": dry_run}),
            )
            .await;
        assert_eq!(status, StatusCode::CONFLICT, "{v}");
        assert_eq!(v["error"]["reason"], "MERGE_CONFLICT", "{v}");
        assert_eq!(v["error"]["details"]["paths"], json!(["c.txt"]), "{v}");
        assert_eq!(v["error"]["details"]["tip_sha"], json!(before));
        assert_eq!(v["error"]["details"]["source_sha"], json!(c));
        assert_eq!(v["error"]["retryable"], json!(false));
    }
    assert_eq!(f.tip("main"), before);
    assert_eq!(git(&["count-objects", "-v"], &f.repo_dir), objects);
    assert!(f.audit_row("repo.cherry_pick").await.is_none());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_revert_conflict_is_a_409_with_paths_and_the_branch_stays() {
    let f = Fixture::new("rv_conflict").await;
    f.create_file("main", "c.txt", "one\n").await;
    let b = f.update_file("main", "c.txt", "two\n").await;
    f.update_file("main", "c.txt", "three\n").await;
    let before = f.tip("main");

    let (status, v) = f
        .pick(
            "revert",
            &b,
            json!({"branch": "main", "expected_sha": before}),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{v}");
    assert_eq!(v["error"]["reason"], "MERGE_CONFLICT", "{v}");
    assert_eq!(v["error"]["details"]["operation"], "revert");
    assert_eq!(v["error"]["details"]["paths"], json!(["c.txt"]), "{v}");
    assert_eq!(f.tip("main"), before);
    assert!(f.audit_row("repo.revert").await.is_none());
}

/// Write access is required; so is the `require_pr` verdict of the target
/// branch — a pick cannot be the way around it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_pick_needs_write_access_and_respects_require_pr() {
    let f = Fixture::new("pick_acl").await;
    let x = feature_commit(&f).await;
    let reader = register_user(
        &f.base,
        "pick_acl_reader",
        "pick_acl_reader@example.com",
        PW,
    )
    .await;
    let before = f.tip("main");
    let body = json!({"branch": "main", "expected_sha": before});

    for op in ["cherry-pick", "revert"] {
        // Anonymous, then authenticated without write access.
        let (status, _) = f.pick_as(op, &x, None, body.clone()).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{op}");
        let (status, v) = f.pick_as(op, &x, Some(&reader), body.clone()).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{op}: {v}");
    }
    assert_eq!(f.tip("main"), before);

    let resp = f
        .client
        .post(f.url("/branches/protection"))
        .bearer_auth(&f.jwt)
        .json(&json!({"branch_name": "main", "require_pr": true}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);
    for op in ["cherry-pick", "revert"] {
        for dry_run in [false, true] {
            let (status, v) = f
                .pick(
                    op,
                    &x,
                    json!({"branch": "main", "expected_sha": before, "dry_run": dry_run}),
                )
                .await;
            assert_eq!(status, StatusCode::FORBIDDEN, "{op} dry_run={dry_run}: {v}");
        }
    }
    assert_eq!(f.tip("main"), before);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_stale_expected_sha_is_a_409_ref_advanced_and_nothing_is_applied() {
    let f = Fixture::new("pick_stale").await;
    let x = feature_commit(&f).await;
    let stale = f.tip("main");
    let current = f.create_file("main", "later.txt", "l\n").await;

    for op in ["cherry-pick", "revert"] {
        let (status, v) = f
            .pick(op, &x, json!({"branch": "main", "expected_sha": stale}))
            .await;
        assert_eq!(status, StatusCode::CONFLICT, "{op}: {v}");
        assert_eq!(v["error"]["reason"], "REF_ADVANCED", "{v}");
        assert_eq!(v["error"]["retryable"], json!(true));
        assert_eq!(v["error"]["details"]["current_sha"], json!(current));
        assert_eq!(v["error"]["details"]["expected_sha"], json!(stale));
    }
    assert_eq!(f.tip("main"), current);

    // A branch that does not exist is a stale plan too, never created.
    let (status, v) = f
        .pick(
            "cherry-pick",
            &x,
            json!({"branch": "nope", "expected_sha": current}),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{v}");
    assert_eq!(v["error"]["reason"], "REF_ADVANCED");
    assert!(v["error"]["details"]["current_sha"].is_null());
    // `expected_sha` is required.
    let (status, _) = f.pick("revert", &x, json!({"branch": "main"})).await;
    assert!(status.is_client_error(), "{status}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_dry_run_reports_and_writes_no_ref() {
    let f = Fixture::new("pick_dry").await;
    let x = feature_commit(&f).await;
    let before = f.tip("main");
    let refs = git(&["for-each-ref"], &f.repo_dir);

    let (status, v) = f
        .pick(
            "cherry-pick",
            &x,
            json!({"branch": "main", "expected_sha": before, "dry_run": true}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(v["dry_run"], json!(true));
    assert_eq!(v["would"][0]["effect"], "ref_update");
    assert_eq!(v["would"][0]["old"], json!(before));
    assert_eq!(v["would"][1]["source_sha"], json!(x));
    // The header means the same as the field (here reverting the tip itself).
    let (status, v) = f
        .post_with(
            &format!("/commits/{before}/revert"),
            Some(&f.jwt),
            json!({"branch": "main", "expected_sha": before}),
            Some(("x-dry-run", "1")),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(v["dry_run"], json!(true));

    assert_eq!(f.tip("main"), before);
    assert_eq!(git(&["for-each-ref"], &f.repo_dir), refs);
    assert!(f.audit_row("repo.cherry_pick").await.is_none());

    // The same call without the flag does what the dry run said.
    let (status, v) = f
        .pick(
            "cherry-pick",
            &x,
            json!({"branch": "main", "expected_sha": before}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(f.tip("main"), v["commit_sha"].as_str().unwrap());
}

/// A merge commit is never picked by guesswork: no `mainline` is a 400 with a
/// stable code, a `mainline` on a plain commit or out of range is too, and a
/// valid `mainline` reverts relative to that parent.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_merge_commit_needs_a_mainline() {
    let f = Fixture::new("pick_merge").await;
    f.branch("side", "main");
    f.create_file("side", "s.txt", "s\n").await;
    let plain = f.create_file("main", "m.txt", "m\n").await;
    // A merge of side into main, built with plumbing in the bare repository.
    let tree = git(
        &[
            "merge-tree",
            "--write-tree",
            "refs/heads/main",
            "refs/heads/side",
        ],
        &f.repo_dir,
    );
    let merge = git(
        &[
            "-c",
            "user.name=T",
            "-c",
            "user.email=t@example.com",
            "commit-tree",
            &tree,
            "-p",
            &f.tip("main"),
            "-p",
            &f.tip("side"),
            "-m",
            "merge side",
        ],
        &f.repo_dir,
    );
    git(&["update-ref", "refs/heads/main", &merge], &f.repo_dir);
    let before = f.create_file("main", "z.txt", "z\n").await;

    for op in ["cherry-pick", "revert"] {
        let (status, v) = f
            .pick(
                op,
                &merge,
                json!({"branch": "main", "expected_sha": before}),
            )
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{op}: {v}");
        assert_eq!(v["error"]["code"], "MAINLINE_REQUIRED", "{v}");
        assert_eq!(v["error"]["details"]["parents"], json!(2));
    }
    for (sha, mainline) in [(&merge, 3), (&plain, 1)] {
        let (status, v) = f
            .pick(
                "revert",
                sha,
                json!({"branch": "main", "expected_sha": before, "mainline": mainline}),
            )
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{v}");
        assert_eq!(v["error"]["code"], "MAINLINE_INVALID", "{v}");
    }
    assert_eq!(f.tip("main"), before);

    let (status, v) = f
        .pick(
            "revert",
            &merge,
            json!({"branch": "main", "expected_sha": before, "mainline": 1}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{v}");
    let new = v["commit_sha"].as_str().unwrap();
    let files = git(&["ls-tree", "--name-only", new], &f.repo_dir);
    assert!(
        !files.contains("s.txt") && files.contains("m.txt") && files.contains("z.txt"),
        "{files}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_unknown_commit_is_a_404_and_a_no_op_pick_is_a_409() {
    let f = Fixture::new("pick_misc").await;
    let x = feature_commit(&f).await;
    let before = f.tip("main");
    for sha in ["deadbeefdeadbeefdeadbeefdeadbeefdeadbeef", "HEAD", "main"] {
        let (status, v) = f
            .pick(
                "cherry-pick",
                sha,
                json!({"branch": "main", "expected_sha": before}),
            )
            .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{sha}: {v}");
    }

    // Pick it once, then again: the second has nothing to add.
    let (status, v) = f
        .pick(
            "cherry-pick",
            &x,
            json!({"branch": "main", "expected_sha": before}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{v}");
    let after = f.tip("main");
    let (status, v) = f
        .pick(
            "cherry-pick",
            &x,
            json!({"branch": "main", "expected_sha": after}),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{v}");
    assert_eq!(v["error"]["reason"], "NOTHING_TO_APPLY", "{v}");
    assert_eq!(f.tip("main"), after);
}

/// A token limited to `agent/*` picks inside that limit and not outside it, and
/// the audit row names the token.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_branch_limited_token_picks_only_inside_its_limit() {
    let f = Fixture::new("pick_token").await;
    let x = feature_commit(&f).await;
    let y = f.create_file("main", "y.txt", "y\n").await;
    f.branch("agent/work", "main");
    let (token_id, pat) = f.token(true).await;
    let main_tip = f.tip("main");

    for op in ["cherry-pick", "revert"] {
        let (status, v) = f
            .pick_as(
                op,
                &x,
                Some(&pat),
                json!({"branch": "main", "expected_sha": main_tip}),
            )
            .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{op}: {v}");
    }
    assert_eq!(f.tip("main"), main_tip);

    let (status, v) = f
        .pick_as(
            "cherry-pick",
            &x,
            Some(&pat),
            json!({"branch": "agent/work", "expected_sha": f.tip("agent/work")}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(f.tip("agent/work"), v["commit_sha"].as_str().unwrap());
    let (status, v) = f
        .pick_as(
            "revert",
            &y,
            Some(&pat),
            json!({"branch": "agent/work", "expected_sha": f.tip("agent/work")}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{v}");

    for action in ["repo.cherry_pick", "repo.revert"] {
        let audit = f.audit_row(action).await.expect("audited");
        assert_eq!(audit["access_token_id"], json!(token_id), "{action}");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_retried_idempotency_key_replays_instead_of_picking_twice() {
    let f = Fixture::new("pick_idem").await;
    let x = feature_commit(&f).await;
    let before = f.tip("main");
    let body = json!({"branch": "main", "expected_sha": before});

    let (status, first) = f
        .post_with(
            &format!("/commits/{x}/cherry-pick"),
            Some(&f.jwt),
            body.clone(),
            Some(("idempotency-key", "pick-key-1")),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{first}");
    let moved = f.tip("main");
    assert_eq!(moved, first["commit_sha"].as_str().unwrap());

    let resp = f
        .client
        .post(f.url(&format!("/commits/{x}/cherry-pick")))
        .bearer_auth(&f.jwt)
        .header("idempotency-key", "pick-key-1")
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers()
            .get("idempotent-replay")
            .and_then(|v| v.to_str().ok()),
        Some("true")
    );
    let replayed: Value = resp.json().await.unwrap();
    assert_eq!(replayed["commit_sha"], first["commit_sha"]);
    assert_eq!(f.tip("main"), moved, "the branch did not move again");
}
