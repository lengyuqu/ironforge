//! Helpers shared by the collaboration integration tests.
#![allow(dead_code)]

use crate::common::{
    build_test_app_state, create_repo, register_bootstrap_admin, register_full, setup_test_db,
};
use reqwest::{Method, Response};
use serde_json::{json, Value};

pub const HDR_SESSION: &str = "x-ironforge-session";
pub const HDR_SECRET: &str = "x-ironforge-session-secret";

pub struct Env {
    pub base: String,
    pub db: rg_db::DatabaseConnection,
    /// Keeps the database and repository directories alive.
    pub dir: std::sync::Arc<tempfile::TempDir>,
    pub owner: String,
    pub owner_token: String,
    pub owner_id: i64,
    pub repo: String,
    pub repo_id: i64,
}

/// A session as seen by its client: id plus the secret it was given once.
#[derive(Clone)]
pub struct Sess {
    pub id: String,
    pub secret: String,
}

/// Start a server instance over `db`; each call is a fresh process-equivalent
/// (new `AppState`, new listener) sharing the same stored data.
async fn serve(db: &rg_db::DatabaseConnection, dir: &std::sync::Arc<tempfile::TempDir>) -> String {
    let repo_root = dir.path().join("repos");
    std::fs::create_dir_all(&repo_root).ok();
    let state = build_test_app_state(db.clone(), repo_root);
    let app = rg_http::create_router_for_test(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let keep = dir.clone();
    tokio::spawn(async move {
        let _keep = keep;
        axum::serve(listener, app).await.unwrap();
    });
    base
}

impl Env {
    /// Simulate a service restart: a new server instance over the same database.
    /// Returns its base URL; the old instance keeps running but should be abandoned.
    pub async fn restart(&self) -> String {
        serve(&self.db, &self.dir).await
    }

    /// Path of the bare repository `name` owned by the fixture owner.
    pub fn bare_path(&self, name: &str) -> std::path::PathBuf {
        self.dir.path().join("repos").join(&self.owner).join(format!("{name}.git"))
    }

    /// Create a commit in the bare repository `name` and return its id. Uses an
    /// explicit identity so it works on machines (and CI) with no git config.
    pub fn push_commit(&self, name: &str, message: &str) -> String {
        let work = tempfile::tempdir().unwrap();
        let run = |args: &[&str]| {
            let out = std::process::Command::new("git")
                .args(args)
                .current_dir(work.path())
                .env("GIT_AUTHOR_NAME", "t")
                .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
                .env("GIT_COMMITTER_NAME", "t")
                .env("GIT_COMMITTER_EMAIL", "t@example.invalid")
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_SYSTEM", "/dev/null")
                .output()
                .unwrap();
            assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
            String::from_utf8(out.stdout).unwrap().trim().to_string()
        };
        run(&["init", "-q", "-b", "main"]);
        std::fs::write(work.path().join("f.txt"), message).unwrap();
        run(&["add", "f.txt"]);
        run(&["commit", "-q", "-m", message]);
        let sha = run(&["rev-parse", "HEAD"]);
        run(&["push", "-q", self.bare_path(name).to_str().unwrap(), "HEAD:refs/heads/main"]);
        sha
    }

    /// Instance with an owner and one public repository `collab-repo`.
    pub async fn new() -> Self {
        Self::with_repo(json!({"name": "collab-repo"})).await
    }

    pub async fn with_repo(repo_body: Value) -> Self {
        let (db, dir) = setup_test_db().await;
        let dir = std::sync::Arc::new(dir);
        let base = serve(&db, &dir).await;
        register_bootstrap_admin(&base).await;
        let (owner_token, owner_id) =
            register_full(&base, "collab_owner", "collab_owner@example.com").await;
        let repo = repo_body["name"].as_str().unwrap().to_string();
        let repo_id = if repo_body.get("is_private").is_some() {
            let resp = reqwest::Client::new()
                .post(format!("{base}/api/v1/repos"))
                .bearer_auth(&owner_token)
                .json(&repo_body)
                .send()
                .await
                .unwrap();
            assert_eq!(resp.status(), 201);
            resp.json::<Value>().await.unwrap()["id"].as_i64().unwrap()
        } else {
            create_repo(&base, &owner_token, &repo).await
        };
        Self {
            base,
            db,
            dir,
            owner: "collab_owner".into(),
            owner_token,
            owner_id,
            repo,
            repo_id,
        }
    }

    /// Register another account and return `(token, user_id)`.
    pub async fn user(&self, name: &str) -> (String, i64) {
        register_full(&self.base, name, &format!("{name}@example.com")).await
    }

    pub async fn add_collaborator(&self, user_id: i64, permission: &str) {
        let resp = reqwest::Client::new()
            .post(format!(
                "{}/api/v1/repos/{}/{}/collaborators",
                self.base, self.owner, self.repo
            ))
            .bearer_auth(&self.owner_token)
            .json(&json!({"user_id": user_id, "permission": permission}))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 201, "add collaborator");
    }

    pub fn url(&self, suffix: &str) -> String {
        self.url_in(&self.repo, suffix)
    }

    pub fn url_in(&self, repo: &str, suffix: &str) -> String {
        format!(
            "{}/api/v1/repos/{}/{}/collaboration{}",
            self.base, self.owner, repo, suffix
        )
    }

    /// Send a request as `token`, optionally acting as `session`.
    pub async fn send(
        &self,
        method: Method,
        url: &str,
        token: Option<&str>,
        session: Option<&Sess>,
        body: Option<Value>,
    ) -> Response {
        let mut req = reqwest::Client::new().request(method, url);
        if let Some(token) = token {
            req = req.bearer_auth(token);
        }
        if let Some(session) = session {
            req = req.header(HDR_SESSION, &session.id).header(HDR_SECRET, &session.secret);
        }
        if let Some(body) = body {
            req = req.json(&body);
        }
        req.send().await.unwrap()
    }

    pub async fn json(
        &self,
        method: Method,
        url: &str,
        token: &str,
        session: Option<&Sess>,
        body: Option<Value>,
    ) -> (u16, Value) {
        let resp = self.send(method, url, Some(token), session, body).await;
        let status = resp.status().as_u16();
        let text = resp.text().await.unwrap();
        let value = if text.is_empty() { Value::Null } else { serde_json::from_str(&text).unwrap_or(Value::String(text)) };
        (status, value)
    }

    /// Register a session as `token`; returns `(session, full response body)`.
    pub async fn register_session(&self, token: &str, name: &str) -> (Sess, Value) {
        let (status, body) = self
            .json(
                Method::POST,
                &self.url("/sessions"),
                token,
                None,
                Some(json!({"display_name": name, "client_kind": "test-client"})),
            )
            .await;
        assert_eq!(status, 201, "register session: {body}");
        let sess = Sess {
            id: body["session"]["id"].as_str().unwrap().to_string(),
            secret: body["session_secret"].as_str().unwrap().to_string(),
        };
        (sess, body)
    }

    pub async fn session(&self, token: &str, name: &str) -> Sess {
        self.register_session(token, name).await.0
    }
}
