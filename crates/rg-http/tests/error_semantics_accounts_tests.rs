//! Error-semantics regression coverage for the accounts / integrations
//! handlers (users, admin, webhooks, mirrors, imports, SSH keys, deploy keys,
//! artifacts, CI).
//!
//! The contract pinned here:
//! * caller mistakes keep their 4xx class (400 invalid input, 403 forbidden,
//!   404 missing, 409 conflict);
//! * real internal failures (storage errors, git/IO failures) are a 500 whose
//!   body is the generic "Internal server error" and never contains the cause
//!   (table names, SQL/driver text, filesystem paths, resolved addresses).

mod common;

use common::{register_full, spawn_test_app, spawn_test_app_with_db};
use sea_orm::ConnectionTrait;
use serde_json::{json, Value};

const VALID_KEY: &str =
    "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA test@ironforge";
const OTHER_KEY: &str =
    "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAICAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA other";

fn client() -> reqwest::Client {
    reqwest::Client::new()
}

async fn body_of(resp: reqwest::Response) -> (u16, Value) {
    let status = resp.status().as_u16();
    let text = resp.text().await.unwrap();
    let json = serde_json::from_str(&text).unwrap_or(Value::String(text));
    (status, json)
}

fn message(body: &Value) -> String {
    body["error"]["message"].as_str().unwrap_or("").to_string()
}

/// Assert that a response is the sanitised internal-error shape: a 500 with the
/// generic message and none of the usual implementation details.
fn assert_sanitised_500(status: u16, body: &Value) {
    assert_eq!(status, 500, "expected a 500, got {status}: {body}");
    assert_eq!(body["error"]["code"], "INTERNAL_ERROR", "{body}");
    assert_eq!(message(body), "Internal server error", "{body}");
    let raw = body.to_string().to_ascii_lowercase();
    for needle in [
        "no such table",
        "sqlite",
        "sql",
        "sea_orm",
        "seaorm",
        "db:",
        "/var/",
        "/private/",
        "/tmp/",
        ".db",
        "rename",
    ] {
        assert!(!raw.contains(needle), "leaked `{needle}` in {body}");
    }
}

/// Make every query against `table` fail like a broken schema would.
async fn break_table(db: &rg_db::DatabaseConnection, table: &str) {
    db.execute_unprepared(&format!("ALTER TABLE {table} RENAME TO {table}_broken"))
        .await
        .unwrap();
}

async fn promote(db: &rg_db::DatabaseConnection, user_id: i64) {
    rg_db::ops::user_ops::update_by_id(db, user_id, None, None, Some(true), None)
        .await
        .unwrap();
}

