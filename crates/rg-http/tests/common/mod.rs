use std::sync::Arc;

pub use rg_db;
pub use rg_http;

use rg_core::package_registry::oci::OciStorage;

struct NoopCiEngine;

impl rg_core::ci::CiTrigger for NoopCiEngine {
    fn has_ci_config(&self, _repo_path: &std::path::Path, _commit_sha: &str) -> bool {
        false
    }

    fn trigger_pipeline<'a>(
        &'a self,
        _params: rg_core::ci::TriggerPipelineParams<'a>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<i64>> + Send + 'a>> {
        Box::pin(async { Ok(0) })
    }

    fn resume_pipeline<'a>(
        &'a self,
        _params: rg_core::ci::ResumePipelineParams<'a>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<()>> + Send + 'a>> {
        Box::pin(async { Ok(()) })
    }
}

/// A migrated SQLite file, shared by every test of every test binary.
///
/// Running all migrations (hundreds of `CREATE TABLE` / index / trigger
/// statements) costs several seconds on a CI runner, and it does not get faster
/// with more threads: SQLite parses DDL under a process-wide allocator mutex, so
/// concurrent tests queue behind each other. Copying a finished file is a few
/// milliseconds and parallelises.
///
/// Each test file is its own process, so a per-process template still paid the
/// full migration cost about a hundred times per `cargo test --workspace`
/// (9 to 13 seconds even for a binary with a single test). The template is
/// therefore kept in the temp directory under a key derived from the contents of
/// `rg-db/src`: the first process builds it, every later process just finds it.
/// Editing a migration changes the key, so a stale schema cannot be picked up.
static MIGRATED_TEMPLATE: tokio::sync::OnceCell<std::path::PathBuf> =
    tokio::sync::OnceCell::const_new();

/// Hash of everything under `rg-db/src` (paths and contents, sorted): the schema
/// is defined there, so any change to it yields a new template.
fn schema_key() -> String {
    use sha2::{Digest, Sha256};

    fn collect(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                collect(&path, out);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                out.push(path);
            }
        }
    }

    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../rg-db/src");
    let mut files = Vec::new();
    collect(&root, &mut files);
    files.sort();
    let mut hasher = Sha256::new();
    for file in &files {
        let relative = file.strip_prefix(&root).unwrap_or(file);
        hasher.update(relative.to_string_lossy().as_bytes());
        hasher.update([0]);
        hasher.update(std::fs::read(file).unwrap_or_default());
        hasher.update([0]);
    }
    let digest = hasher.finalize();
    digest[..8].iter().map(|b| format!("{b:02x}")).collect()
}

async fn build_template(path: &std::path::Path) {
    use sea_orm::{ConnectOptions, ConnectionTrait, Database};
    // Build next to the final name and rename into place: a reader never sees a
    // half-written file, and two processes that race both produce the same
    // content, so whichever rename lands last is fine.
    let building = path.with_extension(format!("building-{}", std::process::id()));
    let _ = std::fs::remove_file(&building);
    let mut opt = ConnectOptions::new(format!("sqlite://{}?mode=rwc", building.display()));
    opt.max_connections(1)
        .min_connections(1)
        .connect_timeout(std::time::Duration::from_secs(5));
    let db = Database::connect(opt).await.expect("failed to connect");
    rg_db::run_migrations(&db).await.expect("migration failed");
    // Fold the write-ahead log into the file so copying the file is enough.
    db.execute_unprepared("PRAGMA wal_checkpoint(TRUNCATE)")
        .await
        .expect("checkpoint failed");
    db.close().await.expect("close failed");
    for suffix in ["-wal", "-shm"] {
        let mut sidecar = building.clone().into_os_string();
        sidecar.push(suffix);
        let _ = std::fs::remove_file(sidecar);
    }
    std::fs::rename(&building, path).expect("failed to publish the migrated template");
}

