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
    search_typed(base, token, q, Some("repos")).await
}

/// `kind = None` exercises the default combined search (all types at once).
async fn search_typed(
    base: &str,
    token: Option<&str>,
    q: &str,
    kind: Option<&str>,
) -> serde_json::Value {
    let client = reqwest::Client::new();
    let suffix = kind.map(|k| format!("&type={k}")).unwrap_or_default();
    let mut request = client.get(format!("{base}/api/v1/search?q={q}{suffix}"));
    if let Some(token) = token {
        request = request.bearer_auth(token);
    }
    let response = request.send().await.expect("search request");
    assert_eq!(
        response.status(),
        200,
        "search (type={kind:?}) should return 200"
    );
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
/// Regression: the visibility predicate references `r.is_private`, so every
/// query that applies it must join `repositories AS r` — including the COUNT
/// query. Missing that join in the issues count made `type=issues` and the
/// default combined search fail with a 500 while `type=repos` still passed.
/// Every search type, and the combined search, must return 200 for anonymous
/// and authenticated callers alike.
#[tokio::test]
async fn every_search_type_succeeds_and_respects_visibility() {
    let (base, _db) = spawn_test_app_with_db().await;
    let (owner_token, _) =
        register_full(&base, "kinds-owner", "kinds-owner@example.com").await;
    let (outsider_token, _) =
        register_full(&base, "kinds-outsider", "kinds-outsider@example.com").await;
    let client = reqwest::Client::new();

    for (name, private) in [("kindspublic", false), ("kindsprivate", true)] {
        let created = client
            .post(format!("{base}/api/v1/repos"))
            .bearer_auth(&owner_token)
            .json(&serde_json::json!({ "name": name, "is_private": private }))
            .send()
            .await
            .expect("create repo");
        assert_eq!(created.status(), 201);
        let issue = client
            .post(format!("{base}/api/v1/repos/kinds-owner/{name}/issues"))
            .bearer_auth(&owner_token)
            .json(&serde_json::json!({
                "title": format!("{name}issuemarker"),
                "body": "body"
            }))
            .send()
            .await
            .expect("create issue");
        assert_eq!(issue.status(), 201, "issue creation should succeed");
    }

    for kind in [None, Some("repos"), Some("issues"), Some("wiki")] {
        // None of these may 500 for any caller (asserted inside search_typed).
        for token in [None, Some(outsider_token.as_str()), Some(owner_token.as_str())] {
            let _ = search_typed(&base, token, "kindspublicissuemarker", kind).await;
        }
    }

    let anon_public = search_typed(&base, None, "kindspublicissuemarker", Some("issues")).await;
    assert!(
        titles(&anon_public).contains(&"kindspublicissuemarker".to_string()),
        "an issue of a public repo must be searchable by anyone: {anon_public}"
    );

    for token in [None, Some(outsider_token.as_str())] {
        for kind in [None, Some("issues")] {
            let hidden = search_typed(&base, token, "kindsprivateissuemarker", kind).await;
            assert!(
                !titles(&hidden).contains(&"kindsprivateissuemarker".to_string()),
                "a private repo's issue must not leak (type={kind:?}): {hidden}"
            );
        }
    }

    let owner_private =
        search_typed(&base, Some(&owner_token), "kindsprivateissuemarker", Some("issues")).await;
    assert!(
        titles(&owner_private).contains(&"kindsprivateissuemarker".to_string()),
        "the owner must find their private repo's issue: {owner_private}"
    );
}

/// Regression: deleting a repository must remove it from search.
///
/// Deleting only soft-deletes the row. The repository's FTS row is re-created
/// by the very `UPDATE` that sets `deleted_at`, and its issue and wiki rows were
/// never touched, so a deleted *public* repository stayed discoverable by
/// anyone — name, issues and wiki alike.
#[tokio::test]
async fn deleted_repositories_disappear_from_every_search_type() {
    let (base, _db) = spawn_test_app_with_db().await;
    let (token, _) = register_full(&base, "gone-owner", "gone-owner@example.com").await;
    let client = reqwest::Client::new();

    let created = client
        .post(format!("{base}/api/v1/repos"))
        .bearer_auth(&token)
        .json(&serde_json::json!({
            "name": "vanishingrepo",
            "is_private": false,
            "description": "vanishingrepo description"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(created.status(), 201);
    let issue = client
        .post(format!("{base}/api/v1/repos/gone-owner/vanishingrepo/issues"))
        .bearer_auth(&token)
        .json(&serde_json::json!({ "title": "vanishingissue", "body": "body" }))
        .send()
        .await
        .unwrap();
    assert_eq!(issue.status(), 201);
    let wiki = client
        .post(format!("{base}/api/v1/repos/gone-owner/vanishingrepo/wiki"))
        .bearer_auth(&token)
        .json(&serde_json::json!({ "title": "vanishingwiki", "content": "vanishingwiki content" }))
        .send()
        .await
        .unwrap();
    assert!(wiki.status().is_success(), "wiki page creation: {}", wiki.status());

    // Sanity: everything is findable before the delete, so the later absence
    // is caused by the delete and not by a broken query.
    for (query, kind) in [
        ("vanishingrepo", "repos"),
        ("vanishingissue", "issues"),
        ("vanishingwiki", "wiki"),
    ] {
        let before = search_typed(&base, None, query, Some(kind)).await;
        assert!(
            before["total"].as_i64().unwrap_or(0) >= 1,
            "{kind} should be searchable before the delete: {before}"
        );
    }

    let deleted = client
        .delete(format!("{base}/api/v1/repos/gone-owner/vanishingrepo"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert_eq!(deleted.status(), 200);

    for (query, kind) in [
        ("vanishingrepo", "repos"),
        ("vanishingissue", "issues"),
        ("vanishingwiki", "wiki"),
        ("vanishingrepo", ""),
    ] {
        let kind = (!kind.is_empty()).then_some(kind);
        for who in [None, Some(token.as_str())] {
            let after = search_typed(&base, who, query, kind).await;
            assert_eq!(
                after["total"].as_i64().unwrap_or(-1),
                0,
                "a deleted repository must not be searchable (type={kind:?}, authed={}): {after}",
                who.is_some()
            );
            assert!(titles(&after).is_empty(), "{after}");
        }
    }
}
