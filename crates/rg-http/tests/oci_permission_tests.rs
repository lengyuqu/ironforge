mod common;

use base64::Engine as _;
use common::{register_full, spawn_test_app_with_db};

async fn create_repo(base: &str, token: &str, name: &str, is_private: bool) {
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
    assert_eq!(resp.status(), 201, "create repo failed");
}

fn basic_auth(username: &str, password: &str) -> String {
    let raw = format!("{}:{}", username, password);
    format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode(raw)
    )
}

async fn request_oci_token(
    base: &str,
    scope: &str,
    auth_header: Option<String>,
) -> rg_core::auth::oci_token::OciTokenClaims {
    let token = request_oci_token_raw(base, scope, auth_header).await;
    rg_core::auth::oci_token::validate_oci_token(&token, "test-secret-key")
        .expect("valid OCI token")
}

/// Same as [`request_oci_token`] but returns the serialized JWT, for tests that
/// need to replay it against `/v2/...`.
async fn request_oci_token_raw(base: &str, scope: &str, auth_header: Option<String>) -> String {
    let client = reqwest::Client::new();
    let mut req = client
        .get(format!("{}/v2/auth/token", base))
        .query(&[("service", "ironforge-registry"), ("scope", scope)]);
    if let Some(auth) = auth_header {
        req = req.header(reqwest::header::AUTHORIZATION, auth);
    }

    let resp = req.send().await.unwrap();
    assert_eq!(resp.status(), 200, "token request failed");
    let body: serde_json::Value = resp.json().await.unwrap();
    body["token"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn private_oci_tags_require_read_access() {
    let (base, _db) = spawn_test_app_with_db().await;
    let client = reqwest::Client::new();
    let (owner_token, _owner_id) = register_full(&base, "oci_owner", "oci_owner@example.com").await;
    let (other_token, _other_id) = register_full(&base, "oci_other", "oci_other@example.com").await;
    create_repo(&base, &owner_token, "private-oci", true).await;

    let anon_resp = client
        .get(format!("{}/v2/oci_owner/private-oci/tags/list", base))
        .send()
        .await
        .unwrap();
    assert_eq!(anon_resp.status(), 401);

    let other_resp = client
        .get(format!("{}/v2/oci_owner/private-oci/tags/list", base))
        .bearer_auth(&other_token)
        .send()
        .await
        .unwrap();
    assert_eq!(other_resp.status(), 401);

    let owner_resp = client
        .get(format!("{}/v2/oci_owner/private-oci/tags/list", base))
        .bearer_auth(&owner_token)
        .send()
        .await
        .unwrap();
    assert_eq!(owner_resp.status(), 404);
}

#[tokio::test]
async fn oci_token_endpoint_grants_only_authorized_scopes() {
    let (base, _db) = spawn_test_app_with_db().await;
    let (owner_token, _owner_id) =
        register_full(&base, "oci_token_owner", "oci_token_owner@example.com").await;
    let (_other_token, _other_id) =
        register_full(&base, "oci_token_other", "oci_token_other@example.com").await;
    create_repo(&base, &owner_token, "public-image", false).await;
    create_repo(&base, &owner_token, "private-image", true).await;

    let public_pull =
        request_oci_token(&base, "repository:oci_token_owner/public-image:pull", None).await;
    assert_eq!(
        public_pull.scope.as_deref(),
        Some("repository:oci_token_owner/public-image:pull")
    );

    let private_anon =
        request_oci_token(&base, "repository:oci_token_owner/private-image:pull", None).await;
    assert!(private_anon.scope.is_none());

    let owner_push = request_oci_token(
        &base,
        "repository:oci_token_owner/private-image:pull,push",
        Some(basic_auth("oci_token_owner", "Qz7$wRtm")),
    )
    .await;
    assert_eq!(
        owner_push.scope.as_deref(),
        Some("repository:oci_token_owner/private-image:pull,push")
    );

    let other_push = request_oci_token(
        &base,
        "repository:oci_token_owner/private-image:pull,push",
        Some(basic_auth("oci_token_other", "Qz7$wRtm")),
    )
    .await;
    assert!(other_push.scope.is_none());
}

/// A token minted while the repo was public must stop working once the repo
/// turns private: the granted scope was computed against the *old* visibility.
///
/// This is the regression that motivated carrying `uid` in the token — the
/// verifier now re-checks the current row instead of trusting the signature.
#[tokio::test]
async fn oci_token_goes_stale_when_repo_turns_private() {
    use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};

    let (base, db) = spawn_test_app_with_db().await;
    let (owner_token, _owner_id) =
        register_full(&base, "oci_flip_owner", "oci_flip_owner@example.com").await;
    create_repo(&base, &owner_token, "flip-image", false).await;

    let scope = "repository:oci_flip_owner/flip-image:pull";
    let raw = request_oci_token_raw(&base, scope, None).await;
    let client = reqwest::Client::new();
    let tags_url = format!("{}/v2/oci_flip_owner/flip-image/tags/list", base);

    let before = client
        .get(&tags_url)
        .bearer_auth(&raw)
        .send()
        .await
        .unwrap();
    // 404 = the image does not exist yet, which means authz *passed*. A 401
    // here would mean the request never got past the token check.
    assert_ne!(
        before.status(),
        401,
        "a public repo must pass authz with a token for it"
    );

    // Flip visibility directly in the DB: the PATCH endpoint is a
    // full-settings form guarded by an optimistic-lock version, which is a
    // different concern from what this test is about.
    let repo = rg_db::entities::repository::Entity::find()
        .filter(rg_db::entities::repository::Column::Name.eq("flip-image"))
        .one(&db)
        .await
        .unwrap()
        .expect("repo row must exist");
    let mut active: rg_db::entities::repository::ActiveModel = repo.into();
    active.is_private = Set(true);
    active.update(&db).await.unwrap();

    let after = client
        .get(&tags_url)
        .bearer_auth(&raw)
        .send()
        .await
        .unwrap();
    assert_eq!(
        after.status(),
        401,
        "a token minted while the repo was public must not survive it turning private"
    );
}

/// The re-check must not break legitimate private-repo access. This is the case
/// an unconditional `can_read_repo(.., None)` would have broken: it asks "is the
/// repo public?", so every authenticated docker push would 403.
#[tokio::test]
async fn oci_token_still_works_for_private_repo_with_access() {
    let (base, _db) = spawn_test_app_with_db().await;
    let (owner_token, _owner_id) =
        register_full(&base, "oci_priv_owner", "oci_priv_owner@example.com").await;
    create_repo(&base, &owner_token, "private-image", true).await;

    let scope = "repository:oci_priv_owner/private-image:pull,push";
    let raw =
        request_oci_token_raw(&base, scope, Some(basic_auth("oci_priv_owner", "Qz7$wRtm"))).await;

    let client = reqwest::Client::new();
    let resp = client
        .get(format!(
            "{}/v2/oci_priv_owner/private-image/tags/list",
            base
        ))
        .bearer_auth(&raw)
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        404,
        "authz must pass for the owner (404 = authorized, image simply absent)"
    );
}
