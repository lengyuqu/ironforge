//! O01 acceptance: collaboration rules discovery.
//!
//! Real HTTP server, SQLite and real Git objects.

mod collab_common;
mod common;

use collab_common::Env;
use reqwest::Method;
use serde_json::Value;

async fn rules(env: &Env, token: &str, query: &str) -> (u16, Value) {
    env.json(
        Method::GET,
        &env.url(&format!("/rules{query}")),
        token,
        None,
        None,
    )
    .await
}

fn paths(body: &Value) -> Vec<&str> {
    body["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["path"].as_str().unwrap())
        .collect()
}

#[tokio::test]
async fn default_discovery_lists_root_then_deeper_rules_with_blob_shas() {
    let env = Env::new().await;
    let sha = env.push_tree(
        &env.repo,
        "main",
        &[
            ("AGENTS.md", "root rules"),
            ("web/AGENTS.md", "web rules"),
            ("web/src/lib.rs", "x"),
            ("docs/AGENTS.md", "docs rules"),
        ],
        &[],
    );

    let (status, body) = rules(&env, &env.owner_token, "?path=web/src").await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["resolved_commit"], sha);
    assert_eq!(body["requested_ref"], "HEAD");
    assert_eq!(body["discovery_version"], 1);
    assert_eq!(body["manifest"]["status"], "absent");
    // Shallowest first; the sibling `docs` rules do not apply.
    assert_eq!(paths(&body), vec!["AGENTS.md", "web/AGENTS.md"]);
    let entries = body["entries"].as_array().unwrap();
    assert_eq!(entries[1]["applies_to"], "web");
    assert_eq!(entries[1]["source"], "default_discovery");
    assert_eq!(entries[1]["origin"], "repository_content");
    assert_eq!(entries[1]["oversize"], false);
    assert_eq!(entries[0]["size"], "root rules".len());
    assert_eq!(
        entries[1]["blob_sha"].as_str().unwrap(),
        env.rev_parse(&env.repo, &format!("{sha}:web/AGENTS.md"))
    );
    assert_eq!(
        entries[1]["read_url"],
        format!(
            "/api/v1/repos/{}/{}/blob/web/AGENTS.md?ref={sha}",
            env.owner, env.repo
        )
    );

    // Root only, and a path with no nested rules, both resolve to the root file.
    let (_, root) = rules(&env, &env.owner_token, "").await;
    assert_eq!(paths(&root), vec!["AGENTS.md"]);
    let (_, none) = rules(&env, &env.owner_token, "?path=other/dir/file.txt").await;
    assert_eq!(paths(&none), vec!["AGENTS.md"]);
    // A trailing slash names the same directory.
    let (_, slash) = rules(&env, &env.owner_token, "?path=web/").await;
    assert_eq!(paths(&slash), vec!["AGENTS.md", "web/AGENTS.md"]);
}

#[tokio::test]
async fn a_pinned_commit_stays_readable_after_the_branch_moves() {
    let env = Env::new().await;
    let first = env.push_tree(&env.repo, "main", &[("AGENTS.md", "v1")], &[]);
    let second = env.push_tree(&env.repo, "main", &[("AGENTS.md", "version two")], &[]);
    assert_ne!(first, second);

    let (_, moved) = rules(&env, &env.owner_token, "").await;
    assert_eq!(moved["resolved_commit"], second);
    assert_eq!(moved["entries"][0]["size"], "version two".len());

    let (_, pinned) = rules(&env, &env.owner_token, &format!("?ref={first}")).await;
    assert_eq!(pinned["requested_ref"], first.as_str());
    assert_eq!(pinned["resolved_commit"], first);
    assert_eq!(pinned["entries"][0]["size"], "v1".len());
    assert_ne!(
        pinned["entries"][0]["blob_sha"],
        moved["entries"][0]["blob_sha"]
    );
}

