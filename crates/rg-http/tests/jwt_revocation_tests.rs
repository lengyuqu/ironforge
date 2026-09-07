//! #5: JWT session revocation via `users.token_version`.
//!
//! Bumping the version (password reset, MFA change, admin deactivation)
//! invalidates every JWT minted before the bump. The session_guard middleware
//! strips revoked credentials instead of hard-rejecting, so auth-free
//! endpoints (login, register) stay reachable for browsers holding a stale
//! cookie. Deactivated accounts additionally lose PAT-minted sessions.

mod common;
use common::{register_full, spawn_test_app_with_db};

/// Bumping token_version rejects previously issued Bearer tokens, and a
/// fresh login mints a working token carrying the new version.
#[tokio::test]
async fn session_rejected_after_version_bump_and_relogin_works() {
    let (base, db) = spawn_test_app_with_db().await;
    let (token, user_id) = register_full(&base, "revoke1", "revoke1@example.com").await;
    let client = reqwest::Client::new();

    // Sanity: the session works before revocation.
    let resp = client
        .get(format!("{}/api/v1/users/me", &base))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    rg_db::ops::user_ops::bump_token_version(&db, user_id)
        .await
        .expect("bump token version");

    // The old token is now revoked.
    let resp = client
        .get(format!("{}/api/v1/users/me", &base))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);

    // Re-login mints a token with the current version.
    let resp = client
        .post(format!("{}/api/v1/users/login", &base))
        .json(&serde_json::json!({"login": "revoke1", "password": "Qz7$wRtm"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let fresh = body["token"].as_str().unwrap();
    let resp = client
        .get(format!("{}/api/v1/users/me", &base))
        .bearer_auth(fresh)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
}

/// A revoked cookie session is stripped (not hard-rejected): authenticated
/// handlers return their normal 401, the stale cookie is cleared, and the
/// login endpoint itself remains usable with the stale cookie still attached.
#[tokio::test]
async fn revoked_cookie_is_stripped_and_cleared() {
    let (base, db) = spawn_test_app_with_db().await;
    let (token, user_id) = register_full(&base, "revoke2", "revoke2@example.com").await;
    rg_db::ops::user_ops::bump_token_version(&db, user_id)
        .await
        .expect("bump token version");
    let client = reqwest::Client::new();

    // Authenticated handler: stripped credentials → 401.
    let resp = client
        .get(format!("{}/api/v1/users/me", &base))
        .header("cookie", format!("ironforge_token={token}"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);

    // The response clears the stale auth cookie.
    let set_cookie = resp
        .headers()
        .get("set-cookie")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();
    assert!(
        set_cookie.starts_with("ironforge_token=;"),
        "unexpected clear cookie: {set_cookie}"
    );
    assert!(set_cookie.contains("Max-Age=0"), "cookie not expired: {set_cookie}");

    // Auth-free endpoint still reachable with the stale cookie attached —
    // the user can re-authenticate from the same browser session.
    let resp = client
        .post(format!("{}/api/v1/users/login", &base))
        .header("cookie", format!("ironforge_token={token}"))
        .json(&serde_json::json!({"login": "revoke2", "password": "Qz7$wRtm"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
}

/// Admin deactivation revokes the target's session JWT and blocks their PATs
/// from minting new sessions.
#[tokio::test]
async fn deactivation_revokes_session_and_pat() {
    let (base, db) = spawn_test_app_with_db().await;
    let client = reqwest::Client::new();

    let (admin_token, admin_id) = register_full(&base, "revokeadm", "revokeadm@example.com").await;
    let (user_token, user_id) = register_full(&base, "revoketgt", "revoketgt@example.com").await;
    rg_db::ops::user_ops::update_by_id(&db, admin_id, None, None, Some(true), None)
        .await
        .expect("promote admin");

    // Mint a PAT for the target user while the account is still active.
    let pat = client
        .post(format!("{}/api/v1/users/tokens", &base))
        .bearer_auth(&user_token)
        .json(&serde_json::json!({"name": "cli", "scopes": "user, repo"}))
        .send()
        .await
        .unwrap();
    assert_eq!(pat.status(), 201);
    let pat_value = pat.json::<serde_json::Value>().await.unwrap()["token"]
        .as_str()
        .unwrap()
        .to_string();

    // PAT works before deactivation.
    let resp = client
        .get(format!("{}/api/v1/users/me", &base))
        .bearer_auth(&pat_value)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    // Admin deactivates the account.
    let resp = client
        .patch(format!("{}/api/v1/admin/users/{user_id}", &base))
        .bearer_auth(&admin_token)
        .json(&serde_json::json!({"is_active": false}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    // Session JWT issued before deactivation is revoked...
    let resp = client
        .get(format!("{}/api/v1/users/me", &base))
        .bearer_auth(&user_token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);

    // ...and the PAT can no longer mint a session either.
    let resp = client
        .get(format!("{}/api/v1/users/me", &base))
        .bearer_auth(&pat_value)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);
}
