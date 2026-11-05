//! Personal inbox read marks (`collab_inbox_reads`).
//!
//! Real HTTP server and SQLite database; with `IRONFORGE_TEST_DATABASE_URL` the
//! same tests run on PostgreSQL or MySQL.

mod collab_common;
mod common;

use collab_common::{Env, Sess};
use reqwest::Method;
use serde_json::{json, Value};

async fn owned_task(env: &Env, s: &Sess) -> String {
    let (code, task) = env
        .json(
            Method::POST,
            &env.url("/tasks"),
            &env.owner_token,
            Some(s),
            Some(json!({"title": "Port the cache", "owner_session_id": s.id})),
        )
        .await;
    assert_eq!(code, 201, "{task}");
    task["id"].as_str().unwrap().to_string()
}

async fn ask(env: &Env, tid: &str, from: &Sess, key: &str) -> Value {
    let (code, q) = env
        .json(
            Method::POST,
            &env.url(&format!("/tasks/{tid}/questions")),
            &env.owner_token,
            Some(from),
            Some(json!({
                "client_question_id": key,
                "recipient": {"kind": "repo_writers"},
                "title": "Which database?",
                "body": "SQLite or PostgreSQL?",
            })),
        )
        .await;
    assert_eq!(code, 201, "{q}");
    q
}

async fn inbox(env: &Env, s: &Sess) -> Vec<Value> {
    let (code, body) = env
        .json(
            Method::GET,
            &env.url("/inbox"),
            &env.owner_token,
            Some(s),
            None,
        )
        .await;
    assert_eq!(code, 200, "{body}");
    body["items"].as_array().unwrap().clone()
}

async fn user_inbox(env: &Env) -> Vec<Value> {
    let (code, body) = env
        .json(
            Method::GET,
            &env.url("/inbox/user"),
            &env.owner_token,
            None,
            None,
        )
        .await;
    assert_eq!(code, 200, "{body}");
    body["items"].as_array().unwrap().clone()
}

fn find<'a>(items: &'a [Value], key: &str) -> &'a Value {
    items
        .iter()
        .find(|i| i["item_key"] == key)
        .unwrap_or_else(|| panic!("{key} not in {items:?}"))
}

async fn mark(env: &Env, s: &Sess, key: &str, version: i64) -> (u16, Value) {
    env.json(
        Method::PUT,
        &env.url("/inbox/read"),
        &env.owner_token,
        Some(s),
        Some(json!({"item_key": key, "observed_version": version})),
    )
    .await
}

#[tokio::test]
async fn a_new_item_is_unread_and_names_its_version() {
    let env = Env::new().await;
    let a = env.session(&env.owner_token, "A").await;
    let tid = owned_task(&env, &a).await;
    let items = inbox(&env, &a).await;
    let item = find(&items, &format!("assigned_task:{tid}"));
    assert_eq!(item["read_state"], "unread");
    assert_eq!(item["object_version"], 1);
    assert_eq!(item["last_read_version"], Value::Null);
}

#[tokio::test]
async fn marking_reads_the_version_and_only_for_the_marking_actor() {
    let env = Env::new().await;
    let a = env.session(&env.owner_token, "A").await;
    let b = env.session(&env.owner_token, "B").await;
    let tid = owned_task(&env, &a).await;
    let q = ask(&env, &tid, &a, "q-1").await;
    let key = format!("open_question:{}", q["id"].as_str().unwrap());

    // B sees A's question to the repository writers; the user door does too.
    assert_eq!(find(&inbox(&env, &b).await, &key)["read_state"], "unread");
    assert_eq!(find(&user_inbox(&env).await, &key)["read_state"], "unread");

    let (code, done) = mark(&env, &b, &key, 1).await;
    assert_eq!(code, 200, "{done}");
    assert_eq!(done["read_state"], "read");
    assert_eq!(done["observed_version"], 1);
    assert_eq!(done["current_version"], 1);

    let items = inbox(&env, &b).await;
    let item = find(&items, &key);
    assert_eq!(item["read_state"], "read");
    assert_eq!(item["last_read_version"], 1);
    assert_ne!(item["last_read_at"], Value::Null);
    // Nobody else's view moved: another session, the user door.
    assert_eq!(find(&user_inbox(&env).await, &key)["read_state"], "unread");
    let (code, user_mark) = env
        .json(
            Method::PUT,
            &env.url("/inbox/user/read"),
            &env.owner_token,
            None,
            Some(json!({"item_key": key, "observed_version": 1})),
        )
        .await;
    assert_eq!(code, 200, "{user_mark}");
    assert_eq!(find(&user_inbox(&env).await, &key)["read_state"], "read");
    // Reading is not answering: the question is untouched.
    let (_, current) = env
        .json(
            Method::GET,
            &env.url(&format!("/questions/{}", q["id"].as_str().unwrap())),
            &env.owner_token,
            None,
            None,
        )
        .await;
    assert_eq!(current["status"], "open");
    assert_eq!(current["version"], 1);
}

