//! Permission-matrix integration tests for the centralized repo access
//! helpers (`repo_access::require_read` / `require_write`).
//!
//! Q1.4 guard: these tests pin the unified authorization semantics that all
//! repo-scoped handlers share after the Q1.1–Q1.3 consolidation. Labels,
//! wiki and boards endpoints are used as representative migrated callers.

mod common;

use common::{register_full, spawn_test_app_with_db};

fn ci_token(repo_id: i64, scopes: &str) -> String {
    rg_core::auth::ci_token::generate_ci_job_token(repo_id, 100, 200, scopes, "test-secret-key")
        .expect("generate ci job token")
}

async fn create_repo(base: &str, token: &str, name: &str, is_private: bool) -> i64 {
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/api/v1/repos", base))
        .bearer_auth(token)
        .json(&serde_json::json!({
            "name": name,
            "is_private": is_private
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201, "create repo {name} failed");
    resp.json::<serde_json::Value>().await.unwrap()["id"]
        .as_i64()
        .unwrap()
}

/// Read matrix on `GET /repos/{owner}/{name}/labels` (migrated caller).
#[tokio::test]
async fn read_access_matrix_labels() {
    let (base, _db) = spawn_test_app_with_db().await;
    let client = reqwest::Client::new();
    let (owner_token, _owner_id) =
        register_full(&base, "matrix_owner", "matrix_owner@example.com").await;
    let (stranger_token, _stranger_id) =
        register_full(&base, "matrix_other", "matrix_other@example.com").await;

    let pub_repo = create_repo(&base, &owner_token, "matrix-public", false).await;
    let priv_repo = create_repo(&base, &owner_token, "matrix-private", true).await;

    let labels = |owner: &str, name: &str| format!("{base}/api/v1/repos/{owner}/{name}/labels");

    // public repo: anonymous and stranger both pass
    assert_eq!(
        client
            .get(labels("matrix_owner", "matrix-public"))
            .send()
            .await
            .unwrap()
            .status(),
        200,
        "public repo must be anonymously readable"
    );
    assert_eq!(
        client
            .get(labels("matrix_owner", "matrix-public"))
            .bearer_auth(&stranger_token)
            .send()
            .await
            .unwrap()
            .status(),
        200
    );

    // private repo: anonymous → 401 (auth required), stranger → 403, owner → 200
    assert_eq!(
        client
            .get(labels("matrix_owner", "matrix-private"))
            .send()
            .await
            .unwrap()
            .status(),
        401,
        "anonymous private read must be 401"
    );
    assert_eq!(
        client
            .get(labels("matrix_owner", "matrix-private"))
            .bearer_auth(&stranger_token)
            .send()
            .await
            .unwrap()
            .status(),
        403,
        "authenticated stranger private read must be 403"
    );
    assert_eq!(
        client
            .get(labels("matrix_owner", "matrix-private"))
            .bearer_auth(&owner_token)
            .send()
            .await
            .unwrap()
            .status(),
        200
    );

    // CI job token with repo:read grants anonymous-CI read on private repo
    assert_eq!(
        client
            .get(labels("matrix_owner", "matrix-private"))
            .bearer_auth(ci_token(priv_repo, "repo:read"))
            .send()
            .await
            .unwrap()
            .status(),
        200,
        "CI job token with repo:read must read private repo"
    );

    // CI job token with a mismatched scope is NOT a repo reader
    assert_eq!(
        client
            .get(labels("matrix_owner", "matrix-private"))
            .bearer_auth(ci_token(priv_repo, "packages:read"))
            .send()
            .await
            .unwrap()
            .status(),
        401,
        "CI job token with wrong scope must not read repo endpoints"
    );

    // CI job token bound to another repo is rejected
    assert_eq!(
        client
            .get(labels("matrix_owner", "matrix-private"))
            .bearer_auth(ci_token(pub_repo, "repo:read"))
            .send()
            .await
            .unwrap()
            .status(),
        401,
        "CI job token bound to another repo must be rejected"
    );
}

/// Write matrix on `POST /repos/{owner}/{name}/labels` (migrated caller).
#[tokio::test]
async fn write_access_matrix_labels() {
    let (base, _db) = spawn_test_app_with_db().await;
    let client = reqwest::Client::new();
    let (owner_token, _owner_id) =
        register_full(&base, "matrix_w_owner", "matrix_w_owner@example.com").await;
    let (stranger_token, _stranger_id) =
        register_full(&base, "matrix_w_other", "matrix_w_other@example.com").await;
    create_repo(&base, &owner_token, "matrix-write", false).await;

    let url = format!("{base}/api/v1/repos/matrix_w_owner/matrix-write/labels");
    let body = serde_json::json!({"name": "bug", "color": "#ff0000"});

    assert_eq!(
        client
            .post(&url)
            .json(&body)
            .send()
            .await
            .unwrap()
            .status(),
        401,
        "anonymous write must be 401"
    );
    assert_eq!(
        client
            .post(&url)
            .bearer_auth(&stranger_token)
            .json(&body)
            .send()
            .await
            .unwrap()
            .status(),
        403,
        "stranger write must be 403"
    );
    let owner_resp = client
        .post(&url)
        .bearer_auth(&owner_token)
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(owner_resp.status(), 201, "owner write must succeed");
}

/// Regression guard for the Q1.2 migrated callers: wiki / boards /
/// collaborators keep the same authorization semantics.
#[tokio::test]
async fn migrated_read_endpoints_keep_private_semantics() {
    let (base, _db) = spawn_test_app_with_db().await;
    let client = reqwest::Client::new();
    let (owner_token, _owner_id) =
        register_full(&base, "mig_owner", "mig_owner@example.com").await;
    create_repo(&base, &owner_token, "mig-private", true).await;

    for path in ["wiki", "boards", "collaborators"] {
        let url = format!("{base}/api/v1/repos/mig_owner/mig-private/{path}");
        assert_eq!(
            client.get(&url).send().await.unwrap().status(),
            401,
            "anonymous GET /{path} on private repo must be 401"
        );
        assert_eq!(
            client
                .get(&url)
                .bearer_auth(&owner_token)
                .send()
                .await
                .unwrap()
                .status(),
            200,
            "owner GET /{path} must succeed"
        );
    }
}

/// Package domain scope isolation (Q1.3): a repo:read CI token must not
/// grant packages:read access, and vice versa.
#[tokio::test]
async fn ci_scopes_are_domain_isolated() {
    let (base, _db) = spawn_test_app_with_db().await;
    let client = reqwest::Client::new();
    let (owner_token, _owner_id) =
        register_full(&base, "iso_owner", "iso_owner@example.com").await;
    let repo_id = create_repo(&base, &owner_token, "iso-private", true).await;

    let packages_list = format!("{base}/api/v1/repos/iso_owner/iso-private/packages/generic/list");

    // publish one package so the list endpoint has data
    let publish = client
        .post(format!(
            "{base}/api/v1/repos/iso_owner/iso-private/packages/generic/publish?name=sample&version=1.0.0"
        ))
        .bearer_auth(&owner_token)
        .header(
            reqwest::header::CONTENT_DISPOSITION,
            "attachment; filename=\"sample.bin\"",
        )
        .body("package-bytes")
        .send()
        .await
        .unwrap();
    assert_eq!(publish.status(), 201);

    assert_eq!(
        client
            .get(&packages_list)
            .bearer_auth(ci_token(repo_id, "packages:read"))
            .send()
            .await
            .unwrap()
            .status(),
        200,
        "packages:read CI token must list private packages"
    );
    assert_eq!(
        client
            .get(&packages_list)
            .bearer_auth(ci_token(repo_id, "repo:read"))
            .send()
            .await
            .unwrap()
            .status(),
        401,
        "repo:read CI token must NOT list private packages"
    );
}
