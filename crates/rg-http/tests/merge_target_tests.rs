//! Merging a pull request must advance **its base branch**, not the repository
//! `HEAD`.
//!
//! The `merge` and `squash` strategies used to resolve "ours" from `HEAD` and
//! commit the result through `HEAD`, so a pull request whose base was not the
//! default branch was merged into whatever branch the bare repository happened
//! to have checked out (normally the default branch) — and its real base
//! branch stayed behind. The `rebase` strategy already used `pr.base_branch`.
//!
//! These tests seed a repository with `main` (default/HEAD) plus a `release`
//! branch, open a pull request with `base = release`, merge it, and assert that
//! `refs/heads/release` moves to the merge commit while `refs/heads/main`
//! stays put.

mod common;

use std::path::Path;

use common::{build_test_app_state, register_full, setup_test_db};

fn git(args: &[&str], cwd: Option<&Path>) -> String {
    let gateway = rg_git::cli_gateway::global_gateway().as_ref().unwrap();
    let output = gateway.run(args, cwd).unwrap();
    output.ensure_success().unwrap();
    output.stdout_str().trim().to_string()
}

fn configure_worktree(path: &Path) {
    git(&["config", "user.name", "Merge Target Test"], Some(path));
    git(
        &["config", "user.email", "merge-target@example.com"],
        Some(path),
    );
}

/// `main` is the default branch (`HEAD`), `release` is the pull request base,
/// and `feature` is its head. Returns the tips of `main` and `release` before
/// the merge.
fn seed_repository(bare_path: &Path) -> (tempfile::TempDir, String, String) {
    let worktree = tempfile::tempdir().unwrap();
    let path = worktree.path();
    let path_arg = path.to_string_lossy();
    git(&["init", "--initial-branch=main", &path_arg], None);
    configure_worktree(path);

    std::fs::write(path.join("README.md"), "base\n").unwrap();
    git(&["add", "."], Some(path));
    git(&["commit", "-m", "base"], Some(path));
    let bare_arg = bare_path.to_string_lossy();
    git(&["remote", "add", "origin", &bare_arg], Some(path));
    git(&["push", "origin", "main"], Some(path));

    // A second long-lived branch, not the default one.
    git(&["checkout", "-b", "release"], Some(path));
    std::fs::write(path.join("release-only.txt"), "release\n").unwrap();
    git(&["add", "."], Some(path));
    git(&["commit", "-m", "release only"], Some(path));
    git(&["push", "origin", "release"], Some(path));

    git(&["checkout", "-b", "feature", "main"], Some(path));
    std::fs::write(path.join("feature.txt"), "feature\n").unwrap();
    git(&["add", "."], Some(path));
    git(&["commit", "-m", "feature"], Some(path));
    git(&["push", "origin", "feature"], Some(path));

    let main_sha = git(&["rev-parse", "refs/heads/main"], Some(bare_path));
    let release_sha = git(&["rev-parse", "refs/heads/release"], Some(bare_path));
    (worktree, main_sha, release_sha)
}

async fn create_repo(base: &str, token: &str, repo: &str) {
    let created = reqwest::Client::new()
        .post(format!("{base}/api/v1/repos"))
        .bearer_auth(token)
        .json(&serde_json::json!({"name": repo, "is_private": false}))
        .send()
        .await
        .unwrap();
    assert_eq!(created.status(), 201, "{}", created.text().await.unwrap());
}

async fn open_pr_into_release(base: &str, token: &str, repo: &str) {
    let pr = reqwest::Client::new()
        .post(format!("{base}/api/v1/repos/merge-owner/{repo}/pulls"))
        .bearer_auth(token)
        .json(&serde_json::json!({
            "title": "Merge into release",
            "head": "feature",
            "base": "release"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(pr.status(), 201, "{}", pr.text().await.unwrap());
}

async fn assert_merges_into_base_branch(strategy: &str) {
    let (db, app_dir) = setup_test_db().await;
    let repo_root = app_dir.path().join("repos");
    std::fs::create_dir_all(&repo_root).unwrap();
    let app = rg_http::create_router_for_test(build_test_app_state(db, repo_root.clone()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let _app_dir = app_dir;
        axum::serve(listener, app).await.unwrap();
    });
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let (token, _) = register_full(&base, "merge-owner", "merge-owner@example.com").await;
    let repo = format!("{strategy}-target-repo");
    let bare_path = repo_root.join(format!("merge-owner/{repo}.git"));
    create_repo(&base, &token, &repo).await;
    let (_worktree, main_before, release_before) = seed_repository(&bare_path);
    open_pr_into_release(&base, &token, &repo).await;

    let merged = reqwest::Client::new()
        .post(format!(
            "{base}/api/v1/repos/merge-owner/{repo}/pulls/1/merge"
        ))
        .bearer_auth(&token)
        .json(&serde_json::json!({"strategy": strategy}))
        .send()
        .await
        .unwrap();
    assert_eq!(
        merged.status(),
        200,
        "{strategy} merge failed: {}",
        merged.text().await.unwrap()
    );
    let merged = merged.json::<serde_json::Value>().await.unwrap();
    let merge_commit = merged["merge_commit_sha"]
        .as_str()
        .expect("merge commit sha")
        .to_string();

    let release_after = git(&["rev-parse", "refs/heads/release"], Some(&bare_path));
    let main_after = git(&["rev-parse", "refs/heads/main"], Some(&bare_path));

    assert_eq!(
        release_after, merge_commit,
        "{strategy}: the base branch must advance to the merge commit"
    );
    assert_ne!(
        release_after, release_before,
        "{strategy}: the base branch must actually move"
    );
    assert_eq!(
        main_after, main_before,
        "{strategy}: the default branch (HEAD) must not move"
    );
    assert_eq!(
        git(
            &["show", "refs/heads/release:feature.txt"],
            Some(&bare_path)
        ),
        "feature",
        "{strategy}: the merged content must be on the base branch"
    );
    assert_eq!(
        git(
            &["show", "refs/heads/release:release-only.txt"],
            Some(&bare_path)
        ),
        "release",
        "{strategy}: the base branch's own content must survive the merge"
    );

    server.abort();
}

#[tokio::test]
async fn merge_strategy_advances_the_base_branch_not_head() {
    assert_merges_into_base_branch("merge").await;
}

#[tokio::test]
async fn squash_strategy_advances_the_base_branch_not_head() {
    assert_merges_into_base_branch("squash").await;
}
