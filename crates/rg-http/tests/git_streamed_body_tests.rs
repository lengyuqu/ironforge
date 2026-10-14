//! End-to-end regression: a real `git push` over HTTP goes through the streamed
//! receive-pack path.
//!
//! The handler used to take `axum::body::Bytes`, buffering the whole pack before
//! the access check ran. It now consumes the request body as an `AsyncRead`
//! (`rg_http::body_reader::BodyReader`). This test drives an actual git client
//! against the server so a framing regression in the adapter cannot slip
//! through the unit tests.

mod common;

use std::path::Path;

use common::{register_user, spawn_test_app};

const PW: &str = "Qz7$wRtm";

fn git(args: &[&str], cwd: Option<&Path>) -> String {
    let gateway = rg_git::cli_gateway::global_gateway().as_ref().unwrap();
    let output = gateway.run(args, cwd).unwrap();
    output.ensure_success().unwrap();
    output.stdout_str().trim().to_string()
}

// The git gateway call is blocking; a current-thread runtime would starve the
// spawned axum server and deadlock the clone. Use a multi-threaded runtime.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn http_push_and_clone_round_trip_through_streamed_body() {
    let base = spawn_test_app().await;
    let token = register_user(&base, "streamowner", "streamowner@example.com", PW).await;

    // Create an auto-initialised repo so it has a commit to clone.
    let resp = reqwest::Client::new()
        .post(format!("{}/api/v1/repos", base))
        .bearer_auth(&token)
        .json(&serde_json::json!({"name": "streamed", "auto_init": true}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201, "create repo failed: {}", resp.status());

    // `git` needs the token in the URL; the server accepts Bearer/Basic.
    let url = base.replacen("http://", &format!("http://streamowner:{}@", token), 1);
    // Route is /{owner}/{repo}.git; owner is the user, repo is `streamed`.
    let remote = format!("{url}/streamowner/streamed.git");

    let worktree = tempfile::tempdir().unwrap();
    let path = worktree.path();
    git(&["clone", &remote, &path.to_string_lossy()], None);
    git(&["config", "user.name", "Stream Owner"], Some(path));
    git(&["config", "user.email", "streamowner@example.com"], Some(path));

    // A payload well above a single duplex buffer exercises chunk carry-over.
    let payload = "x".repeat(300_000);
    std::fs::write(path.join("big.txt"), &payload).unwrap();
    git(&["add", "big.txt"], Some(path));
    git(&["commit", "-m", "add big file"], Some(path));

    // The push is the streamed receive-pack request.
    git(&["push", "origin", "HEAD:main"], Some(path));

    // Read the file back through the server to prove the pack was stored.
    let clone_dir = tempfile::tempdir().unwrap();
    git(&["clone", &remote, &clone_dir.path().to_string_lossy()], None);
    let restored = std::fs::read_to_string(clone_dir.path().join("big.txt")).unwrap();
    assert_eq!(restored.len(), payload.len(), "pushed payload must round-trip");
    assert_eq!(restored, payload, "pushed payload must be byte-identical");
}