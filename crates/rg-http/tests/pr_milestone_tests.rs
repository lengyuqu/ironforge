//! A4: PR milestone vertical slice integration tests.
//!
//! Covers the write path that previously had no entry point:
//!   PATCH /repos/:o/:r/pulls/:number {milestone_id}     — set / clear (null) / guard
//! PR rows are inserted directly via pull_request_ops (existing convention in
//! pr_permission_tests.rs) so tests avoid git branch setup; the milestone
//! set/clear logic and repo-ownership validation are what matter here.

mod common;

use chrono::Utc;
use common::{register_full, spawn_test_app_with_db};
use sea_orm::Set;

const PW: &str = "Qz7$wRtm";

async fn register_owner(base: &str, tag: &str) -> (String, i64) {
    register_full(base, &format!("prms{tag}"), &format!("prms{tag}@example.com")).await
}

async fn create_repo(base: &str, token: &str, name: &str) -> i64 {
    let resp = reqwest::Client::new()
        .post(format!("{base}/api/v1/repos"))
        .bearer_auth(token)
        .json(&serde_json::json!({"name": name}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201, "create repo {name}");
    resp.json::<serde_json::Value>().await.unwrap()["id"].as_i64().unwrap()
}

async fn create_milestone(base: &str, token: &str, owner: &str, repo: &str, title: &str) -> i64 {
    let resp = reqwest::Client::new()
        .post(format!("{base}/api/v1/repos/{owner}/{repo}/milestones"))
        .bearer_auth(token)
        .json(&serde_json::json!({"title": title}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201, "create milestone '{title}'");
    resp.json::<serde_json::Value>().await.unwrap()["id"].as_i64().unwrap()
}

/// Insert a bare open PR directly in the DB (no git data needed).
async fn insert_pr(
    db: &rg_db::DatabaseConnection,
    repo_id: i64,
    author_id: i64,
    number: i64,
) -> rg_db::entities::pull_request::Model {
    rg_db::ops::pull_request_ops::create(
        db,
        rg_db::entities::pull_request::ActiveModel {
            id: sea_orm::NotSet,
            repo_id: Set(repo_id),
            number: Set(number),
            title: Set(format!("PR {number}")),
            body: Set(Some("changes".to_string())),
            state: Set("open".to_string()),
            is_draft: Set(false),
            auto_merge_enabled: Set(false),
            auto_merge_strategy: Set(None),
            auto_merge_enabled_by_id: Set(None),
            auto_merge_enabled_at: Set(None),
            author_id: Set(author_id),
            reviewer_id: Set(None),
            head_branch: Set("feature".to_string()),
            base_branch: Set("main".to_string()),
            head_sha: Set(None),
            merge_strategy: Set(None),
            merge_commit_sha: Set(None),
            head_repo_id: Set(None),
            milestone_id: Set(None),
            labels: Set(None),
            created_at: Set(Utc::now()),
            updated_at: Set(Utc::now()),
            closed_at: Set(None),
            merged_at: Set(None),
        },
    )
    .await
    .unwrap()
}

async fn patch_pr(
    base: &str,
    token: &str,
    owner: &str,
    repo: &str,
    number: i64,
    body: serde_json::Value,
) -> reqwest::Response {
    reqwest::Client::new()
        .patch(format!("{base}/api/v1/repos/{owner}/{repo}/pulls/{number}"))
        .bearer_auth(token)
        .json(&body)
        .send()
        .await
        .unwrap()
}

// ── Set / clear ──────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_pr_milestone_set_and_clear() {
    let (base, db) = spawn_test_app_with_db().await;
    let (token, user_id) = register_owner(&base, "s1").await;
    let repo_id = create_repo(&base, &token, "pr-ms-repo").await;
    let ms_id = create_milestone(&base, &token, "prmss1", "pr-ms-repo", "sprint-1").await;
    insert_pr(&db, repo_id, user_id, 1).await;
    let client = reqwest::Client::new();

    // Attach a milestone.
    let resp = client
        .patch(format!("{base}/api/v1/repos/prmss1/pr-ms-repo/pulls/1"))
        .bearer_auth(&token)
        .json(&serde_json::json!({"milestone_id": ms_id}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["milestone_id"].as_i64(), Some(ms_id), "PR carries milestone");

    // A fresh GET reflects it too.
    let get = client
        .get(format!("{base}/api/v1/repos/prmss1/pr-ms-repo/pulls/1"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert_eq!(get.status(), 200);
    assert_eq!(
        get.json::<serde_json::Value>().await.unwrap()["milestone_id"].as_i64(),
        Some(ms_id)
    );

    // JSON null clears it (tri-state: absent would have left it untouched).
    let resp = patch_pr(&base, &token, "prmss1", "pr-ms-repo", 1, serde_json::json!({"milestone_id": null})).await;
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["milestone_id"], serde_json::Value::Null, "milestone cleared");
}

// ── Ownership guard ──────────────────────────────────────────────────────────

#[tokio::test]
async fn test_pr_milestone_cross_repo_and_unknown_are_rejected() {
    let (base, db) = spawn_test_app_with_db().await;
    let (token, user_id) = register_owner(&base, "g1").await;
    let repo_a = create_repo(&base, &token, "prms-a").await;
    let repo_b = create_repo(&base, &token, "prms-b").await;
    // Milestone lives on repo B only.
    let ms_b = create_milestone(&base, &token, "prmsg1", "prms-b", "b-milestone").await;
    insert_pr(&db, repo_a, user_id, 1).await;

    // PR on repo A cannot take repo B's milestone.
    let resp = patch_pr(&base, &token, "prmsg1", "prms-a", 1, serde_json::json!({"milestone_id": ms_b})).await;
    assert_eq!(resp.status(), 400, "cross-repo milestone rejected");

    // Unknown milestone id is rejected the same way.
    let resp = patch_pr(&base, &token, "prmsg1", "prms-a", 1, serde_json::json!({"milestone_id": 999_999})).await;
    assert_eq!(resp.status(), 400, "unknown milestone rejected");

    // The PR is untouched: still milestone-less.
    let body = reqwest::Client::new()
        .get(format!("{base}/api/v1/repos/prmsg1/prms-a/pulls/1"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap()
        .json::<serde_json::Value>()
        .await
        .unwrap();
    assert_eq!(body["milestone_id"], serde_json::Value::Null);
}
