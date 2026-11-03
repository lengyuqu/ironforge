//! `/api/v1/ai/.../tree` (previously a 501 stub) and `search/code`
//! (`ref` handling, pagination).

mod common;

use common::{create_repo, register_bootstrap_admin, register_full, spawn_test_app_with_db};
use std::path::Path;

/// Run a git command through the sanctioned gateway (`rg-git` forbids
/// spawning `git` directly outside `cli_gateway.rs`).
fn git(args: &[&str], dir: Option<&Path>) {
    let gateway = rg_git::cli_gateway::global_gateway()
        .as_ref()
        .expect("global git gateway");
    let output = gateway.run(args, dir).expect("run git");
    output.ensure_success().expect("git command failed");
}

/// Push `files` as a single commit on `main` into the repository's bare dir.
fn push_files(bare: &Path, files: &[(&str, &str)]) {
    let worktree = tempfile::tempdir().expect("worktree");
    let path = worktree.path();
    git(
        &["init", "--initial-branch=main", &path.to_string_lossy()],
        None,
    );
    git(&["config", "user.email", "test@example.com"], Some(path));
    git(&["config", "user.name", "Test"], Some(path));
    for (name, body) in files {
        let full = path.join(name);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, body).unwrap();
    }
    git(&["add", "."], Some(path));
    git(&["commit", "-m", "seed"], Some(path));
    git(
        &["remote", "add", "origin", &bare.to_string_lossy()],
        Some(path),
    );
    git(&["push", "origin", "main"], Some(path));
}

struct Env {
    base: String,
    token: String,
    bare: std::path::PathBuf,
    _root: tempfile::TempDir,
}

async fn env(repo: &str) -> Env {
    let (db, dir) = common::setup_test_db().await;
    let repo_root = dir.path().join("repos");
    std::fs::create_dir_all(&repo_root).unwrap();
    let app = rg_http::create_router_for_test(common::build_test_app_state(
        db.clone(),
        repo_root.clone(),
    ));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        let _db = db;
        axum::serve(listener, app).await.unwrap();
    });
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    register_bootstrap_admin(&base).await;
    let (token, _) = register_full(&base, "tree_owner", "tree_owner@example.com").await;
    create_repo(&base, &token, repo).await;
    let bare = repo_root.join(format!("tree_owner/{repo}.git"));
    Env {
        base,
        token,
        bare,
        _root: dir,
    }
}

async fn get(env: &Env, url: &str) -> (u16, serde_json::Value) {
    let resp = reqwest::Client::new()
        .get(format!("{}{url}", env.base))
        .bearer_auth(&env.token)
        .send()
        .await
        .unwrap();
    let status = resp.status().as_u16();
    (status, resp.json().await.unwrap_or(serde_json::Value::Null))
}

fn paths(body: &serde_json::Value) -> Vec<String> {
    body["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["path"].as_str().unwrap().to_string())
        .collect()
}

const SEED: &[(&str, &str)] = &[
    ("README.md", "# readme\n"),
    ("src/main.rs", "fn main() {}\n"),
    ("src/lib/util.rs", "pub fn util() {}\n"),
    ("docs/guide.md", "guide\n"),
];

#[tokio::test]
async fn tree_lists_one_level_by_default_with_types_and_sizes() {
    let env = env("tree-basic").await;
    push_files(&env.bare, SEED);

    let (status, body) = get(&env, "/api/v1/ai/repos/tree_owner/tree-basic/tree").await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["ref"], "main");
    assert_eq!(body["commit"].as_str().unwrap().len(), 40);
    assert_eq!(body["truncated"], false);
    assert_eq!(paths(&body), vec!["README.md", "docs", "src"]);

    let readme = &body["entries"][0];
    assert_eq!(readme["type"], "blob");
    assert_eq!(readme["size"], 9);
    assert_eq!(body["entries"][1]["type"], "tree");
    assert!(body["entries"][1]["size"].is_null());
}

#[tokio::test]
async fn tree_depth_and_path_descend_into_directories() {
    let env = env("tree-depth").await;
    push_files(&env.bare, SEED);

    let (_, deep) = get(&env, "/api/v1/ai/repos/tree_owner/tree-depth/tree?depth=3").await;
    assert_eq!(
        paths(&deep),
        vec![
            "README.md",
            "docs",
            "docs/guide.md",
            "src",
            "src/lib",
            "src/lib/util.rs",
            "src/main.rs",
        ]
    );

    let (_, sub) = get(&env, "/api/v1/ai/repos/tree_owner/tree-depth/tree?path=src").await;
    assert_eq!(paths(&sub), vec!["src/lib", "src/main.rs"]);
    assert_eq!(sub["path"], "src");

    let (status, _) = get(
        &env,
        "/api/v1/ai/repos/tree_owner/tree-depth/tree?path=src/main.rs",
    )
    .await;
    assert_eq!(status, 404, "a file is not a listable directory");
    let (status, _) = get(
        &env,
        "/api/v1/ai/repos/tree_owner/tree-depth/tree?path=nope",
    )
    .await;
    assert_eq!(status, 404);
}

#[tokio::test]
async fn tree_pages_reassemble_to_the_full_listing() {
    let env = env("tree-page").await;
    push_files(&env.bare, SEED);

    let (_, full) = get(&env, "/api/v1/ai/repos/tree_owner/tree-page/tree?depth=3").await;
    let expected = paths(&full);

    let mut collected = Vec::new();
    let mut cursor = String::new();
    for _ in 0..16 {
        let (status, page) = get(
            &env,
            &format!("/api/v1/ai/repos/tree_owner/tree-page/tree?depth=3&limit=3&cursor={cursor}"),
        )
        .await;
        assert_eq!(status, 200, "{page}");
        assert!(page["entries"].as_array().unwrap().len() <= 3);
        collected.extend(paths(&page));
        match page["next_cursor"].as_str() {
            Some(next) => {
                assert_eq!(page["truncated"], true);
                cursor = next.to_string();
            }
            None => {
                assert_eq!(page["truncated"], false);
                break;
            }
        }
    }
    assert_eq!(collected, expected);
}

