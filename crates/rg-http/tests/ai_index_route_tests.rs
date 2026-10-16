//! Regression: `POST /api/v1/ai/repos/{owner}/{name}/index` was never reachable.
//!
//! The handler was fully implemented, but its route was commented out because
//! `CodeIndexer::index_repository` held `gix::Repository` (which is `!Send`)
//! across an `.await`, so the future was not `Send` and the handler could not
//! satisfy axum's `Handler` bound. The fix moves all gix work into
//! `spawn_blocking`. This test proves the route is mounted and returns 200,
//! rather than merely that the crate compiles.

mod common;

use common::{build_test_app_state, register_full, setup_test_db};
use std::path::Path;

/// Run a git command through the sanctioned gateway.
///
/// The `rg-git` regression guard forbids spawning `git` directly outside
/// `cli_gateway.rs`, so tests must go through the gateway rather than
/// `std::process::Command`.
fn git(args: &[&str], dir: Option<&Path>) {
    let gateway = rg_git::cli_gateway::global_gateway()
        .as_ref()
        .expect("global git gateway");
    let output = gateway.run(args, dir).expect("run git");
    output.ensure_success().expect("git command failed");
}

fn configure_worktree(path: &Path) {
    git(&["config", "user.email", "test@example.com"], Some(path));
    git(&["config", "user.name", "Test"], Some(path));
}

#[tokio::test]
async fn ai_index_route_is_mounted_and_indexes_a_repository() {
    let (db, app_dir) = setup_test_db().await;
    let repo_root = app_dir.path().join("repos");
    std::fs::create_dir_all(&repo_root).unwrap();
    let app = rg_http::create_router_for_test(build_test_app_state(db.clone(), repo_root.clone()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let _app_dir = app_dir;
        axum::serve(listener, app).await.unwrap();
    });
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let (token, _owner_id) =
        register_full(&base, "ai-index-owner", "ai-index-owner@example.com").await;

    let created = reqwest::Client::new()
        .post(format!("{base}/api/v1/repos"))
        .bearer_auth(&token)
        .json(&serde_json::json!({"name": "indexable", "is_private": false}))
        .send()
        .await
        .expect("create repo request");
    assert_eq!(created.status(), 201, "repo creation should succeed");

    // Push a real commit so there is something to index.
    let bare = repo_root.join("ai-index-owner/indexable.git");
    assert!(bare.exists(), "the bare repo must exist after creation");

    let worktree = tempfile::tempdir().expect("worktree");
    let path = worktree.path();
    let path_arg = path.to_string_lossy().to_string();
    git(&["init", "--initial-branch=main", &path_arg], None);
    configure_worktree(path);
    std::fs::write(path.join("hello.rs"), "fn main() {}\n").expect("write file");
    git(&["add", "."], Some(path));
    git(&["commit", "-m", "add hello"], Some(path));
    let bare_arg = bare.to_string_lossy().to_string();
    git(&["remote", "add", "origin", &bare_arg], Some(path));
    git(&["push", "origin", "main"], Some(path));

    let response = reqwest::Client::new()
        .post(format!(
            "{base}/api/v1/ai/repos/ai-index-owner/indexable/index"
        ))
        .bearer_auth(&token)
        .json(&serde_json::json!({}))
        .send()
        .await
        .expect("index request");

    assert_eq!(
        response.status(),
        200,
        "POST /ai/repos/:owner/:name/index must be mounted and succeed"
    );
    let body: serde_json::Value = response.json().await.expect("index response json");
    assert!(
        body.get("indexed_files").and_then(|v| v.as_u64()).is_some(),
        "the response must report indexed_files, got {body}"
    );

    // And the index must actually contain the file we pushed.
    use sea_orm::{ConnectionTrait, Statement};
    let backend = db.get_database_backend();
    let row = db
        .query_one(Statement::from_string(
            backend,
            "SELECT COUNT(*) AS cnt FROM code_fts WHERE file_path LIKE '%hello.rs'".to_string(),
        ))
        .await
        .expect("query code_fts")
        .expect("row");
    let count: i64 = row.try_get_by_index::<i64>(0).unwrap_or(0);
    assert!(count >= 1, "the pushed file must be present in code_fts");

    server.abort();
}
