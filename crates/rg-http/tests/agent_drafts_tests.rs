//! The agent draft area (#18): a branch directory with a TTL that runs CI but
//! notifies nobody, and disappears on its own.
//!
//! Real HTTP server, real git over HTTP, real bare repository on disk. The CI
//! engine is a recorder (this crate cannot depend on `rg-ci`), so "CI still
//! runs" is observed as "the engine was called with this ref"; the security
//! halves that live inside `rg-ci` (no secrets, no host execution) are tested
//! there.
//!
//! With `IRONFORGE_TEST_DATABASE_URL` set the same tests run on PostgreSQL or
//! MySQL; the namespace table, its lifecycle queries and the cleanup are what
//! that covers.

mod common;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::StreamExt;
use reqwest::{Client, Method, StatusCode};
use serde_json::{json, Value};
use tokio::sync::mpsc;

const PW: &str = "Qz7$wRtm";

// ── a CI engine that records what it was asked to run ─────────────────────

#[derive(Debug, Clone)]
struct CiCall {
    ref_name: String,
    trigger_type: String,
    allow_host_execution: bool,
}

#[derive(Clone, Default)]
struct RecordingCi {
    calls: Arc<Mutex<Vec<CiCall>>>,
}

impl RecordingCi {
    fn calls_for(&self, ref_name: &str) -> Vec<CiCall> {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|call| call.ref_name == ref_name)
            .cloned()
            .collect()
    }
}

impl rg_core::ci::CiTrigger for RecordingCi {
    fn has_ci_config(&self, _repo_path: &Path, _commit_sha: &str) -> bool {
        true
    }

    fn trigger_pipeline<'a>(
        &'a self,
        params: rg_core::ci::TriggerPipelineParams<'a>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<i64>> + Send + 'a>> {
        self.calls.lock().unwrap().push(CiCall {
            ref_name: params.ref_name.to_string(),
            trigger_type: params.trigger_type.to_string(),
            allow_host_execution: params.allow_host_execution,
        });
        Box::pin(async { Ok(1) })
    }

    fn resume_pipeline<'a>(
        &'a self,
        _params: rg_core::ci::ResumePipelineParams<'a>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<()>> + Send + 'a>> {
        Box::pin(async { Ok(()) })
    }
}

// ── fixture ──────────────────────────────────────────────────────────────

struct App {
    base: String,
    client: Client,
    db: rg_db::DatabaseConnection,
    engine: RecordingCi,
    state: rg_http::AppState,
    /// Kept alive for the whole test: the repositories and (on SQLite) the
    /// database file live inside it.
    _dir: Option<tempfile::TempDir>,
}

impl App {
    async fn new() -> App {
        // SQLite by default; PostgreSQL / MySQL when the environment names one.
        // (A local copy of `common`'s logic: this fixture has to mount its own
        // CI engine, which the shared spawn helpers do not allow.)
        let (db, dir) = match std::env::var("IRONFORGE_TEST_DATABASE_URL") {
            Ok(url) => {
                std::env::set_var("IRONFORGE_ALLOW_LOCAL_OUTBOUND", "1");
                let db = rg_db::connect_with_pool(&url, 20, 60, 16).await.unwrap();
                rg_db::run_migrations(&db).await.unwrap();
                (db, None)
            }
            Err(_) => {
                let (db, dir) = common::setup_test_db().await;
                (db, Some(dir))
            }
        };
        let dir_path = dir
            .as_ref()
            .map(|dir| dir.path().to_path_buf())
            .unwrap_or_else(|| {
                let path = std::env::temp_dir().join(format!(
                    "ironforge-drafts-{}",
                    uuid::Uuid::new_v4().simple()
                ));
                std::fs::create_dir_all(&path).unwrap();
                path
            });
        let repo_root = dir_path.join("repos");
        std::fs::create_dir_all(&repo_root).unwrap();

        let engine = RecordingCi::default();
        let mut state = common::build_test_app_state(db.clone(), repo_root.clone());
        state.ci_engine = Arc::new(engine.clone());
        let app_state = state.clone();
        let router = rg_http::create_router_for_test(state);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        tokio::time::sleep(Duration::from_millis(100)).await;

        App {
            base,
            client: Client::new(),
            db,
            engine,
            state: app_state,
            _dir: dir,
        }
    }

