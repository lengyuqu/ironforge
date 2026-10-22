//! Honest error semantics for the repo-content, release and wiki endpoints.
//!
//! Before this work those handlers collapsed every service failure into
//! `400 Bad Request` (or an accidental 404/500) and echoed the underlying
//! cause (SQL text, filesystem paths, gix errors) to the client. These tests
//! pin the contract: caller mistakes keep their specific 4xx status, and an
//! unexpected failure is a sanitised 500 that never contains the cause.

mod common;

use std::path::PathBuf;

use reqwest::StatusCode;
use sea_orm::ConnectionTrait;
use serde_json::{json, Value};

const PW: &str = "Qz7$wRtm";

struct App {
    base: String,
    repo_root: PathBuf,
    db: rg_db::DatabaseConnection,
    client: reqwest::Client,
    _dir: tempfile::TempDir,
}

async fn spawn() -> App {
    let (db, dir) = common::setup_test_db().await;
    let repo_root = dir.path().join("repos");
    std::fs::create_dir_all(&repo_root).unwrap();
    let state = common::build_test_app_state(db.clone(), repo_root.clone());
    let app = common::rg_http::create_router_for_test(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    App {
        base,
        repo_root,
        db,
        client: reqwest::Client::new(),
        _dir: dir,
    }
}

impl App {
    fn url(&self, path: &str) -> String {
        format!("{}/api/v1{}", self.base, path)
    }

    async fn user(&self, name: &str) -> String {
        common::register_user(&self.base, name, &format!("{name}@example.com"), PW).await
    }

    /// Create an auto-initialised public repo (so `main` has a commit).
    async fn init_repo(&self, token: &str, name: &str) {
        let resp = self
            .client
            .post(self.url("/repos"))
            .bearer_auth(token)
            .json(&json!({"name": name, "auto_init": true}))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 201);
    }

    async fn send(
        &self,
        method: reqwest::Method,
        path: &str,
        token: Option<&str>,
        body: Option<Value>,
    ) -> (StatusCode, String) {
        let mut req = self.client.request(method, self.url(path));
        if let Some(t) = token {
            req = req.bearer_auth(t);
        }
        if let Some(b) = body {
            req = req.json(&b);
        }
        let resp = req.send().await.unwrap();
        let status = resp.status();
        (status, resp.text().await.unwrap())
    }

    async fn get(&self, path: &str, token: Option<&str>) -> (StatusCode, String) {
        self.send(reqwest::Method::GET, path, token, None).await
    }
    async fn post(&self, path: &str, token: &str, body: Value) -> (StatusCode, String) {
        self.send(reqwest::Method::POST, path, Some(token), Some(body))
            .await
    }
    async fn patch(&self, path: &str, token: &str, body: Value) -> (StatusCode, String) {
        self.send(reqwest::Method::PATCH, path, Some(token), Some(body))
            .await
    }
    async fn delete(&self, path: &str, token: &str) -> (StatusCode, String) {
        self.send(reqwest::Method::DELETE, path, Some(token), None)
            .await
    }
}

/// A 500 must carry only the generic message, never the cause.
fn assert_sanitised_500(app: &App, status: StatusCode, body: &str) {
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "body: {body}");
    let v: Value = serde_json::from_str(body).expect("error body is JSON");
    assert_eq!(v["error"]["code"], "INTERNAL_ERROR", "body: {body}");
    assert_eq!(v["error"]["message"], "Internal server error", "body: {body}");
    assert_no_internals(app, body);
}

fn assert_no_internals(app: &App, body: &str) {
    let root = app.repo_root.to_string_lossy();
    for needle in [
        root.as_ref(),
        "no such table",
        "sqlite",
        "SQL",
        "gix",
        ".git",
        "/private/",
        "/tmp/",
    ] {
        assert!(
            !body.contains(needle),
            "response leaks internals ({needle:?}): {body}"
        );
    }
}

// ── Repository contents ──────────────────────────────────────────────────

