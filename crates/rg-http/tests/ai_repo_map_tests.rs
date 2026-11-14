//! `/api/v1/ai/repos/{owner}/{name}/map`: the budgeted structural view.
//!
//! The trimming algorithm itself is unit-tested in
//! `rg-core/src/search/map/tests.rs`; these tests pin the HTTP contract around
//! it: ref/path resolution, authorization, error codes and the invariant that
//! the advertised estimate fits the budget.

mod common;

use common::{create_repo, register_bootstrap_admin, register_full, spawn_test_app_with_db};
use std::path::Path;

fn git(args: &[&str], dir: Option<&Path>) {
    let gateway = rg_git::cli_gateway::global_gateway()
        .as_ref()
        .expect("global git gateway");
    let output = gateway.run(args, dir).expect("run git");
    output.ensure_success().expect("git command failed");
}

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
    let (token, _) = register_full(&base, "map_owner", "map_owner@example.com").await;
    create_repo(&base, &token, repo).await;
    let bare = repo_root.join(format!("map_owner/{repo}.git"));
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

fn file_paths(body: &serde_json::Value) -> Vec<String> {
    body["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["path"].as_str().unwrap().to_string())
        .collect()
}

fn skipped_paths(body: &serde_json::Value) -> Vec<String> {
    body["skipped"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["path"].as_str().unwrap().to_string())
        .collect()
}

const SEED: &[(&str, &str)] = &[
    ("README.md", "# readme\n"),
    (
        "src/lib.rs",
        "pub struct Widget { pub id: u32 }\n\npub fn build() -> Widget { Widget { id: 1 } }\n\nfn helper() {}\n",
    ),
    (
        "src/util/strings.rs",
        "pub fn trim(input: &str) -> &str { input }\n",
    ),
    ("web/app.ts", "export class Store {}\nexport function make(): Store { return new Store(); }\n"),
    ("web/App.svelte", "<script lang=\"ts\">\nexport const title = 'x';\n</script>\n<h1>{title}</h1>\n"),
    ("assets/logo.bin", "\u{0}\u{1}\u{2}"),
];

#[tokio::test]
async fn map_lists_source_files_and_reports_the_rest_as_unsupported() {
    let env = env("map-basic").await;
    push_files(&env.bare, SEED);

    let (status, body) = get(&env, "/api/v1/ai/repos/map_owner/map-basic/map").await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["ref"], "main");
    assert_eq!(body["commit_sha"].as_str().unwrap().len(), 40);
    assert_eq!(body["budget"], 4000);
    assert_eq!(body["truncated"], false);
    assert!(body["estimated_tokens"].as_u64().unwrap() <= 4000);

    let files = file_paths(&body);
    assert!(files.contains(&"src/lib.rs".to_string()), "{files:?}");
    assert!(files.contains(&"web/App.svelte".to_string()), "{files:?}");
    let skipped = skipped_paths(&body);
    assert!(skipped.contains(&"README.md".to_string()), "{skipped:?}");
    assert!(
        skipped.contains(&"assets/logo.bin".to_string()),
        "{skipped:?}"
    );
    let readme = body["skipped"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["path"] == "README.md")
        .unwrap();
    assert_eq!(readme["reason"], "unsupported language");

    let dirs: Vec<&str> = body["directories"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["path"].as_str().unwrap())
        .collect();
    assert!(dirs.contains(&"src"), "{dirs:?}");
    assert!(dirs.contains(&"src/util"), "{dirs:?}");
    let web = body["directories"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["path"] == "web")
        .unwrap();
    assert_eq!(web["files"], 2);
    let languages: Vec<&str> = web["languages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l.as_str().unwrap())
        .collect();
    assert_eq!(languages, vec!["svelte", "typescript"]);
}

#[tokio::test]
async fn map_ranks_structs_above_helpers_and_keeps_signatures_whole() {
    let env = env("map-rank").await;
    push_files(&env.bare, SEED);

    let (_, body) = get(&env, "/api/v1/ai/repos/map_owner/map-rank/map").await;
    let files = body["files"].as_array().unwrap();
    let lib = files.iter().find(|f| f["path"] == "src/lib.rs").unwrap();
    let kinds: Vec<&str> = lib["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["kind"].as_str().unwrap())
        .collect();
    assert_eq!(kinds, vec!["struct", "function", "function"]);
    let widget = &lib["items"][0];
    assert_eq!(widget["name"], "Widget");
    assert_eq!(widget["start_line"], 1);
    assert_eq!(widget["exported"], true);
    assert!(widget["signature"]
        .as_str()
        .unwrap()
        .starts_with("pub struct Widget"));
    assert!(lib["items_omitted"].is_null(), "nothing was trimmed");
}

#[tokio::test]
async fn map_flags_svelte_as_a_degraded_outline() {
    let env = env("map-svelte").await;
    push_files(&env.bare, SEED);

    let (_, body) = get(&env, "/api/v1/ai/repos/map_owner/map-svelte/map").await;
    let svelte = body["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["path"] == "web/App.svelte")
        .unwrap();
    let notice = svelte["notice"].as_str().unwrap_or_default();
    assert!(notice.contains("svelte"), "{notice}");
    assert!(notice.contains("markup"), "{notice}");
}

#[tokio::test]
async fn map_trims_to_a_small_budget_without_exceeding_it() {
    let env = env("map-budget").await;
    push_files(&env.bare, SEED);

    let (status, body) = get(&env, "/api/v1/ai/repos/map_owner/map-budget/map?budget=64").await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["budget"], 64);
    assert_eq!(body["truncated"], true);
    assert!(body["estimated_tokens"].as_u64().unwrap() <= 64);

    let (_, full) = get(
        &env,
        "/api/v1/ai/repos/map_owner/map-budget/map?budget=8000",
    )
    .await;
    assert_eq!(full["truncated"], false);
    assert!(full["estimated_tokens"].as_u64().unwrap() > 64);
}

#[tokio::test]
async fn map_rejects_an_invalid_budget() {
    let env = env("map-bad-budget").await;
    push_files(&env.bare, SEED);

    for raw in ["0", "63", "32001", "abc", "-5", "1.5"] {
        let (status, body) = get(
            &env,
            &format!("/api/v1/ai/repos/map_owner/map-bad-budget/map?budget={raw}"),
        )
        .await;
        assert_eq!(status, 400, "budget={raw}: {body}");
        assert_eq!(body["error"]["code"], "INVALID_BUDGET", "budget={raw}");
    }
}

#[tokio::test]
async fn map_reports_a_missing_ref_with_its_own_code() {
    let env = env("map-ref").await;
    push_files(&env.bare, SEED);

    let (status, body) = get(&env, "/api/v1/ai/repos/map_owner/map-ref/map?ref=nope").await;
    assert_eq!(status, 404);
    assert_eq!(body["error"]["code"], "REF_NOT_FOUND");

    for accepted in ["main", "refs/heads/main"] {
        let (status, body) = get(
            &env,
            &format!("/api/v1/ai/repos/map_owner/map-ref/map?ref={accepted}"),
        )
        .await;
        assert_eq!(status, 200, "ref={accepted}: {body}");
    }
}

#[tokio::test]
async fn map_scopes_to_a_subdirectory() {
    let env = env("map-subdir").await;
    push_files(&env.bare, SEED);

    let (status, body) = get(&env, "/api/v1/ai/repos/map_owner/map-subdir/map?path=src").await;
    assert_eq!(status, 200, "{body}");
    let files = file_paths(&body);
    assert_eq!(files, vec!["src/lib.rs", "src/util/strings.rs"]);
    let dirs: Vec<&str> = body["directories"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["path"].as_str().unwrap())
        .collect();
    assert_eq!(dirs, vec!["src", "src/util"]);

    let (status, _) = get(&env, "/api/v1/ai/repos/map_owner/map-subdir/map?path=nope").await;
    assert_eq!(status, 404);
    let (status, _) = get(
        &env,
        "/api/v1/ai/repos/map_owner/map-subdir/map?path=src/lib.rs",
    )
    .await;
    assert_eq!(status, 404, "a file is not a mappable directory");
}

#[tokio::test]
async fn map_of_an_empty_repository_is_empty_not_an_error() {
    let env = env("map-empty").await;

    let (status, body) = get(&env, "/api/v1/ai/repos/map_owner/map-empty/map").await;
    assert_eq!(status, 200, "{body}");
    assert!(body["commit_sha"].is_null());
    assert_eq!(body["files"].as_array().unwrap().len(), 0);
    assert_eq!(body["directories"].as_array().unwrap().len(), 0);
    assert_eq!(body["truncated"], false);
}

#[tokio::test]
async fn map_requires_read_access_to_a_private_repository() {
    let (base, _db) = spawn_test_app_with_db().await;
    register_bootstrap_admin(&base).await;
    let (owner, _) = register_full(&base, "map_owner", "map_owner@example.com").await;
    let (other, _) = register_full(&base, "map_other", "map_other@example.com").await;
    let client = reqwest::Client::new();
    let created = client
        .post(format!("{base}/api/v1/repos"))
        .bearer_auth(&owner)
        .json(&serde_json::json!({"name": "map-private", "is_private": true}))
        .send()
        .await
        .unwrap();
    assert_eq!(created.status(), 201);

    let url = format!("{base}/api/v1/ai/repos/map_owner/map-private/map");
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
async fn map_reports_large_and_binary_files_as_skipped() {
    let env = env("map-large").await;
    let big = "x".repeat(rg_core::search::outline::MAX_FILE_BYTES + 1);
    push_files(
        &env.bare,
        &[
            ("src/big.rs", big.as_str()),
            ("src/ok.rs", "pub fn ok() {}\n"),
        ],
    );

    let (status, body) = get(&env, "/api/v1/ai/repos/map_owner/map-large/map").await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(file_paths(&body), vec!["src/ok.rs"]);
    let big = body["skipped"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["path"] == "src/big.rs")
        .expect("oversized file is reported");
    assert_eq!(big["reason"], "file too large");
}
