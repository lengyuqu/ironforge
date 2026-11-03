//! C02 acceptance: session lifecycle and credentials (T01–T03, T09).
//!
//! Real HTTP server and SQLite database. PostgreSQL/MySQL behaviour of the
//! storage layer is covered by `rg-db/tests/collab_store.rs`.

mod common;
mod collab_common;

use chrono::{Duration, Utc};
use collab_common::{Env, Sess};
use reqwest::Method;
use rg_db::sea_orm::{ActiveModelTrait, EntityTrait, Set};
use serde_json::{json, Value};

async fn set_times(
    env: &Env,
    id: &str,
    last_seen: Option<chrono::DateTime<Utc>>,
    expires: Option<chrono::DateTime<Utc>>,
) {
    use rg_db::entities::agent_session;
    let row = agent_session::Entity::find_by_id(id.to_string())
        .one(&env.db)
        .await
        .unwrap()
        .unwrap();
    let mut active: agent_session::ActiveModel = row.into();
    if let Some(t) = last_seen {
        active.last_seen_at = Set(t);
    }
    if let Some(t) = expires {
        active.expires_at = Set(t);
    }
    active.update(&env.db).await.unwrap();
}

async fn list(env: &Env, token: &str) -> Vec<Value> {
    let (status, body) = env
        .json(Method::GET, &env.url("/sessions"), token, None, None)
        .await;
    assert_eq!(status, 200, "{body}");
    body["items"].as_array().unwrap().clone()
}

/// T01: two sessions under one account, even with the same display name.
#[tokio::test]
async fn t01_same_account_two_sessions_with_identical_names_stay_distinct() {
    let env = Env::new().await;
    let (a, a_body) = env.register_session(&env.owner_token, "Claude").await;
    let (b, _) = env.register_session(&env.owner_token, "Claude").await;
    assert_ne!(a.id, b.id);
    assert_ne!(a.secret, b.secret);
    assert_eq!(a_body["session"]["origin"], "user_content");

    let items = list(&env, &env.owner_token).await;
    let ids: Vec<&str> = items.iter().map(|i| i["id"].as_str().unwrap()).collect();
    assert!(ids.contains(&a.id.as_str()) && ids.contains(&b.id.as_str()));
    assert!(items.iter().all(|i| i["display_name"] == "Claude"));

    // A label confers nothing: only the secret acts as the session.
    let (status, _) = env
        .json(
            Method::POST,
            &env.url(&format!("/sessions/{}/heartbeat", a.id)),
            &env.owner_token,
            Some(&Sess { id: a.id.clone(), secret: b.secret.clone() }),
            None,
        )
        .await;
    assert_eq!(status, 403, "B's secret must not act as A");
}

#[tokio::test]
async fn t01_registration_validates_labels() {
    let env = Env::new().await;
    for body in [
        json!({"display_name": "", "client_kind": "x"}),
        json!({"display_name": "   ", "client_kind": "x"}),
        json!({"display_name": "x".repeat(81), "client_kind": "x"}),
        json!({"display_name": "ok", "client_kind": "k".repeat(65)}),
        json!({"display_name": "bell\u{7}", "client_kind": "x"}),
    ] {
        let (status, resp) = env
            .json(Method::POST, &env.url("/sessions"), &env.owner_token, None, Some(body.clone()))
            .await;
        assert_eq!(status, 400, "{body} -> {resp}");
        assert_eq!(resp["error"]["code"], "BAD_REQUEST");
    }
    assert!(list(&env, &env.owner_token).await.is_empty());
}

