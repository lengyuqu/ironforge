//! Repository namespaces: personal (`alice/x`) vs organization (`acme/x`).
//!
//! `owner_id` of an organization repository is only the member who created it,
//! so keying names and lookups on it caused four problems, all pinned here:
//!
//! * two members could create the same org repo name, and the leftover-directory
//!   quarantine then moved the first repository's live directory aside;
//! * a user's personal repo and an org repo of the same name were rejected as
//!   duplicates;
//! * `/repos/<creator>/<org-repo>` resolved to the organization's repository;
//! * pulls, labels, collaborators and branch protection failed (500) under the
//!   organization's own URL and only worked under the creator's.

mod common;

use common::{register_full, spawn_test_app_with_db};

async fn create(
    base: &str,
    token: &str,
    name: &str,
    org: Option<&str>,
) -> (reqwest::StatusCode, serde_json::Value) {
    let response = reqwest::Client::new()
        .post(format!("{base}/api/v1/repos"))
        .bearer_auth(token)
        .json(&serde_json::json!({ "name": name, "org": org, "is_private": false }))
        .send()
        .await
        .unwrap();
    let status = response.status();
    (status, response.json().await.unwrap_or_default())
}

async fn make_org(base: &str, token: &str, name: &str) {
    let response = reqwest::Client::new()
        .post(format!("{base}/api/v1/orgs"))
        .bearer_auth(token)
        .json(&serde_json::json!({ "name": name, "display_name": name }))
        .send()
        .await
        .unwrap();
    assert!(response.status().is_success(), "create org {name}: {}", response.status());
}

async fn add_member(base: &str, token: &str, org: &str, user_id: i64) {
    let response = reqwest::Client::new()
        .post(format!("{base}/api/v1/orgs/{org}/members"))
        .bearer_auth(token)
        .json(&serde_json::json!({ "user_id": user_id, "role": "member" }))
        .send()
        .await
        .unwrap();
    assert!(response.status().is_success(), "add member: {}", response.status());
}


async fn repo_root_app() -> (String, std::path::PathBuf) {
    use common::{build_test_app_state, setup_test_db};
    let (db, dir) = setup_test_db().await;
    let repo_root = dir.path().join("repos");
    std::fs::create_dir_all(&repo_root).unwrap();
    let state = build_test_app_state(db, repo_root.clone());
    let app = rg_http::create_router_for_test(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        let _dir = dir;
        axum::serve(listener, app).await.unwrap();
    });
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    (base, repo_root)
}

/// Two org members creating the same repo name: the second is a conflict, and
/// the first repository is left completely untouched.
#[tokio::test]
async fn second_member_cannot_create_a_duplicate_org_repo_or_displace_the_first() {
    let (base, repo_root) = repo_root_app().await;
    let (alice, _) = register_full(&base, "alice3", "alice3@example.com").await;
    let (bob, bob_id) = register_full(&base, "bob3", "bob3@example.com").await;
    make_org(&base, &alice, "acme3").await;
    add_member(&base, &alice, "acme3", bob_id).await;

    let first = reqwest::Client::new()
        .post(format!("{base}/api/v1/repos"))
        .bearer_auth(&alice)
        .json(&serde_json::json!({
            "name": "dup", "org": "acme3", "auto_init": true, "readme": "default"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(first.status(), 201);

    let (status, body) = create(&base, &bob, "dup", Some("acme3")).await;
    assert_eq!(status, 409, "a duplicate org repo name must conflict: {body}");

    // Nothing was moved aside and nothing else was created.
    let mut entries: Vec<String> = std::fs::read_dir(repo_root.join("acme3"))
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    entries.sort();
    assert_eq!(entries, vec!["dup.git".to_string()], "first repo's directory must stay put");

    let branches: serde_json::Value = reqwest::Client::new()
        .get(format!("{base}/api/v1/repos/acme3/dup/branches"))
        .bearer_auth(&alice)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(branches.as_array().map(Vec::len), Some(1), "first repo keeps its history: {branches}");

    let listed: serde_json::Value = reqwest::Client::new()
        .get(format!("{base}/api/v1/repos/acme3"))
        .bearer_auth(&alice)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(listed["data"].as_array().map(Vec::len), Some(1), "{listed}");
}

/// A user's personal repository and a repository of an organization they belong
/// to are different namespaces (`alice/shared` vs `acme/shared`): both can
/// exist, on different disk paths, and each URL resolves to its own.
#[tokio::test]
async fn personal_and_org_repo_may_share_a_name() {
    let (base, _db) = spawn_test_app_with_db().await;
    let (alice, _) = register_full(&base, "alice1", "alice1@example.com").await;
    make_org(&base, &alice, "acme1").await;
    let (personal, _) = create(&base, &alice, "shared", None).await;
    assert_eq!(personal, 201);
    let (org, body) = create(&base, &alice, "shared", Some("acme1")).await;
    assert_eq!(org, 201, "{body}");
    make_org(&base, &alice, "beta1").await;
    let (second_org, body) = create(&base, &alice, "shared", Some("beta1")).await;
    assert_eq!(second_org, 201, "the same name may exist in several orgs: {body}");

    // Each URL resolves to its own repository.
    let mut ids = Vec::new();
    for owner in ["alice1", "acme1", "beta1"] {
        let r = reqwest::Client::new()
            .get(format!("{base}/api/v1/repos/{owner}/shared"))
            .bearer_auth(&alice)
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 200, "{owner}/shared");
        let body: serde_json::Value = r.json().await.unwrap();
        ids.push(body["id"].as_i64().unwrap());
    }
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), 3, "three namespaces, three distinct repositories");

    // ...but duplicates *within* a namespace are still refused.
    let (dup, _) = create(&base, &alice, "shared", None).await;
    assert_eq!(dup, 409);
    let (dup, _) = create(&base, &alice, "shared", Some("acme1")).await;
    assert_eq!(dup, 409);
}