async fn migrated_template() -> &'static std::path::Path {
    MIGRATED_TEMPLATE
        .get_or_init(|| async {
            let dir = std::env::temp_dir();
            let path = dir.join(format!("ironforge-test-template-{}.db", schema_key()));
            if !path.exists() {
                build_template(&path).await;
                // Templates of older schemas are only litter; keep recent ones in
                // case another checkout is running tests at the same time.
                if let Ok(entries) = std::fs::read_dir(&dir) {
                    for entry in entries.flatten() {
                        let name = entry.file_name();
                        let name = name.to_string_lossy();
                        let old = entry
                            .metadata()
                            .and_then(|m| m.modified())
                            .ok()
                            .and_then(|t| t.elapsed().ok())
                            .is_some_and(|age| age > std::time::Duration::from_secs(24 * 3600));
                        if name.starts_with("ironforge-test-template-") && old {
                            let _ = std::fs::remove_file(entry.path());
                        }
                    }
                }
            }
            path
        })
        .await
}

/// Create a temporary file-based SQLite database with all migrations applied.
pub async fn setup_test_db() -> (rg_db::DatabaseConnection, tempfile::TempDir) {
    // Webhook/mirror tests deliver to loopback receivers; the outbound URL
    // guard must let them through (scheme allowlists still apply).
    std::env::set_var("IRONFORGE_ALLOW_LOCAL_OUTBOUND", "1");
    let template = migrated_template().await;
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    let db_path = dir.path().join("test.db");
    std::fs::copy(template, &db_path).expect("failed to copy the migrated template");
    let db_url = format!("sqlite://{}?mode=rwc", db_path.display());
    // No migration run here: the template was migrated by this very process, so
    // the copy has exactly the schema the code expects. Connect the way the
    // server does (WAL, busy timeout), not with sqlx's bare defaults.
    let db = rg_db::connect_with_pool(&db_url, 5, 60, 2)
        .await
        .expect("failed to connect");
    (db, dir)
}

pub fn build_test_app_state(
    db: rg_db::DatabaseConnection,
    repo_root: std::path::PathBuf,
) -> rg_http::AppState {
    let db_for_queue = db.clone();
    let tmp_dir = std::env::var("TMPDIR")
        .or_else(|_| std::env::var("TMP"))
        .or_else(|_| std::env::var("TEMP"))
        .unwrap_or_else(|_| "/private/tmp".to_string());
    let oci_storage_path = std::path::Path::new(&tmp_dir).join("ironforge-test-oci");
    let blob_storage: Arc<dyn rg_core::blob_storage::BlobStorage> =
        Arc::new(rg_core::blob_storage::LocalBlobStorage::new(&repo_root));
    rg_http::AppState {
        blob_storage: blob_storage.clone(),
        repo_root: Arc::new(repo_root),
        db,
        jwt_secret: Arc::new("test-secret-key".to_string()),
        docker_enabled: false,
        external_runners: false,
        // Tests exercise the local execution path deliberately.
        allow_host_execution: true,
        rate_limiter: rg_http::rate_limit::RateLimiter::new(10000, 60),
        notification_hub: rg_http::ws::NotificationHub::new(),
        smtp_config: None,
        oci_storage: Arc::new(OciStorage::from_backend(blob_storage, oci_storage_path)),
        log_write_queue: rg_core::ci::log_write_queue::LogWriteQueue::spawn(db_for_queue),
        external_url: None,
        job_timeout_secs: 3600,
        ci_engine: Arc::new(NoopCiEngine),
    }
}