#[tokio::test]
async fn tree_rejects_a_bad_cursor_and_an_unknown_ref() {
    let env = env("tree-errors").await;
    push_files(&env.bare, SEED);

    let (status, _) = get(
        &env,
        "/api/v1/ai/repos/tree_owner/tree-errors/tree?cursor=abc",
    )
    .await;
    assert_eq!(status, 400);
    let (status, _) = get(
        &env,
        "/api/v1/ai/repos/tree_owner/tree-errors/tree?ref=nope",
    )
    .await;
    assert_eq!(status, 404);
}

#[tokio::test]
async fn tree_of_an_empty_repository_is_empty_not_an_error() {
    let env = env("tree-empty").await;
    let (status, body) = get(&env, "/api/v1/ai/repos/tree_owner/tree-empty/tree").await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(paths(&body), Vec::<String>::new());
    assert!(body["commit"].is_null());
}

#[tokio::test]
async fn tree_requires_read_access_to_a_private_repository() {
    let (base, _db) = spawn_test_app_with_db().await;
    register_bootstrap_admin(&base).await;
    let (owner, _) = register_full(&base, "tree_owner", "tree_owner@example.com").await;
    let (other, _) = register_full(&base, "tree_other", "tree_other@example.com").await;
    let client = reqwest::Client::new();
    let created = client
        .post(format!("{base}/api/v1/repos"))
        .bearer_auth(&owner)
        .json(&serde_json::json!({"name": "tree-private", "is_private": true}))
        .send()
        .await
        .unwrap();
    assert_eq!(created.status(), 201);

    let url = format!("{base}/api/v1/ai/repos/tree_owner/tree-private/tree");
    assert_eq!(client.get(&url).send().await.unwrap().status(), 401);
    assert_eq!(
        client
            .get(&url)
            .bearer_auth(&other)
            .send()
            .await
            .unwrap()
            .status(),
        403
    );
    assert_eq!(
        client
            .get(&url)
            .bearer_auth(&owner)
            .send()
            .await
            .unwrap()
            .status(),
        200
    );
}

#[tokio::test]
async fn code_search_rejects_a_ref_other_than_the_indexed_branch() {
    let env = env("search-ref").await;
    push_files(&env.bare, SEED);
    let indexed = reqwest::Client::new()
        .post(format!(
            "{}/api/v1/ai/repos/tree_owner/search-ref/index",
            env.base
        ))
        .bearer_auth(&env.token)
        .json(&serde_json::json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(indexed.status(), 200);

    let (status, body) = get(
        &env,
        "/api/v1/ai/repos/tree_owner/search-ref/search/code?q=util&ref=feature",
    )
    .await;
    assert_eq!(status, 400, "{body}");
    assert!(body["error"]["message"]
        .as_str()
        .unwrap()
        .contains("default branch"));

    for accepted in ["main", "refs/heads/main"] {
        let (status, body) = get(
            &env,
            &format!("/api/v1/ai/repos/tree_owner/search-ref/search/code?q=util&ref={accepted}"),
        )
        .await;
        assert_eq!(status, 200, "ref={accepted}: {body}");
    }
}

#[tokio::test]
async fn code_search_paginates_and_reports_the_total() {
    let env = env("search-page").await;
    let files: Vec<(String, String)> = (0..5)
        .map(|n| {
            (
                format!("src/m{n}.rs"),
                format!("pub fn needle_{n}() {{}}\n"),
            )
        })
        .collect();
    let refs: Vec<(&str, &str)> = files
        .iter()
        .map(|(a, b)| (a.as_str(), b.as_str()))
        .collect();
    push_files(&env.bare, &refs);
    let indexed = reqwest::Client::new()
        .post(format!(
            "{}/api/v1/ai/repos/tree_owner/search-page/index",
            env.base
        ))
        .bearer_auth(&env.token)
        .json(&serde_json::json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(indexed.status(), 200);

    let client = reqwest::Client::new();
    let fetch = |offset: u32| {
        let client = client.clone();
        let url = format!(
            "{}/api/v1/ai/repos/tree_owner/search-page/search/code?q=pub&limit=2&offset={offset}",
            env.base
        );
        let token = env.token.clone();
        async move {
            let resp = client.get(url).bearer_auth(token).send().await.unwrap();
            assert_eq!(resp.status(), 200);
            let total: i64 = resp.headers()["x-total-count"]
                .to_str()
                .unwrap()
                .parse()
                .unwrap();
            let body: Vec<serde_json::Value> = resp.json().await.unwrap();
            (total, body)
        }
    };

    let (total, first) = fetch(0).await;
    assert_eq!(total, 5);
    assert_eq!(first.len(), 2);
    let (_, second) = fetch(2).await;
    let (_, third) = fetch(4).await;
    assert_eq!(second.len(), 2);
    assert_eq!(third.len(), 1);

    let mut seen: Vec<String> = first
        .iter()
        .chain(&second)
        .chain(&third)
        .map(|r| r["file_path"].as_str().unwrap().to_string())
        .collect();
    seen.sort();
    seen.dedup();
    assert_eq!(seen.len(), 5, "pages must not overlap or skip: {seen:?}");
}