    fn url(&self, suffix: &str) -> String {
        format!("{}/api/v1{suffix}", self.base)
    }

    async fn send(
        &self,
        method: Method,
        suffix: &str,
        auth: &str,
        body: Value,
    ) -> (StatusCode, Value) {
        let response = self
            .client
            .request(method, self.url(suffix))
            .bearer_auth(auth)
            .json(&body)
            .send()
            .await
            .unwrap();
        let status = response.status();
        let body = response.json().await.unwrap_or(Value::Null);
        (status, body)
    }
}

struct Repo {
    owner: &'static str,
    jwt: String,
    user_id: i64,
    token: String,
}

impl Repo {
    async fn create(app: &App, owner: &'static str) -> Repo {
        let jwt = common::register_user(
            app.base.as_str(),
            owner,
            &format!("{owner}@example.com"),
            PW,
        )
        .await;
        let me: Value = app
            .client
            .get(app.url("/users/me"))
            .bearer_auth(&jwt)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let user_id = me["id"].as_i64().unwrap();
        let created: Value = app
            .client
            .post(app.url("/repos"))
            .bearer_auth(&jwt)
            .json(&json!({"name": "work", "auto_init": true, "is_private": true}))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(created["name"], "work");
        let (status, token) = app
            .send(
                Method::POST,
                "/users/tokens",
                &jwt,
                json!({
                    "name": "agent",
                    "scopes": "repo:write",
                    "repos": [format!("{owner}/work")],
                }),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{token}");
        Repo {
            owner,
            jwt,
            user_id,
            token: token["token"].as_str().unwrap().to_string(),
        }
    }

    fn drafts(&self) -> String {
        format!("/repos/{}/work/agent-drafts", self.owner)
    }

    fn remote(&self, app: &App) -> String {
        app.base.replacen(
            "http://",
            &format!("http://{}:{}@", self.owner, self.token),
            1,
        ) + &format!("/{}/work.git", self.owner)
    }
}

/// Run git, returning (succeeded, stderr).
fn git_try(args: &[&str], cwd: Option<&Path>) -> (bool, String) {
    let gateway = rg_git::cli_gateway::global_gateway().as_ref().unwrap();
    let out = gateway.run(args, cwd).unwrap();
    (out.success(), out.stderr_str())
}

fn git(args: &[&str], cwd: Option<&Path>) -> String {
    let gateway = rg_git::cli_gateway::global_gateway().as_ref().unwrap();
    let out = gateway.run(args, cwd).unwrap();
    out.ensure_success().unwrap();
    out.stdout_str().trim().to_string()
}

/// Clone, commit one file, and leave the working tree ready to push.
fn working_copy(app: &App, repo: &Repo) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("wc");
    git(&["clone", &repo.remote(app), &path.to_string_lossy()], None);
    git(&["config", "user.name", "Agent"], Some(&path));
    git(&["config", "user.email", "agent@example.com"], Some(&path));
    std::fs::write(path.join("a.txt"), "draft work\n").unwrap();
    git(&["add", "a.txt"], Some(&path));
    git(&["commit", "-m", "draft work"], Some(&path));
    (dir, path)
}

fn push(path: &Path, app: &App, repo: &Repo, refspec: &str) -> (bool, String) {
    git_try(&["push", &repo.remote(app), refspec], Some(path))
}

fn ls_remote(app: &App, repo: &Repo) -> String {
    git(&["ls-remote", &repo.remote(app)], None)
}