#[tokio::test]
async fn contents_write_classifies_client_errors() {
    let app = spawn().await;
    let owner = app.user("cowner").await;
    let other = app.user("cother").await;
    app.init_repo(&owner, "proj").await;
    let p = "/repos/cowner/proj/contents";

    // Create a file.
    let (st, body) = app
        .post(
            &format!("{p}/docs/a.txt"),
            &owner,
            json!({"content": "hello", "message": "add"}),
        )
        .await;
    assert_eq!(st, 200, "{body}");
    let sha = {
        let (st, body) = app.get("/repos/cowner/proj/blob/docs/a.txt", None).await;
        assert_eq!(st, 200, "{body}");
        serde_json::from_str::<Value>(&body).unwrap()["sha"]
            .as_str()
            .unwrap()
            .to_string()
    };

    // Conflict: create over an existing file without sha.
    let (st, body) = app
        .post(
            &format!("{p}/docs/a.txt"),
            &owner,
            json!({"content": "again", "message": "dup"}),
        )
        .await;
    assert_eq!(st, 409, "{body}");

    // Conflict: update with a stale blob sha.
    let (st, body) = app
        .post(
            &format!("{p}/docs/a.txt"),
            &owner,
            json!({"content": "x", "message": "m", "sha": "0".repeat(40)}),
        )
        .await;
    assert_eq!(st, 409, "{body}");

    // Not found: update (sha given) / delete of a file that does not exist.
    let (st, body) = app
        .post(
            &format!("{p}/nope.txt"),
            &owner,
            json!({"content": "x", "message": "m", "sha": sha}),
        )
        .await;
    assert_eq!(st, 404, "{body}");
    let (st, body) = app
        .send(
            reqwest::Method::DELETE,
            &format!("{p}/nope.txt?message=rm&sha={sha}"),
            Some(&owner),
            None,
        )
        .await;
    assert_eq!(st, 404, "{body}");

    // Not found: unknown repository.
    let (st, body) = app
        .post(
            "/repos/cowner/ghost/contents/a.txt",
            &owner,
            json!({"content": "x", "message": "m"}),
        )
        .await;
    assert_eq!(st, 404, "{body}");

    // Invalid input: malformed branch name and malformed file path.
    let (st, body) = app
        .post(
            &format!("{p}/b.txt"),
            &owner,
            json!({"content": "x", "message": "m", "branch": "bad..branch"}),
        )
        .await;
    assert_eq!(st, 400, "{body}");
    let (st, body) = app
        .post(
            &format!("{p}/a%2F%2Fb.txt"),
            &owner,
            json!({"content": "x", "message": "m"}),
        )
        .await;
    assert_eq!(st, 400, "{body}");

    // Forbidden / unauthenticated.
    let (st, _) = app
        .post(
            &format!("{p}/c.txt"),
            &other,
            json!({"content": "x", "message": "m"}),
        )
        .await;
    assert_eq!(st, 403);
    let (st, _) = app
        .send(
            reqwest::Method::POST,
            &format!("{p}/c.txt"),
            None,
            Some(json!({"content": "x", "message": "m"})),
        )
        .await;
    assert_eq!(st, 401);
}

#[tokio::test]
async fn contents_read_classifies_missing_ref_and_path() {
    let app = spawn().await;
    let owner = app.user("rowner").await;
    app.init_repo(&owner, "proj").await;
    let base = "/repos/rowner/proj";

    // Tree: unknown ref and unknown / non-directory sub-path are 404, not 500.
    let (st, body) = app.get(&format!("{base}/tree?ref=nope"), None).await;
    assert_eq!(st, 404, "{body}");
    let (st, body) = app.get(&format!("{base}/tree?path=missing"), None).await;
    assert_eq!(st, 404, "{body}");
    let (st, body) = app.get(&format!("{base}/tree?path=README.md"), None).await;
    assert_eq!(st, 404, "{body}");
    let (st, _) = app.get(&format!("{base}/tree"), None).await;
    assert_eq!(st, 200);

    // Blob: unknown path/ref are 404 without echoing the gix error.
    let (st, body) = app.get(&format!("{base}/blob/missing.txt"), None).await;
    assert_eq!(st, 404, "{body}");
    assert_no_internals(&app, &body);
    let (st, body) = app.get(&format!("{base}/blob/README.md?ref=nope"), None).await;
    assert_eq!(st, 404, "{body}");
    assert_no_internals(&app, &body);

    // Signature: bad format 400, unknown commit 404 (short and full length).
    let (st, _) = app.get(&format!("{base}/commits/zzz/signature"), None).await;
    assert_eq!(st, 400);
    let (st, body) = app
        .get(&format!("{base}/commits/{}/signature", "a".repeat(40)), None)
        .await;
    assert_eq!(st, 404, "{body}");
    let (st, body) = app
        .get(&format!("{base}/commits/abcdef1/signature"), None)
        .await;
    assert_eq!(st, 404, "{body}");

    // Unknown repository.
    let (st, _) = app.get("/repos/rowner/ghost/tree", None).await;
    assert_eq!(st, 404);
}