#[tokio::test]
async fn manifest_replaces_default_discovery_and_filters_by_scope() {
    let env = Env::new().await;
    env.push_tree(
        &env.repo,
        "main",
        &[
            (".ironforge/agent-context.toml", "version = 1\n[[rule]]\npath = \"CONTRIBUTING.md\"\n[[rule]]\npath = \"web/RULES.md\"\napplies_to = \"web\"\n[[rule]]\npath = \"ghost.md\"\n"),
            ("CONTRIBUTING.md", "c"),
            ("web/RULES.md", "w"),
            ("AGENTS.md", "ignored once a manifest is in force"),
        ],
        &[],
    );
    let (status, web) = rules(&env, &env.owner_token, "?path=web/app").await;
    assert_eq!(status, 200, "{web}");
    assert_eq!(web["manifest"]["status"], "valid");
    assert_eq!(paths(&web), vec!["CONTRIBUTING.md", "web/RULES.md"]);
    assert_eq!(web["entries"][0]["source"], "manifest");
    assert_eq!(web["entries"][0]["applies_to"], "");

    // Outside `web` only the repository-wide rule applies; the missing optional
    // file never appears as a rule.
    let (_, elsewhere) = rules(&env, &env.owner_token, "?path=server").await;
    assert_eq!(paths(&elsewhere), vec!["CONTRIBUTING.md"]);
}

#[tokio::test]
async fn invalid_manifest_is_reported_and_default_discovery_applies() {
    let env = Env::new().await;
    env.push_tree(
        &env.repo,
        "main",
        &[
            (
                ".ironforge/agent-context.toml",
                "[[rule]]\npath = \"../../etc/passwd\"\n",
            ),
            ("AGENTS.md", "root"),
        ],
        &[],
    );
    let (status, body) = rules(&env, &env.owner_token, "").await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["manifest"]["status"], "invalid");
    assert!(body["manifest"]["error"].as_str().unwrap().contains("path"));
    assert_eq!(paths(&body), vec!["AGENTS.md"]);
    assert_eq!(body["entries"][0]["source"], "default_discovery");
}

#[tokio::test]
async fn symlinked_rule_files_are_never_followed() {
    let env = Env::new().await;
    env.push_tree(
        &env.repo,
        "main",
        &[("README.md", "r")],
        &[("AGENTS.md", "/etc/passwd")],
    );
    let (status, body) = rules(&env, &env.owner_token, "").await;
    assert_eq!(status, 200, "{body}");
    assert!(paths(&body).is_empty());
    assert_eq!(body["skipped"][0]["path"], "AGENTS.md");
    assert_eq!(body["skipped"][0]["reason"], "symlink");
}

#[tokio::test]
async fn bad_input_unknown_ref_and_empty_repository_have_clear_results() {
    let env = Env::new().await;

    // Repository without commits: nothing to discover, not an error.
    let (status, empty) = rules(&env, &env.owner_token, "").await;
    assert_eq!(status, 200, "{empty}");
    assert!(empty["resolved_commit"].is_null());
    assert!(paths(&empty).is_empty());

    env.push_tree(&env.repo, "main", &[("AGENTS.md", "r")], &[]);
    for bad in ["../x", "a//b", "a/../b", "/abs", "a%5Cb", "a%00b"] {
        let (status, _) = rules(&env, &env.owner_token, &format!("?path={bad}")).await;
        assert_eq!(status, 400, "path {bad:?}");
    }
    let (status, _) = rules(&env, &env.owner_token, "?ref=no-such-branch").await;
    assert_eq!(status, 404);
}

#[tokio::test]
async fn rules_need_an_account_with_access() {
    let env = Env::new().await;
    env.push_tree(&env.repo, "main", &[("AGENTS.md", "r")], &[]);

    let resp = env
        .send(Method::GET, &env.url("/rules"), None, None, None)
        .await;
    assert_eq!(resp.status(), 401);
    let (stranger, _) = env.user("stranger").await;
    let (status, _) = rules(&env, &stranger, "").await;
    assert_eq!(status, 403);
    let (reader, reader_id) = env.user("reader").await;
    env.add_collaborator(reader_id, "read").await;
    let (status, _) = rules(&env, &reader, "").await;
    assert_eq!(status, 200);
}
