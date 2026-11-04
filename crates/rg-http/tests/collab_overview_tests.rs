//! O02 acceptance: the repository overview for people.
//!
//! Real HTTP server and SQLite database; with `IRONFORGE_TEST_DATABASE_URL` the
//! same tests run on PostgreSQL or MySQL (not run here).

mod collab_common;
mod common;

use chrono::{Duration, Utc};
use collab_common::{Env, Sess};
use reqwest::Method;
use rg_db::entities::agent_session;
use rg_db::sea_orm::{ActiveModelTrait, EntityTrait, Set};
use serde_json::{json, Value};

async fn owned_task(env: &Env, s: &Sess, status: &str) -> String {
    let (code, task) = env
        .json(
            Method::POST,
            &env.url("/tasks"),
            &env.owner_token,
            Some(s),
            Some(json!({"title": format!("task {status}"), "owner_session_id": s.id})),
        )
        .await;
    assert_eq!(code, 201, "{task}");
    let id = task["id"].as_str().unwrap().to_string();
    if status != "open" {
        set_status(env, s, &id, status).await;
    }
    id
}

async fn unowned_task(env: &Env, s: &Sess, status: &str) -> String {
    let (code, task) = env
        .json(
            Method::POST,
            &env.url("/tasks"),
            &env.owner_token,
            Some(s),
            Some(json!({"title": format!("loose {status}")})),
        )
        .await;
    assert_eq!(code, 201, "{task}");
    let id = task["id"].as_str().unwrap().to_string();
    set_status(env, s, &id, status).await;
    id
}

async fn set_status(env: &Env, s: &Sess, id: &str, status: &str) {
    let (code, body) = env
        .json(
            Method::PATCH,
            &env.url(&format!("/tasks/{id}")),
            &env.owner_token,
            Some(s),
            Some(json!({"expected_version": 1, "status": status})),
        )
        .await;
    assert_eq!(code, 200, "{body}");
}

async fn expire(env: &Env, id: &str) {
    let row = agent_session::Entity::find_by_id(id.to_string())
        .one(&env.db)
        .await
        .unwrap()
        .unwrap();
    let mut active: agent_session::ActiveModel = row.into();
    active.expires_at = Set(Utc::now() - Duration::minutes(1));
    active.update(&env.db).await.unwrap();
}

async fn overview(env: &Env, token: &str, query: &str) -> (u16, Value) {
    env.json(
        Method::GET,
        &env.url(&format!("/overview{query}")),
        token,
        None,
        None,
    )
    .await
}

fn section<'a>(body: &'a Value, name: &str) -> &'a Value {
    body["sections"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == name)
        .unwrap_or_else(|| panic!("no section {name}: {body}"))
}

