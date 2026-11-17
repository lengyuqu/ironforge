//! `GET /api/v1/ai/repos/{owner}/{name}/symbols` (#63 step 3, T29).
//!
//! Every case here is about honesty of the answer, not just plumbing:
//! `confidence` must say whether the row is the symbol that was asked for,
//! `impl` rows must never surface, an unindexed commit must fail instead of
//! being answered from a different commit, and `partial`/`limitations` must
//! tell the caller what static parsing cannot see.

mod common;

use common::{register_full, spawn_test_app_on_env_db_with_root};
use serde_json::{json, Value};
use std::path::Path;

fn git(args: &[&str], dir: Option<&Path>) {
    let gateway = rg_git::cli_gateway::global_gateway()
        .as_ref()
        .expect("global git gateway");
    let output = gateway.run(args, dir).expect("run git");
    output.ensure_success().expect("git command failed");
}

fn git_stdout(args: &[&str], dir: Option<&Path>) -> String {
    let gateway = rg_git::cli_gateway::global_gateway()
        .as_ref()
        .expect("global git gateway");
    let output = gateway.run(args, dir).expect("run git");
    output.ensure_success().expect("git command failed");
    output.stdout_str().trim().to_string()
}

fn configure_worktree(path: &Path) {
    git(&["config", "user.email", "test@example.com"], Some(path));
    git(&["config", "user.name", "Test"], Some(path));
}

/// The seeded source. `index_commit` / `index_repository` exercise exact and
/// prefix search; the two `impl` blocks give rows whose `name` is a first line.
const LIB_RS: &str = "pub struct Widget;\n\
\n\
impl Widget {\n    \
    pub fn build() -> Self { Widget }\n\
}\n\
\n\
impl std::fmt::Display for Widget {\n    \
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { Ok(()) }\n\
}\n";

const INDEXER_RS: &str = "pub async fn index_commit() {}\n\npub fn index_repository() {}\n";

/// Create `owner/name`, push one commit, and return its SHA.
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
        .json(&json!({"name": name, "is_private": true}))
        .send()
        .await
        .expect("create repo");
    assert_eq!(created.status(), 201, "repo creation should succeed");

    let bare = repo_root.join(format!("{owner}/{name}.git"));
    let worktree = tempfile::tempdir().expect("worktree");
    let path = worktree.path();
    let path_arg = path.to_string_lossy().to_string();
    git(&["init", "--initial-branch=main", &path_arg], None);
    configure_worktree(path);
    std::fs::create_dir_all(path.join("src")).unwrap();
    std::fs::write(path.join("src/lib.rs"), LIB_RS).expect("write lib.rs");
    std::fs::write(path.join("src/indexer.rs"), INDEXER_RS).expect("write indexer.rs");
    git(&["add", "."], Some(path));
    git(&["commit", "-m", "symbols"], Some(path));
    let bare_arg = bare.to_string_lossy().to_string();
    git(&["remote", "add", "origin", &bare_arg], Some(path));
    git(&["push", "origin", "main"], Some(path));
    git_stdout(&["rev-parse", "HEAD"], Some(path))
}

/// Index `commit_sha` (or the default branch when `None`) and return the
/// report body.
async fn index(
    base: &str,
    token: &str,
    owner: &str,
    name: &str,
    commit_sha: Option<&str>,
) -> Value {
    let body = match commit_sha {
        Some(sha) => json!({ "commit_sha": sha }),
        None => json!({}),
    };
    let response = reqwest::Client::new()
        .post(format!("{base}/api/v1/ai/repos/{owner}/{name}/index"))
        .bearer_auth(token)
        .json(&body)
        .send()
        .await
        .expect("index request");
    assert_eq!(response.status(), 200, "indexing must succeed");
    response.json().await.expect("index json")
}

