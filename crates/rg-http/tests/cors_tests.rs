//! Regression: the default CORS layer reflected any `Origin` while also
//! allowing credentials.
//!
//! `AllowOrigin::mirror_request()` + `allow_credentials(true)` means any website
//! can make authenticated cross-origin requests with the victim's session cookie.
//! The default must not echo an attacker-chosen origin.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

async fn preflight(origin: &str) -> (StatusCode, Option<String>) {
    // Rebuild the router with the same CORS policy the server uses.
    let app = axum::Router::new()
        .route("/ping", axum::routing::get(|| async { "ok" }))
        .layer(rg_http::test_cors_layer());
    let response = app
        .oneshot(
            Request::builder()
                .method("OPTIONS")
                .uri("/ping")
                .header("Origin", origin)
                .header("Access-Control-Request-Method", "GET")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let allow_origin = response
        .headers()
        .get("access-control-allow-origin")
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    (status, allow_origin)
}

#[tokio::test]
async fn default_cors_does_not_reflect_arbitrary_origin() {
    // With no `IRONFORGE_CORS_ORIGINS` configured, an attacker origin must not
    // be echoed back.
    let (_, allow_origin) = preflight("https://evil.example").await;
    assert_eq!(
        allow_origin,
        None,
        "the default policy must not reflect an arbitrary Origin"
    );
}

#[tokio::test]
async fn default_cors_does_not_allow_credentials() {
    let app = axum::Router::new()
        .route("/ping", axum::routing::get(|| async { "ok" }))
        .layer(rg_http::test_cors_layer());
    let response = app
        .oneshot(
            Request::builder()
                .method("OPTIONS")
                .uri("/ping")
                .header("Origin", "https://evil.example")
                .header("Access-Control-Request-Method", "GET")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    // Either the request is refused outright, or credentials are not allowed.
    let allows_credentials = response
        .headers()
        .get("access-control-allow-credentials")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value == "true");
    let reflected = response
        .headers()
        .get("access-control-allow-origin")
        .is_some();
    assert!(
        !(allows_credentials && reflected),
        "must never combine credential allowance with a reflected origin"
    );
}