fn ids(section: &Value) -> Vec<String> {
    let mut found: Vec<String> = section["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["object_id"].as_str().unwrap().to_string())
        .collect();
    found.sort();
    found
}

#[tokio::test]
async fn o02_an_empty_repository_has_four_empty_sections_and_says_how_fresh_they_are() {
    let env = Env::new().await;
    let (status, body) = overview(&env, &env.owner_token, "").await;
    assert_eq!(status, 200, "{body}");
    let names: Vec<&str> = body["sections"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        [
            "awaiting_me",
            "blocked_waiting",
            "pending_handoffs",
            "in_progress"
        ]
    );
    for s in body["sections"].as_array().unwrap() {
        assert!(s["items"].as_array().unwrap().is_empty(), "{s}");
        assert_eq!(s["truncated"], false);
        assert!(s["as_of"].is_string());
        assert!(s["continue_with"].is_string());
    }
    assert_eq!(body["consistency"], "per_section_live_query");
    assert!(body["notes"].as_array().unwrap().len() >= 3);
}

#[tokio::test]
async fn o02_sections_locate_the_records_and_flag_unreliable_declarations() {
    let env = Env::new().await;
    let a = env.session(&env.owner_token, "A").await;
    let b = env.session(&env.owner_token, "B").await;

    let working = owned_task(&env, &a, "in_progress").await;
    let abandoned = owned_task(&env, &b, "in_progress").await;
    let blocked = owned_task(&env, &a, "blocked").await;
    let waiting = unowned_task(&env, &a, "waiting").await;
    let _open = owned_task(&env, &a, "open").await;
    let finished = owned_task(&env, &a, "done").await;
    expire(&env, &b.id).await;

    // A handoff to B, whose lease has now run out.
    let (code, handoff) = env
        .json(
            Method::POST,
            &env.url(&format!("/tasks/{working}/handoffs")),
            &env.owner_token,
            Some(&a),
            Some(json!({
                "client_handoff_id": "h1", "to_session_id": b.id, "summary": "s",
                "completed": "c", "next_steps": "n", "blockers": "b", "evidence": []
            })),
        )
        .await;
    assert_eq!(code, 201, "{handoff}");

    // An open question for repository writers.
    let (code, question) = env
        .json(
            Method::POST,
            &env.url(&format!("/tasks/{working}/questions")),
            &env.owner_token,
            Some(&a),
            Some(json!({
                "client_question_id": "q1", "recipient": {"kind": "repo_writers"},
                "title": "which way?", "body": "left or right"
            })),
        )
        .await;
    assert_eq!(code, 201, "{question}");

    let (status, body) = overview(&env, &env.owner_token, "").await;
    assert_eq!(status, 200, "{body}");

    let progress = section(&body, "in_progress");
    let mut expected = vec![working.clone(), abandoned.clone()];
    expected.sort();
    assert_eq!(ids(progress), expected);
    let by_id = |s: &Value, id: &str| -> Value {
        s["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i["object_id"] == id)
            .unwrap()
            .clone()
    };
    let live = by_id(progress, &working);
    assert_eq!(live["session_state"], "online");
    assert!(live["flags"].as_array().unwrap().is_empty());
    assert_eq!(live["origin"], "user_content");
    let stale = by_id(progress, &abandoned);
    assert_eq!(stale["session_state"], "expired");
    assert_eq!(stale["flags"], json!(["session_not_online"]));
    assert_ne!(stale["state"], "done", "an expired owner is never 'done'");

    let stuck = section(&body, "blocked_waiting");
    let mut expected = vec![blocked.clone(), waiting.clone()];
    expected.sort();
    assert_eq!(ids(stuck), expected);
    let loose = by_id(stuck, &waiting);
    assert_eq!(loose["flags"], json!(["unowned"]));
    assert!(loose["session_id"].is_null());

    let handoffs = section(&body, "pending_handoffs");
    assert_eq!(
        ids(handoffs),
        vec![handoff["id"].as_str().unwrap().to_string()]
    );
    let item = &handoffs["items"][0];
    assert_eq!(item["object_type"], "pending_handoff");
    assert_eq!(item["task_id"], working);
    assert_eq!(item["session_id"], b.id);
    assert_eq!(item["flags"], json!(["session_not_online"]));

    let mine = section(&body, "awaiting_me");
    assert_eq!(mine["available"], true);
    assert_eq!(
        ids(mine),
        vec![question["id"].as_str().unwrap().to_string()]
    );
    assert_eq!(mine["items"][0]["object_type"], "open_question");

    // Finished work is on none of the lists.
    let everywhere: Vec<String> = body["sections"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(ids)
        .collect();
    assert!(!everywhere.contains(&finished));
}

#[tokio::test]
async fn o02_sections_are_selectable_unknown_ones_rejected_and_truncation_is_honest() {
    let env = Env::new().await;
    let a = env.session(&env.owner_token, "A").await;
    for _ in 0..3 {
        owned_task(&env, &a, "in_progress").await;
    }

    let (status, body) = overview(&env, &env.owner_token, "?sections=in_progress&limit=2").await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["sections"].as_array().unwrap().len(), 1);
    let progress = section(&body, "in_progress");
    assert_eq!(progress["items"].as_array().unwrap().len(), 2);
    assert_eq!(progress["truncated"], true);

    let (status, body) = overview(&env, &env.owner_token, "?sections=in_progress&limit=3").await;
    assert_eq!(status, 200);
    assert_eq!(section(&body, "in_progress")["truncated"], false);

    let (status, _) = overview(&env, &env.owner_token, "?sections=in_progress,all_of_it").await;
    assert_eq!(status, 400);
}

#[tokio::test]
async fn o02_readers_see_the_work_but_have_nothing_waiting_on_them() {
    let env = Env::new().await;
    let a = env.session(&env.owner_token, "A").await;
    let working = owned_task(&env, &a, "in_progress").await;
    let (code, _) = env
        .json(
            Method::POST,
            &env.url(&format!("/tasks/{working}/questions")),
            &env.owner_token,
            Some(&a),
            Some(json!({
                "client_question_id": "q1", "recipient": {"kind": "repo_writers"},
                "title": "t", "body": "b"
            })),
        )
        .await;
    assert_eq!(code, 201);

    let (reader, reader_id) = env.user("reader").await;
    env.add_collaborator(reader_id, "read").await;
    let (status, body) = overview(&env, &reader, "").await;
    assert_eq!(status, 200, "{body}");
    let mine = section(&body, "awaiting_me");
    assert_eq!(mine["available"], false);
    assert!(mine["reason"].is_string());
    assert!(mine["items"].as_array().unwrap().is_empty());
    assert_eq!(ids(section(&body, "in_progress")), vec![working]);
}

#[tokio::test]
async fn o02_no_access_means_no_names_and_no_counts() {
    let env = Env::with_repo(json!({"name": "private-repo", "is_private": true})).await;
    let a = env.session(&env.owner_token, "A").await;
    owned_task(&env, &a, "in_progress").await;
    let (stranger, _) = env.user("stranger").await;
    let (status, body) = overview(&env, &stranger, "").await;
    assert!(status == 403 || status == 404, "got {status}");
    assert!(body.get("sections").is_none(), "{body}");
    let (status, _) = overview(&env, "", "").await;
    assert!(status == 401 || status == 403 || status == 404, "{status}");
}
