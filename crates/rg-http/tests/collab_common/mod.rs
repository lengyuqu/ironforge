//! Helpers shared by the collaboration integration tests.
#![allow(dead_code)]

use crate::common::{build_test_app_state, create_repo, register_full, setup_test_db};
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

fn unique() -> String {
    uuid::Uuid::new_v4().simple().to_string()[..10].to_string()
}

/// SQLite by default; with `IRONFORGE_TEST_DATABASE_URL` set, the PostgreSQL or
/// MySQL database it names (migrated, shared between tests, never reset).
async fn test_database() -> (rg_db::DatabaseConnection, tempfile::TempDir) {
    let Ok(url) = std::env::var("IRONFORGE_TEST_DATABASE_URL") else {
        return setup_test_db().await;
    };
    static SETUP: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    let _guard = SETUP.lock().await;
    std::env::set_var("IRONFORGE_ALLOW_LOCAL_OUTBOUND", "1");
    let db = rg_db::connect_with_pool(&url, 20, 60, 16).await.expect("connect");
    rg_db::run_migrations(&db).await.expect("migrate");
    (db, tempfile::tempdir().unwrap())
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

    /// Create a commit in the bare repository `name` and return its id. Goes
    /// through the sanctioned git gateway (the `rg-git` guard forbids raw
    /// `git` processes) and takes its identity from the environment, so it works
    /// on machines and CI with no git configuration.
    pub fn push_commit(&self, name: &str, message: &str) -> String {
        let gateway = rg_git::cli_gateway::global_gateway().as_ref().expect("git gateway");
        let env = [
            ("GIT_AUTHOR_NAME", "t"),
            ("GIT_AUTHOR_EMAIL", "t@example.invalid"),
            ("GIT_COMMITTER_NAME", "t"),
            ("GIT_COMMITTER_EMAIL", "t@example.invalid"),
        ];
        let work = tempfile::tempdir().unwrap();
        let dir = work.path();
        let run = |args: &[&str], cwd: Option<&std::path::Path>| {
            let out = gateway.run_with_env(args, cwd, &env).expect("run git");
            out.ensure_success().expect("git failed");
            out.stdout_str().trim().to_string()
        };
        run(&["init", "-q", "--initial-branch=main", dir.to_str().unwrap()], None);
        std::fs::write(dir.join("f.txt"), message).unwrap();
        run(&["add", "f.txt"], Some(dir));
        run(&["commit", "-q", "-m", message], Some(dir));
        let sha = run(&["rev-parse", "HEAD"], Some(dir));
        run(&["push", "-q", self.bare_path(name).to_str().unwrap(), "HEAD:refs/heads/main"], Some(dir));
        sha
    }

    /// Resolve `rev` (e.g. `<sha>^{tree}`) in the bare repository `name`.
    pub fn rev_parse(&self, name: &str, rev: &str) -> String {
        let gateway = rg_git::cli_gateway::global_gateway().as_ref().expect("git gateway");
        let out = gateway
            .run(&["--git-dir", self.bare_path(name).to_str().unwrap(), "rev-parse", rev], None)
            .expect("run git");
        out.ensure_success().expect("git rev-parse failed");
        out.stdout_str().trim().to_string()
    }

    /// Instance with an owner and one public repository `collab-repo`.
    pub async fn new() -> Self {
        Self::with_repo(json!({"name": "collab-repo"})).await
    }

    pub async fn with_repo(repo_body: Value) -> Self {
        let (db, dir) = test_database().await;
        let dir = std::sync::Arc::new(dir);
        let base = serve(&db, &dir).await;
        // The first account of an instance becomes its administrator. A shared
        // server database may already have one, so a repeat is not an error.
        let _ = reqwest::Client::new()
            .post(format!("{base}/api/v1/users/register"))
            .json(&json!({"username": "bootstrap_admin", "email": "bootstrap_admin@example.com", "password": "Qz7$wRtm"}))
            .send()
            .await;
        let owner = format!("collab_owner_{}", unique());
        let (owner_token, owner_id) =
            register_full(&base, &owner, &format!("{owner}@example.com")).await;
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
            owner,
            owner_token,
            owner_id,
            repo,
            repo_id,
        }
    }

    /// Register another account and return `(token, user_id)`.
    pub async fn user(&self, name: &str) -> (String, i64) {
        let name = format!("{name}_{}", unique());
        register_full(&self.base, &name, &format!("{name}@example.com")).await
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