async fn symbols(
    base: &str,
    token: &str,
    owner: &str,
    name: &str,
    query: &[(&str, &str)],
) -> (u16, Value) {
    let response = reqwest::Client::new()
        .get(format!(
            "{base}/api/v1/ai/repos/{owner}/{name}/symbols"
        ))
        .bearer_auth(token)
        .query(query)
        .send()
        .await
        .expect("symbols request");
    let status = response.status().as_u16();
    let body = response.json().await.expect("symbols json");
    (status, body)
}

fn names(body: &Value) -> Vec<String> {
    body["items"]
        .as_array()
        .expect("items array")
        .iter()
        .map(|item| item["name"].as_str().unwrap_or("").to_string())
        .collect()
}

fn assert_coded_error(body: &Value, code: &str) {
    assert_eq!(body["error"]["code"], code, "{body}");
}

#[tokio::test]
async fn symbols_answers_name_path_limit_and_honesty_contract() {
    let (base, _db, repo_root) = spawn_test_app_on_env_db_with_root().await;
    let (token, _) = register_full(&base, "symbols-owner-a", "symbols-owner-a@example.com").await;
    let sha = seed_repository(&base, &repo_root, &token, "symbols-owner-a", "queryable").await;
    let report = index(&base, &token, "symbols-owner-a", "queryable", None).await;
    assert_eq!(report["commit_sha"], sha.as_str());

    // 1. Exact name: one row, marked exact, with the right file.
    let (status, body) = symbols(&base, &token, "symbols-owner-a", "queryable", &[("name", "index_commit")]).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["commit_sha"], sha.as_str());
    assert_eq!(names(&body), ["index_commit"]);
    assert_eq!(body["items"][0]["confidence"], "exact");
    assert_eq!(body["items"][0]["path"], "src/indexer.rs");
    assert_eq!(body["items"][0]["kind"], "function");
    assert!(body["items"][0]["start_line"].as_i64().unwrap() >= 1);
    assert!(body["items"][0]["signature"]
        .as_str()
        .unwrap()
        .contains("index_commit"));

    // 2. Prefix search: several rows, every one a `name_match`.
    let (status, body) = symbols(&base, &token, "symbols-owner-a", "queryable", &[("name", "index_")]).await;
    assert_eq!(status, 200, "{body}");
    assert!(names(&body).len() >= 2, "{body}");
    for item in body["items"].as_array().unwrap() {
        assert_eq!(item["confidence"], "name_match", "{item}");
        assert!(item["name"].as_str().unwrap().starts_with("index_"));
    }

    // 3. By path: that file's declarations only, and no `impl` row.
    let (status, body) = symbols(&base, &token, "symbols-owner-a", "queryable", &[("path", "src/lib.rs")]).await;
    assert_eq!(status, 200, "{body}");
    let listed = names(&body);
    assert!(listed.contains(&"Widget".to_string()), "{body}");
    assert!(listed.contains(&"build".to_string()), "{body}");
    assert!(
        body["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["kind"] != "impl"),
        "impl rows must never be listed: {body}"
    );

    // 5. `limit` is a SQL LIMIT, and above the maximum is a 400, not a clamp.
    let (status, body) = symbols(
        &base,
        &token,
        "symbols-owner-a",
        "queryable",
        &[("name", "index_"), ("limit", "5")],
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert!(body["items"].as_array().unwrap().len() <= 5, "{body}");

    let (status, body) = symbols(
        &base,
        &token,
        "symbols-owner-a",
        "queryable",
        &[("path", "src/lib.rs"), ("limit", "1")],
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["items"].as_array().unwrap().len(), 1, "{body}");

    let (status, body) = symbols(
        &base,
        &token,
        "symbols-owner-a",
        "queryable",
        &[("name", "index_"), ("limit", "500")],
    )
    .await;
    assert_eq!(status, 400, "{body}");
    assert_coded_error(&body, "INVALID_LIMIT");

    // 6. Neither name nor path.
    let (status, body) = symbols(&base, &token, "symbols-owner-a", "queryable", &[]).await;
    assert_eq!(status, 400, "{body}");

    // 7. Completeness: every file here parses cleanly, so the indexer recorded
    //    no failure and `partial` is false. The static language limits are
    //    still stated — they are the extractor's boundary, not this commit's
    //    defect — and no per-commit failure is invented.
    let (status, body) = symbols(&base, &token, "symbols-owner-a", "queryable", &[("name", "index_commit")]).await;
    assert_eq!(status, 200);
    assert_eq!(body["partial"], false, "{body}");
    let limitations = body["limitations"].as_array().expect("limitations array");
    assert!(!limitations.is_empty(), "{body}");
    let joined = limitations
        .iter()
        .filter_map(|line| line.as_str())
        .collect::<Vec<_>>()
        .join(" | ");
    assert!(joined.contains("svelte"), "{joined}");
    assert!(joined.contains("Rust macro"), "{joined}");
    assert!(joined.contains("Go interface"), "{joined}");
    assert!(!joined.contains("failed to parse"), "{joined}");
    assert!(!joined.contains("unknown"), "{joined}");

    // 9. A name that only exists as an `impl` first line returns nothing —
    //    with either an exact or a prefix query.
    let (status, body) = symbols(
        &base,
        &token,
        "symbols-owner-a",
        "queryable",
        &[("name", "std::fmt::Display for Widget")],
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert!(body["items"].as_array().unwrap().is_empty(), "{body}");

    let (status, body) = symbols(&base, &token, "symbols-owner-a", "queryable", &[("name", "std::fmt::")]).await;
    assert_eq!(status, 200, "{body}");
    assert!(body["items"].as_array().unwrap().is_empty(), "{body}");
}

/// Create `owner/name` and push **two** commits, returning both SHAs in order.
async fn seed_two_commits(
    base: &str,
    repo_root: &Path,
    token: &str,
    owner: &str,
    name: &str,
) -> (String, String) {
    let created = reqwest::Client::new()
        .post(format!("{base}/api/v1/repos"))
        .bearer_auth(token)
        .json(&json!({"name": name, "is_private": true}))
        .send()
        .await
        .expect("create repo");
    assert_eq!(created.status(), 201, "repo creation should succeed");

    let bare = repo_root.join(format!("{owner}/{name}.git"));
    let worktree = tempfile::tempdir().expect("worktree");
    let path = worktree.path();
    let path_arg = path.to_string_lossy().to_string();
    git(&["init", "--initial-branch=main", &path_arg], None);
    configure_worktree(path);
    std::fs::create_dir_all(path.join("src")).unwrap();
    std::fs::write(path.join("src/lib.rs"), LIB_RS).expect("write lib.rs");
    std::fs::write(path.join("src/indexer.rs"), INDEXER_RS).expect("write indexer.rs");
    git(&["add", "."], Some(path));
    git(&["commit", "-m", "first"], Some(path));
    let bare_arg = bare.to_string_lossy().to_string();
    git(&["remote", "add", "origin", &bare_arg], Some(path));
    git(&["push", "origin", "main"], Some(path));
    let first = git_stdout(&["rev-parse", "HEAD"], Some(path));

    std::fs::write(path.join("src/extra.rs"), "pub fn extra() {}\n").expect("write extra.rs");
    git(&["add", "."], Some(path));
    git(&["commit", "-m", "second"], Some(path));
    git(&["push", "origin", "main"], Some(path));
    let second = git_stdout(&["rev-parse", "HEAD"], Some(path));

    assert_ne!(first, second);
    (first, second)
}

#[tokio::test]
async fn an_unindexed_commit_is_404_and_never_answered_from_head() {
    let (base, _db, repo_root) = spawn_test_app_on_env_db_with_root().await;
    let (token, _) = register_full(&base, "symbols-owner-b", "symbols-owner-b@example.com").await;
    let (first, second) = seed_two_commits(&base, &repo_root, &token, "symbols-owner-b", "two-commits").await;

    // Index the *first* commit only.
    index(&base, &token, "symbols-owner-b", "two-commits", Some(&first)).await;

    // The default branch now resolves to the second, unindexed commit.
    let (status, body) = symbols(&base, &token, "symbols-owner-b", "two-commits", &[("name", "index_commit")]).await;
    assert_eq!(status, 404, "{body}");
    assert_coded_error(&body, "SYMBOLS_NOT_INDEXED");
    let message = body["error"]["message"].as_str().unwrap_or("");
    assert!(
        message.contains(&second),
        "the failure must name the commit that was asked for: {body}"
    );
    // No fallback trace: the indexed commit's rows must not leak into the answer.
    assert!(
        !body.to_string().contains("index_commit"),
        "an unindexed commit must not be answered from another one: {body}"
    );
    assert!(
        !body.to_string().contains(&first),
        "the response must not mention the other indexed commit: {body}"
    );
    let hint = body["error"]["hint"].as_str().unwrap_or("");
    assert!(
        hint.contains("/index"),
        "the failure must tell the caller to index the commit: {body}"
    );
    assert_eq!(body["error"]["retryable"], true, "{body}");
}

/// `partial` must distinguish a clean index from a defective one: a commit
/// containing a file the parser rejected is `partial: true` **and** says how
/// many files were affected. Hedging on every commit would make the flag
/// useless; being silent here would hide real gaps.
#[tokio::test]
async fn a_commit_with_a_parse_failure_is_partial_and_names_the_count() {
    let (base, _db, repo_root) = spawn_test_app_on_env_db_with_root().await;
    let (token, _) = register_full(&base, "symbols-owner-c", "symbols-owner-c@example.com").await;
    let created = reqwest::Client::new()
        .post(format!("{base}/api/v1/repos"))
        .bearer_auth(&token)
        .json(&json!({"name": "broken", "is_private": true}))
        .send()
        .await
        .expect("create repo");
    assert_eq!(created.status(), 201);

    let bare = repo_root.join("symbols-owner-c/broken.git");
    let worktree = tempfile::tempdir().expect("worktree");
    let path = worktree.path();
    let path_arg = path.to_string_lossy().to_string();
    git(&["init", "--initial-branch=main", &path_arg], None);
    configure_worktree(path);
    std::fs::create_dir_all(path.join("src")).unwrap();
    std::fs::write(path.join("src/lib.rs"), LIB_RS).expect("write lib.rs");
    // Unbalanced paren: tree-sitter reports an ERROR node for this file.
    std::fs::write(
        path.join("src/broken.ts"),
        "export function broken( {\n    return 1;\n}\n",
    )
    .expect("write broken.ts");
    git(&["add", "."], Some(path));
    git(&["commit", "-m", "symbols"], Some(path));
    let bare_arg = bare.to_string_lossy().to_string();
    git(&["remote", "add", "origin", &bare_arg], Some(path));
    git(&["push", "origin", "main"], Some(path));
    let sha = git_stdout(&["rev-parse", "HEAD"], Some(path));

    let report = index(&base, &token, "symbols-owner-c", "broken", None).await;
    assert_eq!(report["commit_sha"], sha.as_str());

    let (status, body) = symbols(&base, &token, "symbols-owner-c", "broken", &[("name", "Widget")]).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["partial"], true, "{body}");
    let joined = body["limitations"]
        .as_array()
        .expect("limitations array")
        .iter()
        .filter_map(|line| line.as_str())
        .collect::<Vec<_>>()
        .join(" | ");
    assert!(
        joined.contains("1 files failed to parse and are only partially indexed"),
        "{joined}"
    );
    assert!(
        joined.contains("svelte"),
        "static limits stay listed: {joined}"
    );
    // The clean file is still answered.
    assert!(!names(&body).is_empty(), "{body}");
}