/// T02: credentials and authorization.
#[tokio::test]
async fn t02_wrong_unknown_foreign_and_cross_repo_credentials_are_indistinguishable() {
    let env = Env::new().await;
    let other_repo = "second-repo";
    crate::common::create_repo(&env.base, &env.owner_token, other_repo).await;
    let a = env.session(&env.owner_token, "A").await;
    let hb = |id: &str| env.url(&format!("/sessions/{id}/heartbeat"));

    // Wrong secret, unknown id, malformed id: same status, code and message.
    let wrong = Sess { id: a.id.clone(), secret: "ifs_".to_string() + &"0".repeat(64) };
    let unknown = Sess { id: uuid::Uuid::new_v4().to_string(), secret: a.secret.clone() };
    let malformed = Sess { id: "not-a-uuid".into(), secret: a.secret.clone() };
    let mut seen = Vec::new();
    for s in [&wrong, &unknown, &malformed] {
        let (status, body) = env
            .json(Method::POST, &hb(&s.id), &env.owner_token, Some(s), None)
            .await;
        assert_eq!(status, 403);
        seen.push((body["error"]["code"].clone(), body["error"]["message"].clone()));
    }
    assert!(seen.iter().all(|s| s == &seen[0]), "credential errors must not be an oracle: {seen:?}");
    assert_eq!(seen[0].0, "SESSION_CREDENTIALS_INVALID");

    // Missing headers.
    let (status, body) = env.json(Method::POST, &hb(&a.id), &env.owner_token, None, None).await;
    assert_eq!((status, body["error"]["code"].as_str()), (400, Some("SESSION_CREDENTIALS_REQUIRED")));

    // Path id and header id must agree.
    let b = env.session(&env.owner_token, "B").await;
    let (status, _) = env.json(Method::POST, &hb(&b.id), &env.owner_token, Some(&a), None).await;
    assert_eq!(status, 403, "A's credentials cannot act on B's path");

    // Same id and secret in another repository of the same owner.
    let other_hb = env.url_in(other_repo, &format!("/sessions/{}/heartbeat", a.id));
    let (status, _) = env.json(Method::POST, &other_hb, &env.owner_token, Some(&a), None).await;
    assert_eq!(status, 403);

    // No account credential at all.
    let resp = env.send(Method::POST, &hb(&a.id), None, Some(&a), None).await;
    assert_eq!(resp.status(), 401);

    // The right secret still works for its owner.
    let (status, _) = env.json(Method::POST, &hb(&a.id), &env.owner_token, Some(&a), None).await;
    assert_eq!(status, 200);
}

#[tokio::test]
async fn t02_another_account_cannot_take_over_a_session_even_with_its_secret() {
    let env = Env::new().await;
    let (bob_token, bob_id) = env.user("bob_writer").await;
    env.add_collaborator(bob_id, "write").await;
    let a = env.session(&env.owner_token, "owner-session").await;

    // Bob has write access and (hypothetically) learned both id and secret.
    let (status, body) = env
        .json(
            Method::POST,
            &env.url(&format!("/sessions/{}/heartbeat", a.id)),
            &bob_token,
            Some(&a),
            None,
        )
        .await;
    assert_eq!(status, 403, "{body}");
    assert_eq!(body["error"]["code"], "SESSION_CREDENTIALS_INVALID");
    let (status, _) = env
        .json(
            Method::POST,
            &env.url(&format!("/sessions/{}/end", a.id)),
            &bob_token,
            Some(&a),
            None,
        )
        .await;
    assert_eq!(status, 403);

    // The session is untouched.
    let items = list(&env, &env.owner_token).await;
    assert_eq!(items[0]["state"], "online");
    assert!(items[0]["ended_at"].is_null());
}

#[tokio::test]
async fn t02_registration_and_discovery_follow_repository_permissions() {
    let env = Env::with_repo(json!({"name": "private-repo", "is_private": true})).await;
    let (reader_token, reader_id) = env.user("reader_user").await;
    let (_stranger_token, _) = env.user("stranger_user").await;
    env.add_collaborator(reader_id, "read").await;
    let body = json!({"display_name": "x", "client_kind": "t"});

    // Read-only collaborator: cannot register or list sessions.
    let (status, _) = env.json(Method::POST, &env.url("/sessions"), &reader_token, None, Some(body.clone())).await;
    assert_eq!(status, 403);
    let (status, _) = env.json(Method::GET, &env.url("/sessions"), &reader_token, None, None).await;
    assert_eq!(status, 403);

    // Stranger on a private repo and anonymous callers are refused too.
    let stranger = env.user("stranger_two").await.0;
    let (status, _) = env.json(Method::POST, &env.url("/sessions"), &stranger, None, Some(body.clone())).await;
    assert_eq!(status, 403);
    let resp = env.send(Method::POST, &env.url("/sessions"), None, None, Some(body)).await;
    assert_eq!(resp.status(), 401);
    let resp = env.send(Method::GET, &env.url("/sessions"), None, None, None).await;
    assert_eq!(resp.status(), 401);

    // Unknown repository.
    let (status, _) = env
        .json(Method::GET, &env.url_in("nope", "/sessions"), &env.owner_token, None, None)
        .await;
    assert_eq!(status, 404);
}

