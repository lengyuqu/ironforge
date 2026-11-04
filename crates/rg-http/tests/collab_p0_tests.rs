//! P0 acceptance: takeovers (P01–P03), questions and answers (P05–P08), work
//! packages (P09–P10).
//!
//! Real HTTP server and SQLite database; with `IRONFORGE_TEST_DATABASE_URL` the
//! same tests run on PostgreSQL or MySQL.

mod common;
mod collab_common;

use chrono::{Duration, Utc};
use collab_common::{Env, Sess};
use reqwest::Method;
use rg_db::sea_orm::{ActiveModelTrait, EntityTrait, Set};
use serde_json::{json, Value};

async fn new_task(env: &Env, token: &str, s: &Sess, extra: Value) -> Value {
    let mut body = json!({"title": "task"});
    body.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
    let (status, task) = env
        .json(Method::POST, &env.url("/tasks"), token, Some(s), Some(body))
        .await;
    assert_eq!(status, 201, "{task}");
    task
}

async fn expire_session(env: &Env, id: &str, last_seen_ago: Option<Duration>, expired: bool) {
    use rg_db::entities::agent_session;
    let row = agent_session::Entity::find_by_id(id.to_string())
        .one(&env.db)
        .await
        .unwrap()
        .unwrap();
    let mut active: agent_session::ActiveModel = row.into();
    if let Some(ago) = last_seen_ago {
        active.last_seen_at = Set(Utc::now() - ago);
    }
    if expired {
        active.expires_at = Set(Utc::now() - Duration::minutes(1));
    }
    active.update(&env.db).await.unwrap();
}

fn takeover_body(key: &str, version: i64, owner: Option<&str>, reason: &str) -> Value {
    json!({
        "client_takeover_id": key,
        "expected_task_version": version,
        "expected_owner_session_id": owner,
        "reason": reason,
    })
}

async fn takeovers_of(env: &Env, task_id: &str) -> Vec<Value> {
    let (status, body) = env
        .json(Method::GET, &env.url(&format!("/tasks/{task_id}/takeovers")), &env.owner_token, None, None)
        .await;
    assert_eq!(status, 200, "{body}");
    body["items"].as_array().unwrap().clone()
}

// ---------------------------------------------------------------- takeovers

#[tokio::test]
async fn p01_unowned_task_can_be_claimed_and_replays_are_idempotent() {
    let env = Env::new().await;
    let a = env.session(&env.owner_token, "A").await;
    let b = env.session(&env.owner_token, "B").await;
    let task = new_task(&env, &env.owner_token, &a, json!({})).await;
    let tid = task["id"].as_str().unwrap();
    let url = env.url(&format!("/tasks/{tid}/takeovers"));

    let (status, claimed) = env
        .json(Method::POST, &url, &env.owner_token, Some(&b), Some(takeover_body("k1", 1, None, "A is gone")))
        .await;
    assert_eq!(status, 201, "{claimed}");
    assert_eq!(claimed["task"]["owner_session_id"], b.id.as_str());
    assert_eq!(claimed["task"]["version"], 2);
    assert_eq!(claimed["takeover"]["outcome"], "claimed");
    assert_eq!(claimed["takeover"]["actor_kind"], "session");
    assert_eq!(claimed["takeover"]["previous_task_version"], 1);
    assert_eq!(claimed["takeover"]["new_task_version"], 2);
    assert_eq!(claimed["replayed"], false);

    // Same key, same body: same takeover, nothing changes.
    let (status, replay) = env
        .json(Method::POST, &url, &env.owner_token, Some(&b), Some(takeover_body("k1", 1, None, "A is gone")))
        .await;
    assert_eq!(status, 200, "{replay}");
    assert_eq!(replay["replayed"], true);
    assert_eq!(replay["takeover"]["id"], claimed["takeover"]["id"]);
    assert_eq!(replay["task"]["version"], 2);

    // Same key, different body.
    let (status, reused) = env
        .json(Method::POST, &url, &env.owner_token, Some(&b), Some(takeover_body("k1", 1, None, "other reason")))
        .await;
    assert_eq!(status, 409);
    assert_eq!(reused["error"]["code"], "IDEMPOTENCY_KEY_REUSED");

    assert_eq!(takeovers_of(&env, tid).await.len(), 1);
}

#[tokio::test]
async fn p01_a_possibly_active_owner_blocks_a_claim_until_it_ends_or_expires() {
    let env = Env::new().await;
    let a = env.session(&env.owner_token, "A").await;
    let b = env.session(&env.owner_token, "B").await;
    let task = new_task(&env, &env.owner_token, &a, json!({"owner_session_id": a.id})).await;
    let tid = task["id"].as_str().unwrap();
    let url = env.url(&format!("/tasks/{tid}/takeovers"));
    let claim = |key: &'static str| {
        let url = url.clone();
        let b = b.clone();
        let env = &env;
        let a_id = a.id.clone();
        async move {
            env.json(
                Method::POST,
                &url,
                &env.owner_token,
                Some(&b),
                Some(takeover_body(key, 1, Some(&a_id), "take over")),
            )
            .await
        }
    };

    // Online owner.
    let (status, body) = claim("c1").await;
    assert_eq!(status, 409, "{body}");
    assert_eq!(body["error"]["code"], "OWNER_MAY_BE_ACTIVE");
    assert_eq!(body["error"]["details"]["owner_state"], "online");

    // A missed heartbeat proves nothing: still blocked.
    expire_session(&env, &a.id, Some(Duration::minutes(30)), false).await;
    let (status, body) = claim("c2").await;
    assert_eq!(status, 409, "{body}");
    assert_eq!(body["error"]["details"]["owner_state"], "offline");

    // Expired owner: claimable. Nothing was written by the refusals.
    expire_session(&env, &a.id, None, true).await;
    let (status, body) = claim("c3").await;
    assert_eq!(status, 201, "{body}");
    assert_eq!(body["task"]["owner_session_id"], b.id.as_str());
    assert_eq!(body["takeover"]["from_session_id"], a.id.as_str());
    assert_eq!(takeovers_of(&env, tid).await.len(), 1);
}

#[tokio::test]
async fn p01_an_ended_owner_can_be_replaced() {
    let env = Env::new().await;
    let a = env.session(&env.owner_token, "A").await;
    let b = env.session(&env.owner_token, "B").await;
    let task = new_task(&env, &env.owner_token, &a, json!({"owner_session_id": a.id})).await;
    let tid = task["id"].as_str().unwrap();
    let (status, _) = env
        .json(Method::POST, &env.url(&format!("/sessions/{}/end", a.id)), &env.owner_token, Some(&a), None)
        .await;
    assert_eq!(status, 204);
    let (status, body) = env
        .json(
            Method::POST,
            &env.url(&format!("/tasks/{tid}/takeovers")),
            &env.owner_token,
            Some(&b),
            Some(takeover_body("k", 1, Some(&a.id), "A ended")),
        )
        .await;
    assert_eq!(status, 201, "{body}");
}