/// A loopback receiver that counts what reaches it.
struct Receiver {
    url: String,
    pokes: mpsc::UnboundedReceiver<()>,
    _task: tokio::task::JoinHandle<()>,
}

impl Receiver {
    async fn spawn() -> Receiver {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let (tx, pokes) = mpsc::unbounded_channel();
        let task = tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                use tokio::io::{AsyncReadExt, AsyncWriteExt};
                let tx = tx.clone();
                tokio::spawn(async move {
                    let mut buffer = vec![0u8; 16384];
                    let _ = socket.read(&mut buffer).await;
                    let _ = tx.send(());
                    let _ = socket
                        .write_all(
                            b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                        )
                        .await;
                });
            }
        });
        Receiver {
            url: format!("http://127.0.0.1:{port}/hook"),
            pokes,
            _task: task,
        }
    }

    /// Wait (bounded) for at least one delivery; `false` when none arrived.
    async fn got_a_delivery(&mut self) -> bool {
        tokio::time::timeout(Duration::from_millis(1500), self.pokes.recv())
            .await
            .is_ok()
    }
}

/// The number of `webhook_deliveries` rows of a repository.
async fn deliveries(app: &App, repo_id: i64) -> u64 {
    let hooks = rg_db::ops::webhook_ops::list_by_repo(&app.db, repo_id)
        .await
        .unwrap();
    let mut total = 0u64;
    for hook in hooks {
        total += rg_db::ops::webhook_ops::list_deliveries_by_webhook(&app.db, hook.id)
            .await
            .unwrap()
            .len() as u64;
    }
    total
}

