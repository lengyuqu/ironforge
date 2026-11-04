//! Client-facing metrics (`pat_auth_total`, `ws_connections_total`).
//!
//! These back the "the CLI is broken" reports: a PAT that silently stops
//! working, or `ifg ci logs --follow` that cannot stream, both show up here.

mod common;

use common::{register_full, spawn_test_app_with_db};
use serde_json::json;

/// Scrape `/metrics` — deliberately **without** a token: Prometheus scrapers
/// do not carry application credentials, and the route is public by design.
async fn scrape(base: &str) -> String {
    let response = reqwest::Client::new()
        .get(format!("{base}/metrics"))
        .send()
        .await
        .expect("metrics request");
    let status = response.status();
    let body = response.text().await.expect("metrics body");
    assert_eq!(status.as_u16(), 200, "scrape status {status}: {body}");
    assert!(!body.is_empty(), "empty metrics body");
    body
}

/// True when the scrape contains `series{labels}` with a value >= 1.
fn has_series(metrics: &str, series: &str) -> bool {
    metrics.lines().any(|line| {
        line.starts_with(series)
            && line
                .rsplit(' ')
                .next()
                .and_then(|value| value.parse::<f64>().ok())
                .is_some_and(|value| value >= 1.0)
    })
}

#[tokio::test]
async fn pat_outcomes_are_counted() {
    let (base, _db) = spawn_test_app_with_db().await;
    let (token, _) = register_full(&base, "meter", "meter@example.com").await;

    // A bad PAT must be visible as `invalid_token`, not just as a 401.
    let response = reqwest::Client::new()
        .get(format!("{base}/api/v1/users/me"))
        .bearer_auth("ifp_not_a_real_token")
        .send()
        .await
        .expect("request");
    assert_eq!(response.status().as_u16(), 401);

    let metrics = scrape(&base).await;
    assert!(
        has_series(&metrics, "pat_auth_total{outcome=\"invalid_token\"}"),
        "invalid_token series missing:\n{metrics}"
    );

    // A working PAT counts as `ok` (the session JWT above is not a PAT and
    // never reaches the PAT path).
    let pat = reqwest::Client::new()
        .post(format!("{base}/api/v1/users/tokens"))
        .bearer_auth(&token)
        .json(&json!({"name": "meter", "scopes": "repo,user"}))
        .send()
        .await
        .expect("mint pat")
        .json::<serde_json::Value>()
        .await
        .expect("pat body")["token"]
        .as_str()
        .expect("pat")
        .to_string();
    let response = reqwest::Client::new()
        .get(format!("{base}/api/v1/users/me"))
        .bearer_auth(&pat)
        .send()
        .await
        .expect("request");
    assert_eq!(response.status().as_u16(), 200);
    let metrics = scrape(&base).await;
    assert!(
        has_series(&metrics, "pat_auth_total{outcome=\"ok\"}"),
        "ok series missing:\n{metrics}"
    );
}

#[tokio::test]
async fn websocket_connections_are_counted() {
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;

    let (base, db) = spawn_test_app_with_db().await;
    let (token, user_id) = register_full(&base, "streamer", "streamer@example.com").await;

    let repo = reqwest::Client::new()
        .post(format!("{base}/api/v1/repos"))
        .bearer_auth(&token)
        .json(&json!({"name": "streaming"}))
        .send()
        .await
        .expect("create repo")
        .json::<serde_json::Value>()
        .await
        .expect("repo body");
    let repo_id = repo["id"].as_i64().expect("repo id");

    // Seed a pipeline → stage → job so the job-log socket has something to
    // attach to.
    let pipeline = rg_db::ops::pipeline_ops::create_pipeline(
        &db,
        repo_id,
        "deadbeef",
        "main",
        "push",
        Some(user_id),
    )
    .await
    .expect("pipeline");
    let stage = rg_db::ops::pipeline_ops::create_stage(&db, pipeline.id, "build", 1)
        .await
        .expect("stage");
    let job = rg_db::ops::pipeline_ops::create_job(
        &db, stage.id, "build", "echo hi", None, None, None, None, None, false, None, None, None,
    )
    .await
    .expect("job");

    let ws_base = base.replacen("http://", "ws://", 1);
    let url = format!("{ws_base}/api/v1/ws/job/{}?since=0", job.id);
    let mut request = url.as_str().into_client_request().expect("ws url");
    request.headers_mut().insert(
        "authorization",
        format!("Bearer {token}").parse().expect("header"),
    );
    let (mut socket, _) = tokio_tungstenite::connect_async(request)
        .await
        .expect("ws handshake");
    // The handshake completed, so the server counted the connection when it
    // upgraded; give the handler a moment to start before closing.
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    let _ = socket.close(None).await;

    let metrics = scrape(&base).await;
    assert!(
        has_series(&metrics, "ws_connections_total{channel=\"job_log\"}"),
        "job_log connection series missing:\n{metrics}"
    );
    // The gauge must be back to zero once the socket closed.
    let active_zero = metrics.lines().any(|line| {
        line.starts_with("ws_active_connections{channel=\"job_log\"}") && line.ends_with(" 0")
    });
    assert!(
        active_zero,
        "job_log gauge should return to 0 after close:\n{metrics}"
    );
}