#[tokio::test]
async fn p02_racing_claims_have_one_winner_and_consistent_history() {
    let env = Env::new().await;
    let a = env.session(&env.owner_token, "A").await;
    let task = new_task(&env, &env.owner_token, &a, json!({})).await;
    let tid = task["id"].as_str().unwrap().to_string();
    let url = env.url(&format!("/tasks/{tid}/takeovers"));

    let mut sessions = Vec::new();
    for i in 0..5 {
        sessions.push(env.session(&env.owner_token, &format!("racer{i}")).await);
    }
    let results = futures_join_all(sessions.iter().enumerate().map(|(i, s)| {
        let url = url.clone();
        let env = &env;
        async move {
            env.json(
                Method::POST,
                &url,
                &env.owner_token,
                Some(s),
                Some(takeover_body(&format!("race{i}"), 1, None, "me")),
            )
            .await
        }
    }))
    .await;
    let winners: Vec<_> = results.iter().filter(|(s, _)| *s == 201).collect();
    assert_eq!(winners.len(), 1, "exactly one claim wins: {results:?}");
    for (status, body) in results.iter().filter(|(s, _)| *s != 201) {
        assert_eq!(*status, 409, "{body}");
        assert_eq!(body["error"]["code"], "VERSION_CONFLICT");
    }

    let history = takeovers_of(&env, &tid).await;
    assert_eq!(history.len(), 1, "history never disagrees with the task");
    let (_, task_now) = env.json(Method::GET, &env.url(&format!("/tasks/{tid}")), &env.owner_token, None, None).await;
    assert_eq!(task_now["owner_session_id"], history[0]["to_session_id"]);
    assert_eq!(task_now["version"], 2);
}

#[tokio::test]
async fn p01_stale_version_finished_tasks_and_missing_write_access_are_refused() {
    let env = Env::new().await;
    let a = env.session(&env.owner_token, "A").await;
    let b = env.session(&env.owner_token, "B").await;
    let task = new_task(&env, &env.owner_token, &a, json!({})).await;
    let tid = task["id"].as_str().unwrap();
    let url = env.url(&format!("/tasks/{tid}/takeovers"));

    let (status, body) = env
        .json(Method::POST, &url, &env.owner_token, Some(&b), Some(takeover_body("s", 7, None, "x")))
        .await;
    assert_eq!(status, 409, "{body}");
    assert_eq!(body["error"]["code"], "VERSION_CONFLICT");
    assert_eq!(body["error"]["details"]["current_version"], 1);

    // Missing session credentials.
    let (status, _) = env
        .json(Method::POST, &url, &env.owner_token, None, Some(takeover_body("n", 1, None, "x")))
        .await;
    assert_eq!(status, 400);

    // Finished tasks are not taken over.
    let (status, _) = env
        .json(Method::PATCH, &env.url(&format!("/tasks/{tid}")), &env.owner_token, Some(&a), Some(json!({"expected_version": 1, "status": "done"})))
        .await;
    assert_eq!(status, 200);
    let (status, body) = env
        .json(Method::POST, &url, &env.owner_token, Some(&b), Some(takeover_body("d", 2, None, "x")))
        .await;
    assert_eq!(status, 409, "{body}");
    assert_eq!(body["error"]["code"], "TASK_DONE");

    // A read-only collaborator cannot write, even with someone's session.
    let (reader, reader_id) = env.user("reader").await;
    env.add_collaborator(reader_id, "read").await;
    let (status, _) = env
        .json(Method::POST, &url, &reader, Some(&b), Some(takeover_body("r", 2, None, "x")))
        .await;
    assert_eq!(status, 403);
    let (status, _) = env
        .json(
            Method::POST,
            &env.url(&format!("/tasks/{tid}/reassign")),
            &reader,
            None,
            Some(json!({"client_takeover_id": "r2", "expected_task_version": 2, "to_session_id": b.id, "reason": "x"})),
        )
        .await;
    assert_eq!(status, 403);
}

#[tokio::test]
async fn p03_user_reassignment_records_the_reason_and_leaves_handoffs_alone() {
    let env = Env::new().await;
    let a = env.session(&env.owner_token, "A").await;
    let b = env.session(&env.owner_token, "B").await;
    let c = env.session(&env.owner_token, "C").await;
    let task = new_task(&env, &env.owner_token, &a, json!({"owner_session_id": a.id})).await;
    let tid = task["id"].as_str().unwrap();

    // A hands the task to B; the handoff stays pending.
    let (status, handoff) = env
        .json(
            Method::POST,
            &env.url(&format!("/tasks/{tid}/handoffs")),
            &env.owner_token,
            Some(&a),
            Some(json!({"client_handoff_id": "h1", "to_session_id": b.id, "summary": "yours"})),
        )
        .await;
    assert_eq!(status, 201, "{handoff}");

    // The (online) owner cannot be displaced by a claim, but a user may reassign.
    let (status, body) = env
        .json(
            Method::POST,
            &env.url(&format!("/tasks/{tid}/reassign")),
            &env.owner_token,
            None,
            Some(json!({
                "client_takeover_id": "re1",
                "expected_task_version": 1,
                "expected_owner_session_id": a.id,
                "to_session_id": c.id,
                "reason": "A is stuck",
            })),
        )
        .await;
    assert_eq!(status, 201, "{body}");
    assert_eq!(body["task"]["owner_session_id"], c.id.as_str());
    assert_eq!(body["takeover"]["outcome"], "reassigned");
    assert_eq!(body["takeover"]["actor_kind"], "user");
    assert_eq!(body["takeover"]["actor_id"], env.owner_id.to_string());
    assert_eq!(body["takeover"]["reason"], "A is stuck");
    assert_eq!(body["takeover"]["origin"], "user_content");

    let (_, handoffs) = env
        .json(Method::GET, &env.url(&format!("/tasks/{tid}/handoffs")), &env.owner_token, None, None)
        .await;
    assert_eq!(handoffs["items"][0]["status"], "pending", "reassignment never decides a handoff");

    // Reassigning to a session that does not exist is a client error.
    let (status, _) = env
        .json(
            Method::POST,
            &env.url(&format!("/tasks/{tid}/reassign")),
            &env.owner_token,
            None,
            Some(json!({"client_takeover_id": "re2", "expected_task_version": 2, "expected_owner_session_id": c.id,
                "to_session_id": uuid::Uuid::new_v4().to_string(), "reason": "x"})),
        )
        .await;
    assert_eq!(status, 400);
}

// ---------------------------------------------------------------- questions

async fn ask(env: &Env, tid: &str, who: Option<&Sess>, extra: Value) -> (u16, Value) {
    let mut body = json!({
        "client_question_id": uuid::Uuid::new_v4().to_string(),
        "recipient": {"kind": "repo_writers"},
        "title": "Which database?",
        "body": "SQLite or PostgreSQL for the cache?",
    });
    body.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
    env.json(Method::POST, &env.url(&format!("/tasks/{tid}/questions")), &env.owner_token, who, Some(body))
        .await
}