/// T02: a revoked collaborator is refused on the very next request, even though
/// the revocation bypassed the permission-cache invalidation helpers.
#[tokio::test]
async fn t02_revoked_write_access_blocks_the_next_request_despite_a_warm_permission_cache() {
    let env = Env::with_repo(json!({"name": "revoke-repo", "is_private": true})).await;
    let (bob_token, bob_id) = env.user("bob_revoked").await;
    env.add_collaborator(bob_id, "write").await;
    let bob = env.session(&bob_token, "bob-session").await;
    let hb = env.url(&format!("/sessions/{}/heartbeat", bob.id));
    let (status, _) = env.json(Method::POST, &hb, &bob_token, Some(&bob), None).await;
    assert_eq!(status, 200);

    // Revoke straight in the database (what an out-of-band admin edit or
    // another instance would do): no cache invalidation happens.
    rg_db::ops::repo_collaborator_ops::delete_by_repo_and_user(&env.db, env.repo_id, bob_id)
        .await
        .unwrap();

    // Control: the ordinary cached check still believes Bob can write, which is
    // the documented staleness window. Skipped when the cache is disabled.
    if std::env::var("IRONFORGE_DISABLE_PERM_CACHE").as_deref() != Ok("1") {
        let repo = rg_db::entities::repository::Entity::find_by_id(env.repo_id)
            .one(&env.db)
            .await
            .unwrap()
            .unwrap();
        let cached = rg_core::repo::service::can_write_repo(&env.db, &repo, Some(bob_id))
            .await
            .unwrap();
        assert!(cached, "control: cache should still be warm for the generic check");
    }

    let (status, _) = env.json(Method::POST, &hb, &bob_token, Some(&bob), None).await;
    assert_eq!(status, 403, "revocation must apply immediately to collaboration requests");
    let (status, _) = env
        .json(Method::POST, &env.url("/sessions"), &bob_token, None, Some(json!({"display_name": "n", "client_kind": "t"})))
        .await;
    assert_eq!(status, 403);
}

#[tokio::test]
async fn t02_ended_and_expired_sessions_cannot_be_revived() {
    let env = Env::new().await;
    let a = env.session(&env.owner_token, "A").await;
    let hb = env.url(&format!("/sessions/{}/heartbeat", a.id));
    let end = env.url(&format!("/sessions/{}/end", a.id));

    let (status, _) = env.json(Method::POST, &end, &env.owner_token, Some(&a), None).await;
    assert_eq!(status, 204);
    // Ending again is idempotent.
    let (status, _) = env.json(Method::POST, &end, &env.owner_token, Some(&a), None).await;
    assert_eq!(status, 204);

    let (status, body) = env.json(Method::POST, &hb, &env.owner_token, Some(&a), None).await;
    assert_eq!((status, body["error"]["code"].as_str()), (409, Some("SESSION_EXPIRED")));
    assert_eq!(list(&env, &env.owner_token).await[0]["state"], "ended");

    // Hard expiry.
    let b = env.session(&env.owner_token, "B").await;
    set_times(&env, &b.id, None, Some(Utc::now() - Duration::seconds(5))).await;
    let (status, body) = env
        .json(Method::POST, &env.url(&format!("/sessions/{}/heartbeat", b.id)), &env.owner_token, Some(&b), None)
        .await;
    assert_eq!((status, body["error"]["code"].as_str()), (409, Some("SESSION_EXPIRED")));
    let items = list(&env, &env.owner_token).await;
    let b_item = items.iter().find(|i| i["id"] == b.id.as_str()).unwrap();
    assert_eq!(b_item["state"], "expired");
    // An expired session may still be ended (valid credentials), and stays unusable.
    let (status, _) = env
        .json(Method::POST, &env.url(&format!("/sessions/{}/end", b.id)), &env.owner_token, Some(&b), None)
        .await;
    assert_eq!(status, 204);
}

