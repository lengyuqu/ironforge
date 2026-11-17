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

// ── T28: symbol definitions on the same endpoint ──────────────────────────

/// `git` helper of the original test, returning stdout.
fn git_stdout(args: &[&str], dir: Option<&Path>) -> String {
    let gateway = rg_git::cli_gateway::global_gateway()
        .as_ref()
        .expect("global git gateway");
    let output = gateway.run(args, dir).expect("run git");
    output.ensure_success().expect("git command failed");
    output.stdout_str().trim().to_string()
}

/// Create `owner/name`, push one commit with a struct, an impl and a method and
/// return the commit SHA.
async fn seed_repository(
    base: &str,
    repo_root: &Path,
    token: &str,
    owner: &str,
    name: &str,
) -> String {
    let created = reqwest::Client::new()
        .post(format!("{base}/api/v1/repos"))
        .bearer_auth(token)
        .json(&serde_json::json!({"name": name, "is_private": true}))
        .send()
        .await
        .expect("create repo request");
    assert_eq!(created.status(), 201, "repo creation should succeed");

    let bare = repo_root.join(format!("{owner}/{name}.git"));
    let worktree = tempfile::tempdir().expect("worktree");
    let path = worktree.path();
    let path_arg = path.to_string_lossy().to_string();
    git(&["init", "--initial-branch=main", &path_arg], None);
    configure_worktree(path);
    std::fs::create_dir_all(path.join("src")).unwrap();
    std::fs::write(
        path.join("src/lib.rs"),
        "pub struct Widget;\n\nimpl Widget {\n    pub fn build() {}\n}\n",
    )
    .expect("write file");
    git(&["add", "."], Some(path));
    git(&["commit", "-m", "symbols"], Some(path));
    let bare_arg = bare.to_string_lossy().to_string();
    git(&["remote", "add", "origin", &bare_arg], Some(path));
    git(&["push", "origin", "main"], Some(path));
    git_stdout(&["rev-parse", "HEAD"], Some(path))
}

async fn scalar(db: &rg_db::DatabaseConnection, sql: &str) -> i64 {
    use sea_orm::{ConnectionTrait, Statement};
    let row = db
        .query_one(Statement::from_string(
            db.get_database_backend(),
            sql.to_string(),
        ))
        .await
        .expect("query")
        .expect("row");
    row.try_get_by_index::<i64>(0).unwrap_or(-1)
}

/// Indexing is a write: a user who can read a public repository but cannot
/// write it gets 403.
#[tokio::test]
async fn index_requires_write_access() {
    let (base, _db, repo_root) = common::spawn_test_app_with_root().await;
    let (owner_token, _) =
        register_full(&base, "sym-write-owner", "sym-write-owner@example.com").await;
    let (reader_token, _) =
        register_full(&base, "sym-write-reader", "sym-write-reader@example.com").await;

    // Public, so the second user can read it but has no write access.
    let created = reqwest::Client::new()
        .post(format!("{base}/api/v1/repos"))
        .bearer_auth(&owner_token)
        .json(&serde_json::json!({"name": "guarded", "is_private": false}))
        .send()
        .await
        .expect("create repo");
    assert_eq!(created.status(), 201);
    assert!(repo_root.join("sym-write-owner/guarded.git").exists());

    let response = reqwest::Client::new()
        .post(format!(
            "{base}/api/v1/ai/repos/sym-write-owner/guarded/index"
        ))
        .bearer_auth(&reader_token)
        .json(&serde_json::json!({}))
        .send()
        .await
        .expect("index request");
    assert_eq!(
        response.status(),
        403,
        "a reader must not be allowed to rewrite the index"
    );
}

/// The endpoint resolves the requested commit before touching anything, and an
/// unknown SHA is a 404.
#[tokio::test]
async fn index_unknown_commit_is_not_found() {
    let (base, db, repo_root) = common::spawn_test_app_with_root().await;
    let (token, _) = register_full(&base, "sym-404-owner", "sym-404-owner@example.com").await;
    seed_repository(&base, &repo_root, &token, "sym-404-owner", "target").await;

    let response = reqwest::Client::new()
        .post(format!("{base}/api/v1/ai/repos/sym-404-owner/target/index"))
        .bearer_auth(&token)
        .json(&serde_json::json!({
            "commit_sha": "0000000000000000000000000000000000000000"
        }))
        .send()
        .await
        .expect("index request");
    assert_eq!(response.status(), 404, "unknown commit must be a 404");
    assert_eq!(
        scalar(&db, "SELECT COUNT(*) FROM symbol_commits").await,
        0,
        "nothing may be written when the commit does not resolve"
    );
}

/// A successful call reports both passes and the symbols are queryable in the
/// database, including the `impl` row.
#[tokio::test]
async fn index_returns_the_symbol_report() {
    let (base, db, repo_root) = common::spawn_test_app_with_root().await;
    let (token, _) = register_full(&base, "sym-rep-owner", "sym-rep-owner@example.com").await;
    let sha = seed_repository(&base, &repo_root, &token, "sym-rep-owner", "report").await;

    let response = reqwest::Client::new()
        .post(format!("{base}/api/v1/ai/repos/sym-rep-owner/report/index"))
        .bearer_auth(&token)
        .json(&serde_json::json!({}))
        .send()
        .await
        .expect("index request");
    assert_eq!(response.status(), 200);
    let body: serde_json::Value = response.json().await.expect("json");

    assert!(
        body.get("indexed_files").and_then(|v| v.as_u64()).is_some(),
        "the full-text count is still reported: {body}"
    );
    assert_eq!(body["commit_sha"], serde_json::json!(sha));
    assert!(
        body["commit_id"].as_i64().unwrap_or(0) > 0,
        "commit_id must be a symbol_commits row: {body}"
    );
    let written = body["definitions_written"].as_u64().unwrap_or(0);
    assert!(written >= 3, "struct + impl + method expected: {body}");
    assert_eq!(body["pruned_commits"], serde_json::json!(0));

    assert_eq!(
        scalar(&db, "SELECT COUNT(*) FROM symbol_definitions").await,
        written as i64
    );
    assert_eq!(
        scalar(
            &db,
            "SELECT COUNT(*) FROM symbol_definitions WHERE kind = 'impl'"
        )
        .await,
        1,
        "impl rows are stored, not filtered"
    );
    assert_eq!(
        scalar(
            &db,
            "SELECT COUNT(*) FROM symbol_definitions WHERE name = 'build' AND kind = 'method'"
        )
        .await,
        1
    );
}
