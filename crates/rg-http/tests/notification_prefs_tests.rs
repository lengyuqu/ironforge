//! `GET`/`PUT /api/v1/user/notification-preferences` (#19 step 5, T12).
//!
//! The endpoint is the settings page's only view of the preference table. The
//! semantics under test are the whole point of the feature:
//!
//! * a fresh account has **no** rows and every channel is on (the GET derives
//!   the default instead of initialising rows);
//! * a mute is stored as an explicit row and shows up as `enabled = false`
//!   with `explicit = true`;
//! * a repository override and the global default coexist and are reported
//!   separately;
//! * unknown channels and unknown repositories are a 400, not a 500.

mod common;

use common::{create_repo, register_full, spawn_test_app_with_db};

use reqwest::StatusCode;

async fn get_prefs(
    client: &reqwest::Client,
    base: &str,
    token: &str,
) -> (StatusCode, serde_json::Value) {
    let resp = client
        .get(format!("{base}/api/v1/user/notification-preferences"))
        .bearer_auth(token)
        .send()
        .await
        .unwrap();
    let status = resp.status();
    let body = resp.json().await.unwrap_or(serde_json::Value::Null);
    (status, body)
}

async fn put_prefs(
    client: &reqwest::Client,
    base: &str,
    token: &str,
    body: serde_json::Value,
) -> (StatusCode, serde_json::Value) {
    let resp = client
        .put(format!("{base}/api/v1/user/notification-preferences"))
        .bearer_auth(token)
        .json(&body)
        .send()
        .await
        .unwrap();
    let status = resp.status();
    let body = resp.json().await.unwrap_or(serde_json::Value::Null);
    (status, body)
}

fn entry<'a>(
    body: &'a serde_json::Value,
    event_type: &str,
    channel: &str,
    repo_id: Option<i64>,
) -> &'a serde_json::Value {
    body["preferences"]
        .as_array()
        .expect("preferences array")
        .iter()
        .find(|entry| {
            entry["event_type"] == event_type
                && entry["channel"] == channel
                && entry["repo_id"].as_i64() == repo_id
        })
        .unwrap_or_else(|| panic!("no {event_type}/{channel}/repo={repo_id:?} entry in {body}"))
}

#[tokio::test]
async fn preferences_require_authentication() {
    let (base, _db) = spawn_test_app_with_db().await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{base}/api/v1/user/notification-preferences"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    let resp = client
        .put(format!("{base}/api/v1/user/notification-preferences"))
        .json(&serde_json::json!([]))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn a_fresh_account_sees_every_channel_on_and_no_stored_rows() {
    let (base, db) = spawn_test_app_with_db().await;
    let (token, user_id) = register_full(&base, "prefnew", "prefnew@example.com").await;
    let client = reqwest::Client::new();

    let (status, body) = get_prefs(&client, &base, &token).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["default_enabled"], true);
    let entries = body["preferences"].as_array().unwrap();
    assert!(!entries.is_empty(), "the catalogue must be expanded");
    for entry in entries {
        assert_eq!(entry["enabled"], true, "默认开启: {entry}");
        assert_eq!(entry["explicit"], false, "未显式设置: {entry}");
        assert!(entry["repo_id"].is_null(), "only global rows: {entry}");
    }
    assert_eq!(
        entry(&body, "push", "email", None)["enabled"],
        true,
        "email defaults on"
    );

    // The point of "absence means on": no rows were written for the new user.
    let rows = rg_db::ops::notification_preference_ops::list_for_user(&db, user_id)
        .await
        .unwrap();
    assert!(rows.is_empty(), "a GET must not initialise rows: {rows:?}");
}

