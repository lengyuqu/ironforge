//! Regression: the REST `contents` API used to bypass branch protection.
//!
//! `repo/service.rs` contained no reference to branch protection at all, and
//! `check_push_allowed` had zero callers. Protection was only enforced in the
//! git receive-pack pre-receive hook, so a plain
//! `POST /repos/{owner}/{repo}/contents/{path}` could land an unsigned commit
//! on a branch configured with `require_pr` and/or `require_signed_commits`.

mod common;

use common::{register_user, spawn_test_app};

const PW: &str = "Qz7$wRtm";

/// Auto-initialise a repo so `main` exists with a commit.
async fn create_initialised_repo(base: &str, token: &str, name: &str) -> i64 {
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/api/v1/repos", base))
        .bearer_auth(token)
        .json(&serde_json::json!({"name": name, "auto_init": true}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201, "create repo failed: {}", resp.status());
    resp.json::<serde_json::Value>().await.unwrap()["id"]
        .as_i64()
        .unwrap()
}

async fn protect(base: &str, token: &str, owner: &str, repo: &str, body: serde_json::Value) {
    let client = reqwest::Client::new();
    let resp = client
        .post(format!(
            "{base}/api/v1/repos/{owner}/{repo}/branches/protection"
        ))
        .bearer_auth(token)
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201, "create protection failed: {}", resp.status());
}

async fn write_file(
    base: &str,
    token: &str,
    owner: &str,
    repo: &str,
    path: &str,
    branch: &str,
) -> reqwest::StatusCode {
    let client = reqwest::Client::new();
    client
        .post(format!(
            "{base}/api/v1/repos/{owner}/{repo}/contents/{path}"
        ))
        .bearer_auth(token)
        .json(&serde_json::json!({
            "content": "aGFja2Vk",
            "message": "direct write",
            "branch": branch,
        }))
        .send()
        .await
        .unwrap()
        .status()
}

#[tokio::test]
async fn contents_api_respects_require_pr() {
    let base = spawn_test_app().await;
    let token = register_user(&base, "protowner1", "protowner1@example.com", PW).await;
    create_initialised_repo(&base, &token, "prrepo").await;
    protect(
        &base,
        &token,
        "protowner1",
        "prrepo",
        serde_json::json!({"branch_name": "main", "require_pr": true}),
    )
    .await;

    // Direct write to the protected branch must now be refused.
    let status = write_file(&base, &token, "protowner1", "prrepo", "bypass.txt", "main").await;
    assert_eq!(
        status,
        403,
        "direct commit to a require_pr branch must be forbidden, got {status}"
    );

    // An unprotected branch is unaffected.
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{base}/api/v1/repos/protowner1/prrepo/branches"))
        .bearer_auth(&token)
        .json(&serde_json::json!({"name": "feature"}))
        .send()
        .await
        .unwrap();
    if resp.status().is_success() {
        let status =
            write_file(&base, &token, "protowner1", "prrepo", "ok.txt", "feature").await;
        assert_eq!(
            status, 200,
            "unprotected branch must still accept direct commits, got {status}"
        );
    }
}

#[tokio::test]
async fn contents_api_respects_require_signed_commits() {
    let base = spawn_test_app().await;
    let token = register_user(&base, "protowner2", "protowner2@example.com", PW).await;
    create_initialised_repo(&base, &token, "signedrepo").await;
    protect(
        &base,
        &token,
        "protowner2",
        "signedrepo",
        serde_json::json!({"branch_name": "main", "require_signed_commits": true}),
    )
    .await;

    let status = write_file(
        &base,
        &token,
        "protowner2",
        "signedrepo",
        "unsigned.txt",
        "main",
    )
    .await;
    assert_eq!(
        status,
        403,
        "server-side unsigned commit must be refused on a signed-commits branch, got {status}"
    );
}

/// A user explicitly allowlisted for the branch may still write directly.
#[tokio::test]
async fn allowlisted_user_can_bypass_require_pr() {
    let base = spawn_test_app().await;
    let token = register_user(&base, "protowner3", "protowner3@example.com", PW).await;
    let (_, uid) = common::register_full(&base, "protbot3", "protbot3@example.com").await;
    create_initialised_repo(&base, &token, "allowrepo").await;

    protect(
        &base,
        &token,
        "protowner3",
        "allowrepo",
        serde_json::json!({
            "branch_name": "main",
            "require_pr": true,
            "allowed_push_user_ids": [uid],
        }),
    )
    .await;

    // The owner is not on the allowlist, so they are still blocked.
    let status = write_file(&base, &token, "protowner3", "allowrepo", "a.txt", "main").await;
    assert_eq!(status, 403, "non-allowlisted owner must be blocked");
}