#[allow(dead_code)]
pub async fn spawn_test_app() -> String {
    let (db, dir) = setup_test_db().await;
    let repo_root = dir.path().join("repos");
    std::fs::create_dir_all(&repo_root).ok();
    let state = build_test_app_state(db, repo_root);
    let app = rg_http::create_router_for_test(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{}", addr);
    tokio::spawn(async move {
        let _dir = dir;
        axum::serve(listener, app).await.unwrap();
    });
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    base_url
}

/// Spawn the test app and keep the db handle alive for tests that need
/// to manipulate data directly (e.g. promoting admin users).
#[allow(dead_code)]
pub async fn spawn_test_app_with_db() -> (String, rg_db::DatabaseConnection) {
    let (base, db, _) = spawn_test_app_with_root().await;
    (base, db)
}

/// Like [`spawn_test_app_with_db`], also returning the directory that holds the
/// bare repositories, for tests that need to look at (or alter) git directly.
#[allow(dead_code)]
pub async fn spawn_test_app_with_root() -> (String, rg_db::DatabaseConnection, std::path::PathBuf) {
    let (db, dir) = setup_test_db().await;
    let repo_root = dir.path().join("repos");
    std::fs::create_dir_all(&repo_root).ok();
    let root_for_caller = repo_root.clone();
    let state = build_test_app_state(db.clone(), repo_root);
    let app = rg_http::create_router_for_test(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{}", addr);
    tokio::spawn(async move {
        let _dir = dir;
        axum::serve(listener, app).await.unwrap();
    });
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    (base_url, db, root_for_caller)
}

/// Like [`spawn_test_app_with_db`], but on the PostgreSQL or MySQL database
/// named by `IRONFORGE_TEST_DATABASE_URL` when it is set (migrated, shared
/// between the tests of one process, never reset: start each test target on a
/// fresh database and give each test its own account names). SQLite otherwise.
#[allow(dead_code)]
pub async fn spawn_test_app_on_env_db() -> (String, rg_db::DatabaseConnection) {
    let (base, db, _) = spawn_test_app_on_env_db_with_root().await;
    (base, db)
}

/// [`spawn_test_app_on_env_db`], also returning the directory that holds the
/// bare repositories.
#[allow(dead_code)]
pub async fn spawn_test_app_on_env_db_with_root(
) -> (String, rg_db::DatabaseConnection, std::path::PathBuf) {
    let Ok(url) = std::env::var("IRONFORGE_TEST_DATABASE_URL") else {
        return spawn_test_app_with_root().await;
    };
    static SETUP: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    let _guard = SETUP.lock().await;
    std::env::set_var("IRONFORGE_ALLOW_LOCAL_OUTBOUND", "1");
    let db = rg_db::connect_with_pool(&url, 20, 60, 16)
        .await
        .expect("connect");
    rg_db::run_migrations(&db).await.expect("migrate");
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    let repo_root = dir.path().join("repos");
    std::fs::create_dir_all(&repo_root).ok();
    let root_for_caller = repo_root.clone();
    let state = build_test_app_state(db.clone(), repo_root);
    let app = rg_http::create_router_for_test(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base_url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        let _dir = dir;
        axum::serve(listener, app).await.unwrap();
    });
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    (base_url, db, root_for_caller)
}

pub async fn register_user(base: &str, username: &str, email: &str, password: &str) -> String {
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/api/v1/users/register", base))
        .json(&serde_json::json!({"username": username, "email": email, "password": password}))
        .send()
        .await
        .unwrap();
    assert!(
        resp.status().is_success(),
        "register failed for '{}': {}",
        username,
        resp.status()
    );
    let body: serde_json::Value = resp.json().await.unwrap();
    body["token"].as_str().unwrap().to_string()
}

/// Register and return (token, user_id).
#[allow(dead_code)]
pub async fn register_full(base: &str, username: &str, email: &str) -> (String, i64) {
    let token = register_user(base, username, email, "Qz7$wRtm").await;
    let client = reqwest::Client::new();
    let body: serde_json::Value = client
        .get(format!("{}/api/v1/users/me", base))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    (token, body["id"].as_i64().unwrap())
}

/// Register the instance's first account. The first registration is promoted
/// to administrator, so tests that need an ordinary (non-admin) user must
/// register one of these first and only then register their own users.
#[allow(dead_code)]
pub async fn register_bootstrap_admin(base: &str) {
    register_user(
        base,
        "bootstrap_admin",
        "bootstrap_admin@example.com",
        "Qz7$wRtm",
    )
    .await;
}

/// Create a repo and return its id.
#[allow(dead_code)]
pub async fn create_repo(base: &str, token: &str, name: &str) -> i64 {
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/api/v1/repos", base))
        .bearer_auth(token)
        .json(&serde_json::json!({"name": name}))
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        201,
        "create_repo '{}' failed: {}",
        name,
        resp.status()
    );
    resp.json::<serde_json::Value>().await.unwrap()["id"]
        .as_i64()
        .unwrap()
}

/// Create an issue and return (id, number).
#[allow(dead_code)]
pub async fn create_issue(
    base: &str,
    token: &str,
    owner: &str,
    repo: &str,
    title: &str,
) -> (i64, i64) {
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/api/v1/repos/{}/{}/issues", base, owner, repo))
        .bearer_auth(token)
        .json(&serde_json::json!({"title": title}))
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        201,
        "create_issue '{}' failed: {}",
        title,
        resp.status()
    );
    let body: serde_json::Value = resp.json().await.unwrap();
    (
        body["id"].as_i64().unwrap(),
        body["number"].as_i64().unwrap(),
    )
}