#[tokio::test]
async fn a_mute_is_stored_and_reflected_in_the_effective_view() {
    let (base, db) = spawn_test_app_with_db().await;
    let (token, user_id) = register_full(&base, "prefmute", "prefmute@example.com").await;
    let client = reqwest::Client::new();

    let (status, body) = put_prefs(
        &client,
        &base,
        &token,
        serde_json::json!([
            {"repo_id": null, "event_type": "push", "channel": "email", "enabled": false}
        ]),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let email = entry(&body, "push", "email", None);
    assert_eq!(email["enabled"], false);
    assert_eq!(email["explicit"], true);
    assert_eq!(
        entry(&body, "push", "in_app", None)["enabled"],
        true,
        "the other channel is untouched"
    );

    // It is a real row, and the dispatcher sees the mute.
    let rows = rg_db::ops::notification_preference_ops::list_for_user(&db, user_id)
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert!(!rows[0].enabled);
    assert_eq!(rows[0].channel, "email");
}

#[tokio::test]
async fn a_repository_override_is_reported_next_to_the_global_default() {
    let (base, _db) = spawn_test_app_with_db().await;
    let (token, _user_id) = register_full(&base, "prefscope", "prefscope@example.com").await;
    let repo_id = create_repo(&base, &token, "pref-scoped-repo").await;
    let client = reqwest::Client::new();

    // Globally off, this repository on again — the two rows coexist.
    let (status, body) = put_prefs(
        &client,
        &base,
        &token,
        serde_json::json!([
            {"repo_id": null, "event_type": "push", "channel": "in_app", "enabled": false},
            {"repo_id": repo_id, "event_type": "push", "channel": "in_app", "enabled": true}
        ]),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let global = entry(&body, "push", "in_app", None);
    assert_eq!(global["enabled"], false);
    assert_eq!(global["explicit"], true);
    let scoped = entry(&body, "push", "in_app", Some(repo_id));
    assert_eq!(scoped["enabled"], true, "the repository override wins");
    assert_eq!(scoped["explicit"], true);

    // Re-sending the same override updates instead of adding a second row.
    let (status, _) = put_prefs(
        &client,
        &base,
        &token,
        serde_json::json!([
            {"repo_id": repo_id, "event_type": "push", "channel": "in_app", "enabled": false}
        ]),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, body) = get_prefs(&client, &base, &token).await;
    let overrides = body["preferences"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| entry["repo_id"].as_i64() == Some(repo_id))
        .count();
    assert_eq!(overrides, 1, "one row per repository scope: {body}");
    assert_eq!(
        entry(&body, "push", "in_app", Some(repo_id))["enabled"],
        false
    );
}

#[tokio::test]
async fn an_unknown_channel_is_a_bad_request() {
    let (base, _db) = spawn_test_app_with_db().await;
    let (token, _user_id) = register_full(&base, "prefbadchan", "prefbadchan@example.com").await;
    let client = reqwest::Client::new();

    let (status, _) = put_prefs(
        &client,
        &base,
        &token,
        serde_json::json!([
            {"repo_id": null, "event_type": "push", "channel": "carrier_pigeon", "enabled": false}
        ]),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn an_unknown_repository_is_a_bad_request_not_a_server_error() {
    let (base, _db) = spawn_test_app_with_db().await;
    let (token, _user_id) = register_full(&base, "prefbadrepo", "prefbadrepo@example.com").await;
    let client = reqwest::Client::new();

    let (status, _) = put_prefs(
        &client,
        &base,
        &token,
        serde_json::json!([
            {"repo_id": 987654321, "event_type": "push", "channel": "email", "enabled": false}
        ]),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn an_empty_event_type_is_a_bad_request() {
    let (base, _db) = spawn_test_app_with_db().await;
    let (token, _user_id) = register_full(&base, "prefbadevent", "prefbadevent@example.com").await;
    let client = reqwest::Client::new();

    let (status, _) = put_prefs(
        &client,
        &base,
        &token,
        serde_json::json!([
            {"repo_id": null, "event_type": "   ", "channel": "email", "enabled": false}
        ]),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

/// Preferences are per user: one account's mute must not touch another's view.
#[tokio::test]
async fn preferences_are_scoped_to_the_caller() {
    let (base, _db) = spawn_test_app_with_db().await;
    let (first_token, _) = register_full(&base, "prefone", "prefone@example.com").await;
    let (second_token, _) = register_full(&base, "preftwo", "preftwo@example.com").await;
    let client = reqwest::Client::new();

    let (status, _) = put_prefs(
        &client,
        &base,
        &first_token,
        serde_json::json!([
            {"repo_id": null, "event_type": "push", "channel": "in_app", "enabled": false}
        ]),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (_, body) = get_prefs(&client, &base, &second_token).await;
    assert_eq!(
        entry(&body, "push", "in_app", None)["enabled"],
        true,
        "the second account is untouched"
    );
}

/// The whole round trip on the backend named by `IRONFORGE_TEST_DATABASE_URL`
/// (PostgreSQL / MySQL) when it is set, SQLite otherwise: the upsert's
/// find-then-write, the nullable `repo_id` in the unique key and the resolution
/// order are exactly the places where the three backends could disagree.
///
/// One test on purpose: the database is shared between the tests of this
/// target, so it uses its own account and repository names.
#[tokio::test]
async fn preferences_round_trip_on_the_backend_from_the_environment() {
    let (base, db) = common::spawn_test_app_on_env_db().await;
    let (token, user_id) = register_full(&base, "prefxbackend", "prefxbackend@example.com").await;
    let repo_id = create_repo(&base, &token, "pref-xbackend-repo").await;
    let client = reqwest::Client::new();

    let (status, body) = put_prefs(
        &client,
        &base,
        &token,
        serde_json::json!([
            {"repo_id": null, "event_type": "push", "channel": "in_app", "enabled": false},
            {"repo_id": repo_id, "event_type": "push", "channel": "in_app", "enabled": true},
            {"repo_id": null, "event_type": "push", "channel": "email", "enabled": false}
        ]),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(entry(&body, "push", "in_app", None)["enabled"], false);
    assert_eq!(
        entry(&body, "push", "in_app", Some(repo_id))["enabled"],
        true
    );

    // The dispatcher resolves the same way on this backend.
    let global = rg_core::notification::dispatch::dispatch(
        &db,
        None,
        rg_core::notification::dispatch::NotifyRequest {
            recipients: vec![user_id],
            event_type: "push".to_string(),
            repo_id: None,
            title: "xbackend global".to_string(),
            body: None,
            action_url: None,
        },
    )
    .await;
    assert_eq!(global.in_app, 0, "the global mute applies");
    assert_eq!(
        global.skipped_by_preference, 2,
        "in_app mute and email mute"
    );
    let scoped = rg_core::notification::dispatch::dispatch(
        &db,
        None,
        rg_core::notification::dispatch::NotifyRequest {
            recipients: vec![user_id],
            event_type: "push".to_string(),
            repo_id: Some(repo_id),
            title: "xbackend scoped".to_string(),
            body: None,
            action_url: None,
        },
    )
    .await;
    assert_eq!(scoped.in_app, 1, "the repository override re-enables it");
}