/// Wait (bounded) until a repository has at least `at_least` delivery rows: the
/// row is written after the receiver has answered, so it is not there the
/// instant the request arrives.
async fn wait_for_deliveries(app: &App, repo_id: i64, at_least: u64) -> u64 {
    for _ in 0..100 {
        let count = deliveries(app, repo_id).await;
        if count >= at_least {
            return count;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    deliveries(app, repo_id).await
}

async fn repo_id(app: &App, owner: &str) -> i64 {
    let repo = rg_core::repo::service::find_repo_by_owner_name(&app.db, owner, "work")
        .await
        .unwrap()
        .unwrap();
    repo.id
}

async fn events(
    app: &App,
    repo_id: i64,
    types: &[&str],
) -> Vec<rg_db::entities::repo_event::Model> {
    let types: Vec<String> = types.iter().map(|t| t.to_string()).collect();
    rg_db::ops::repo_event_ops::list_after(&app.db, repo_id, 0, &types, 100)
        .await
        .unwrap()
}

/// Read messages from the notification socket until one is not the welcome
/// frame, or the timeout passes.
async fn next_event(
    socket: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    within: Duration,
) -> Option<Value> {
    let deadline = tokio::time::Instant::now() + within;
    loop {
        let left = deadline.saturating_duration_since(tokio::time::Instant::now());
        match tokio::time::timeout(left, socket.next()).await {
            Ok(Some(Ok(tokio_tungstenite::tungstenite::Message::Text(text)))) => {
                let value: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
                if value["type"] == "connected" {
                    continue;
                }
                return Some(value);
            }
            Ok(Some(Ok(_))) => continue,
            _ => return None,
        }
    }
}

async fn connect_notifications(
    app: &App,
    jwt: &str,
) -> tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>> {
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    let url = format!(
        "{}/api/v1/ws/notifications",
        app.base.replacen("http://", "ws://", 1)
    );
    let mut request = url.into_client_request().unwrap();
    request.headers_mut().insert(
        "sec-websocket-protocol",
        format!("bearer.{jwt}").parse().unwrap(),
    );
    let (socket, response) = tokio_tungstenite::connect_async(request).await.unwrap();
    assert_eq!(response.status(), 101);
    socket
}

// ── registration ─────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_namespace_is_registered_validated_and_listed() {
    let app = App::new().await;
    let repo = Repo::create(&app, "drafts_register").await;
    let prefix = format!("agent/{}/", repo.user_id);

    // A prefix that is not a directory under the caller's agent id is refused.
    let (status, body) = app
        .send(
            Method::POST,
            &repo.drafts(),
            &repo.jwt,
            json!({"prefix": "agent/someone-else/", "ttl_secs": 3600}),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");

    // Each malformed shape is a 400 with the reason.
    for prefix in ["agent/42", "other/42/", "agent/42/*", "agent/42//"] {
        let (status, body) = app
            .send(
                Method::POST,
                &repo.drafts(),
                &repo.jwt,
                json!({"prefix": prefix, "ttl_secs": 3600}),
            )
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{prefix}: {body}");
    }

    // TTL bounds.
    let (status, _) = app
        .send(
            Method::POST,
            &repo.drafts(),
            &repo.jwt,
            json!({"prefix": prefix, "ttl_secs": 1}),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, created) = app
        .send(
            Method::POST,
            &repo.drafts(),
            &repo.jwt,
            json!({"prefix": prefix, "ttl_secs": 3600}),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    assert_eq!(created["prefix"], prefix);
    assert_eq!(created["state"], "active");
    assert_eq!(created["ttl_secs"], 3600);
    assert_eq!(created["grace_secs"], 86400);
    assert_eq!(created["extended_count"], 0);

    // The same prefix again is a conflict, not a second row.
    let (status, body) = app
        .send(
            Method::POST,
            &repo.drafts(),
            &repo.jwt,
            json!({"prefix": prefix, "ttl_secs": 3600}),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");

    let response = app
        .client
        .get(app.url(&repo.drafts()))
        .bearer_auth(&repo.jwt)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let listed: Value = response.json().await.unwrap();
    assert_eq!(listed.as_array().unwrap().len(), 1);
    assert_eq!(listed[0]["prefix"], prefix);
}

// ── push behaviour ───────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_draft_push_runs_ci_and_is_logged_but_notifies_nobody() {
    let app = App::new().await;
    let repo = Repo::create(&app, "drafts_quiet").await;
    let repo_id = repo_id(&app, repo.owner).await;
    let prefix = format!("agent/{}/", repo.user_id);

    // Control: an ordinary branch (main) delivers. This also proves the
    // receiver and the socket work, so "nothing arrived" below is meaningful.
    let mut receiver = Receiver::spawn().await;
    let (status, body) = app
        .send(
            Method::POST,
            &format!("/repos/{}/work/hooks", repo.owner),
            &repo.jwt,
            json!({
                "url": receiver.url,
                "events": ["push", "branch.created", "branch.deleted"],
            }),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");

    let mut socket = connect_notifications(&app, &repo.jwt).await;
    let (_dir, path) = working_copy(&app, &repo);

    let (ok, err) = push(&path, &app, &repo, "HEAD:refs/heads/not-a-draft");
    assert!(ok, "{err}");
    assert!(
        receiver.got_a_delivery().await,
        "an ordinary branch push must reach the webhook"
    );
    assert!(
        next_event(&mut socket, Duration::from_millis(2000))
            .await
            .is_some(),
        "an ordinary branch push must reach the notification socket"
    );
    // `ci_triggered` and `push` are two separate frames (plus the branch event
    // fan-out); drain them so the assertion below is about the draft push.
    while next_event(&mut socket, Duration::from_millis(300))
        .await
        .is_some()
    {}
    let baseline_deliveries = wait_for_deliveries(&app, repo_id, 1).await;
    assert!(
        baseline_deliveries >= 1,
        "the control push creates a delivery row"
    );
    // The control push delivers several events (push + branch.created) in the
    // background; drain them so the assertions below are about the draft push.
    while receiver.got_a_delivery().await {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    // The draft namespace.
    let (status, created) = app
        .send(
            Method::POST,
            &repo.drafts(),
            &repo.jwt,
            json!({"prefix": prefix, "ttl_secs": 3600, "grace_secs": 60}),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");

    let draft_ref = format!("refs/heads/{prefix}fix-parser");
    let (ok, err) = push(&path, &app, &repo, &format!("HEAD:{draft_ref}"));
    assert!(ok, "push to the draft: {err}");

    // CI still ran — this is the point of the area.
    let mut ci_calls = Vec::new();
    for _ in 0..100 {
        ci_calls = app.engine.calls_for(&draft_ref);
        if !ci_calls.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(ci_calls.len(), 1, "draft push must trigger CI");
    assert_eq!(ci_calls[0].trigger_type, "push");
    assert!(
        !ci_calls[0].allow_host_execution,
        "a draft branch never gets host execution, even when the instance allows it"
    );

    // The agent can still wait on its own push.
    let mut pushes = Vec::new();
    for _ in 0..100 {
        pushes = events(&app, repo_id, &["push"]).await;
        if pushes.iter().any(|event| {
            event
                .payload
                .as_deref()
                .unwrap_or_default()
                .contains(&draft_ref)
        }) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(
        pushes.iter().any(|event| event
            .payload
            .as_deref()
            .unwrap_or_default()
            .contains(&draft_ref)),
        "the draft push must be in repo_events so the agent can wait for it"
    );

    // …and nobody was woken.
    assert!(
        !receiver.got_a_delivery().await,
        "a draft push must not reach any webhook"
    );
    assert_eq!(
        deliveries(&app, repo_id).await,
        baseline_deliveries,
        "no delivery row may be created for a draft push"
    );
    assert!(
        next_event(&mut socket, Duration::from_millis(800))
            .await
            .is_none(),
        "a draft push must not fan out over the notification socket"
    );
    // A `git push --delete` of a draft branch is the fourth notification
    // channel: it fires inside `on_delete`, before `post_push_hooks` ever runs,
    // so it needs its own guard (R18-5). It must be silent too.
    let (ok, err) = git_try(
        &["push", &repo.remote(&app), &format!(":{draft_ref}")],
        Some(&path),
    );
    assert!(ok, "delete the draft branch: {err}");
    assert!(
        !receiver.got_a_delivery().await,
        "deleting a draft branch must not reach any webhook"
    );
    assert_eq!(
        deliveries(&app, repo_id).await,
        baseline_deliveries,
        "deleting a draft branch must not create a delivery row"
    );
    assert!(
        next_event(&mut socket, Duration::from_millis(800))
            .await
            .is_none(),
        "deleting a draft branch must not fan out over the notification socket"
    );

    let branch_events = events(&app, repo_id, &["branch.created", "branch.deleted"]).await;
    assert!(
        branch_events.iter().all(|event| !event
            .payload
            .as_deref()
            .unwrap_or_default()
            .contains(&prefix)),
        "a draft branch produces no branch event, on create or on delete: {branch_events:?}"
    );
}

// ── expiry → delete ──────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn expiry_closes_the_gate_before_the_cleanup_deletes_anything() {
    let app = App::new().await;
    let repo = Repo::create(&app, "drafts_expiry").await;
    let prefix = format!("agent/{}/", repo.user_id);
    let (_dir, path) = working_copy(&app, &repo);

    let (status, created) = app
        .send(
            Method::POST,
            &repo.drafts(),
            &repo.jwt,
            json!({"prefix": prefix, "ttl_secs": 3600, "grace_secs": 3600}),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let id = created["id"].as_i64().unwrap();

    let draft_ref = format!("refs/heads/{prefix}work");
    let (ok, err) = push(&path, &app, &repo, &format!("HEAD:{draft_ref}"));
    assert!(ok, "{err}");

    // Expire it: the next tick closes the gate but must not delete anything.
    let expires_at = chrono::Utc::now() - chrono::Duration::seconds(5);
    rg_db::ops::agent_draft_ops::find_by_id(&app.db, id)
        .await
        .unwrap()
        .unwrap();
    sql_set_expiry(&app, id, expires_at).await;

    let summary = rg_http::api::drafts::run_cleanup_once(&app.state)
        .await
        .unwrap();
    assert_eq!(summary.gated, 1, "the tick must close the gate");
    assert_eq!(
        summary.deleted, 0,
        "nothing is deleted before the grace period"
    );
    let after_gate = rg_db::ops::agent_draft_ops::find_by_id(&app.db, id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(after_gate.state, "expiring");
    assert!(
        ls_remote(&app, &repo).contains(&draft_ref),
        "the branch survives the gate closing"
    );

    // A push into the closed namespace is refused, with a message that says how
    // to push again.
    std::fs::write(path.join("a.txt"), "more work\n").unwrap();
    git(&["add", "a.txt"], Some(&path));
    git(&["commit", "-m", "more work"], Some(&path));
    let (ok, err) = push(&path, &app, &repo, &format!("HEAD:{draft_ref}"));
    assert!(!ok, "a push into an expired namespace must be refused");
    assert!(err.contains("expired at"), "{err}");
    assert!(err.contains("agent-drafts"), "{err}");

    // The grace period was 1h, so a second tick still deletes nothing.
    let summary = rg_http::api::drafts::run_cleanup_once(&app.state)
        .await
        .unwrap();
    assert_eq!(summary.deleted, 0);

    // Move the expiry far enough back that the grace period is over.
    sql_set_expiry(&app, id, expires_at - chrono::Duration::seconds(7200)).await;
    let summary = rg_http::api::drafts::run_cleanup_once(&app.state)
        .await
        .unwrap();
    assert_eq!(summary.deleted, 1, "the grace period is over: delete");
    assert_eq!(summary.deleted_refs, 1);
    assert_eq!(summary.failures, 0);
    assert!(
        !ls_remote(&app, &repo).contains(&draft_ref),
        "the draft branch is gone"
    );
    let after = rg_db::ops::agent_draft_ops::find_by_id(&app.db, id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(after.state, "deleted");

    // The prefix cannot be reused: the branch would land in a dead directory.
    let (_ok, err) = push(&path, &app, &repo, &format!("HEAD:{draft_ref}"));
    assert!(err.contains("cleaned up"), "{err}");
    let (status, body) = app
        .send(
            Method::POST,
            &repo.drafts(),
            &repo.jwt,
            json!({"prefix": prefix, "ttl_secs": 3600}),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert!(body["error"]["message"]
        .as_str()
        .unwrap_or_default()
        .contains("cleaned up"));
}

/// Backdate a namespace's deadline. The tests cannot wait an hour, and the
/// column is the only clock the cleanup reads.
async fn sql_set_expiry(app: &App, id: i64, at: chrono::DateTime<chrono::Utc>) {
    use sea_orm::ConnectionTrait;
    let backend = app.db.get_database_backend();
    let statement = sea_orm::Statement::from_sql_and_values(
        backend,
        rg_db::prepare_sql(
            backend,
            "UPDATE agent_draft_namespaces SET expires_at = ? WHERE id = ?",
        ),
        [at.into(), id.into()],
    );
    app.db.execute(statement).await.unwrap();
}

// ── the guard the cleanup deliberately bypasses ──────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cleanup_deletes_a_branch_an_open_pull_request_occupies_and_audits_the_bypass() {
    let app = App::new().await;
    let repo = Repo::create(&app, "drafts_guard").await;
    let prefix = format!("agent/{}/", repo.user_id);
    let (_dir, path) = working_copy(&app, &repo);

    let (status, created) = app
        .send(
            Method::POST,
            &repo.drafts(),
            &repo.jwt,
            json!({"prefix": prefix, "ttl_secs": 3600, "grace_secs": 0}),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let id = created["id"].as_i64().unwrap();

    let branch = format!("{prefix}with-a-pr");
    let (ok, err) = push(&path, &app, &repo, &format!("HEAD:refs/heads/{branch}"));
    assert!(ok, "{err}");

    // An open pull request from the draft branch: this is exactly what makes
    // `delete_branch` refuse, so the REST delete would never clean it up.
    let (status, pr) = app
        .send(
            Method::POST,
            &format!("/repos/{}/work/pulls", repo.owner),
            &repo.jwt,
            json!({"title": "draft work", "head": branch, "base": "main"}),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{pr}");

    // The user-facing delete refuses it.
    let (status, body) = app
        .send(
            Method::DELETE,
            &format!("/repos/{}/work/branches/{branch}", repo.owner),
            &repo.jwt,
            Value::Null,
        )
        .await;
    assert!(
        status.is_client_error(),
        "the branch guard must still refuse a branch an open PR occupies: {status} {body}"
    );

    // The cleanup ignores that guard.
    sql_set_expiry(&app, id, chrono::Utc::now() - chrono::Duration::seconds(10)).await;
    // A zero grace period: one tick closes the gate and deletes in the same
    // pass. (The ordering "gate first, delete after the grace" is what the
    // expiry test pins, with a non-zero grace.)
    let summary = rg_http::api::drafts::run_cleanup_once(&app.state)
        .await
        .unwrap();
    assert_eq!(summary.gated, 1);
    assert_eq!(summary.deleted, 1, "{summary:?}");
    assert!(!ls_remote(&app, &repo).contains(&format!("refs/heads/{branch}")));

    // The bypass is in the audit trail, with its reason.
    let audit = rg_db::ops::audit_log_ops::list_before_limit(
        &app.db,
        chrono::Utc::now() + chrono::Duration::hours(1),
        200,
    )
    .await
    .unwrap()
    .into_iter()
    .rev()
    .find(|row| row.action == "agent_draft.cleaned")
    .expect("a cleanup audit entry");
    let details: Value = serde_json::from_str(audit.details.as_deref().unwrap()).unwrap();
    assert_eq!(details["prefix"], prefix);
    assert_eq!(details["deleted_refs"], 1);
    assert_eq!(details["bypassed_guard"], "on_delete");
    assert!(
        details["bypass_reason"]
            .as_str()
            .unwrap()
            .contains("open pull request"),
        "{details}"
    );
}

// ── extend ───────────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn extend_never_shortens_and_stops_at_four_times_the_ttl() {
    let app = App::new().await;
    let repo = Repo::create(&app, "drafts_extend").await;
    let prefix = format!("agent/{}/", repo.user_id);

    let (status, created) = app
        .send(
            Method::POST,
            &repo.drafts(),
            &repo.jwt,
            json!({"prefix": prefix, "ttl_secs": 3600, "grace_secs": 600}),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let id = created["id"].as_i64().unwrap();
    let created_at: chrono::DateTime<chrono::Utc> =
        chrono::DateTime::parse_from_rfc3339(created["created_at"].as_str().unwrap())
            .unwrap()
            .into();

    let mut last = created;
    for _ in 0..8 {
        let (status, body) = app
            .send(
                Method::PATCH,
                &format!("{}/{id}", repo.drafts()),
                &repo.jwt,
                json!({"ttl_secs": 3600}),
            )
            .await;
        if status == StatusCode::OK {
            last = body;
            continue;
        }
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert!(
            body["error"]["message"]
                .as_str()
                .unwrap_or_default()
                .contains("maximum lifetime"),
            "{body}"
        );
        break;
    }

    let deadline: chrono::DateTime<chrono::Utc> =
        chrono::DateTime::parse_from_rfc3339(last["expires_at"].as_str().unwrap())
            .unwrap()
            .into();
    assert_eq!(
        deadline,
        created_at + chrono::Duration::seconds(4 * 3600),
        "the total lifetime is capped at 4 x the TTL"
    );
    assert!(last["extended_count"].as_i64().unwrap() >= 1);
}

// ── secrets (external runner path) ───────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_draft_pipeline_is_handed_no_repository_secrets() {
    let app = App::new().await;
    let repo = Repo::create(&app, "drafts_secrets").await;
    let prefix = format!("agent/{}/", repo.user_id);
    let repo_id = repo_id(&app, repo.owner).await;
    // Registering a runner needs an administrator. On a fresh database the first
    // account is promoted automatically; on the shared PostgreSQL / MySQL
    // database another test's account may have been first, so say it explicitly.
    rg_db::ops::user_ops::update_by_id(&app.db, repo.user_id, None, None, Some(true), None)
        .await
        .unwrap();

    // A secret on the repository, and a runner allowed to pick jobs up.
    let (status, body) = app
        .send(
            Method::PUT,
            &format!("/repos/{}/work/actions/secrets/DEPLOY_TOKEN", repo.owner),
            &repo.jwt,
            json!({"value": "super-secret-value"}),
        )
        .await;
    assert!(status.is_success(), "store the secret: {status} {body}");

    let (status, runner) = app
        .send(
            Method::POST,
            "/runners/register",
            &repo.jwt,
            json!({"name": "draft-runner"}),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{runner}");
    let runner_id = runner["id"].as_i64().unwrap();
    let runner_token = runner["token"].as_str().unwrap().to_string();

    // A pending job for a draft ref, and the same for an ordinary ref.
    let (status, created) = app
        .send(
            Method::POST,
            &repo.drafts(),
            &repo.jwt,
            json!({"prefix": prefix, "ttl_secs": 3600}),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");

    let draft_job = pending_job(&app, repo_id, &format!("refs/heads/{prefix}secret-probe")).await;
    let polled = poll(&app, &runner_token, runner_id).await;
    assert_eq!(polled["job_id"], draft_job, "{polled}");
    let variables = polled["variables"].as_object().unwrap();
    assert!(
        !variables.contains_key("DEPLOY_TOKEN"),
        "a draft branch is never handed the repository's secrets: {polled}"
    );
    assert_eq!(
        variables["CI_REF"],
        format!("refs/heads/{prefix}secret-probe")
    );

    // Control: the same job on an ordinary branch does get the secret.
    let main_job = pending_job(&app, repo_id, "refs/heads/main").await;
    let polled = poll(&app, &runner_token, runner_id).await;
    assert_eq!(polled["job_id"], main_job, "{polled}");
    assert_eq!(
        polled["variables"]["DEPLOY_TOKEN"], "super-secret-value",
        "an ordinary branch still gets the secret: {polled}"
    );
}

async fn pending_job(app: &App, repo_id: i64, ref_name: &str) -> i64 {
    let pipeline = rg_db::ops::pipeline_ops::create_pipeline(
        &app.db,
        repo_id,
        "abcdefabcdefabcdefabcdefabcdefabcdefabcd",
        ref_name,
        "push",
        Some(1),
    )
    .await
    .unwrap();
    let stage = rg_db::ops::pipeline_ops::create_stage(&app.db, pipeline.id, "test", 0)
        .await
        .unwrap();
    rg_db::ops::pipeline_ops::create_job(
        &app.db, stage.id, "probe", "true", None, None, None, None, None, false, None, None, None,
        None,
    )
    .await
    .unwrap()
    .id
}

async fn poll(app: &App, runner_token: &str, runner_id: i64) -> Value {
    let response = app
        .client
        .get(app.url(&format!("/runners/{runner_id}/jobs/poll")))
        .bearer_auth(runner_token)
        .send()
        .await
        .unwrap();
    let status = response.status();
    let body: Value = response.json().await.unwrap_or(Value::Null);
    assert_eq!(status, StatusCode::OK, "{body}");
    body
}