#[tokio::test]
async fn contents_missing_storage_is_sanitised_500() {
    let app = spawn().await;
    let owner = app.user("sowner").await;
    app.init_repo(&owner, "proj").await;

    // The database still knows the repo but its bare repository vanished.
    let repo_dir = app.repo_root.join("sowner/proj.git");
    assert!(repo_dir.exists());
    std::fs::remove_dir_all(&repo_dir).unwrap();

    // Write path (service layer used to answer 404 and print the path).
    let (st, body) = app
        .post(
            "/repos/sowner/proj/contents/a.txt",
            &owner,
            json!({"content": "x", "message": "m"}),
        )
        .await;
    assert_sanitised_500(&app, st, &body);

    let (st, body) = app
        .send(
            reqwest::Method::DELETE,
            &format!("/repos/sowner/proj/contents/a.txt?message=rm&sha={}", "a".repeat(40)),
            Some(&owner),
            None,
        )
        .await;
    assert_sanitised_500(&app, st, &body);

    // Read paths.
    for path in [
        "/repos/sowner/proj/tree",
        "/repos/sowner/proj/blob/README.md",
        "/repos/sowner/proj/log",
        "/repos/sowner/proj/branches",
        "/repos/sowner/proj/tags",
    ] {
        let (st, body) = app.get(path, None).await;
        assert_sanitised_500(&app, st, &body);
    }
}

#[tokio::test]
async fn contents_corrupt_repository_is_sanitised_500() {
    let app = spawn().await;
    let owner = app.user("downer").await;
    app.init_repo(&owner, "proj").await;

    // Directory exists but is no longer a git repository.
    let repo_dir = app.repo_root.join("downer/proj.git");
    std::fs::remove_dir_all(&repo_dir).unwrap();
    std::fs::create_dir_all(&repo_dir).unwrap();

    for path in [
        "/repos/downer/proj/tree",
        "/repos/downer/proj/blob/README.md",
        "/repos/downer/proj/branches",
        "/repos/downer/proj/commits/abcdef1/signature",
    ] {
        let (st, body) = app.get(path, None).await;
        assert_sanitised_500(&app, st, &body);
    }
    let (st, body) = app
        .post(
            "/repos/downer/proj/contents/a.txt",
            &owner,
            json!({"content": "x", "message": "m"}),
        )
        .await;
    assert_sanitised_500(&app, st, &body);
}

// ── Releases ─────────────────────────────────────────────────────────────

