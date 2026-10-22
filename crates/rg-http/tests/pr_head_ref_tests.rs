//! `owner:branch` head references name a *namespace*.
//!
//! `resolve_head_ref` decided "same repository or a fork?" by comparing the
//! head owner's user id with `repo.owner_id`. For an organization repository
//! `owner_id` is only the member who created it, so
//!
//! * `acme:feature` (the natural spelling) failed with "head owner not found",
//! * the creator's `carol:feature` was always read as "this repository" even
//!   when carol held her own fork of it - the PR would be built from the wrong
//!   repository.

mod common;

use common::{register_full, spawn_test_app_with_db};

async fn ok(r: reqwest::Response, what: &str) -> serde_json::Value {
    let status = r.status();
    let body: serde_json::Value = r.json().await.unwrap_or_default();
    assert!(status.is_success(), "{what}: {status} {body}");
    body
}

/// An unresolvable head is the caller's mistake: it must classify as invalid
/// input (HTTP 400), not as an internal failure (a sanitised 500).
async fn assert_invalid_input(db: &rg_db::DatabaseConnection, repo_id: i64, head: &str) {
    match rg_core::pull_request::resolve_head_ref(db, repo_id, head).await {
        Err(rg_core::error::CoreError::InvalidInput(_)) => {}
        other => panic!("{head}: expected InvalidInput, got {other:?}"),
    }
}

async fn resolve(
    db: &rg_db::DatabaseConnection,
    repo_id: i64,
    head: &str,
) -> Result<(String, Option<i64>), String> {
    rg_core::pull_request::resolve_head_ref(db, repo_id, head)
        .await
        .map_err(|e| e.to_string())
}

#[tokio::test]
async fn head_owner_is_resolved_as_a_namespace() {
    let (base, db) = spawn_test_app_with_db().await;
    let (owner, _) = register_full(&base, "hr-owner", "hr-owner@example.com").await;
    let (carol, carol_id) = register_full(&base, "hr-carol", "hr-carol@example.com").await;
    let (dave, dave_id) = register_full(&base, "hr-dave", "hr-dave@example.com").await;
    let client = reqwest::Client::new();

    ok(client.post(format!("{base}/api/v1/orgs")).bearer_auth(&owner)
        .json(&serde_json::json!({"name":"hr-acme","display_name":"a"})).send().await.unwrap(), "org").await;
    for id in [carol_id, dave_id] {
        ok(client.post(format!("{base}/api/v1/orgs/hr-acme/members")).bearer_auth(&owner)
            .json(&serde_json::json!({"user_id": id, "role":"member"})).send().await.unwrap(), "member").await;
    }

    // carol (a plain member) creates the organization's repository, so its
    // `owner_id` is carol.
    let org_repo = ok(client.post(format!("{base}/api/v1/repos")).bearer_auth(&carol)
        .json(&serde_json::json!({"name":"lib","org":"hr-acme","is_private":false})).send().await.unwrap(), "org repo").await;
    let org_repo_id = org_repo["id"].as_i64().unwrap();

    // The organization's own name means "a branch of this repository".
    assert_eq!(resolve(&db, org_repo_id, "hr-acme:feat").await, Ok(("feat".into(), None)));
    // A bare branch name too.
    assert_eq!(resolve(&db, org_repo_id, "feat").await, Ok(("feat".into(), None)));

    // The creating member is just another user: without a fork it is an error,
    // not a silent alias for the organization's repository.
    assert_invalid_input(&db, org_repo_id, "hr-carol:feat").await;

    // Once carol forks it, `hr-carol:feat` names her fork.
    let carol_fork = ok(client.post(format!("{base}/api/v1/repos/hr-acme/lib/fork")).bearer_auth(&carol)
        .json(&serde_json::json!({})).send().await.unwrap(), "carol fork").await;
    let carol_fork_id = carol_fork["id"].as_i64().unwrap();
    assert_eq!(
        resolve(&db, org_repo_id, "hr-carol:feat").await,
        Ok(("feat".into(), Some(carol_fork_id))),
        "the creator's fork must be used, not the organization's repository"
    );

    // Another member's fork resolves as before.
    let dave_fork = ok(client.post(format!("{base}/api/v1/repos/hr-acme/lib/fork")).bearer_auth(&dave)
        .json(&serde_json::json!({})).send().await.unwrap(), "dave fork").await;
    assert_eq!(
        resolve(&db, org_repo_id, "hr-dave:feat").await,
        Ok(("feat".into(), Some(dave_fork["id"].as_i64().unwrap())))
    );

    // A namespace that holds no fork of it is rejected.
    assert_invalid_input(&db, org_repo_id, "hr-owner:feat").await;
    assert_invalid_input(&db, org_repo_id, "nobody-at-all:feat").await;
}

#[tokio::test]
async fn personal_repository_head_refs_still_work() {
    let (base, db) = spawn_test_app_with_db().await;
    let (alice, _) = register_full(&base, "hp-alice", "hp-alice@example.com").await;
    let (bob, _) = register_full(&base, "hp-bob", "hp-bob@example.com").await;
    let client = reqwest::Client::new();

    let repo = ok(client.post(format!("{base}/api/v1/repos")).bearer_auth(&alice)
        .json(&serde_json::json!({"name":"tool","is_private":false})).send().await.unwrap(), "repo").await;
    let id = repo["id"].as_i64().unwrap();

    assert_eq!(resolve(&db, id, "hp-alice:feat").await, Ok(("feat".into(), None)));
    assert_invalid_input(&db, id, "hp-bob:feat").await;

    let fork = ok(client.post(format!("{base}/api/v1/repos/hp-alice/tool/fork")).bearer_auth(&bob)
        .json(&serde_json::json!({})).send().await.unwrap(), "fork").await;
    assert_eq!(
        resolve(&db, id, "hp-bob:feat").await,
        Ok(("feat".into(), Some(fork["id"].as_i64().unwrap())))
    );
}
