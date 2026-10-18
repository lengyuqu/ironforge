//! Regression: search must not expose private repositories to unauthenticated
//! or unauthorised callers, while still letting authorised users find them.
//!
//! `GET /api/v1/search` originally took no actor and `search_repos` applied no
//! visibility filter, so a private repository whose name matched the query was
//! returned to anyone — including its description. These tests pin both halves.

mod common;

use common::{register_full, spawn_test_app_with_db};

fn titles(body: &serde_json::Value) -> Vec<String> {
    body.get("results")
        .and_then(|r| r.as_array())
        .map(|rows| {
            rows.iter()
                .filter_map(|r| {
                    r.get("title")
                        .or_else(|| r.get("name"))
                        .and_then(|t| t.as_str())
                        .map(str::to_string)
                })
                .collect()
        })
        .unwrap_or_default()
}

async fn search_as(base: &str, token: Option<&str>, q: &str) -> serde_json::Value {
    let client = reqwest::Client::new();
    let mut request = client.get(format!("{base}/api/v1/search?q={q}&type=repos"));
    if let Some(token) = token {
        request = request.bearer_auth(token);
    }
    let response = request.send().await.expect("search request");
    assert_eq!(response.status(), 200, "search should return 200");
    response.json().await.expect("search json")
}

#[tokio::test]
async fn anonymous_search_does_not_expose_private_repositories() {
    let (base, _db) = spawn_test_app_with_db().await;
    let (token, _owner_id) =
        register_full(&base, "search-owner", "search-owner@example.com").await;

    let created = reqwest::Client::new()
        .post(format!("{base}/api/v1/repos"))
        .bearer_auth(&token)
        .json(&serde_json::json!({
            "name": "secretproject",
            "is_private": true,
            "description": "secretproject private repo"
        }))
        .send()
        .await
        .expect("create private repo");
    assert_eq!(created.status(), 201, "private repo creation should succeed");

    // Anonymous: must not discover it.
    let anon = search_as(&base, None, "secretproject").await;
    assert!(
        !titles(&anon).contains(&"secretproject".to_string()),
        "an anonymous caller must not see the private repo: {anon}"
    );

    // The owner: must still find their own repository.
    let owner = search_as(&base, Some(&token), "secretproject").await;
    assert!(
        titles(&owner).contains(&"secretproject".to_string()),
        "the owner must still find their own private repo: {owner}"
    );

    // A different authenticated user with no grant: must not see it.
    let (other_token, _) =
        register_full(&base, "search-outsider", "search-outsider@example.com").await;
    let outsider = search_as(&base, Some(&other_token), "secretproject").await;
    assert!(
        !titles(&outsider).contains(&"secretproject".to_string()),
        "an unrelated user must not see the private repo: {outsider}"
    );
}

#[tokio::test]
async fn collaborators_can_find_private_repositories() {
    let (base, db) = spawn_test_app_with_db().await;
    let (owner_token, _owner_id) =
        register_full(&base, "search-collab-owner", "search-collab-owner@example.com").await;
    let (collab_token, collab_id) =
        register_full(&base, "search-collab", "search-collab@example.com").await;

    let created = reqwest::Client::new()
        .post(format!("{base}/api/v1/repos"))
        .bearer_auth(&owner_token)
        .json(&serde_json::json!({
            "name": "collabsecret",
            "is_private": true,
            "description": "collabsecret private repo"
        }))
        .send()
        .await
        .expect("create private repo");
    assert_eq!(created.status(), 201);
    let repo_id = created.json::<serde_json::Value>().await.unwrap()["id"]
        .as_i64()
        .unwrap();

    // Before the grant the collaborator cannot see it.
    let before = search_as(&base, Some(&collab_token), "collabsecret").await;
    assert!(
        !titles(&before).contains(&"collabsecret".to_string()),
        "a non-collaborator must not see the private repo: {before}"
    );

    // Grant read access, then the collaborator must find it.
    let _ = repo_id;
    rg_core::collaborator::service::add_collaborator(
        &db,
        "search-collab-owner",
        "collabsecret",
        collab_id,
        "read".to_string(),
    )
    .await
    .expect("add collaborator");
    rg_core::repo::service::invalidate_perm_cache_all();

    let after = search_as(&base, Some(&collab_token), "collabsecret").await;
    assert!(
        titles(&after).contains(&"collabsecret".to_string()),
        "a collaborator must see the private repo: {after}"
    );
}