#[tokio::test]
async fn releases_classify_errors() {
    let app = spawn().await;
    let owner = app.user("relowner").await;
    let other = app.user("relother").await;
    app.init_repo(&owner, "proj").await;
    let r = "/repos/relowner/proj/releases";

    let (st, body) = app
        .post(r, &owner, json!({"tag_name": "v1", "title": "One"}))
        .await;
    assert_eq!(st, 201, "{body}");
    let id = serde_json::from_str::<Value>(&body).unwrap()["id"]
        .as_i64()
        .unwrap();

    // Invalid input.
    let (st, _) = app
        .post(r, &owner, json!({"tag_name": "", "title": "Empty tag"}))
        .await;
    assert_eq!(st, 400);
    let (st, _) = app
        .post(r, &owner, json!({"tag_name": "v2", "title": ""}))
        .await;
    assert_eq!(st, 400);

    // Conflict: duplicate tag.
    let (st, body) = app
        .post(r, &owner, json!({"tag_name": "v1", "title": "Again"}))
        .await;
    assert_eq!(st, 409, "{body}");

    // Not found.
    let (st, _) = app.get(&format!("{r}/9999"), None).await;
    assert_eq!(st, 404);
    let (st, body) = app
        .patch(&format!("{r}/9999"), &owner, json!({"title": "x"}))
        .await;
    assert_eq!(st, 404, "{body}");
    let (st, body) = app.delete(&format!("{r}/9999"), &owner).await;
    assert_eq!(st, 404, "{body}");
    let (st, _) = app.get(&format!("{r}/assets/9999"), None).await;
    assert_eq!(st, 404);
    let (st, _) = app.get(&format!("{r}/assets/9999/download"), None).await;
    assert_eq!(st, 404);
    let (st, body) = app.delete(&format!("{r}/assets/9999"), &owner).await;
    assert_eq!(st, 404, "{body}");
    // Unknown repository: a missing resource, not a server error.
    let (st, body) = app
        .patch("/repos/relowner/ghost/releases/1", &owner, json!({"title": "x"}))
        .await;
    assert_eq!(st, 404, "{body}");
    let (st, body) = app
        .post("/repos/relowner/ghost/releases", &owner, json!({"tag_name": "v1", "title": "x"}))
        .await;
    assert_eq!(st, 404, "{body}");

    // Upload to a release that does not exist -> 404; no filename -> 400.
    let upload = |release: i64, name: Option<&str>, token: String| {
        let mut req = app
            .client
            .post(app.url(&format!("{r}/{release}/assets")))
            .bearer_auth(token)
            .body("data");
        if let Some(n) = name {
            req = req.header("content-disposition", format!("attachment; filename={n}"));
        }
        async move {
            let resp = req.send().await.unwrap();
            let st = resp.status();
            (st, resp.text().await.unwrap())
        }
    };
    let (st, body) = upload(9999, Some("a.txt"), owner.clone()).await;
    assert_eq!(st, 404, "{body}");
    let (st, body) = upload(id, None, owner.clone()).await;
    assert_eq!(st, 400, "{body}");

    // Forbidden for a non-collaborator.
    let (st, _) = app
        .post(r, &other, json!({"tag_name": "v9", "title": "x"}))
        .await;
    assert_eq!(st, 403);
    let (st, _) = app.patch(&format!("{r}/{id}"), &other, json!({"title": "x"})).await;
    assert_eq!(st, 403);
    let (st, _) = app.delete(&format!("{r}/{id}"), &other).await;
    assert_eq!(st, 403);
    let (st, _) = upload(id, Some("a.txt"), other.clone()).await;
    assert_eq!(st, 403);
}

#[tokio::test]
async fn release_asset_storage_failure_is_sanitised_500() {
    let app = spawn().await;
    let owner = app.user("relsowner").await;
    app.init_repo(&owner, "proj").await;
    let r = "/repos/relsowner/proj/releases";
    let (_, body) = app
        .post(r, &owner, json!({"tag_name": "v1", "title": "One"}))
        .await;
    let id = serde_json::from_str::<Value>(&body).unwrap()["id"]
        .as_i64()
        .unwrap();

    // Block the blob backend: `releases` is a file where a directory is needed.
    std::fs::write(app.repo_root.join("releases"), b"in the way").unwrap();

    let resp = app
        .client
        .post(app.url(&format!("{r}/{id}/assets")))
        .bearer_auth(&owner)
        .header("content-disposition", "attachment; filename=a.txt")
        .body("data")
        .send()
        .await
        .unwrap();
    let st = resp.status();
    let body = resp.text().await.unwrap();
    assert_sanitised_500(&app, st, &body);
}

