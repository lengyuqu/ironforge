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
    git(
        &["config", "user.email", "streamowner@example.com"],
        Some(path),
    );

    // A payload well above a single duplex buffer exercises chunk carry-over.
    let payload = "x".repeat(300_000);
    std::fs::write(path.join("big.txt"), &payload).unwrap();
    git(&["add", "big.txt"], Some(path));
    git(&["commit", "-m", "add big file"], Some(path));

    // The push is the streamed receive-pack request.
    git(&["push", "origin", "HEAD:main"], Some(path));

    // Read the file back through the server to prove the pack was stored.
    let clone_dir = tempfile::tempdir().unwrap();
    git(
        &["clone", &remote, &clone_dir.path().to_string_lossy()],
        None,
    );
    let restored = std::fs::read_to_string(clone_dir.path().join("big.txt")).unwrap();
    assert_eq!(
        restored.len(),
        payload.len(),
        "pushed payload must round-trip"
    );
    assert_eq!(restored, payload, "pushed payload must be byte-identical");
}

/// T38: the V1 upload-pack path streams its response body too.
///
/// A modern client negotiates protocol v2, so the V1 branch only runs for old
/// clients (`-c protocol.version=1`) — which is exactly why it needs its own
/// end-to-end test: a streaming regression there would not show up in any v2
/// test. The payload crosses several 64 KiB duplex buffers on the way out.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn http_v1_clone_streams_the_pack() {
    let base = spawn_test_app().await;
    let token = register_user(&base, "v1owner", "v1owner@example.com", PW).await;

    let resp = reqwest::Client::new()
        .post(format!("{}/api/v1/repos", base))
        .bearer_auth(&token)
        .json(&serde_json::json!({"name": "v1streamed", "auto_init": true}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201, "create repo failed: {}", resp.status());

    let url = base.replacen("http://", &format!("http://v1owner:{}@", token), 1);
    let remote = format!("{url}/v1owner/v1streamed.git");

    let worktree = tempfile::tempdir().unwrap();
    let path = worktree.path();
    git(&["clone", &remote, &path.to_string_lossy()], None);
    git(&["config", "user.name", "V1 Owner"], Some(path));
    git(&["config", "user.email", "v1owner@example.com"], Some(path));

    let payload = "y".repeat(400_000);
    std::fs::write(path.join("payload.txt"), &payload).unwrap();
    git(&["add", "payload.txt"], Some(path));
    git(&["commit", "-m", "add payload"], Some(path));
    git(&["push", "origin", "HEAD:main"], Some(path));

    // Force the V1 negotiation (`multi_ack` / NAK then pack) for this clone.
    let v1_dir = tempfile::tempdir().unwrap();
    git(
        &[
            "-c",
            "protocol.version=1",
            "clone",
            &remote,
            &v1_dir.path().to_string_lossy(),
        ],
        None,
    );
    let restored = std::fs::read_to_string(v1_dir.path().join("payload.txt")).unwrap();
    assert_eq!(
        restored, payload,
        "V1 clone must deliver the payload verbatim"
    );
}