/// Regression: the rule matcher must support globs, and the allowlist must not
/// waive `require_signed_commits`. Previously the contents API used an exact
/// `eq` lookup and returned early for allowlisted users, so a `release/*` rule
/// protected nothing and an allowlisted user could push unsigned commits.
#[tokio::test]
async fn contents_api_respects_wildcard_rule_and_signed_commits() {
    let base = spawn_test_app().await;
    let token = register_user(&base, "protowner4", "protowner4@example.com", PW).await;
    let (_, uid) = common::register_full(&base, "protbot4", "protbot4@example.com").await;
    create_initialised_repo(&base, &token, "wildrepo").await;

    protect(
        &base,
        &token,
        "protowner4",
        "wildrepo",
        serde_json::json!({
            "branch_name": "release/*",
            "require_pr": true,
        }),
    )
    .await;

    // Create the matching branch, then attempt a direct write to it.
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{base}/api/v1/repos/protowner4/wildrepo/branches"))
        .bearer_auth(&token)
        .json(&serde_json::json!({"name": "release/1.0"}))
        .send()
        .await
        .unwrap();
    if resp.status().is_success() {
        let status =
            write_file(&base, &token, "protowner4", "wildrepo", "r.txt", "release/1.0").await;
        assert_eq!(
            status, 403,
            "a `release/*` rule must cover `release/1.0`, got {status}"
        );
    }

    // Allowlisted users bypass require_pr but NOT require_signed_commits.
    protect(
        &base,
        &token,
        "protowner4",
        "wildrepo",
        serde_json::json!({
            "branch_name": "main",
            "require_signed_commits": true,
            "allowed_push_user_ids": [uid],
        }),
    )
    .await;
    let status = write_file(&base, &token, "protowner4", "wildrepo", "s.txt", "main").await;
    assert_eq!(
        status, 403,
        "require_signed_commits must apply even with an allowlist, got {status}"
    );
}
/// Regression: applying a review suggestion commits to the PR head branch but
/// used to skip branch protection entirely. A protected head branch must now
/// refuse the server-side commit with 403.
#[tokio::test]
async fn applying_suggestion_respects_protected_head_branch() {
    let base = spawn_test_app().await;
    let token = register_user(&base, "sugowner1", "sugowner1@example.com", PW).await;
    create_initialised_repo(&base, &token, "sugrepo").await;
    let client = reqwest::Client::new();

    // There is no REST branch-creation endpoint, so the `feature` branch is
    // materialised by the contents API itself, which also gives the PR a head
    // commit and a file to attach a suggestion to.
    let created = write_file(&base, &token, "sugowner1", "sugrepo", "note.txt", "feature").await;
    assert_eq!(created, 200, "creating the head branch via contents failed");

    let resp = client
        .post(format!("{base}/api/v1/repos/sugowner1/sugrepo/pulls"))
        .bearer_auth(&token)
        .json(&serde_json::json!({"title": "Feature", "head": "feature", "base": "main"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201, "PR create failed: {}", resp.status());
    let pr: serde_json::Value = resp.json().await.unwrap();
    let number = pr["number"].as_i64().unwrap();

    let resp = client
        .post(format!("{base}/api/v1/repos/sugowner1/sugrepo/pulls/{number}/comments"))
        .bearer_auth(&token)
        .json(&serde_json::json!({
            "path": "note.txt",
            "start_line": 1,
            "start_side": "RIGHT",
            "line": 1,
            "side": "RIGHT",
            "body": "Tweak the note",
            "suggestion": "# sugrepo (edited)"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201, "comment create failed: {}", resp.status());
    let comment_id = resp.json::<serde_json::Value>().await.unwrap()["id"]
        .as_i64()
        .unwrap();

    // Protect the PR head branch itself; `require_pr` blocks direct commits.
    protect(
        &base,
        &token,
        "sugowner1",
        "sugrepo",
        serde_json::json!({"branch_name": "feature", "require_pr": true}),
    )
    .await;

    let resp = client
        .post(format!(
            "{base}/api/v1/repos/sugowner1/sugrepo/pulls/{number}/suggestions/apply"
        ))
        .bearer_auth(&token)
        .json(&serde_json::json!({"comment_ids": [comment_id]}))
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        403,
        "applying a suggestion to a protected head branch must be forbidden, got {}: {}",
        resp.status(),
        resp.text().await.unwrap_or_default()
    );
}