fn answer_body(q: &Value, key: &str, body: &str, option: Option<&str>) -> Value {
    json!({
        "client_answer_id": key,
        "expected_version": q["version"],
        "body": body,
        "option_id": option,
        "acknowledged_context_hash": q["context_hash"],
    })
}

async fn get_q(env: &Env, id: &str) -> Value {
    let (status, q) = env
        .json(Method::GET, &env.url(&format!("/questions/{id}")), &env.owner_token, None, None)
        .await;
    assert_eq!(status, 200, "{q}");
    q
}

#[tokio::test]
async fn p05_a_session_asks_and_a_user_answers_without_a_session() {
    let env = Env::new().await;
    let a = env.session(&env.owner_token, "A").await;
    let task = new_task(&env, &env.owner_token, &a, json!({})).await;
    let tid = task["id"].as_str().unwrap();

    let (status, q) = ask(&env, tid, Some(&a), json!({"options": ["SQLite", "PostgreSQL"]})).await;
    assert_eq!(status, 201, "{q}");
    assert_eq!(q["status"], "open");
    assert_eq!(q["creator_kind"], "session");
    assert_eq!(q["options"][0]["id"], "o1");
    assert_eq!(q["options"][1]["label"], "PostgreSQL");
    assert_eq!(q["context"]["task_version"], 1);
    assert_eq!(q["origin"], "user_content");
    let qid = q["id"].as_str().unwrap();

    let open = get_q(&env, qid).await;
    assert_eq!(open["context_freshness"], "fresh");
    assert!(open["answer"].is_null());

    // The user answers through the account alone.
    let url = env.url(&format!("/questions/{qid}/answer"));
    let (status, answered) = env
        .json(Method::POST, &url, &env.owner_token, None, Some(answer_body(&open, "ans1", "use pg", Some("o2"))))
        .await;
    assert_eq!(status, 200, "{answered}");
    assert_eq!(answered["status"], "answered");
    assert_eq!(answered["version"], 2);
    assert_eq!(answered["answer"]["option_id"], "o2");
    assert_eq!(answered["answer"]["actor_kind"], "user");
    assert_eq!(answered["answer"]["actor_id"], env.owner_id.to_string());

    // A retry of the same answer replays; a different answer finds it closed.
    let (status, replay) = env
        .json(Method::POST, &url, &env.owner_token, None, Some(answer_body(&open, "ans1", "use pg", Some("o2"))))
        .await;
    assert_eq!(status, 200, "{replay}");
    assert_eq!(replay["answer"]["id"], answered["answer"]["id"]);
    let (status, closed) = env
        .json(Method::POST, &url, &env.owner_token, None, Some(answer_body(&open, "ans2", "use sqlite", Some("o1"))))
        .await;
    assert_eq!(status, 409);
    assert_eq!(closed["error"]["code"], "QUESTION_NOT_OPEN");
    assert_eq!(closed["error"]["details"]["status"], "answered");
    let (status, reused) = env
        .json(Method::POST, &url, &env.owner_token, None, Some(answer_body(&open, "ans1", "something else", Some("o1"))))
        .await;
    assert_eq!(status, 409);
    assert_eq!(reused["error"]["code"], "IDEMPOTENCY_KEY_REUSED");

    // Listing filters by status.
    let (_, listed) = env
        .json(Method::GET, &env.url(&format!("/tasks/{tid}/questions?status=open")), &env.owner_token, None, None)
        .await;
    assert!(listed["items"].as_array().unwrap().is_empty());
    let (_, listed) = env
        .json(Method::GET, &env.url(&format!("/tasks/{tid}/questions?status=answered")), &env.owner_token, None, None)
        .await;
    assert_eq!(listed["items"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn p06_answer_and_cancel_race_leaves_exactly_one_terminal_state() {
    let env = Env::new().await;
    let a = env.session(&env.owner_token, "A").await;
    let task = new_task(&env, &env.owner_token, &a, json!({})).await;
    let tid = task["id"].as_str().unwrap();

    for round in 0..4 {
        let (_, q) = ask(&env, tid, Some(&a), json!({})).await;
        let qid = q["id"].as_str().unwrap().to_string();
        let answer_url = env.url(&format!("/questions/{qid}/answer"));
        let cancel_url = env.url(&format!("/questions/{qid}/cancel"));
        let answer = answer_body(&q, &format!("ra{round}"), "yes", None);
        let results = futures_join_all(vec![
            Box::pin(env.json(Method::POST, &answer_url, &env.owner_token, None, Some(answer))) as std::pin::Pin<Box<dyn std::future::Future<Output = (u16, Value)> + '_>>,
            Box::pin(env.json(Method::POST, &cancel_url, &env.owner_token, Some(&a), Some(json!({"expected_version": q["version"]})))),
        ])
        .await;
        let ok = results.iter().filter(|(s, _)| *s == 200).count();
        assert_eq!(ok, 1, "exactly one terminal transition wins: {results:?}");
        let loser = results.iter().find(|(s, _)| *s != 200).unwrap();
        assert_eq!(loser.0, 409);
        let code = loser.1["error"]["code"].as_str().unwrap();
        assert!(code == "QUESTION_NOT_OPEN" || code == "VERSION_CONFLICT", "{loser:?}");

        let final_q = get_q(&env, &qid).await;
        let status = final_q["status"].as_str().unwrap();
        assert!(status == "answered" || status == "cancelled");
        assert_eq!(final_q["answer"].is_null(), status == "cancelled", "an answer exists iff answered");
    }
}

#[tokio::test]
async fn p07_a_changed_task_makes_the_context_stale_and_blocks_the_answer() {
    let env = Env::new().await;
    let a = env.session(&env.owner_token, "A").await;
    let task = new_task(&env, &env.owner_token, &a, json!({})).await;
    let tid = task["id"].as_str().unwrap();
    let (_, q) = ask(&env, tid, Some(&a), json!({})).await;
    let qid = q["id"].as_str().unwrap();
    let seen = get_q(&env, qid).await;

    let (status, _) = env
        .json(Method::PATCH, &env.url(&format!("/tasks/{tid}")), &env.owner_token, Some(&a), Some(json!({"expected_version": 1, "status": "blocked"})))
        .await;
    assert_eq!(status, 200);

    let stale = get_q(&env, qid).await;
    assert_eq!(stale["context_freshness"], "stale");
    let (status, body) = env
        .json(Method::POST, &env.url(&format!("/questions/{qid}/answer")), &env.owner_token, None, Some(answer_body(&seen, "late", "go", None)))
        .await;
    assert_eq!(status, 409, "{body}");
    assert_eq!(body["error"]["code"], "CONTEXT_STALE");
    assert_eq!(body["error"]["details"]["current_task_version"], 2);
    assert_eq!(get_q(&env, qid).await["status"], "open", "nothing was written");

    // A question asked against an outdated task version is refused up front.
    let (status, body) = ask(&env, tid, Some(&a), json!({"context": {"task_version": 1}})).await;
    assert_eq!(status, 409, "{body}");
    assert_eq!(body["error"]["code"], "VERSION_CONFLICT");
}

#[tokio::test]
async fn p07_branch_head_decides_freshness_of_commit_context_or_says_unknown() {
    let env = Env::new().await;
    let first = env.push_tree(&env.repo, "feature", &[("a.txt", "1")], &[]);
    let a = env.session(&env.owner_token, "A").await;
    let on_branch = new_task(&env, &env.owner_token, &a, json!({"branch": "feature"})).await;
    let no_branch = new_task(&env, &env.owner_token, &a, json!({})).await;

    let (status, q) = ask(&env, on_branch["id"].as_str().unwrap(), Some(&a), json!({"context": {"head_sha": first}})).await;
    assert_eq!(status, 201, "{q}");
    assert_eq!(get_q(&env, q["id"].as_str().unwrap()).await["context_freshness"], "fresh");

    // The branch moves on: the commit the question was asked about is outdated.
    env.push_tree(&env.repo, "feature", &[("a.txt", "2")], &[]);
    assert_eq!(get_q(&env, q["id"].as_str().unwrap()).await["context_freshness"], "stale");

    // Without a declared branch there is nothing to compare with.
    let (_, q2) = ask(&env, no_branch["id"].as_str().unwrap(), Some(&a), json!({"context": {"head_sha": first}})).await;
    assert_eq!(get_q(&env, q2["id"].as_str().unwrap()).await["context_freshness"], "unknown");

    // A commit that does not exist is rejected.
    let (status, _) = ask(&env, no_branch["id"].as_str().unwrap(), Some(&a), json!({"context": {"head_sha": "0".repeat(40)}})).await;
    assert_eq!(status, 400);
}

#[tokio::test]
async fn p08_recipients_creators_and_access_are_enforced() {
    let env = Env::new().await;
    let a = env.session(&env.owner_token, "A").await;
    let b = env.session(&env.owner_token, "B").await;
    let task = new_task(&env, &env.owner_token, &a, json!({})).await;
    let tid = task["id"].as_str().unwrap();

    // Addressed to session B: A (and the user door) are not recipients.
    let (status, q) = ask(&env, tid, Some(&a), json!({"recipient": {"kind": "session", "id": b.id}})).await;
    assert_eq!(status, 201, "{q}");
    let qid = q["id"].as_str().unwrap();
    let url = env.url(&format!("/questions/{qid}/answer"));
    for who in [Some(&a), None] {
        let (status, _) = env.json(Method::POST, &url, &env.owner_token, who, Some(answer_body(&q, "x", "no", None))).await;
        assert_eq!(status, 403);
    }
    // Wrong acknowledged hash and a free-text answer without text are 400s.
    let mut bad = answer_body(&q, "y", "ok", None);
    bad["acknowledged_context_hash"] = json!("0".repeat(64));
    let (status, _) = env.json(Method::POST, &url, &env.owner_token, Some(&b), Some(bad)).await;
    assert_eq!(status, 400);
    let (status, _) = env.json(Method::POST, &url, &env.owner_token, Some(&b), Some(answer_body(&q, "z", "  ", None))).await;
    assert_eq!(status, 400);
    let (status, answered) = env.json(Method::POST, &url, &env.owner_token, Some(&b), Some(answer_body(&q, "w", "fine", None))).await;
    assert_eq!(status, 200, "{answered}");
    assert_eq!(answered["answer"]["actor_kind"], "session");

    // Option rules.
    let (_, with_options) = ask(&env, tid, Some(&a), json!({"options": ["yes", "no"]})).await;
    let oid = with_options["id"].as_str().unwrap();
    let ourl = env.url(&format!("/questions/{oid}/answer"));
    let (status, _) = env.json(Method::POST, &ourl, &env.owner_token, None, Some(answer_body(&with_options, "o", "x", None))).await;
    assert_eq!(status, 400, "an option is required");
    let (status, _) = env.json(Method::POST, &ourl, &env.owner_token, None, Some(answer_body(&with_options, "o", "x", Some("o9")))).await;
    assert_eq!(status, 400, "unknown option");

    // Only the creator cancels.
    let (status, _) = env
        .json(Method::POST, &env.url(&format!("/questions/{oid}/cancel")), &env.owner_token, Some(&b), Some(json!({"expected_version": 1})))
        .await;
    assert_eq!(status, 403);
    let (status, cancelled) = env
        .json(Method::POST, &env.url(&format!("/questions/{oid}/cancel")), &env.owner_token, Some(&a), Some(json!({"expected_version": 1})))
        .await;
    assert_eq!(status, 200, "{cancelled}");
    assert_eq!(cancelled["status"], "cancelled");

    // Recipients must exist; strangers see nothing; readers read but do not write.
    let (status, _) = ask(&env, tid, Some(&a), json!({"recipient": {"kind": "session", "id": uuid::Uuid::new_v4().to_string()}})).await;
    assert_eq!(status, 400);
    let (stranger, _) = env.user("stranger").await;
    let (status, _) = env.json(Method::GET, &env.url(&format!("/questions/{qid}")), &stranger, None, None).await;
    assert_eq!(status, 403);
    let (reader, reader_id) = env.user("reader").await;
    env.add_collaborator(reader_id, "read").await;
    let (status, _) = env.json(Method::GET, &env.url(&format!("/questions/{qid}")), &reader, None, None).await;
    assert_eq!(status, 200);
    let (status, _) = env.json(Method::POST, &env.url(&format!("/tasks/{tid}/questions")), &reader, None,
        Some(json!({"client_question_id": "q", "recipient": {"kind": "repo_writers"}, "title": "t", "body": "b"}))).await;
    assert_eq!(status, 403);

    // A question of another repository's task is a 404.
    let other = Env::new().await;
    let (status, _) = other.json(Method::GET, &other.url(&format!("/questions/{qid}")), &other.owner_token, None, None).await;
    assert_eq!(status, 404);
}

// ------------------------------------------------------------ work packages

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

#[tokio::test]
async fn p09_packages_are_numbered_immutable_and_idempotent() {
    let env = Env::new().await;
    let a = env.session(&env.owner_token, "A").await;
    let task = new_task(&env, &env.owner_token, &a, json!({})).await;
    let tid = task["id"].as_str().unwrap();
    let url = env.url(&format!("/tasks/{tid}/work-packages"));

    let (status, p1) = env.json(Method::POST, &url, &env.owner_token, Some(&a), Some(package("w1"))).await;
    assert_eq!(status, 201, "{p1}");
    assert_eq!(p1["revision"], 1);
    assert_eq!(p1["author_kind"], "session");
    assert_eq!(p1["origin"], "user_content");
    assert_eq!(p1["missing_fields"], json!(["commit_sha", "evidence_refs"]));

    let (status, replay) = env.json(Method::POST, &url, &env.owner_token, Some(&a), Some(package("w1"))).await;
    assert_eq!(status, 200, "{replay}");
    assert_eq!(replay["id"], p1["id"]);
    let mut changed = package("w1");
    changed["goal"] = json!("something else");
    let (status, reused) = env.json(Method::POST, &url, &env.owner_token, Some(&a), Some(changed)).await;
    assert_eq!(status, 409);
    assert_eq!(reused["error"]["code"], "IDEMPOTENCY_KEY_REUSED");

    let sha = env.push_tree(&env.repo, "main", &[("x", "y")], &[]);
    let mut second = package("w2");
    second["commit_sha"] = json!(sha);
    second["expected_task_version"] = json!(1);
    let (status, p2) = env.json(Method::POST, &url, &env.owner_token, None, Some(second)).await;
    assert_eq!(status, 201, "{p2}");
    assert_eq!(p2["revision"], 2);
    assert_eq!(p2["author_kind"], "user");
    assert_eq!(p2["commit_sha"], sha.as_str());

    // Newest first; the earlier revision is unchanged.
    let (_, listed) = env.json(Method::GET, &url, &env.owner_token, None, None).await;
    let revisions: Vec<i64> = listed["items"].as_array().unwrap().iter().map(|p| p["revision"].as_i64().unwrap()).collect();
    assert_eq!(revisions, vec![2, 1]);
    let (_, first_again) = env
        .json(Method::GET, &env.url(&format!("/work-packages/{}", p1["id"].as_str().unwrap())), &env.owner_token, None, None)
        .await;
    assert_eq!(first_again["goal"], "ship the cache");
    assert_eq!(first_again["evidence_resolution"], json!([]));

    // Stale task version, unknown commit and oversized input are refused.
    let mut stale = package("w3");
    stale["expected_task_version"] = json!(9);
    let (status, body) = env.json(Method::POST, &url, &env.owner_token, Some(&a), Some(stale)).await;
    assert_eq!(status, 409, "{body}");
    assert_eq!(body["error"]["code"], "VERSION_CONFLICT");
    let mut ghost = package("w4");
    ghost["commit_sha"] = json!("1".repeat(40));
    let (status, _) = env.json(Method::POST, &url, &env.owner_token, Some(&a), Some(ghost)).await;
    assert_eq!(status, 400);
    let mut big = package("w5");
    big["completed"] = json!(vec!["x"; 51]);
    let (status, _) = env.json(Method::POST, &url, &env.owner_token, Some(&a), Some(big)).await;
    assert_eq!(status, 400);
    let mut bad_kind = package("w6");
    bad_kind["evidence_refs"] = json!([{"kind": "tests-passed", "reference": "yes"}]);
    let (status, _) = env.json(Method::POST, &url, &env.owner_token, Some(&a), Some(bad_kind)).await;
    assert_eq!(status, 400);
}

#[tokio::test]
async fn p09_concurrent_packages_get_distinct_revisions() {
    let env = Env::new().await;
    let a = env.session(&env.owner_token, "A").await;
    let task = new_task(&env, &env.owner_token, &a, json!({})).await;
    let tid = task["id"].as_str().unwrap().to_string();
    let url = env.url(&format!("/tasks/{tid}/work-packages"));
    let results = futures_join_all((0..6).map(|i| {
        let url = url.clone();
        let env = &env;
        let a = a.clone();
        async move { env.json(Method::POST, &url, &env.owner_token, Some(&a), Some(package(&format!("c{i}")))).await }
    }))
    .await;
    assert!(results.iter().all(|(s, _)| *s == 201), "{results:?}");
    let mut revisions: Vec<i64> = results.iter().map(|(_, b)| b["revision"].as_i64().unwrap()).collect();
    revisions.sort();
    assert_eq!(revisions, vec![1, 2, 3, 4, 5, 6]);
}

#[tokio::test]
async fn p10_only_platform_pipelines_of_this_repo_are_resolved_as_evidence() {
    let env = Env::new().await;
    let a = env.session(&env.owner_token, "A").await;
    let task = new_task(&env, &env.owner_token, &a, json!({})).await;
    let tid = task["id"].as_str().unwrap();
    let sha = env.push_tree(&env.repo, "main", &[("x", "y")], &[]);

    let pipeline = rg_db::ops::pipeline_ops::create_pipeline(&env.db, env.repo_id, &sha, "main", "push", None)
        .await
        .unwrap();
    rg_db::ops::pipeline_ops::update_pipeline_status(&env.db, pipeline.id, "success", None, Some(Utc::now().naive_utc()))
        .await
        .unwrap();
    let other = Env::new().await;
    let foreign = rg_db::ops::pipeline_ops::create_pipeline(&other.db, other.repo_id, &sha, "main", "push", None)
        .await
        .unwrap();

    let mut body = package("e1");
    body["evidence_refs"] = json!([
        {"kind": "pipeline", "reference": pipeline.id.to_string()},
        {"kind": "pipeline", "reference": "999999"},
        {"kind": "pipeline", "reference": foreign.id.to_string()},
        {"kind": "note", "reference": "all tests pass, trust me"},
    ]);
    let (status, created) = env
        .json(Method::POST, &env.url(&format!("/tasks/{tid}/work-packages")), &env.owner_token, Some(&a), Some(body))
        .await;
    assert_eq!(status, 201, "{created}");
    let (_, read) = env
        .json(Method::GET, &env.url(&format!("/work-packages/{}", created["id"].as_str().unwrap())), &env.owner_token, None, None)
        .await;
    let resolution = read["evidence_resolution"].as_array().unwrap();
    assert_eq!(resolution.len(), 3, "only pipeline references are looked up: {resolution:?}");
    assert_eq!(resolution[0]["verification"], "verified_reference");
    assert_eq!(resolution[0]["status"], "success");
    assert_eq!(resolution[0]["commit_sha"], sha.as_str());
    assert_eq!(resolution[1]["verification"], "unavailable");
    // Another repository's pipeline reads exactly like a missing one when the
    // ids happen to collide in this database.
    if foreign.id != pipeline.id {
        assert_eq!(resolution[2]["verification"], "unavailable");
    }
    // The author's own claim stays a claim.
    assert_eq!(read["evidence_refs"][3]["reference"], "all tests pass, trust me");
}

/// Run futures concurrently so the requests overlap on the server.
async fn futures_join_all<F: std::future::Future>(futs: impl IntoIterator<Item = F>) -> Vec<F::Output> {
    futures::future::join_all(futs).await
}

// -------------------------------------------------------------------- inbox

async fn inbox(env: &Env, token: &str, who: Option<&Sess>, path: &str, query: &str) -> (u16, Value) {
    env.json(Method::GET, &env.url(&format!("{path}{query}")), token, who, None).await
}

fn keys(body: &Value) -> Vec<String> {
    body["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["type"].as_str().unwrap().to_string())
        .collect()
}

#[tokio::test]
async fn p11_inbox_projects_live_records_and_drops_resolved_ones() {
    let env = Env::new().await;
    let a = env.session(&env.owner_token, "A").await;
    let b = env.session(&env.owner_token, "B").await;
    let task = new_task(&env, &env.owner_token, &a, json!({"owner_session_id": a.id})).await;
    let tid = task["id"].as_str().unwrap();

    // A hands off to B, asks B a question, and asks all writers another.
    let (status, _) = env
        .json(Method::POST, &env.url(&format!("/tasks/{tid}/handoffs")), &env.owner_token, Some(&a),
            Some(json!({"client_handoff_id": "h", "to_session_id": b.id, "summary": "yours now"})))
        .await;
    assert_eq!(status, 201);
    let (_, to_b) = ask(&env, tid, Some(&a), json!({"recipient": {"kind": "session", "id": b.id}, "title": "for B"})).await;
    let (_, to_writers) = ask(&env, tid, Some(&a), json!({"title": "for writers"})).await;

    // B: handoff + both questions. A: its owned task only (own questions are not "for" A).
    let (status, b_inbox) = inbox(&env, &env.owner_token, Some(&b), "/inbox", "").await;
    assert_eq!(status, 200, "{b_inbox}");
    let mut b_types = keys(&b_inbox);
    b_types.sort();
    assert_eq!(b_types, vec!["open_question", "open_question", "pending_handoff"]);
    assert_eq!(b_inbox["consistency"], "live_query");
    assert_eq!(b_inbox["truncated"], false);
    let (_, a_inbox) = inbox(&env, &env.owner_token, Some(&a), "/inbox", "").await;
    assert_eq!(keys(&a_inbox), vec!["assigned_task"]);

    // The user door sees the writers question, not the one addressed to B.
    let (status, user_inbox) = inbox(&env, &env.owner_token, None, "/inbox/user", "").await;
    assert_eq!(status, 200, "{user_inbox}");
    let user_titles: Vec<_> = user_inbox["items"].as_array().unwrap().iter().map(|i| i["title"].as_str().unwrap()).collect();
    assert_eq!(user_titles, vec!["for writers"]);

    // The user answers: it leaves the user's and B's inbox and comes back to A.
    let q = get_q(&env, to_writers["id"].as_str().unwrap()).await;
    let (status, _) = env
        .json(Method::POST, &env.url(&format!("/questions/{}/answer", q["id"].as_str().unwrap())), &env.owner_token, None,
            Some(answer_body(&q, "ans", "go ahead", None)))
        .await;
    assert_eq!(status, 200);
    let (_, user_after) = inbox(&env, &env.owner_token, None, "/inbox/user", "").await;
    assert!(user_after["items"].as_array().unwrap().is_empty());
    let (_, b_after) = inbox(&env, &env.owner_token, Some(&b), "/inbox", "").await;
    assert_eq!(b_after["items"].as_array().unwrap().iter().filter(|i| i["type"] == "open_question").count(), 1);
    let (_, a_after) = inbox(&env, &env.owner_token, Some(&a), "/inbox", "").await;
    let mut a_types = keys(&a_after);
    a_types.sort();
    assert_eq!(a_types, vec!["assigned_task", "question_answer"]);
    assert_eq!(
        a_after["items"].as_array().unwrap().iter().find(|i| i["type"] == "question_answer").unwrap()["object_id"],
        to_writers["id"]
    );

    // Accepting the handoff removes it; `since` and `limit` narrow the view.
    let (_, handoffs) = env.json(Method::GET, &env.url(&format!("/tasks/{tid}/handoffs")), &env.owner_token, None, None).await;
    let hid = handoffs["items"][0]["id"].as_str().unwrap();
    let (status, _) = env
        .json(Method::POST, &env.url(&format!("/handoffs/{hid}/accept")), &env.owner_token, Some(&b), Some(json!({"expected_version": 1})))
        .await;
    assert_eq!(status, 200);
    let (_, b_final) = inbox(&env, &env.owner_token, Some(&b), "/inbox", "").await;
    assert!(!keys(&b_final).contains(&"pending_handoff".to_string()));
    let future = (Utc::now() + Duration::minutes(5)).to_rfc3339().replace('+', "%2B");
    let (_, none_since) = inbox(&env, &env.owner_token, Some(&a), "/inbox", &format!("?since={future}")).await;
    assert!(none_since["items"].as_array().unwrap().is_empty());
    let (_, limited) = inbox(&env, &env.owner_token, Some(&a), "/inbox", "?limit=1").await;
    assert_eq!(limited["items"].as_array().unwrap().len(), 1);
    assert_eq!(limited["truncated"], true);
    let _ = to_b;
}

#[tokio::test]
async fn p12_inbox_access_rules() {
    let env = Env::new().await;
    let a = env.session(&env.owner_token, "A").await;

    let (status, _) = inbox(&env, &env.owner_token, None, "/inbox", "").await;
    assert_eq!(status, 400, "the session inbox needs session credentials");
    let (status, _) = inbox(&env, &env.owner_token, Some(&a), "/inbox", "?since=yesterday").await;
    assert_eq!(status, 400);
    let (stranger, _) = env.user("stranger").await;
    assert_eq!(inbox(&env, &stranger, None, "/inbox/user", "").await.0, 403);
    let (reader, reader_id) = env.user("reader").await;
    env.add_collaborator(reader_id, "read").await;
    assert_eq!(inbox(&env, &reader, None, "/inbox/user", "").await.0, 403, "readers cannot answer, so there is nothing for them");
    let resp = env.send(Method::GET, &env.url("/inbox/user"), None, None, None).await;
    assert_eq!(resp.status(), 401);
}

// ---------------------------------------------------------------- retention

async fn count_rows(env: &Env, table: &str, repo_id: i64) -> i64 {
    use rg_db::sea_orm::{ConnectionTrait, Statement};
    let backend = env.db.get_database_backend();
    let row = env
        .db
        .query_one(Statement::from_string(
            backend,
            format!("SELECT COUNT(*) AS n FROM {table} WHERE repo_id = {repo_id}"),
        ))
        .await
        .unwrap()
        .unwrap();
    row.try_get::<i64>("", "n").unwrap()
}

async fn seed_everything(env: &Env, task_owner: &Sess, token: &str) -> String {
    let task = new_task(env, token, task_owner, json!({"owner_session_id": task_owner.id})).await;
    let tid = task["id"].as_str().unwrap().to_string();
    let (status, _) = env
        .json(Method::POST, &env.url(&format!("/tasks/{tid}/messages")), token, Some(task_owner),
            Some(json!({"client_message_id": "m1", "kind": "note", "body": "hello"})))
        .await;
    assert_eq!(status, 201);
    let (status, q) = env
        .json(Method::POST, &env.url(&format!("/tasks/{tid}/questions")), token, Some(task_owner),
            Some(json!({"client_question_id": "q", "recipient": {"kind": "repo_writers"}, "title": "t", "body": "b"})))
        .await;
    assert_eq!(status, 201, "{q}");
    let q = get_q(env, q["id"].as_str().unwrap()).await;
    let (status, _) = env
        .json(Method::POST, &env.url(&format!("/questions/{}/answer", q["id"].as_str().unwrap())), &env.owner_token, None,
            Some(answer_body(&q, "a", "ok", None)))
        .await;
    assert_eq!(status, 200);
    let (status, _) = env
        .json(Method::POST, &env.url(&format!("/tasks/{tid}/work-packages")), token, Some(task_owner), Some(package("wp")))
        .await;
    assert_eq!(status, 201);
    tid
}

#[tokio::test]
async fn o03a_repository_delete_is_soft_and_keeps_every_collaboration_record() {
    let env = Env::new().await;
    let a = env.session(&env.owner_token, "A").await;
    let b = env.session(&env.owner_token, "B").await;
    seed_everything(&env, &a, &env.owner_token).await;
    let tid = seed_everything(&env, &b, &env.owner_token).await;
    let (status, _) = env
        .json(Method::POST, &env.url(&format!("/tasks/{tid}/takeovers")), &env.owner_token, Some(&a), Some(takeover_body("t", 2, Some(&b.id), "x")))
        .await;
    assert_eq!(status, 409, "B is still online; no takeover row is written");

    let (status, impact) = env
        .json(Method::GET, &env.url("/deletion-impact"), &env.owner_token, None, None)
        .await;
    assert_eq!(status, 200, "{impact}");
    let counts = &impact["records"];
    assert_eq!(impact["on_repository_delete"], "kept");
    assert_eq!(impact["on_repository_row_removal"], "cascade");
    assert_eq!(impact["retention_policy"], "none");
    assert!(impact["git_data_retention_days"].as_i64().unwrap() >= 0);
    for (field, table) in [
        ("sessions", "agent_sessions"),
        ("tasks", "collab_tasks"),
        ("messages", "collab_messages"),
        ("handoffs", "collab_handoffs"),
        ("takeovers", "collab_takeovers"),
        ("questions", "collab_questions"),
        ("answers", "collab_answers"),
        ("work_packages", "collab_work_packages"),
    ] {
        assert_eq!(counts[field], count_rows(&env, table, env.repo_id).await, "{field}");
    }
    assert_eq!(counts["tasks"], 2);
    assert_eq!(counts["questions"], 2);
    assert_eq!(counts["answers"], 2);
    assert_eq!(counts["work_packages"], 2);

    // Only repository admins may look; a writer may not.
    let (writer, writer_id) = env.user("writer").await;
    env.add_collaborator(writer_id, "write").await;
    assert_eq!(env.json(Method::GET, &env.url("/deletion-impact"), &writer, None, None).await.0, 403);

    // The application's delete is soft: the row stays, so no record is lost.
    let resp = env
        .send(Method::DELETE, &format!("{}/api/v1/repos/{}/{}", env.base, env.owner, env.repo), Some(&env.owner_token), None, None)
        .await;
    assert!(resp.status().is_success(), "delete repo: {}", resp.status());
    for (field, table) in [
        ("sessions", "agent_sessions"),
        ("tasks", "collab_tasks"),
        ("messages", "collab_messages"),
        ("questions", "collab_questions"),
        ("answers", "collab_answers"),
        ("work_packages", "collab_work_packages"),
    ] {
        assert_eq!(counts[field], count_rows(&env, table, env.repo_id).await, "{field} kept after soft delete");
    }

    // Removing the repository row itself (database administration) cascades.
    use rg_db::sea_orm::{ConnectionTrait, Statement};
    env.db
        .execute(Statement::from_string(
            env.db.get_database_backend(),
            format!("DELETE FROM repositories WHERE id = {}", env.repo_id),
        ))
        .await
        .expect("remove repository row");
    for table in [
        "agent_sessions", "collab_tasks", "collab_messages", "collab_handoffs",
        "collab_takeovers", "collab_questions", "collab_answers", "collab_work_packages",
    ] {
        assert_eq!(count_rows(&env, table, env.repo_id).await, 0, "{table} after row removal");
    }
}

#[tokio::test]
async fn o03a_account_deletion_preview_matches_what_deletion_removes() {
    let env = Env::new().await;
    let (leaver, leaver_id) = env.user("leaver").await;
    env.add_collaborator(leaver_id, "write").await;
    let owner_session = env.session(&env.owner_token, "owner-agent").await;
    let leaver_session = env.session(&leaver, "leaver-agent").await;

    // A task owned by the leaver's session, with a message from the owner's session.
    let leaver_task = seed_everything(&env, &leaver_session, &leaver).await;
    let (status, _) = env
        .json(Method::POST, &env.url(&format!("/tasks/{leaver_task}/messages")), &env.owner_token, Some(&owner_session),
            Some(json!({"client_message_id": "from-owner", "kind": "note", "body": "hi"})))
        .await;
    assert_eq!(status, 201);
    // A task owned by the owner's session, with a message from the leaver's session.
    let kept = new_task(&env, &env.owner_token, &owner_session, json!({"owner_session_id": owner_session.id})).await;
    let kept_id = kept["id"].as_str().unwrap();
    let (status, _) = env
        .json(Method::POST, &env.url(&format!("/tasks/{kept_id}/messages")), &leaver, Some(&leaver_session),
            Some(json!({"client_message_id": "from-leaver", "kind": "note", "body": "bye"})))
        .await;
    assert_eq!(status, 201);

    let admin_token = {
        let resp: Value = reqwest::Client::new()
            .post(format!("{}/api/v1/users/login", env.base))
            .json(&json!({"username": "bootstrap_admin", "password": "Qz7$wRtm"}))
            .send().await.unwrap().json().await.unwrap();
        resp["token"].as_str().expect("admin login").to_string()
    };
    let impact_url = format!("{}/api/v1/admin/users/{leaver_id}/collaboration-impact", env.base);
    let (status, impact) = env.json(Method::GET, &impact_url, &admin_token, None, None).await;
    assert_eq!(status, 200, "{impact}");
    assert_eq!(impact["sessions_removed"], 1);
    assert_eq!(impact["owned_tasks_removed"], 1);
    assert_eq!(impact["removed_with_owned_tasks"]["messages"], 2, "both participants' messages in the owned task");
    assert_eq!(impact["messages_in_other_tasks_removed"], 1);
    assert_eq!(impact["removed_with_owned_tasks"]["questions"], 1);
    assert_eq!(impact["removed_with_owned_tasks"]["answers"], 1);
    assert_eq!(impact["removed_with_owned_tasks"]["work_packages"], 1);
    assert_eq!(env.json(Method::GET, &impact_url, &leaver, None, None).await.0, 403, "site admins only");

    let before = (
        count_rows(&env, "collab_tasks", env.repo_id).await,
        count_rows(&env, "collab_messages", env.repo_id).await,
        count_rows(&env, "agent_sessions", env.repo_id).await,
    );
    let resp = env
        .send(Method::DELETE, &format!("{}/api/v1/admin/users/{leaver_id}", env.base), Some(&admin_token), None, None)
        .await;
    assert!(resp.status().is_success(), "delete user: {}", resp.status());
    let after = (
        count_rows(&env, "collab_tasks", env.repo_id).await,
        count_rows(&env, "collab_messages", env.repo_id).await,
        count_rows(&env, "agent_sessions", env.repo_id).await,
    );
    assert_eq!(before.0 - after.0, 1, "owned tasks");
    assert_eq!(before.1 - after.1, 3, "2 in the owned task + 1 authored elsewhere");
    assert_eq!(before.2 - after.2, 1, "sessions");
}

// ---------------------------------------------------------- completion status

async fn completion(env: &Env, tid: &str) -> Value {
    let (status, body) = env
        .json(Method::GET, &env.url(&format!("/tasks/{tid}/completion-status")), &env.owner_token, None, None)
        .await;
    assert_eq!(status, 200, "{body}");
    body
}

fn cond<'a>(body: &'a Value, id: &str) -> &'a Value {
    body["conditions"].as_array().unwrap().iter().find(|c| c["id"] == id).unwrap()
}

#[tokio::test]
async fn o04_done_is_a_claim_and_ci_only_counts_for_the_exact_commit() {
    let env = Env::new().await;
    let a = env.session(&env.owner_token, "A").await;
    let first = env.push_tree(&env.repo, "feature", &[("a", "1")], &[]);
    let task = new_task(&env, &env.owner_token, &a, json!({"branch": "feature"})).await;
    let tid = task["id"].as_str().unwrap();

    // Nothing written yet.
    let start = completion(&env, tid).await;
    assert_eq!(cond(&start, "work_package_present")["status"], "unsatisfied");
    assert_eq!(cond(&start, "ci_passed_for_commit")["status"], "unknown");
    assert_eq!(start["overall"], "unsatisfied");
    assert_eq!(start["observed_branch_head"], first.as_str());

    // The agent declares done with nothing behind it.
    let (status, _) = env
        .json(Method::PATCH, &env.url(&format!("/tasks/{tid}")), &env.owner_token, Some(&a), Some(json!({"expected_version": 1, "status": "done"})))
        .await;
    assert_eq!(status, 200, "marking done is never blocked");
    let claimed = completion(&env, tid).await;
    assert_eq!(claimed["declared_status"], "done");
    assert_eq!(claimed["declared_done_without_evidence"], true);

    // A package names the commit; no pipeline exists for it yet: unknown, not passed.
    let mut pkg = package("p1");
    pkg["commit_sha"] = json!(first);
    pkg["evidence_refs"] = json!([{"kind": "note", "reference": "tests pass, trust me"}]);
    let (status, _) = env.json(Method::POST, &env.url(&format!("/tasks/{tid}/work-packages")), &env.owner_token, Some(&a), Some(pkg)).await;
    assert_eq!(status, 201);
    let with_pkg = completion(&env, tid).await;
    assert_eq!(cond(&with_pkg, "work_package_present")["status"], "satisfied");
    assert_eq!(cond(&with_pkg, "ci_passed_for_commit")["status"], "unknown");
    assert_eq!(with_pkg["overall"], "incomplete");
    assert_eq!(with_pkg["declared_done_without_evidence"], true);

    // A failed CI pipeline for the commit contradicts the claim.
    let failed = rg_db::ops::pipeline_ops::create_pipeline(&env.db, env.repo_id, &first, "feature", "push", None).await.unwrap();
    rg_db::ops::pipeline_ops::update_pipeline_status(&env.db, failed.id, "failed", None, Some(Utc::now().naive_utc())).await.unwrap();
    let broken = completion(&env, tid).await;
    assert_eq!(cond(&broken, "ci_passed_for_commit")["status"], "unsatisfied");
    assert_eq!(cond(&broken, "ci_passed_for_commit")["evidence"][0]["pipeline_id"], failed.id);
    assert_eq!(broken["overall"], "unsatisfied");

    // A rerun succeeds for the same commit: satisfied; with no open question, all.
    let rerun = rg_db::ops::pipeline_ops::create_pipeline(&env.db, env.repo_id, &first, "feature", "push", None).await.unwrap();
    rg_db::ops::pipeline_ops::update_pipeline_status(&env.db, rerun.id, "success", None, Some(Utc::now().naive_utc())).await.unwrap();
    let good = completion(&env, tid).await;
    assert_eq!(cond(&good, "ci_passed_for_commit")["status"], "satisfied");
    assert_eq!(good["overall"], "all_satisfied");
    assert_eq!(good["declared_done_without_evidence"], false);

    // An open question contradicts "done" even with green CI.
    let (_, q) = ask(&env, tid, Some(&a), json!({})).await;
    let open = completion(&env, tid).await;
    assert_eq!(cond(&open, "questions_resolved")["status"], "unsatisfied");
    assert_eq!(open["overall"], "unsatisfied");
    let q = get_q(&env, q["id"].as_str().unwrap()).await;
    let (status, _) = env
        .json(Method::POST, &env.url(&format!("/questions/{}/cancel", q["id"].as_str().unwrap())), &env.owner_token, Some(&a), Some(json!({"expected_version": q["version"]})))
        .await;
    assert_eq!(status, 200);
    assert_eq!(completion(&env, tid).await["overall"], "all_satisfied");

    // The branch moves on: green CI for the old commit no longer vouches for it.
    let second = env.push_tree(&env.repo, "feature", &[("a", "2")], &[]);
    let moved = completion(&env, tid).await;
    assert_eq!(cond(&moved, "ci_passed_for_commit")["status"], "stale");
    assert!(cond(&moved, "ci_passed_for_commit")["detail"].as_str().unwrap().contains(&second));
    assert_eq!(moved["overall"], "incomplete");
    assert_eq!(moved["declared_done_without_evidence"], true);

    // Nothing here ever changed the task.
    let (_, after) = env.json(Method::GET, &env.url(&format!("/tasks/{tid}")), &env.owner_token, None, None).await;
    assert_eq!(after["status"], "done");

    // Access: readers may look, strangers may not.
    let (stranger, _) = env.user("stranger").await;
    let (status, _) = env.json(Method::GET, &env.url(&format!("/tasks/{tid}/completion-status")), &stranger, None, None).await;
    assert_eq!(status, 403);
}
