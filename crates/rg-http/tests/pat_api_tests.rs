//! Verifies that a Personal Access Token authenticates REST API calls, not
//! just git-over-HTTP. The API handlers validate JWTs; a middleware translates
//! a PAT into an equivalent Bearer JWT so PAT-based API access works.

mod common;
use common::{register_user, spawn_test_app};

use base64::Engine as _;

async fn create_pat(base: &str, jwt: &str) -> String {
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/api/v1/users/tokens", base))
        .bearer_auth(jwt)
        .json(&serde_json::json!({ "name": "api-cli" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201, "create token failed");
    let body: serde_json::Value = resp.json().await.unwrap();
    body["token"].as_str().unwrap().to_string()
}

/// Creating a repo requires auth; do it with a PAT as a Bearer token.
#[tokio::test]
async fn pat_authenticates_api_via_bearer() {
    let base = spawn_test_app().await;
    let jwt = register_user(&base, "patapi1", "patapi1@example.com", "Qz7$wRtm").await;
    let pat = create_pat(&base, &jwt).await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/api/v1/repos", base))
        .bearer_auth(&pat) // PAT, not JWT
        .json(&serde_json::json!({ "name": "via-pat" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201, "PAT should authenticate repo creation");
}

/// Same, but the PAT presented via HTTP Basic auth (`user:token`).
#[tokio::test]
async fn pat_authenticates_api_via_basic() {
    let base = spawn_test_app().await;
    let jwt = register_user(&base, "patapi2", "patapi2@example.com", "Qz7$wRtm").await;
    let pat = create_pat(&base, &jwt).await;

    let client = reqwest::Client::new();
    let basic = base64::engine::general_purpose::STANDARD.encode(format!("patapi2:{}", pat));
    let resp = client
        .post(format!("{}/api/v1/repos", base))
        .header("Authorization", format!("Basic {}", basic))
        .json(&serde_json::json!({ "name": "via-basic" }))
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        201,
        "PAT via Basic auth should authenticate the API"
    );
}

/// An invalid/garbage token must NOT authenticate.
#[tokio::test]
async fn invalid_token_is_rejected_by_api() {
    let base = spawn_test_app().await;
    let _ = register_user(&base, "patapi3", "patapi3@example.com", "Qz7$wRtm").await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/api/v1/repos", base))
        .bearer_auth("not-a-real-token")
        .json(&serde_json::json!({ "name": "nope" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401, "garbage token must be rejected");
}