/// An organization's repository is addressable only under the organization, not
/// under whichever member created it (and is not in that member's repo list).
#[tokio::test]
async fn user_namespace_does_not_resolve_org_repositories() {
    let (base, _db) = spawn_test_app_with_db().await;
    let (alice, _) = register_full(&base, "alice4", "alice4@example.com").await;
    make_org(&base, &alice, "acme4").await;
    let (status, _) = create(&base, &alice, "orgonly", Some("acme4")).await;
    assert_eq!(status, 201);

    let via_org = reqwest::Client::new()
        .get(format!("{base}/api/v1/repos/acme4/orgonly"))
        .bearer_auth(&alice)
        .send()
        .await
        .unwrap();
    assert_eq!(via_org.status(), 200);
    let via_user = reqwest::Client::new()
        .get(format!("{base}/api/v1/repos/alice4/orgonly"))
        .bearer_auth(&alice)
        .send()
        .await
        .unwrap();
    assert_eq!(via_user.status(), 404, "an org repo must not resolve under a user");

    let listed: serde_json::Value = reqwest::Client::new()
        .get(format!("{base}/api/v1/repos/alice4"))
        .bearer_auth(&alice)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        listed["data"].as_array().map(Vec::len),
        Some(0),
        "a user's repo list holds personal repositories only: {listed}"
    );
}

/// Pulls, labels, collaborators and branch protection resolved the owner as a
/// *user*, so every one of them answered 500 under the organization's URL and
/// only worked under the creating member's personal URL.
#[tokio::test]
async fn org_repository_features_work_under_the_org_url() {
    let (base, _db) = spawn_test_app_with_db().await;
    let (alice, _) = register_full(&base, "alice9", "alice9@example.com").await;
    make_org(&base, &alice, "acme9").await;
    let (status, _) = create(&base, &alice, "r", Some("acme9")).await;
    assert_eq!(status, 201);

    for endpoint in ["pulls", "labels", "collaborators", "branches/protection", "issues", "branches"] {
        let under_org = reqwest::Client::new()
            .get(format!("{base}/api/v1/repos/acme9/r/{endpoint}"))
            .bearer_auth(&alice)
            .send()
            .await
            .unwrap();
        assert_eq!(under_org.status(), 200, "GET /repos/acme9/r/{endpoint}");
        let under_user = reqwest::Client::new()
            .get(format!("{base}/api/v1/repos/alice9/r/{endpoint}"))
            .bearer_auth(&alice)
            .send()
            .await
            .unwrap();
        assert_eq!(under_user.status(), 404, "GET /repos/alice9/r/{endpoint}");
    }
}

async fn transfer(base: &str, token: &str, owner: &str, name: &str, to: &str) -> (reqwest::StatusCode, serde_json::Value) {
    let response = reqwest::Client::new()
        .post(format!("{base}/api/v1/repos/{owner}/{name}/transfer"))
        .bearer_auth(token)
        .json(&serde_json::json!({ "new_owner": to }))
        .send()
        .await
        .unwrap();
    let status = response.status();
    (status, response.json().await.unwrap_or_default())
}

/// Transfer checked the destination by `new_owner_id`, which for an
/// organization is the org owner's *user* id — i.e. it looked at that user's
/// personal repositories instead of the organization's.
#[tokio::test]
async fn transfer_checks_the_destination_namespace() {
    let (base, repo_root) = repo_root_app().await;
    let (alice, _) = register_full(&base, "alice7", "alice7@example.com").await;
    make_org(&base, &alice, "acme7").await;

    // alice7 also has a personal repo "moved" -- unrelated to acme7's namespace.
    let (status, _) = create(&base, &alice, "moved", None).await;
    assert_eq!(status, 201);
    let (status, _) = create(&base, &alice, "other", None).await;
    assert_eq!(status, 201);

    // Moving a personal repo into the org succeeds when the *org* has no
    // such name, even though the org owner has a personal repo of that name.
    let (status, body) = transfer(&base, &alice, "alice7", "other", "acme7").await;
    assert_eq!(status, 200, "{body}");
    assert!(repo_root.join("acme7/other.git").exists());
    assert!(!repo_root.join("alice7/other.git").exists());
    let r = reqwest::Client::new()
        .get(format!("{base}/api/v1/repos/acme7/other"))
        .bearer_auth(&alice)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);

    // A name taken *in the org* conflicts, and nothing moves.
    let (status, _) = create(&base, &alice, "moved", Some("acme7")).await;
    assert_eq!(status, 201, "org may reuse a name the owner has personally");
    let (status, body) = transfer(&base, &alice, "alice7", "moved", "acme7").await;
    assert_eq!(status, 409, "{body}");
    assert!(repo_root.join("alice7/moved.git").exists(), "failed transfer must not move data");
}