#[tokio::test]
async fn a_newer_version_of_the_same_item_is_unread_again() {
    let env = Env::new().await;
    let a = env.session(&env.owner_token, "A").await;
    let tid = owned_task(&env, &a).await;
    let key = format!("assigned_task:{tid}");
    assert_eq!(mark(&env, &a, &key, 1).await.0, 200);
    assert_eq!(find(&inbox(&env, &a).await, &key)["read_state"], "read");

    let (code, body) = env
        .json(
            Method::PATCH,
            &env.url(&format!("/tasks/{tid}")),
            &env.owner_token,
            Some(&a),
            Some(json!({"expected_version": 1, "status": "in_progress"})),
        )
        .await;
    assert_eq!(code, 200, "{body}");

    let items = inbox(&env, &a).await;
    let item = find(&items, &key);
    assert_eq!(item["object_version"], 2);
    assert_eq!(item["read_state"], "unread");
    assert_eq!(item["last_read_version"], 1, "the old mark stays visible");

    assert_eq!(mark(&env, &a, &key, 2).await.0, 200);
    assert_eq!(find(&inbox(&env, &a).await, &key)["read_state"], "read");
}

#[tokio::test]
async fn marks_are_monotonic_and_a_version_from_the_future_is_refused() {
    let env = Env::new().await;
    let a = env.session(&env.owner_token, "A").await;
    let tid = owned_task(&env, &a).await;
    let key = format!("assigned_task:{tid}");
    let (code, body) = env
        .json(
            Method::PATCH,
            &env.url(&format!("/tasks/{tid}")),
            &env.owner_token,
            Some(&a),
            Some(json!({"expected_version": 1, "status": "in_progress"})),
        )
        .await;
    assert_eq!(code, 200, "{body}");

    // Newer than the record: refused, nothing stored.
    assert_eq!(mark(&env, &a, &key, 3).await.0, 400);
    assert_eq!(mark(&env, &a, &key, 0).await.0, 400);
    assert_eq!(
        find(&inbox(&env, &a).await, &key)["last_read_version"],
        Value::Null
    );

    // An older version is accepted but does not make the current one read.
    let (code, old) = mark(&env, &a, &key, 1).await;
    assert_eq!(code, 200, "{old}");
    assert_eq!(old["read_state"], "unread");
    // The current version, then an older one again: the mark never moves back.
    assert_eq!(mark(&env, &a, &key, 2).await.0, 200);
    let (code, again) = mark(&env, &a, &key, 1).await;
    assert_eq!(code, 200, "{again}");
    assert_eq!(again["observed_version"], 2);
    assert_eq!(again["read_state"], "read");
}

#[tokio::test]
async fn only_items_in_my_own_inbox_can_be_marked_and_absence_is_uniform() {
    let env = Env::new().await;
    let a = env.session(&env.owner_token, "A").await;
    let b = env.session(&env.owner_token, "B").await;
    let tid = owned_task(&env, &a).await;
    let other = "00000000-0000-4000-8000-000000000000";
    for key in [
        format!("assigned_task:{tid}"), // owned by A, not B
        format!("assigned_task:{other}"),
        format!("open_question:{other}"),
        "bogus".to_string(),
        format!("direct_message:{tid}"),
        "assigned_task:not-a-uuid".to_string(),
    ] {
        let (code, body) = mark(&env, &b, &key, 1).await;
        assert_eq!(code, 404, "{key}: {body}");
    }
    // The user door has no assigned tasks or handoffs at all.
    let (code, _) = env
        .json(
            Method::PUT,
            &env.url("/inbox/user/read"),
            &env.owner_token,
            None,
            Some(json!({"item_key": format!("assigned_task:{tid}"), "observed_version": 1})),
        )
        .await;
    assert_eq!(code, 404);
    // An own question to repo writers is not in the asker's inbox.
    let q = ask(&env, &tid, &a, "q-own").await;
    let (code, _) = mark(
        &env,
        &a,
        &format!("open_question:{}", q["id"].as_str().unwrap()),
        1,
    )
    .await;
    assert_eq!(code, 404);
    // Unknown fields and a missing session are rejected, not guessed.
    let (code, _) = env
        .json(
            Method::PUT,
            &env.url("/inbox/read"),
            &env.owner_token,
            Some(&b),
            Some(json!({"item_key": "x", "observed_version": 1, "extra": true})),
        )
        .await;
    assert_eq!(code, 400);
    let (code, _) = env
        .json(
            Method::PUT,
            &env.url("/inbox/read"),
            &env.owner_token,
            None,
            Some(json!({"item_key": format!("assigned_task:{tid}"), "observed_version": 1})),
        )
        .await;
    assert!(code == 400 || code == 403, "{code}");
}

#[tokio::test]
async fn racing_marks_converge_on_the_highest_version() {
    let env = Env::new().await;
    let a = env.session(&env.owner_token, "A").await;
    let tid = owned_task(&env, &a).await;
    let key = format!("assigned_task:{tid}");
    for status in ["in_progress", "waiting", "blocked"] {
        let (_, t) = env
            .json(
                Method::GET,
                &env.url(&format!("/tasks/{tid}")),
                &env.owner_token,
                None,
                None,
            )
            .await;
        let (code, body) = env
            .json(
                Method::PATCH,
                &env.url(&format!("/tasks/{tid}")),
                &env.owner_token,
                Some(&a),
                Some(json!({"expected_version": t["version"], "status": status})),
            )
            .await;
        assert_eq!(code, 200, "{body}");
    }
    let results = futures::future::join_all((1..=4).map(|v| mark(&env, &a, &key, v))).await;
    for (code, body) in &results {
        assert_eq!(*code, 200, "{body}");
    }
    let items = inbox(&env, &a).await;
    let item = find(&items, &key);
    assert_eq!(item["last_read_version"], 4);
    assert_eq!(item["read_state"], "read");
}
