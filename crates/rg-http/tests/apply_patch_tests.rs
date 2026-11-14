//! `POST /repos/{o}/{n}/commits/apply-patch`.
//!
//! The endpoint is a write fed by arbitrary text, so the tests pin the things
//! that would be an incident if they regressed: a patch that does not apply
//! must leave the branch exactly where it was; a patch that escapes the
//! repository or names `.git` must be refused before anything is parsed; the
//! size and file-count limits must not fail open; a secret in the patch must be
//! refused like a push; `expected_sha` must be a real optimistic lock; and a
//! dry run must write nothing at all — no ref, no object, no audit row.
//!
//! The success path is pinned too: one commit on top of the tip, authored by the
//! actor, announced like a push and audited. Runs against PostgreSQL / MySQL as
//! well when `IRONFORGE_TEST_DATABASE_URL` is set.

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

/// A unified diff that creates `path` with `content` (one line per array item).
fn add_file_patch(path: &str, lines: &[&str]) -> String {
    let body: String = lines.iter().map(|line| format!("+{line}\n")).collect();
    format!(
        "diff --git a/{path} b/{path}\nnew file mode 100644\n--- /dev/null\n+++ b/{path}\n@@ -0,0 +1,{} @@\n{body}",
        lines.len()
    )
}

/// A unified diff that rewrites the single-line file `path`.
fn replace_line_patch(path: &str, from: &str, to: &str) -> String {
    format!(
        "diff --git a/{path} b/{path}\n--- a/{path}\n+++ b/{path}\n@@ -1 +1 @@\n-{from}\n+{to}\n"
    )
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

    fn branch(&self, branch: &str, from: &str) {
        git(&["branch", branch, from], &self.repo_dir);
    }

    /// One file committed through `/commits`; returns the new tip.
    async fn create_file(&self, branch: &str, path: &str, content: &str) -> String {
        let (status, v) = self
            .post_with(
                "/commits",
                Some(&self.jwt),
                json!({
                    "branch": branch,
                    "expected_sha": self.tip(branch),
                    "message": "seed",
                    "files": [{"operation": "create", "path": path, "content": content}],
                }),
                None,
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{v}");
        v["commit_sha"].as_str().unwrap().to_string()
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

    async fn apply(&self, body: Value) -> (StatusCode, Value) {
        self.apply_as(Some(&self.jwt.clone()), body).await
    }

    async fn apply_as(&self, bearer: Option<&str>, body: Value) -> (StatusCode, Value) {
        self.post_with("/commits/apply-patch", bearer, body, None)
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
            .filter(rg_db::entities::audit_log::Column::Username.eq(self.owner))
            .one(&self.db)
            .await
            .unwrap()?;
        serde_json::from_str(&row.details.unwrap()).ok()
    }

    /// The secret-scan rows are written without a username (only the actor id),
    /// so they are looked up by action alone.
    async fn scan_audit_row(&self, action: &str) -> Option<Value> {
        use rg_db::sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
        let row = rg_db::entities::audit_log::Entity::find()
            .filter(rg_db::entities::audit_log::Column::Action.eq(action))
            .one(&self.db)
            .await
            .unwrap()?;
        serde_json::from_str(&row.details.unwrap()).ok()
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_patch_lands_as_one_commit_and_is_announced_and_audited() {
    let f = Fixture::new("ap_ok").await;
    f.create_file("main", "a.txt", "one\n").await;
    let before = f.tip("main");
    let patch = format!(
        "{}{}",
        replace_line_patch("a.txt", "one", "ONE"),
        add_file_patch("b.txt", &["hello"])
    );

    let (status, v) = f
        .apply(json!({"branch": "main", "expected_sha": before, "patch": patch}))
        .await;
    assert_eq!(status, StatusCode::OK, "{v}");
    let commit = v["commit_sha"].as_str().unwrap().to_string();
    assert_eq!(v["dry_run"], json!(false));
    assert_eq!(v["branch"], "main");
    assert_eq!(v["tip_before"], json!(before));
    assert_eq!(v["files_changed"], json!(2));
    assert_eq!(
        v["changes"],
        json!([
            {"path": "a.txt", "status": "modified"},
            {"path": "b.txt", "status": "added"},
        ]),
        "{v}"
    );
    let tree = v["tree_sha"].as_str().unwrap();
    assert_eq!(
        git(&["rev-parse", &format!("{commit}^{{tree}}")], &f.repo_dir),
        tree
    );

    // The branch moved by exactly this commit, on top of the old tip.
    assert_eq!(f.tip("main"), commit);
    assert_eq!(
        git(&["rev-parse", &format!("{commit}^")], &f.repo_dir),
        before
    );
    assert_eq!(
        git(&["show", &format!("{commit}:a.txt")], &f.repo_dir),
        "ONE"
    );
    assert_eq!(
        git(&["show", &format!("{commit}:b.txt")], &f.repo_dir),
        "hello"
    );
    // The actor is author and committer, as on every other REST commit, and the
    // message is generated when the caller omits one.
    assert_eq!(
        git(
            &["log", "-1", "--format=%an <%ae>|%cn <%ce>", &commit],
            &f.repo_dir
        ),
        "ap_ok <ap_ok@example.com>|ap_ok <ap_ok@example.com>"
    );
    assert_eq!(
        git(&["log", "-1", "--format=%B", &commit], &f.repo_dir),
        "Apply patch to main\n\n2 file(s) changed."
    );

    // Announced like a push: the event log gets a `push` for main.
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
            e["payload"]["after"] == json!(commit) && e["payload"]["ref"] == "refs/heads/main"
        });
        if seen {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
    assert!(seen, "no push event for the applied patch");

    let audit = f.audit_row("repo.apply_patch").await.expect("audited");
    assert_eq!(audit["branch"], "main");
    assert_eq!(audit["commit_sha"], json!(commit));
    assert_eq!(audit["before"], json!(before));
    assert_eq!(audit["files_changed"], json!(2));
    assert_eq!(audit["paths"], json!(["a.txt", "b.txt"]));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_custom_message_is_kept() {
    let f = Fixture::new("ap_msg").await;
    f.create_file("main", "a.txt", "one\n").await;
    let (status, v) = f
        .apply(json!({
            "branch": "main",
            "expected_sha": f.tip("main"),
            "patch": add_file_patch("n.txt", &["n"]),
            "message": "Add n",
        }))
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
        "Add n"
    );
}

/// A patch whose context does not match is a 409 with the paths git named, and
/// the branch does not move. Unlike a merge conflict there is nothing to resolve
/// — the caller re-reads the branch and rebases.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_patch_that_does_not_apply_is_a_409_and_the_branch_stays() {
    let f = Fixture::new("ap_conflict").await;
    f.create_file("main", "c.txt", "one\n").await;
    let before = f.tip("main");
    let objects = git(&["count-objects", "-v"], &f.repo_dir);

    let (status, v) = f
        .apply(json!({
            "branch": "main",
            "expected_sha": before,
            "patch": replace_line_patch("c.txt", "two", "three"),
        }))
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{v}");
    assert_eq!(v["error"]["reason"], "PATCH_CONFLICT", "{v}");
    assert_eq!(v["error"]["retryable"], json!(false));
    assert_eq!(v["error"]["details"]["paths"], json!(["c.txt"]), "{v}");
    assert_eq!(v["error"]["details"]["tip_sha"], json!(before));

    assert_eq!(f.tip("main"), before);
    // The preview runs in a scratch object database, so a refused patch leaves
    // not even an unreferenced blob behind.
    assert_eq!(git(&["count-objects", "-v"], &f.repo_dir), objects);
    assert!(f.audit_row("repo.apply_patch").await.is_none());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_dry_run_reports_the_change_and_writes_nothing() {
    let f = Fixture::new("ap_dry").await;
    f.create_file("main", "a.txt", "one\n").await;
    let before = f.tip("main");
    let raw = git(&["for-each-ref"], &f.repo_dir);
    let objects = git(&["count-objects", "-v"], &f.repo_dir);
    let patch = format!(
        "{}{}",
        replace_line_patch("a.txt", "one", "ONE"),
        add_file_patch("b.txt", &["hello"])
    );

    let (status, v) = f
        .apply(json!({
            "branch": "main",
            "expected_sha": before,
            "patch": patch,
            "dry_run": true,
        }))
        .await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(v["dry_run"], json!(true));
    assert_eq!(v["tip_before"], json!(before));
    assert_eq!(v["files_changed"], json!(2));
    assert_eq!(
        v["changes"],
        json!([
            {"path": "a.txt", "status": "modified"},
            {"path": "b.txt", "status": "added"},
        ])
    );
    assert_eq!(v["would"][0]["effect"], "ref_update");
    assert_eq!(v["would"][0]["old"], json!(before));
    assert!(v["tree_sha"].as_str().is_some_and(|tree| !tree.is_empty()));
    // `X-Dry-Run: 1` means the same.
    let (status, v) = f
        .post_with(
            "/commits/apply-patch",
            Some(&f.jwt),
            json!({"branch": "main", "expected_sha": before, "patch": patch}),
            Some(("x-dry-run", "1")),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(v["dry_run"], json!(true));

    // Nothing moved, no object was written, no audit row.
    assert_eq!(f.tip("main"), before);
    assert_eq!(git(&["for-each-ref"], &f.repo_dir), raw);
    assert_eq!(git(&["count-objects", "-v"], &f.repo_dir), objects);
    assert!(f.audit_row("repo.apply_patch").await.is_none());

    // The same call without the flag does what the dry run said.
    let (status, v) = f
        .apply(json!({"branch": "main", "expected_sha": before, "patch": patch}))
        .await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(f.tip("main"), v["commit_sha"].as_str().unwrap());
}

/// A patch that would write outside the repository, or into `.git`, is a 400
/// before any of it is applied.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn paths_outside_the_repository_are_a_400() {
    let f = Fixture::new("ap_paths").await;
    f.create_file("main", "a.txt", "one\n").await;
    let before = f.tip("main");
    let patches = [
        "diff --git a/a.txt b/../../evil.txt\n--- a/a.txt\n+++ b/../../evil.txt\n@@ -1 +1 @@\n-one\n+evil\n".to_string(),
        "diff --git a/a.txt b//etc/passwd\n--- a/a.txt\n+++ b//etc/passwd\n@@ -1 +1 @@\n-one\n+evil\n".to_string(),
        "diff --git a/a.txt b/.git/config\nnew file mode 100644\n--- /dev/null\n+++ b/.git/config\n@@ -0,0 +1 @@\n+evil\n".to_string(),
    ];
    for patch in patches {
        for dry_run in [false, true] {
            let (status, v) = f
                .apply(json!({
                    "branch": "main",
                    "expected_sha": before,
                    "patch": patch,
                    "dry_run": dry_run,
                }))
                .await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "dry_run={dry_run}: {v}");
            assert_eq!(v["error"]["code"], "BAD_REQUEST", "{v}");
        }
    }
    assert_eq!(f.tip("main"), before);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn oversized_patches_are_a_400() {
    let f = Fixture::new("ap_limits").await;
    f.create_file("main", "a.txt", "one\n").await;
    let before = f.tip("main");

    // Above the 1 MiB patch ceiling.
    let huge = format!(
        "diff --git a/huge.txt b/huge.txt\nnew file mode 100644\n--- /dev/null\n+++ b/huge.txt\n@@ -0,0 +1 @@\n+{}\n",
        "x".repeat(1024 * 1024)
    );
    let (status, v) = f
        .apply(json!({"branch": "main", "expected_sha": before, "patch": huge}))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{v}");
    assert!(
        v["error"]["message"]
            .as_str()
            .unwrap_or("")
            .contains("limit"),
        "{v}"
    );

    // Above the 500 path ceiling.
    let mut many = String::new();
    for index in 0..=500 {
        many.push_str(&add_file_patch(
            &format!("f{index}.txt"),
            &[&index.to_string()],
        ));
    }
    let (status, v) = f
        .apply(json!({"branch": "main", "expected_sha": before, "patch": many}))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{v}");
    assert!(
        v["error"]["message"]
            .as_str()
            .unwrap_or("")
            .contains("file limit"),
        "{v}"
    );

    assert_eq!(f.tip("main"), before);
}

/// The blobs a patch introduces go through the same R7 rules as a push.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_secret_in_the_patch_is_a_422_and_is_audited() {
    let f = Fixture::new("ap_secret").await;
    f.create_file("main", "a.txt", "one\n").await;
    let before = f.tip("main");
    let objects = git(&["count-objects", "-v"], &f.repo_dir);
    let patch = add_file_patch(
        "key.pem",
        &[
            "-----BEGIN RSA PRIVATE KEY-----",
            "MIIEowIBAAKCAQEA1234567890abcdefghijklmnopqrstuvwxyz",
            "-----END RSA PRIVATE KEY-----",
        ],
    );

    let (status, v) = f
        .apply(json!({"branch": "main", "expected_sha": before, "patch": patch}))
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{v}");
    assert_eq!(v["error"]["code"], "SECRET_DETECTED", "{v}");
    // The message names the path and the rule, never the matched text.
    let message = v["error"]["message"].as_str().unwrap_or("");
    assert!(message.contains("key.pem"), "{v}");
    assert!(!message.contains("MIIEowIBAA"), "{v}");

    assert_eq!(f.tip("main"), before);
    assert_eq!(git(&["count-objects", "-v"], &f.repo_dir), objects);

    let audit = f
        .scan_audit_row("repo.secret_scan_rejected")
        .await
        .expect("a rejected scan is audited");
    assert!(audit["findings"][0]["path"] == json!("key.pem"), "{audit}");
    assert_eq!(audit["findings"][0]["rule"], "private_key");
    assert!(f.audit_row("repo.apply_patch").await.is_none());
}

/// A dry run must not write an audit row, even when the scan would refuse it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_secret_is_refused_on_a_dry_run_without_an_audit_row() {
    let f = Fixture::new("ap_secret_dry").await;
    f.create_file("main", "a.txt", "one\n").await;
    let before = f.tip("main");
    let patch = add_file_patch("key.pem", &["-----BEGIN RSA PRIVATE KEY-----"]);

    let (status, v) = f
        .apply(json!({
            "branch": "main",
            "expected_sha": before,
            "patch": patch,
            "dry_run": true,
        }))
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{v}");
    assert_eq!(v["error"]["code"], "SECRET_DETECTED");
    assert_eq!(f.tip("main"), before);
    assert!(f
        .scan_audit_row("repo.secret_scan_rejected")
        .await
        .is_none());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_missing_branch_or_stale_expected_sha_is_ref_advanced() {
    let f = Fixture::new("ap_stale").await;
    f.create_file("main", "a.txt", "one\n").await;
    let stale = f.tip("main");
    let patch = add_file_patch("n.txt", &["n"]);
    let current = f.create_file("main", "later.txt", "l\n").await;

    let (status, v) = f
        .apply(json!({"branch": "main", "expected_sha": stale, "patch": patch}))
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{v}");
    assert_eq!(v["error"]["reason"], "REF_ADVANCED", "{v}");
    assert_eq!(v["error"]["retryable"], json!(true));
    assert_eq!(v["error"]["details"]["current_sha"], json!(current));
    assert_eq!(v["error"]["details"]["expected_sha"], json!(stale));

    // A branch that does not exist is never created.
    let (status, v) = f
        .apply(json!({"branch": "nope", "expected_sha": current, "patch": patch}))
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{v}");
    assert_eq!(v["error"]["reason"], "REF_ADVANCED");
    assert!(v["error"]["details"]["current_sha"].is_null());
    let missing = f
        .client
        .get(f.url(""))
        .bearer_auth(&f.jwt)
        .send()
        .await
        .unwrap();
    assert_eq!(missing.status(), StatusCode::OK);
    assert!(!git(&["for-each-ref", "refs/heads"], &f.repo_dir).contains("nope"));
    assert_eq!(f.tip("main"), current);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_patch_needs_write_access_and_respects_require_pr() {
    let f = Fixture::new("ap_acl").await;
    f.create_file("main", "a.txt", "one\n").await;
    let reader = register_user(&f.base, "ap_acl_reader", "ap_acl_reader@example.com", PW).await;
    let before = f.tip("main");
    let body = json!({
        "branch": "main",
        "expected_sha": before,
        "patch": add_file_patch("n.txt", &["n"]),
    });

    let (status, _) = f.apply_as(None, body.clone()).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, v) = f.apply_as(Some(&reader), body.clone()).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{v}");
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
    for dry_run in [false, true] {
        let mut body = body.clone();
        body["dry_run"] = json!(dry_run);
        let (status, v) = f.apply(body).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "dry_run={dry_run}: {v}");
    }
    assert_eq!(f.tip("main"), before);
}

/// A token limited to `agent/*` applies inside that limit and nowhere else, and
/// the audit row names the token.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_branch_limited_token_applies_only_inside_its_limit() {
    let f = Fixture::new("ap_token").await;
    f.create_file("main", "a.txt", "one\n").await;
    f.branch("agent/work", "main");
    let (token_id, pat) = f.token(true).await;
    let main_tip = f.tip("main");
    let patch = add_file_patch("n.txt", &["n"]);

    let (status, v) = f
        .apply_as(
            Some(&pat),
            json!({"branch": "main", "expected_sha": main_tip, "patch": patch}),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{v}");
    assert_eq!(f.tip("main"), main_tip);

    let (status, v) = f
        .apply_as(
            Some(&pat),
            json!({
                "branch": "agent/work",
                "expected_sha": f.tip("agent/work"),
                "patch": patch,
            }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(f.tip("agent/work"), v["commit_sha"].as_str().unwrap());

    let audit = f.audit_row("repo.apply_patch").await.expect("audited");
    assert_eq!(audit["access_token_id"], json!(token_id));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_empty_or_malformed_request_is_refused() {
    let f = Fixture::new("ap_bad").await;
    f.create_file("main", "a.txt", "one\n").await;
    let before = f.tip("main");

    // Empty and non-diff patches are a 400.
    for patch in ["", "   ", "not a patch at all\n"] {
        let (status, v) = f
            .apply(json!({"branch": "main", "expected_sha": before, "patch": patch}))
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{patch:?}: {v}");
    }
    // A short or non-hex `expected_sha` is a 400; a missing one never reaches the
    // handler (the body is rejected while deserialising).
    for sha in ["", "deadbeef", "zzzz", &before[..7]] {
        let (status, v) = f
            .apply(json!({
                "branch": "main",
                "expected_sha": sha,
                "patch": add_file_patch("n.txt", &["n"]),
            }))
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{sha:?}: {v}");
    }
    let (status, _) = f
        .apply(json!({"branch": "main", "patch": add_file_patch("n.txt", &["n"])}))
        .await;
    assert!(status.is_client_error(), "{status}");

    assert_eq!(f.tip("main"), before);
}

/// The route, its request schema and its required fields are in the generated
/// spec, so `/api-docs` describes what the endpoint actually accepts.
#[test]
fn the_openapi_spec_documents_the_route_and_its_required_fields() {
    let spec: Value = serde_json::from_str(&rg_http::openapi::openapi_spec()).unwrap();
    let operation = &spec["paths"]["/repos/{owner}/{name}/commits/apply-patch"]["post"];
    assert!(operation.is_object(), "{operation}");
    assert_eq!(
        operation["requestBody"]["content"]["application/json"]["schema"]["$ref"],
        "#/components/schemas/ApplyPatchRequest"
    );
    let required = spec["components"]["schemas"]["ApplyPatchRequest"]["required"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    for field in ["branch", "expected_sha", "patch"] {
        assert!(
            required.contains(&json!(field)),
            "ApplyPatchRequest must require {field}: {required:?}"
        );
    }
}