/// T03: reconnect, lost secret, offline.
#[tokio::test]
async fn t03_original_credentials_reconnect_until_hard_expiry_and_offline_is_not_ended() {
    let env = Env::new().await;
    let a = env.session(&env.owner_token, "A").await;
    let hb = env.url(&format!("/sessions/{}/heartbeat", a.id));
    let created = list(&env, &env.owner_token).await[0].clone();
    assert_eq!(created["state"], "online");

    // Silent for five minutes: offline, not ended.
    set_times(&env, &a.id, Some(Utc::now() - Duration::minutes(5)), None).await;
    let item = list(&env, &env.owner_token).await[0].clone();
    assert_eq!(item["state"], "offline");
    assert!(item["ended_at"].is_null());

    // The client reconnects with the credentials it kept.
    let (status, body) = env.json(Method::POST, &hb, &env.owner_token, Some(&a), None).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["state"], "online");
    assert_eq!(body["expires_at"], created["expires_at"], "heartbeat must not extend the hard expiry");
    assert_eq!(body["id"], a.id.as_str());
}

#[tokio::test]
async fn t03_lost_secret_means_a_new_session_and_the_old_one_cannot_be_claimed() {
    let env = Env::new().await;
    let old = env.session(&env.owner_token, "agent").await;
    // The process lost its secret: it can only register again.
    let fresh = env.session(&env.owner_token, "agent").await;
    assert_ne!(old.id, fresh.id);

    // Knowing the old id is not enough, with any secret it can invent.
    for guess in [fresh.secret.clone(), String::new(), "ifs_".to_string()] {
        let (status, _) = env
            .json(
                Method::POST,
                &env.url(&format!("/sessions/{}/heartbeat", old.id)),
                &env.owner_token,
                Some(&Sess { id: old.id.clone(), secret: guess }),
                None,
            )
            .await;
        assert!(status == 403 || status == 400, "status {status}");
    }
    // The old session simply ages out; the new one works.
    let (status, _) = env
        .json(Method::POST, &env.url(&format!("/sessions/{}/heartbeat", fresh.id)), &env.owner_token, Some(&fresh), None)
        .await;
    assert_eq!(status, 200);
}

#[tokio::test]
async fn session_list_paginates_with_opaque_cursors() {
    let env = Env::new().await;
    for i in 0..5 {
        env.session(&env.owner_token, &format!("s{i}")).await;
    }
    let mut seen = std::collections::BTreeSet::new();
    let mut cursor: Option<String> = None;
    let mut pages = 0;
    loop {
        let mut url = env.url("/sessions?limit=2");
        if let Some(c) = &cursor {
            url.push_str(&format!("&cursor={c}"));
        }
        let (status, body) = env.json(Method::GET, &url, &env.owner_token, None, None).await;
        assert_eq!(status, 200, "{body}");
        for item in body["items"].as_array().unwrap() {
            assert!(seen.insert(item["id"].as_str().unwrap().to_string()));
        }
        pages += 1;
        match body["next_cursor"].as_str() {
            Some(c) => cursor = Some(c.to_string()),
            None => break,
        }
    }
    assert_eq!((seen.len(), pages), (5, 3));

    // Tampered or foreign cursors are rejected, not reinterpreted.
    for bad in ["garbage", "abc.def", ""] {
        let (status, body) = env
            .json(Method::GET, &env.url(&format!("/sessions?cursor={bad}")), &env.owner_token, None, None)
            .await;
        if bad.is_empty() {
            assert_eq!(status, 400, "{body}");
        } else {
            assert_eq!(status, 400, "{bad}: {body}");
        }
    }
}