async fn create_public_repo(base: &str, token: &str, name: &str) {
    let resp = client()
        .post(format!("{base}/api/v1/repos"))
        .bearer_auth(token)
        .json(&json!({"name": name}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);
}

// ── users: register / login / password reset / tokens ────────────────────

#[tokio::test]
async fn register_classifies_validation_and_duplicates() {
    let base = spawn_test_app().await;
    let c = client();
    let url = format!("{base}/api/v1/users/register");

    let short = c
        .post(&url)
        .json(&json!({"username": "ab", "email": "ab@example.com", "password": "Qz7$wRtm"}))
        .send()
        .await
        .unwrap();
    let (status, body) = body_of(short).await;
    assert_eq!(status, 400, "{body}");
    assert!(message(&body).contains("username"), "{body}");

    let weak = c
        .post(&url)
        .json(&json!({"username": "weakpw", "email": "weak@example.com", "password": "abc"}))
        .send()
        .await
        .unwrap();
    assert_eq!(weak.status(), 400);

    let bad_email = c
        .post(&url)
        .json(&json!({"username": "bademail", "email": "no-at-sign", "password": "Qz7$wRtm"}))
        .send()
        .await
        .unwrap();
    assert_eq!(bad_email.status(), 400);

    let ok = c
        .post(&url)
        .json(&json!({"username": "firstuser", "email": "first@example.com", "password": "Qz7$wRtm"}))
        .send()
        .await
        .unwrap();
    assert_eq!(ok.status(), 201);

    let dup_name = c
        .post(&url)
        .json(&json!({"username": "firstuser", "email": "other@example.com", "password": "Qz7$wRtm"}))
        .send()
        .await
        .unwrap();
    let (status, body) = body_of(dup_name).await;
    assert_eq!(status, 409, "duplicate username is a conflict: {body}");
    assert_eq!(body["error"]["code"], "CONFLICT");

    let dup_email = c
        .post(&url)
        .json(&json!({"username": "seconduser", "email": "first@example.com", "password": "Qz7$wRtm"}))
        .send()
        .await
        .unwrap();
    assert_eq!(dup_email.status(), 409);
}

#[tokio::test]
async fn login_keeps_credential_failures_indistinguishable_and_sanitises_storage_errors() {
    let (base, db) = spawn_test_app_with_db().await;
    let c = client();
    register_full(&base, "loginuser", "loginuser@example.com").await;
    let url = format!("{base}/api/v1/users/login");

    // Wrong password and unknown account answer identically.
    let wrong = c
        .post(&url)
        .json(&json!({"login": "loginuser", "password": "Wrong#pass1"}))
        .send()
        .await
        .unwrap();
    let (wrong_status, wrong_body) = body_of(wrong).await;
    let unknown = c
        .post(&url)
        .json(&json!({"login": "nobody-here", "password": "Wrong#pass1"}))
        .send()
        .await
        .unwrap();
    let (unknown_status, unknown_body) = body_of(unknown).await;
    assert_eq!(wrong_status, 401);
    assert_eq!(unknown_status, 401);
    assert_eq!(message(&wrong_body), "invalid credentials");
    assert_eq!(message(&unknown_body), "invalid credentials");

    // A storage failure is not "invalid credentials".
    break_table(&db, "users").await;
    let broken = c
        .post(&url)
        .json(&json!({"login": "loginuser", "password": "Qz7$wRtm"}))
        .send()
        .await
        .unwrap();
    let (status, body) = body_of(broken).await;
    assert_sanitised_500(status, &body);
}

#[tokio::test]
async fn reset_password_classifies_token_and_password_errors() {
    use sha2::Digest;
    let (base, db) = spawn_test_app_with_db().await;
    let c = client();
    let (_, user_id) = register_full(&base, "resetuser", "resetuser@example.com").await;
    let url = format!("{base}/api/v1/users/reset-password");

    let bogus = c
        .post(&url)
        .json(&json!({"token": "does-not-exist", "new_password": "Zx9#plmQ"}))
        .send()
        .await
        .unwrap();
    let (status, body) = body_of(bogus).await;
    assert_eq!(status, 400);
    assert_eq!(message(&body), "invalid or expired reset token");

    let raw_token = "reset-token-for-test";
    let hash = hex::encode(sha2::Sha256::digest(raw_token.as_bytes()));
    rg_db::ops::password_reset_token_ops::create(
        &db,
        user_id,
        &hash,
        chrono::Utc::now() + chrono::Duration::minutes(10),
    )
    .await
    .unwrap();

    let weak = c
        .post(&url)
        .json(&json!({"token": raw_token, "new_password": "abc"}))
        .send()
        .await
        .unwrap();
    assert_eq!(weak.status(), 400, "weak replacement password is invalid input");

    let ok = c
        .post(&url)
        .json(&json!({"token": raw_token, "new_password": "Zx9#plmQ"}))
        .send()
        .await
        .unwrap();
    assert_eq!(ok.status(), 200);
}

#[tokio::test]
async fn personal_access_tokens_classify_errors() {
    let (base, db) = spawn_test_app_with_db().await;
    let c = client();
    let (owner, _) = register_full(&base, "pat_owner", "pat_owner@example.com").await;
    let (other, _) = register_full(&base, "pat_other", "pat_other@example.com").await;

    let bad_scope = c
        .post(format!("{base}/api/v1/users/tokens"))
        .bearer_auth(&owner)
        .json(&json!({"name": "t", "scopes": "delete_everything"}))
        .send()
        .await
        .unwrap();
    let (status, body) = body_of(bad_scope).await;
    assert_eq!(status, 400);
    assert!(message(&body).contains("unsupported token scope"), "{body}");

    let created = c
        .post(format!("{base}/api/v1/users/tokens"))
        .bearer_auth(&owner)
        .json(&json!({"name": "ci"}))
        .send()
        .await
        .unwrap();
    let (status, created) = body_of(created).await;
    assert_eq!(status, 201);
    let id = created["id"].as_i64().unwrap();

    let forbidden = c
        .delete(format!("{base}/api/v1/users/tokens/{id}"))
        .bearer_auth(&other)
        .send()
        .await
        .unwrap();
    assert_eq!(forbidden.status(), 403);
    let missing = c
        .delete(format!("{base}/api/v1/users/tokens/99999"))
        .bearer_auth(&owner)
        .send()
        .await
        .unwrap();
    assert_eq!(missing.status(), 404);

    break_table(&db, "access_tokens").await;
    let list = c
        .get(format!("{base}/api/v1/users/tokens"))
        .bearer_auth(&owner)
        .send()
        .await
        .unwrap();
    let (status, body) = body_of(list).await;
    assert_sanitised_500(status, &body);
    // A storage failure while looking the token up is not "token not found".
    let del = c
        .delete(format!("{base}/api/v1/users/tokens/{id}"))
        .bearer_auth(&owner)
        .send()
        .await
        .unwrap();
    let (status, body) = body_of(del).await;
    assert_sanitised_500(status, &body);
}

// ── admin ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn admin_user_management_classifies_errors() {
    let (base, db) = spawn_test_app_with_db().await;
    let c = client();
    let (admin, admin_id) = register_full(&base, "adm_root", "adm_root@example.com").await;
    let (plain, _) = register_full(&base, "adm_plain", "adm_plain@example.com").await;
    promote(&db, admin_id).await;

    // Non-admin is forbidden.
    let denied = c
        .patch(format!("{base}/api/v1/admin/users/{admin_id}"))
        .bearer_auth(&plain)
        .json(&json!({"bio": "x"}))
        .send()
        .await
        .unwrap();
    assert_eq!(denied.status(), 403);

    // Unknown target: 404 (was a 400 / 500 depending on the endpoint).
    let patch_missing = c
        .patch(format!("{base}/api/v1/admin/users/424242"))
        .bearer_auth(&admin)
        .json(&json!({"bio": "x"}))
        .send()
        .await
        .unwrap();
    let (status, body) = body_of(patch_missing).await;
    assert_eq!(status, 404, "{body}");
    let delete_missing = c
        .delete(format!("{base}/api/v1/admin/users/424242"))
        .bearer_auth(&admin)
        .send()
        .await
        .unwrap();
    assert_eq!(delete_missing.status(), 404);

    // Caller mistake: deleting yourself stays a 400.
    let self_delete = c
        .delete(format!("{base}/api/v1/admin/users/{admin_id}"))
        .bearer_auth(&admin)
        .send()
        .await
        .unwrap();
    assert_eq!(self_delete.status(), 400);
}

