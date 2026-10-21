//! Error responses must not disclose server filesystem paths.
//!
//! Many handlers turn an internal error into the response text. When the failure
//! is a filesystem or git problem that text embeds an absolute path, e.g.
//! `target repository path already exists: /srv/ironforge/repos/alice/x.git`,
//! which tells an unauthenticated-enough caller where the data lives.

mod common;

use common::{build_test_app_state, register_full, setup_test_db};
use std::path::PathBuf;

struct App {
    base: String,
    repo_root: PathBuf,
}

async fn spawn() -> App {
    let (db, dir) = setup_test_db().await;
    let repo_root = dir.path().join("repos");
    std::fs::create_dir_all(&repo_root).unwrap();
    let state = build_test_app_state(db, repo_root.clone());
    let app = rg_http::create_router_for_test(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        let _dir = dir;
        axum::serve(listener, app).await.unwrap();
    });
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    App { base, repo_root }
}

/// Every server-side path variant a message could carry (the canonical form
/// differs from the configured one on macOS, where /var -> /private/var).
fn server_paths(app: &App) -> Vec<String> {
    let mut paths = vec![app.repo_root.display().to_string()];
    if let Ok(canonical) = app.repo_root.canonicalize() {
        paths.push(canonical.display().to_string());
    }
    paths
}

#[tokio::test]
async fn failing_repository_endpoints_do_not_reveal_server_paths() {
    let app = spawn().await;
    let (token, _) = register_full(&app.base, "leak-owner", "leak-owner@example.com").await;
    let client = reqwest::Client::new();

    let created = client
        .post(format!("{}/api/v1/repos", app.base))
        .bearer_auth(&token)
        .json(&serde_json::json!({
            "name": "broken", "is_private": false, "auto_init": true, "readme": "default"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(created.status(), 201);

    // The row exists but its directory vanished (disk failure, bad restore,
    // manual cleanup): every git-backed endpoint now fails with a path-bearing
    // internal error.
    std::fs::remove_dir_all(app.repo_root.join("leak-owner/broken.git")).unwrap();

    let paths = server_paths(&app);
    let mut checked = 0;
    for endpoint in [
        "tree",
        "log",
        "branches",
        "tags",
        "commits",
        "blob/README.md",
        "archive/main.zip",
        "blame/README.md",
    ] {
        let response = client
            .get(format!("{}/api/v1/repos/leak-owner/broken/{endpoint}", app.base))
            .bearer_auth(&token)
            .send()
            .await
            .unwrap();
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        for path in &paths {
            assert!(
                !body.contains(path.as_str()),
                "GET {endpoint} ({status}) leaked the server path {path}: {body}"
            );
        }
        if status.is_client_error() || status.is_server_error() {
            checked += 1;
        }
    }
    assert!(checked > 0, "at least one endpoint should have failed");

    // Forking the broken repository fails inside the git copy.
    let (other_token, _) = register_full(&app.base, "leak-forker", "leak-forker@example.com").await;
    let response = client
        .post(format!("{}/api/v1/repos/leak-owner/broken/fork", app.base))
        .bearer_auth(&other_token)
        .json(&serde_json::json!({}))
        .send()
        .await
        .unwrap();
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    // An I/O failure inside the fork is an internal error, not a client
    // mistake: it must be a sanitized 500, not a 400 echoing the cause.
    assert_eq!(status, 500, "{body}");
    assert!(body.contains("Internal server error"), "{body}");
    for path in &paths {
        assert!(
            !body.contains(path.as_str()),
            "fork ({status}) leaked the server path {path}: {body}"
        );
    }
}

/// The scrubber must also cover responses that never go through `AppError`
/// (plain-text git/OCI errors, ad-hoc JSON) and must leave successful responses
/// alone — a 200 may legitimately contain any text the user stored.
#[tokio::test]
async fn scrubber_covers_text_and_json_errors_but_not_success_responses() {
    use axum::http::StatusCode;
    use axum::routing::get;

    let secret = std::env::temp_dir().join("ironforge-scrub-test-root/repos");
    std::fs::create_dir_all(&secret).unwrap();
    rg_http::redact::register_server_path(&secret);
    let path = format!("{}/alice/x.git", secret.display());

    let router = axum::Router::new()
        .route(
            "/text-500",
            get({
                let path = path.clone();
                move || async move { (StatusCode::INTERNAL_SERVER_ERROR, format!("error: cannot open {path}")) }
            }),
        )
        .route(
            "/json-400",
            get({
                let path = path.clone();
                move || async move {
                    (
                        StatusCode::BAD_REQUEST,
                        axum::Json(serde_json::json!({"error": {"code": "X", "message": format!("bad {path}")}})),
                    )
                }
            }),
        )
        .route(
            "/ok-200",
            get({
                let path = path.clone();
                move || async move { format!("a user's file literally says {path}") }
            }),
        )
        .layer(axum::middleware::from_fn(rg_http::middleware::request_id_middleware));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let client = reqwest::Client::new();

    let text = client.get(format!("{base}/text-500")).send().await.unwrap();
    assert_eq!(text.status(), 500);
    let body = text.text().await.unwrap();
    assert!(!body.contains("ironforge-scrub-test-root"), "text error leaked: {body}");
    assert!(body.contains(rg_http::redact::REDACTED), "{body}");

    let json = client.get(format!("{base}/json-400")).send().await.unwrap();
    assert_eq!(json.status(), 400);
    let body = json.text().await.unwrap();
    assert!(!body.contains("ironforge-scrub-test-root"), "json error leaked: {body}");
    let parsed: serde_json::Value = serde_json::from_str(&body).expect("still valid JSON");
    assert!(parsed["error"]["request_id"].is_string(), "request id still injected: {body}");

    let ok = client.get(format!("{base}/ok-200")).send().await.unwrap();
    assert_eq!(ok.status(), 200);
    assert!(
        ok.text().await.unwrap().contains("ironforge-scrub-test-root"),
        "success responses must not be rewritten"
    );
}
