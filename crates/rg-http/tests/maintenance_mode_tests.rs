//! Regression: maintenance mode returned 200 and blocked `git-upload-pack`.
//!
//! The middleware rejected mutating requests with a JSON body but no status
//! code, so clients saw `200 OK` and treated the rejection as success. It also
//! matched on the method alone, which blocked `POST .../git-upload-pack` — a
//! read-only git operation that every clone and fetch uses.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

fn app() -> axum::Router {
    // Mirror the real git route shapes so the middleware sees realistic paths.
    axum::Router::new()
        .route("/echo", axum::routing::post(|| async { "ok" }))
        .route(
            "/repos/{owner}/{repo}/git-upload-pack",
            axum::routing::post(|| async { "pack" }),
        )
        .route(
            "/repos/{owner}/{repo}/git-receive-pack",
            axum::routing::post(|| async { "updated" }),
        )
        .route("/plain", axum::routing::get(|| async { "ok" }))
        .layer(axum::middleware::from_fn(
            rg_http::middleware::maintenance_middleware,
        ))
}

async fn post(router: axum::Router, path: &str) -> (StatusCode, String) {
    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(path)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, String::from_utf8_lossy(&body).to_string())
}

#[tokio::test]
async fn maintenance_returns_503_and_keeps_git_fetch_working() {
    // NOTE: instance settings are process-global; this test owns the toggle and
    // restores it. `spawn_test_app` tests do not set maintenance mode.
    rg_http::instance::update_settings(|settings| settings.maintenance_mode = true);

    // A plain mutating request is refused with 503, not 200.
    let (status, _) = post(app(), "/echo").await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);

    // Read-only git transport must still be served.
    let (status, body) = post(app(), "/repos/owner/repo/git-upload-pack").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "git fetch must keep working during maintenance, got {status}"
    );
    assert_eq!(body, "pack");

    // The write path stays blocked.
    let (status, _) = post(app(), "/repos/owner/repo/git-receive-pack").await;
    assert_eq!(
        status,
        StatusCode::SERVICE_UNAVAILABLE,
        "git push must stay blocked during maintenance"
    );

    rg_http::instance::update_settings(|settings| settings.maintenance_mode = false);
}