#[tokio::test]
async fn admin_can_delete_an_org_whose_owner_id_differs_from_the_org_id() {
    let (base, db) = spawn_test_app_with_db().await;
    let c = client();
    let (admin, admin_id) = register_full(&base, "org_adm", "org_adm@example.com").await;
    let (owner, owner_id) = register_full(&base, "org_own", "org_own@example.com").await;
    promote(&db, admin_id).await;
    assert_ne!(admin_id, owner_id);

    let created = c
        .post(format!("{base}/api/v1/orgs"))
        .bearer_auth(&owner)
        .json(&json!({"name": "mismatch-org"}))
        .send()
        .await
        .unwrap();
    let (status, org) = body_of(created).await;
    assert_eq!(status, 201);
    // Org ids and user ids come from separate sequences; make sure the two
    // differ, otherwise the old id mix-up would be invisible.
    assert_ne!(org["id"].as_i64().unwrap(), owner_id, "{org}");

    let deleted = c
        .delete(format!("{base}/api/v1/admin/orgs/mismatch-org"))
        .bearer_auth(&admin)
        .send()
        .await
        .unwrap();
    let (status, body) = body_of(deleted).await;
    assert_eq!(status, 200, "admin delete must succeed for any org: {body}");

    let missing = c
        .delete(format!("{base}/api/v1/admin/orgs/mismatch-org"))
        .bearer_auth(&admin)
        .send()
        .await
        .unwrap();
    assert_eq!(missing.status(), 404);
}

