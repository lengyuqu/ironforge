use std::sync::Arc;

pub use rg_db;
pub use rg_http;

/// Create a temporary file-based SQLite database with all migrations applied.
pub async fn setup_test_db() -> (rg_db::DatabaseConnection, tempfile::TempDir) {
    use sea_orm::{ConnectOptions, Database};
    use std::time::Duration;
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    let db_path = dir.path().join("test.db");
    let db_url = format!("sqlite://{}?mode=rwc", db_path.display());
    let mut opt = ConnectOptions::new(db_url);
    opt.max_connections(2).min_connections(1).connect_timeout(Duration::from_secs(5));
    let db = Database::connect(opt).await.expect("failed to connect");
    rg_db::run_migrations(&db).await.expect("migration failed");
    (db, dir)
}

pub fn build_test_app_state(db: rg_db::DatabaseConnection, repo_root: std::path::PathBuf) -> rg_http::AppState {
    rg_http::AppState {
        repo_root: Arc::new(repo_root), db,
        jwt_secret: Arc::new("test-secret-key".to_string()),
        docker_enabled: false, external_runners: false,
        rate_limiter: rg_http::rate_limit::RateLimiter::new(10000, 60),
        notification_hub: rg_http::ws::NotificationHub::new(),
        smtp_config: None,
    }
}

pub async fn spawn_test_app() -> String {
    let (db, dir) = setup_test_db().await;
    let repo_root = dir.path().join("repos");
    std::fs::create_dir_all(&repo_root).ok();
    let state = build_test_app_state(db, repo_root);
    let app = rg_http::create_router_for_test(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{}", addr);
    tokio::spawn(async move { let _dir = dir; axum::serve(listener, app).await.unwrap(); });
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    base_url
}

pub async fn register_user(base: &str, username: &str, email: &str, password: &str) -> String {
    let client = reqwest::Client::new();
    let resp = client.post(format!("{}/api/v1/users/register", base))
        .json(&serde_json::json!({"username": username, "email": email, "password": password}))
        .send().await.unwrap();
    assert!(resp.status().is_success(), "register failed for '{}': {}", username, resp.status());
    let body: serde_json::Value = resp.json().await.unwrap();
    body["token"].as_str().unwrap().to_string()
}