#[tokio::test]
async fn release_asset_with_lost_content_is_not_found_without_path() {
    let app = spawn().await;
    let owner = app.user("rellost").await;
    app.init_repo(&owner, "proj").await;
    let r = "/repos/rellost/proj/releases";
    let (_, body) = app
        .post(r, &owner, json!({"tag_name": "v1", "title": "One"}))
        .await;
    let id = serde_json::from_str::<Value>(&body).unwrap()["id"]
        .as_i64()
        .unwrap();
    let resp = app
        .client
        .post(app.url(&format!("{r}/{id}/assets")))
        .bearer_auth(&owner)
        .header("content-disposition", "attachment; filename=a.txt")
        .body("data")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);
    let asset_id = resp.json::<Value>().await.unwrap()["id"].as_i64().unwrap();

    std::fs::remove_dir_all(app.repo_root.join("releases")).unwrap();

    let (st, body) = app
        .get(&format!("{r}/assets/{asset_id}/download"), Some(&owner))
        .await;
    assert_eq!(st, 404, "{body}");
    assert_no_internals(&app, &body);
}

// ── Wiki ─────────────────────────────────────────────────────────────────

#[tokio::test]
async fn wiki_classifies_errors() {
    let app = spawn().await;
    let owner = app.user("wowner").await;
    let other = app.user("wother").await;
    app.init_repo(&owner, "proj").await;
    let w = "/repos/wowner/proj/wiki";

    let (st, body) = app
        .post(w, &owner, json!({"title": "Home", "content": "hi"}))
        .await;
    assert_eq!(st, 201, "{body}");

    // Invalid input.
    let (st, _) = app
        .post(w, &owner, json!({"title": "", "content": "hi"}))
        .await;
    assert_eq!(st, 400);

    // Conflict.
    let (st, body) = app
        .post(w, &owner, json!({"title": "Home", "content": "dup"}))
        .await;
    assert_eq!(st, 409, "{body}");

    // Not found.
    let (st, body) = app
        .patch(&format!("{w}/Missing"), &owner, json!({"content": "x"}))
        .await;
    assert_eq!(st, 404, "{body}");
    let (st, body) = app.delete(&format!("{w}/Missing"), &owner).await;
    assert_eq!(st, 404, "{body}");
    let (st, _) = app.get(&format!("{w}/Missing"), None).await;
    assert_eq!(st, 404);
    let (st, _) = app.get(&format!("{w}/Missing/history"), None).await;
    assert_eq!(st, 404);
    let (st, _) = app.get(&format!("{w}/Home/revisions/9999"), None).await;
    assert_eq!(st, 404);

    // Forbidden / unauthenticated.
    let (st, _) = app
        .post(w, &other, json!({"title": "X", "content": "y"}))
        .await;
    assert_eq!(st, 403);
    let (st, _) = app
        .send(
            reqwest::Method::POST,
            w,
            None,
            Some(json!({"title": "X", "content": "y"})),
        )
        .await;
    assert_eq!(st, 401);
}

#[tokio::test]
async fn wiki_database_failure_is_sanitised_500() {
    let app = spawn().await;
    let owner = app.user("wdbowner").await;
    app.init_repo(&owner, "proj").await;
    let w = "/repos/wdbowner/proj/wiki";

    // Simulate a database fault in the wiki storage.
    app.db
        .execute_unprepared("DROP TABLE wiki_pages")
        .await
        .unwrap();

    // Writes used to come back as 400 carrying the raw SQL error text.
    let (st, body) = app
        .post(w, &owner, json!({"title": "Home", "content": "hi"}))
        .await;
    assert_sanitised_500(&app, st, &body);
    let (st, body) = app
        .patch(&format!("{w}/Home"), &owner, json!({"content": "x"}))
        .await;
    assert_sanitised_500(&app, st, &body);
    let (st, body) = app.delete(&format!("{w}/Home"), &owner).await;
    assert_sanitised_500(&app, st, &body);

    // Reads.
    let (st, body) = app.get(w, None).await;
    assert_sanitised_500(&app, st, &body);
    let (st, body) = app.get(&format!("{w}/Home"), None).await;
    assert_sanitised_500(&app, st, &body);
    let (st, body) = app.get(&format!("{w}/Home/history"), None).await;
    assert_sanitised_500(&app, st, &body);
}
