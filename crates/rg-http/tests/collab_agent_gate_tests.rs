//! The collaboration "user door" serves people only (issue #34).
//!
//! A request without session headers acts as the signed-in account. That is a
//! person at a UI or CLI speaking for themselves; an agent must act through a
//! collaboration session instead, so its writes are attributed to the session.
//! Reading stays open to an agent that may read the repository.
//!
//! Real HTTP server and SQLite database; with `IRONFORGE_TEST_DATABASE_URL` the
//! same tests run on PostgreSQL or MySQL (not run here).

mod collab_common;
mod common;

use collab_common::{Env, Sess};
use reqwest::Method;
use serde_json::{json, Value};

const MUST_USE_SESSION: &str = "AGENT_MUST_USE_SESSION";

/// The permission cache is one process-wide map keyed by `(repo id, user id)`;
/// each test has its own database, so run them one at a time.
static ONE_AT_A_TIME: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

struct Gate {
    _exclusive: tokio::sync::MutexGuard<'static, ()>,
    env: Env,
    /// Token of an agent owned by the repository owner, with write access.
    agent: String,
    /// A session of the owner, which created `task`.
    owner_session: Sess,
    task: String,
}

async fn gate() -> Gate {
    let exclusive = ONE_AT_A_TIME.lock().await;
    rg_core::repo::service::invalidate_perm_cache_all();
    let env = Env::new().await;

    let (status, body) = env
        .json(
            Method::POST,
            &format!("{}/api/v1/agents", env.base),
            &env.owner_token,
            None,
            Some(json!({"name": "bot"})),
        )
        .await;
    assert_eq!(status, 201, "create agent: {body}");
    let username = body["username"].as_str().unwrap().to_string();
    let agent_id = body["id"].as_i64().unwrap();
    let (status, body) = env
        .json(
            Method::POST,
            &format!("{}/api/v1/agents/{username}/tokens", env.base),
            &env.owner_token,
            None,
            Some(json!({"name": "t", "scopes": "repo:collab"})),
        )
        .await;
    assert_eq!(status, 201, "agent token: {body}");
    let agent = body["token"].as_str().unwrap().to_string();
    env.add_collaborator(agent_id, "write").await;

    let owner_session = env.session(&env.owner_token, "owner-session").await;
    let (status, task) = env
        .json(
            Method::POST,
            &env.url("/tasks"),
            &env.owner_token,
            Some(&owner_session),
            Some(json!({"title": "task", "owner_session_id": owner_session.id})),
        )
        .await;
    assert_eq!(status, 201, "task: {task}");
    Gate {
        _exclusive: exclusive,
        env,
        agent,
        owner_session,
        task: task["id"].as_str().unwrap().to_string(),
    }
}

fn question(key: &str) -> Value {
    json!({
        "client_question_id": key,
        "recipient": {"kind": "repo_writers"},
        "title": "Which database?",
        "body": "SQLite or PostgreSQL?",
    })
}

fn package(key: &str) -> Value {
    json!({
        "client_package_id": key,
        "goal": "ship the cache",
        "scope": "crates/cache",
        "completed": ["wrote the cache"],
        "open_items": ["benchmarks"],
        "next_steps": ["run benchmarks"],
    })
}

fn conditions(key: &str) -> Value {
    json!({
        "client_request_id": key,
        "expected_version": 0,
        "conditions": [{"type": "manual_confirmation", "label": "Owner signs off"}],
    })
}

fn assert_refused(status: u16, body: &Value, reason: &str) {
    assert_eq!(status, 403, "{body}");
    assert_eq!(body["error"]["reason"], reason, "{body}");
}

#[tokio::test]
async fn an_agent_without_a_session_cannot_write_through_the_user_door() {
    let g = gate().await;
    let env = &g.env;
    let task = &g.task;

    // Three different write handlers that share the door.
    let attempts = [
        (
            Method::POST,
            format!("/tasks/{task}/questions"),
            question("q-agent"),
        ),
        (
            Method::POST,
            format!("/tasks/{task}/work-packages"),
            package("w-agent"),
        ),
        (
            Method::PUT,
            format!("/tasks/{task}/completion-conditions"),
            conditions("c-agent"),
        ),
    ];
    for (method, path, body) in attempts {
        let (status, body) = env
            .json(method, &env.url(&path), &g.agent, None, Some(body))
            .await;
        assert_refused(status, &body, MUST_USE_SESSION);
    }

    // Nothing was written by the refused requests.
    let (status, list) = env
        .json(
            Method::GET,
            &env.url(&format!("/tasks/{task}/questions")),
            &env.owner_token,
            None,
            None,
        )
        .await;
    assert_eq!(status, 200, "{list}");
    assert_eq!(list["items"].as_array().unwrap().len(), 0, "{list}");
}