#[tokio::test]
async fn admin_sso_providers_classify_errors() {
    let (base, db) = spawn_test_app_with_db().await;
    let c = client();
    let (admin, admin_id) = register_full(&base, "sso_root", "sso_root@example.com").await;
    promote(&db, admin_id).await;
    let url = format!("{base}/api/v1/admin/sso/providers");

    let missing = c
        .get(format!("{url}/9999"))
        .bearer_auth(&admin)
        .send()
        .await
        .unwrap();
    assert_eq!(missing.status(), 404);

    let invalid = c
        .post(&url)
        .bearer_auth(&admin)
        .json(&json!({
            "name": "Dir", "slug": "dir", "provider_type": "ldap",
            "ldap_port": 70000, "enabled": false
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(invalid.status(), 400);

    let provider = json!({"name": "Corp", "slug": "corp", "provider_type": "oidc", "enabled": false});
    let created = c
        .post(&url)
        .bearer_auth(&admin)
        .json(&provider)
        .send()
        .await
        .unwrap();
    assert_eq!(created.status(), 201);
    let duplicate = c
        .post(&url)
        .bearer_auth(&admin)
        .json(&provider)
        .send()
        .await
        .unwrap();
    let (status, body) = body_of(duplicate).await;
    assert_eq!(status, 409, "duplicate slug is a conflict: {body}");
    assert!(!message(&body).to_ascii_lowercase().contains("unique"), "{body}");

    break_table(&db, "sso_providers").await;
    let broken = c
        .get(&url)
        .bearer_auth(&admin)
        .send()
        .await
        .unwrap();
    let (status, body) = body_of(broken).await;
    assert_sanitised_500(status, &body);
}

// ── webhooks ─────────────────────────────────────────────────────────────

#[tokio::test]
async fn webhooks_classify_errors() {
    let (base, db) = spawn_test_app_with_db().await;
    let c = client();
    let (owner, _) = register_full(&base, "hook_owner", "hook_owner@example.com").await;
    let (other, _) = register_full(&base, "hook_other", "hook_other@example.com").await;
    create_public_repo(&base, &owner, "hooked").await;
    let hooks = format!("{base}/api/v1/repos/hook_owner/hooked/hooks");

    // forbidden for a non-writer, 401 without credentials
    let denied = c
        .post(&hooks)
        .bearer_auth(&other)
        .json(&json!({"url": "http://127.0.0.1:9/hook", "events": ["push"]}))
        .send()
        .await
        .unwrap();
    assert_eq!(denied.status(), 403);

    // invalid input: empty events, unparsable URL
    let no_events = c
        .post(&hooks)
        .bearer_auth(&owner)
        .json(&json!({"url": "http://127.0.0.1:9/hook", "events": []}))
        .send()
        .await
        .unwrap();
    assert_eq!(no_events.status(), 400);
    let bad_url = c
        .post(&hooks)
        .bearer_auth(&owner)
        .json(&json!({"url": "http://[::1/hook", "events": ["push"]}))
        .send()
        .await
        .unwrap();
    let (status, body) = body_of(bad_url).await;
    assert_eq!(status, 400, "{body}");
    assert!(message(&body).starts_with("invalid webhook URL"), "{body}");

    let created = c
        .post(&hooks)
        .bearer_auth(&owner)
        .json(&json!({"url": "http://127.0.0.1:9/hook", "events": ["push"]}))
        .send()
        .await
        .unwrap();
    let (status, hook) = body_of(created).await;
    assert_eq!(status, 201, "{hook}");
    let id = hook["id"].as_i64().unwrap();

    // missing resources are 404
    for resp in [
        c.get(format!("{hooks}/99999")).bearer_auth(&owner).send().await.unwrap(),
        c.delete(format!("{hooks}/99999")).bearer_auth(&owner).send().await.unwrap(),
        c.patch(format!("{hooks}/99999"))
            .bearer_auth(&owner)
            .json(&json!({"active": false}))
            .send()
            .await
            .unwrap(),
        c.post(format!("{hooks}/{id}/deliveries/99999/redeliver"))
            .bearer_auth(&owner)
            .send()
            .await
            .unwrap(),
    ] {
        assert_eq!(resp.status(), 404);
    }

    // updating to an invalid URL is a caller error
    let bad_update = c
        .patch(format!("{hooks}/{id}"))
        .bearer_auth(&owner)
        .json(&json!({"url": "http://[::1/hook"}))
        .send()
        .await
        .unwrap();
    assert_eq!(bad_update.status(), 400);

    // storage failure => sanitised 500, not a 400 that echoes the cause
    break_table(&db, "webhooks").await;
    let list = c.get(&hooks).bearer_auth(&owner).send().await.unwrap();
    let (status, body) = body_of(list).await;
    assert_sanitised_500(status, &body);
    let create = c
        .post(&hooks)
        .bearer_auth(&owner)
        .json(&json!({"url": "http://127.0.0.1:9/hook", "events": ["push"]}))
        .send()
        .await
        .unwrap();
    let (status, body) = body_of(create).await;
    assert_sanitised_500(status, &body);
}

// ── mirrors ──────────────────────────────────────────────────────────────

#[tokio::test]
async fn mirrors_classify_errors() {
    let (base, db) = spawn_test_app_with_db().await;
    let c = client();
    let (owner, _) = register_full(&base, "mir_owner", "mir_owner@example.com").await;
    let (other, _) = register_full(&base, "mir_other", "mir_other@example.com").await;
    create_public_repo(&base, &owner, "mirrored").await;
    let url = format!("{base}/api/v1/repos/mir_owner/mirrored/mirror");
    let good = json!({"url": "http://127.0.0.1:9/remote.git", "sync_interval_seconds": 3600});

    assert_eq!(c.post(&url).json(&good).send().await.unwrap().status(), 401);
    assert_eq!(
        c.post(&url).bearer_auth(&other).json(&good).send().await.unwrap().status(),
        403
    );

    // missing mirror: 404 on every verb
    assert_eq!(c.get(&url).send().await.unwrap().status(), 404);
    let patch = c
        .patch(&url)
        .bearer_auth(&owner)
        .json(&json!({"status": "inactive"}))
        .send()
        .await
        .unwrap();
    assert_eq!(patch.status(), 404);
    assert_eq!(c.delete(&url).bearer_auth(&owner).send().await.unwrap().status(), 404);
    let sync = c
        .post(format!("{url}/sync"))
        .bearer_auth(&owner)
        .send()
        .await
        .unwrap();
    assert_eq!(sync.status(), 404);

    // dangerous transports are rejected as invalid input
    for bad in ["ext::sh -c id", "file:///etc/passwd", "git@host:o/r.git"] {
        let resp = c
            .post(&url)
            .bearer_auth(&owner)
            .json(&json!({"url": bad, "sync_interval_seconds": 3600}))
            .send()
            .await
            .unwrap();
        let (status, body) = body_of(resp).await;
        assert_eq!(status, 400, "{bad}: {body}");
        assert!(message(&body).starts_with("invalid mirror URL"), "{body}");
    }

    let created = c.post(&url).bearer_auth(&owner).json(&good).send().await.unwrap();
    assert_eq!(created.status(), 201);
    let duplicate = c.post(&url).bearer_auth(&owner).json(&good).send().await.unwrap();
    let (status, body) = body_of(duplicate).await;
    assert_eq!(status, 409, "{body}");

    let bad_patch = c
        .patch(&url)
        .bearer_auth(&owner)
        .json(&json!({"url": "ext::sh -c id"}))
        .send()
        .await
        .unwrap();
    assert_eq!(bad_patch.status(), 400);

    break_table(&db, "mirrors").await;
    let broken = c.get(&url).send().await.unwrap();
    let (status, body) = body_of(broken).await;
    assert_sanitised_500(status, &body);
    let broken_sync = c
        .post(format!("{url}/sync"))
        .bearer_auth(&owner)
        .send()
        .await
        .unwrap();
    let (status, body) = body_of(broken_sync).await;
    assert_sanitised_500(status, &body);
}

// ── imports ──────────────────────────────────────────────────────────────

#[tokio::test]
async fn imports_classify_errors_and_hide_internal_details() {
    let (base, db) = spawn_test_app_with_db().await;
    let c = client();
    let (owner, _) = register_full(&base, "imp_owner", "imp_owner@example.com").await;
    let (other, _) = register_full(&base, "imp_other", "imp_other@example.com").await;
    let imports = format!("{base}/api/v1/imports");

    let unauth = c
        .post(&imports)
        .json(&json!({"platform": "git", "source_url": "http://x/y", "target_owner": "imp_owner"}))
        .send()
        .await
        .unwrap();
    assert_eq!(unauth.status(), 401);
    let bad_platform = c
        .post(&imports)
        .bearer_auth(&owner)
        .json(&json!({"platform": "svn", "source_url": "http://x/y", "target_owner": "imp_owner"}))
        .send()
        .await
        .unwrap();
    assert_eq!(bad_platform.status(), 400);

    // Failures that happen in the background are stored on the task and shown
    // to its owner. Ones the user can act on keep their reason...
    let bad_url = start_import(&c, &imports, &owner, "github", "not-a-url", "imp_owner").await;
    let failed = wait_failed(&c, &imports, &owner, bad_url).await;
    assert!(
        failed["error"].as_str().unwrap_or_default().contains("invalid GitHub URL"),
        "{failed}"
    );
    let unknown_owner =
        start_import(&c, &imports, &owner, "git", "http://127.0.0.1:9/a/b.git", "no-such-owner").await;
    let failed = wait_failed(&c, &imports, &owner, unknown_owner).await;
    assert!(
        failed["error"].as_str().unwrap_or_default().contains("not found"),
        "{failed}"
    );

    // ...other users cannot see the task at all
    let foreign = c
        .get(format!("{imports}/{unknown_owner}"))
        .bearer_auth(&other)
        .send()
        .await
        .unwrap();
    assert_eq!(foreign.status(), 404);

    // ...and a storage failure in the pipeline is reduced to a generic line:
    // no table names or driver text in the task's `error` field.
    break_table(&db, "organizations").await;
    let internal =
        start_import(&c, &imports, &owner, "git", "http://127.0.0.1:9/a/b.git", "ghost-owner").await;
    let failed = wait_failed(&c, &imports, &owner, internal).await;
    let error = failed["error"].as_str().unwrap_or_default().to_string();
    assert_eq!(error, "import failed due to an internal error; see the server logs", "{failed}");

    // Creating the task itself failing is a sanitised 500.
    break_table(&db, "import_tasks").await;
    let broken = c
        .post(&imports)
        .bearer_auth(&owner)
        .json(&json!({
            "platform": "git",
            "source_url": "http://127.0.0.1:9/a/b.git",
            "target_owner": "imp_owner"
        }))
        .send()
        .await
        .unwrap();
    let (status, body) = body_of(broken).await;
    assert_sanitised_500(status, &body);
}

async fn start_import(
    c: &reqwest::Client,
    imports: &str,
    token: &str,
    platform: &str,
    source_url: &str,
    target_owner: &str,
) -> i64 {
    let resp = c
        .post(imports)
        .bearer_auth(token)
        .json(&json!({
            "platform": platform,
            "source_url": source_url,
            "target_owner": target_owner
        }))
        .send()
        .await
        .unwrap();
    let (status, task) = body_of(resp).await;
    assert_eq!(status, 201, "{task}");
    task["id"].as_i64().unwrap()
}

async fn wait_failed(c: &reqwest::Client, imports: &str, token: &str, id: i64) -> Value {
    let mut last = Value::Null;
    for _ in 0..100 {
        last = c
            .get(format!("{imports}/{id}"))
            .bearer_auth(token)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        if last["status"] == "failed" {
            return last;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    panic!("import {id} did not fail: {last}");
}

// ── SSH keys & deploy keys ───────────────────────────────────────────────

#[tokio::test]
async fn ssh_keys_classify_errors() {
    let (base, db) = spawn_test_app_with_db().await;
    let c = client();
    let (owner, _) = register_full(&base, "key_owner", "key_owner@example.com").await;
    let (other, _) = register_full(&base, "key_other", "key_other@example.com").await;
    let keys = format!("{base}/api/v1/users/ssh-keys");

    // every flavour of malformed key is the caller's mistake (400), including
    // bad base64, which used to be classified as an internal error.
    for bad in ["ssh-ed25519 !!!", "ssh-ed25519 AAAAA", "ssh-rsa AAAA", "ssh-dss AAAA", "", "ssh-ed25519"] {
        let resp = c
            .post(&keys)
            .bearer_auth(&owner)
            .json(&json!({"title": "t", "public_key": bad}))
            .send()
            .await
            .unwrap();
        let (status, body) = body_of(resp).await;
        assert_eq!(status, 400, "`{bad}`: {body}");
    }

    let created = c
        .post(&keys)
        .bearer_auth(&owner)
        .json(&json!({"title": "laptop", "public_key": VALID_KEY}))
        .send()
        .await
        .unwrap();
    let (status, key) = body_of(created).await;
    assert_eq!(status, 201, "{key}");
    let id = key["id"].as_i64().unwrap();

    let duplicate = c
        .post(&keys)
        .bearer_auth(&other)
        .json(&json!({"title": "dup", "public_key": VALID_KEY}))
        .send()
        .await
        .unwrap();
    assert_eq!(duplicate.status(), 409);

    let forbidden = c.delete(format!("{keys}/{id}")).bearer_auth(&other).send().await.unwrap();
    assert_eq!(forbidden.status(), 403);
    let missing = c.delete(format!("{keys}/99999")).bearer_auth(&owner).send().await.unwrap();
    assert_eq!(missing.status(), 404);

    break_table(&db, "ssh_keys").await;
    let list = c.get(&keys).bearer_auth(&owner).send().await.unwrap();
    let (status, body) = body_of(list).await;
    assert_sanitised_500(status, &body);
    let create = c
        .post(&keys)
        .bearer_auth(&owner)
        .json(&json!({"title": "again", "public_key": OTHER_KEY}))
        .send()
        .await
        .unwrap();
    let (status, body) = body_of(create).await;
    assert_sanitised_500(status, &body);
}

#[tokio::test]
async fn deploy_keys_classify_errors() {
    let (base, db) = spawn_test_app_with_db().await;
    let c = client();
    let (owner, _) = register_full(&base, "dk_owner", "dk_owner@example.com").await;
    let (other, _) = register_full(&base, "dk_other", "dk_other@example.com").await;
    create_public_repo(&base, &owner, "deployed").await;
    let keys = format!("{base}/api/v1/repos/dk_owner/deployed/keys");

    let denied = c
        .post(&keys)
        .bearer_auth(&other)
        .json(&json!({"title": "t", "public_key": VALID_KEY}))
        .send()
        .await
        .unwrap();
    assert_eq!(denied.status(), 403);

    let malformed = c
        .post(&keys)
        .bearer_auth(&owner)
        .json(&json!({"title": "t", "public_key": "ssh-ed25519 !!!"}))
        .send()
        .await
        .unwrap();
    assert_eq!(malformed.status(), 400);

    let created = c
        .post(&keys)
        .bearer_auth(&owner)
        .json(&json!({"title": "t", "public_key": VALID_KEY}))
        .send()
        .await
        .unwrap();
    assert_eq!(created.status(), 201);
    let duplicate = c
        .post(&keys)
        .bearer_auth(&owner)
        .json(&json!({"title": "t2", "public_key": VALID_KEY}))
        .send()
        .await
        .unwrap();
    assert_eq!(duplicate.status(), 409);
    let missing = c.delete(format!("{keys}/99999")).bearer_auth(&owner).send().await.unwrap();
    assert_eq!(missing.status(), 404);

    break_table(&db, "deploy_keys").await;
    let list = c.get(&keys).bearer_auth(&owner).send().await.unwrap();
    let (status, body) = body_of(list).await;
    assert_sanitised_500(status, &body);
}

// ── artifacts & CI ───────────────────────────────────────────────────────

#[tokio::test]
async fn artifact_and_ci_endpoints_classify_errors() {
    let (base, db) = spawn_test_app_with_db().await;
    let c = client();
    let (owner, _) = register_full(&base, "ci_err_owner", "ci_err_owner@example.com").await;
    let (other, _) = register_full(&base, "ci_err_other", "ci_err_other@example.com").await;
    create_public_repo(&base, &owner, "pipes").await;
    let pipelines = format!("{base}/api/v1/repos/ci_err_owner/pipes/pipelines");

    // missing resources
    assert_eq!(c.get(format!("{pipelines}/9999")).send().await.unwrap().status(), 404);
    assert_eq!(
        c.get(format!("{base}/api/v1/artifacts/9999")).send().await.unwrap().status(),
        404
    );
    let cancel_missing = c
        .post(format!("{pipelines}/9999/cancel"))
        .bearer_auth(&owner)
        .send()
        .await
        .unwrap();
    assert_eq!(cancel_missing.status(), 404);
    let rerun_missing = c
        .post(format!("{pipelines}/9999/jobs/1/rerun"))
        .bearer_auth(&owner)
        .send()
        .await
        .unwrap();
    assert_eq!(rerun_missing.status(), 404);

    // forbidden for a non-writer
    let trigger_denied = c
        .post(&pipelines)
        .bearer_auth(&other)
        .json(&json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(trigger_denied.status(), 403);

    // an empty repository has no ref/CI config: caller error, not a 500
    let trigger_empty = c
        .post(&pipelines)
        .bearer_auth(&owner)
        .json(&json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(trigger_empty.status(), 400);

    // storage failure => sanitised 500
    break_table(&db, "pipelines").await;
    let list = c.get(&pipelines).send().await.unwrap();
    let (status, body) = body_of(list).await;
    assert_sanitised_500(status, &body);
}