#[tokio::test]
async fn an_agent_with_its_own_session_can_still_write() {
    let g = gate().await;
    let env = &g.env;
    let session = env.session(&g.agent, "bot-session").await;

    let (status, body) = env
        .json(
            Method::POST,
            &env.url(&format!("/tasks/{}/questions", g.task)),
            &g.agent,
            Some(&session),
            Some(question("q-session")),
        )
        .await;
    assert_eq!(status, 201, "{body}");
    assert_eq!(body["creator_kind"], "session", "{body}");
}

#[tokio::test]
async fn a_person_without_a_session_still_uses_the_user_door() {
    let g = gate().await;
    let env = &g.env;

    let (status, body) = env
        .json(
            Method::PUT,
            &env.url(&format!("/tasks/{}/completion-conditions", g.task)),
            &env.owner_token,
            None,
            Some(conditions("c-person")),
        )
        .await;
    assert_eq!(status, 201, "{body}");
    assert_eq!(body["set"]["conditions"][0]["created_by_kind"], "user");
}

#[tokio::test]
async fn an_agent_can_still_read_collaboration_data() {
    let g = gate().await;
    let env = &g.env;

    for path in [
        format!("/tasks/{}", g.task),
        format!("/tasks/{}/questions", g.task),
        format!("/tasks/{}/completion-status", g.task),
    ] {
        let (status, body) = env
            .json(Method::GET, &env.url(&path), &g.agent, None, None)
            .await;
        assert_eq!(status, 200, "GET {path}: {body}");
    }
}

#[tokio::test]
async fn confirming_a_condition_is_a_person_s_act() {
    let g = gate().await;
    let env = &g.env;
    let (status, defined) = env
        .json(
            Method::PUT,
            &env.url(&format!("/tasks/{}/completion-conditions", g.task)),
            &env.owner_token,
            None,
            Some(conditions("c-1")),
        )
        .await;
    assert_eq!(status, 201, "{defined}");
    let condition = defined["set"]["conditions"][0]["id"].as_str().unwrap();
    let url = env.url(&format!(
        "/tasks/{}/completion-conditions/{condition}/confirmations",
        g.task
    ));

    // An agent cannot confirm, with or without a session of its own.
    let session = env.session(&g.agent, "bot-session").await;
    for session in [None, Some(&session)] {
        let (status, body) = env
            .json(
                Method::POST,
                &url,
                &g.agent,
                session,
                Some(json!({"client_request_id": "agent-confirm"})),
            )
            .await;
        assert_refused(status, &body, "AGENT_MAY_NOT_CONFIRM");
    }

    let (status, body) = env
        .json(
            Method::POST,
            &url,
            &env.owner_token,
            None,
            Some(json!({"client_request_id": "person-confirm"})),
        )
        .await;
    assert_eq!(status, 201, "a person can confirm: {body}");
}

#[tokio::test]
async fn reassigning_a_task_is_a_person_s_act() {
    let g = gate().await;
    let env = &g.env;
    let other = env.session(&env.owner_token, "other").await;
    let url = env.url(&format!("/tasks/{}/reassign", g.task));
    let body = |key: &str| {
        json!({
            "client_takeover_id": key,
            "expected_task_version": 1,
            "expected_owner_session_id": g.owner_session.id,
            "to_session_id": other.id,
            "reason": "handing over",
        })
    };

    let (status, refused) = env
        .json(
            Method::POST,
            &url,
            &g.agent,
            None,
            Some(body("agent-reassign")),
        )
        .await;
    assert_refused(status, &refused, "AGENT_MAY_NOT_REASSIGN");

    let (status, done) = env
        .json(
            Method::POST,
            &url,
            &env.owner_token,
            None,
            Some(body("person-reassign")),
        )
        .await;
    assert_eq!(status, 201, "a person can reassign: {done}");
}
