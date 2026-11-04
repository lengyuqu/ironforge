//! P1 U04 acceptance: delivery receipts (N08), handoff notices (N09) and durable
//! notices (N10).
//!
//! Real HTTP server and SQLite database; with `IRONFORGE_TEST_DATABASE_URL` the
//! same tests run on PostgreSQL or MySQL (not run here).

mod common;
mod collab_common;

use std::sync::Arc;

use chrono::{Duration, Utc};
use collab_common::{Env, Sess};
use reqwest::Method;
use rg_db::sea_orm::{ActiveModelTrait, EntityTrait, Set};
use serde_json::{json, Value};
use tokio::sync::Barrier;

async fn new_task(env: &Env, s: &Sess) -> String {
    let (status, task) = env
        .json(Method::POST, &env.url("/tasks"), &env.owner_token, Some(s), Some(json!({"title": "task"})))
        .await;
    assert_eq!(status, 201, "{task}");
    task["id"].as_str().unwrap().to_string()
}

async fn expire(env: &Env, id: &str) {
    use rg_db::entities::agent_session;
    let row = agent_session::Entity::find_by_id(id.to_string()).one(&env.db).await.unwrap().unwrap();
    let mut active: agent_session::ActiveModel = row.into();
    active.expires_at = Set(Utc::now() - Duration::minutes(1));
    active.update(&env.db).await.unwrap();
}

async fn send(env: &Env, tid: &str, from: &Sess, key: &str, extra: Value) -> (u16, Value) {
    let mut body = json!({"client_message_id": key, "kind": "note", "body": "hello"});
    body.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
    env.json(Method::POST, &env.url(&format!("/tasks/{tid}/messages")), &env.owner_token, Some(from), Some(body)).await
}

async fn notices(env: &Env, who: &Sess, query: &str) -> Vec<Value> {
    let (status, body) = env
        .json(Method::GET, &env.url(&format!("/notices{query}")), &env.owner_token, Some(who), None)
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["authority"], "hint");
    body["items"].as_array().unwrap().clone()
}

fn kinds(items: &[Value]) -> Vec<String> {
    items.iter().map(|n| n["kind"].as_str().unwrap().to_string()).collect()
}

fn handoff_body(key: &str, to: &str) -> Value {
    json!({
        "client_handoff_id": key, "to_session_id": to, "summary": "s",
        "completed": "c", "next_steps": "n", "blockers": "b", "evidence": []
    })
}

async fn parallel<F, Fut>(n: usize, make: F) -> Vec<(u16, Value)>
where
    F: Fn(usize) -> Fut,
    Fut: std::future::Future<Output = (u16, Value)> + Send + 'static,
{
    let barrier = Arc::new(Barrier::new(n));
    let handles: Vec<_> = (0..n)
        .map(|i| {
            let fut = make(i);
            let barrier = barrier.clone();
            tokio::spawn(async move {
                barrier.wait().await;
                fut.await
            })
        })
        .collect();
    let mut out = Vec::new();
    for h in handles {
        out.push(h.await.unwrap());
    }
    out
}

async fn raw(method: Method, url: String, token: String, sess: Option<Sess>, body: Option<Value>) -> (u16, Value) {
    let mut req = reqwest::Client::new().request(method, url).bearer_auth(token);
    if let Some(s) = sess {
        req = req.header(collab_common::HDR_SESSION, s.id).header(collab_common::HDR_SECRET, s.secret);
    }
    if let Some(b) = body {
        req = req.json(&b);
    }
    let resp = req.send().await.unwrap();
    let status = resp.status().as_u16();
    let text = resp.text().await.unwrap();
    (status, serde_json::from_str(&text).unwrap_or(Value::String(text)))
}

// ------------------------------------------------------------------ N08

#[tokio::test]
async fn n08_receipt_watermark_and_reply_are_reported_separately() {
    let env = Env::new().await;
    let a = env.session(&env.owner_token, "A").await;
    let b = env.session(&env.owner_token, "B").await;
    let c = env.session(&env.owner_token, "C").await;
    let tid = new_task(&env, &a).await;

    let (status, msg) = send(&env, &tid, &a, "m1", json!({"target_session_id": b.id})).await;
    assert_eq!(status, 201, "{msg}");
    let mid = msg["id"].as_str().unwrap();
    let delivery_url = env.url(&format!("/tasks/{tid}/messages/{mid}/delivery"));
    let receipt_url = env.url(&format!("/tasks/{tid}/messages/{mid}/read-receipt"));

    // Saved, but nothing is confirmed: unknown, not "unread".
    let (status, d) = env.json(Method::GET, &delivery_url, &env.owner_token, Some(&a), None).await;
    assert_eq!(status, 200, "{d}");
    assert_eq!(d["saved"], true);
    assert_eq!(d["status"], "unknown");
    assert_eq!(d["target_session_id"], b.id);
    assert!(d["read_at"].is_null() && d["replied_at"].is_null());

    // Pushing the target's read watermark past the message creates no receipt.
    let (status, w) = env
        .json(Method::PUT, &env.url(&format!("/tasks/{tid}/read")), &env.owner_token, Some(&b), Some(json!({"last_read_seq": 1})))
        .await;
    assert_eq!(status, 200, "{w}");
    assert_eq!(w["last_read_seq"], 1);
    let (_, d) = env.json(Method::GET, &delivery_url, &env.owner_token, Some(&a), None).await;
    assert_eq!(d["status"], "unknown", "the watermark never forges a receipt");

    // Only the target can confirm.
    for who in [&a, &c] {
        let (status, body) = env.json(Method::POST, &receipt_url, &env.owner_token, Some(who), None).await;
        assert_eq!(status, 403, "{body}");
    }
    let (status, r) = env.json(Method::POST, &receipt_url, &env.owner_token, Some(&b), None).await;
    assert_eq!(status, 201, "{r}");
    assert_eq!(r["newly_confirmed"], true);
    let (status, again) = env.json(Method::POST, &receipt_url, &env.owner_token, Some(&b), None).await;
    assert_eq!(status, 200, "{again}");
    assert_eq!(again["newly_confirmed"], false);
    assert_eq!(again["read_at"], r["read_at"], "the original confirmation time is kept");

    let (_, d) = env.json(Method::GET, &delivery_url, &env.owner_token, Some(&a), None).await;
    assert_eq!(d["status"], "read");
    assert_eq!(d["read_at"], r["read_at"]);
    assert!(d["replied_at"].is_null());

    // A receipt does not move the watermark of a third message.
    let (_, m2) = send(&env, &tid, &a, "m2", json!({"target_session_id": b.id})).await;
    let (_, w) = env
        .json(Method::PUT, &env.url(&format!("/tasks/{tid}/read")), &env.owner_token, Some(&b), Some(json!({"last_read_seq": 0})))
        .await;
    assert_eq!(w["last_read_seq"], 1, "confirming m1 did not touch the watermark; m2 ({}) is still unread", m2["seq"]);

    // A reply by someone else is not the target's reply; the target's is.
    let (status, _) = send(&env, &tid, &c, "r-c", json!({"reply_to_id": mid})).await;
    assert_eq!(status, 201);
    let (_, d) = env.json(Method::GET, &delivery_url, &env.owner_token, Some(&a), None).await;
    assert_eq!(d["status"], "read");
    let (_, reply) = send(&env, &tid, &b, "r-b", json!({"reply_to_id": mid})).await;
    let (_, d) = env.json(Method::GET, &delivery_url, &env.owner_token, Some(&a), None).await;
    assert_eq!(d["status"], "replied");
    assert_eq!(d["reply_message_id"], reply["id"]);
    assert_eq!(d["replied_at"], reply["created_at"]);

    // Only the author sees the personal reading state.
    let (status, _) = env.json(Method::GET, &delivery_url, &env.owner_token, Some(&b), None).await;
    assert_eq!(status, 403);
    let (status, _) = env.json(Method::GET, &delivery_url, &env.owner_token, None, None).await;
    assert_eq!(status, 400);
}

#[tokio::test]
async fn n08_untargeted_messages_have_nothing_to_confirm() {
    let env = Env::new().await;
    let a = env.session(&env.owner_token, "A").await;
    let b = env.session(&env.owner_token, "B").await;
    let tid = new_task(&env, &a).await;
    let (_, msg) = send(&env, &tid, &a, "plain", json!({})).await;
    let mid = msg["id"].as_str().unwrap();
    let (status, d) = env
        .json(Method::GET, &env.url(&format!("/tasks/{tid}/messages/{mid}/delivery")), &env.owner_token, Some(&a), None)
        .await;
    assert_eq!(status, 200);
    assert_eq!(d["status"], "not_targeted");
    let (status, _) = env
        .json(Method::POST, &env.url(&format!("/tasks/{tid}/messages/{mid}/read-receipt")), &env.owner_token, Some(&b), None)
        .await;
    assert_eq!(status, 403);
    // Unknown ids read as absent.
    let ghost = uuid::Uuid::new_v4();
    let (status, _) = env
        .json(Method::GET, &env.url(&format!("/tasks/{tid}/messages/{ghost}/delivery")), &env.owner_token, Some(&a), None)
        .await;
    assert_eq!(status, 404);
}

#[tokio::test]
async fn n08_racing_confirmations_leave_one_receipt() {
    let env = Env::new().await;
    let a = env.session(&env.owner_token, "A").await;
    let b = env.session(&env.owner_token, "B").await;
    let tid = new_task(&env, &a).await;
    let (_, msg) = send(&env, &tid, &a, "m", json!({"target_session_id": b.id})).await;
    let url = env.url(&format!("/tasks/{tid}/messages/{}/read-receipt", msg["id"].as_str().unwrap()));
    let results = parallel(8, |_| raw(Method::POST, url.clone(), env.owner_token.clone(), Some(b.clone()), None)).await;
    assert_eq!(results.iter().filter(|(s, _)| *s == 201).count(), 1, "{results:?}");
    assert_eq!(results.iter().filter(|(s, _)| *s == 200).count(), 7, "{results:?}");
    let times: std::collections::BTreeSet<_> = results.iter().map(|(_, b)| b["read_at"].to_string()).collect();
    assert_eq!(times.len(), 1);
}

// ------------------------------------------------------------------ N10

#[tokio::test]
async fn n10_every_addressed_event_leaves_one_durable_notice() {
    let env = Env::new().await;
    let a = env.session(&env.owner_token, "A").await;
    let b = env.session(&env.owner_token, "B").await;
    let tid = new_task(&env, &a).await;

    // handoff A -> B, targeted message A -> B, question A -> B.
    let (s, h) = env
        .json(Method::POST, &env.url(&format!("/tasks/{tid}/handoffs")), &env.owner_token, Some(&a), Some(handoff_body("h1", &b.id)))
        .await;
    assert_eq!(s, 201, "{h}");
    let (_, m) = send(&env, &tid, &a, "m1", json!({"target_session_id": b.id, "kind": "help"})).await;
    let (s, q) = env
        .json(
            Method::POST,
            &env.url(&format!("/tasks/{tid}/questions")),
            &env.owner_token,
            Some(&a),
            Some(json!({
                "client_question_id": "q1", "recipient": {"kind": "session", "id": b.id},
                "title": "t", "body": "which?"
            })),
        )
        .await;
    assert_eq!(s, 201, "{q}");

    let got = notices(&env, &b, "").await;
    assert_eq!(got.len(), 3);
    for (kind, source) in [("handoff_received", &h["id"]), ("message_targeted", &m["id"]), ("question_asked", &q["id"])] {
        assert!(got.iter().any(|n| n["kind"] == kind && &n["source_id"] == source));
    }
    assert!(got.iter().all(|n| n["state"] == "pending" && n["task_id"] == tid.as_str()));
    // The hint carries platform facts only, never the client's text.
    let text = serde_json::to_string(&got).unwrap();
    assert!(!text.contains("which?") && !text.contains("hello"), "{text}");
    // The sender gets nothing for its own actions.
    assert!(notices(&env, &a, "").await.is_empty());

    // B answers A's question -> A is told. (B asked nothing; the answer goes to the creator.)
    let (_, qfull) = env.json(Method::GET, &env.url(&format!("/questions/{}", q["id"].as_str().unwrap())), &env.owner_token, None, None).await;
    let (s, ans) = env
        .json(
            Method::POST,
            &env.url(&format!("/questions/{}/answer", q["id"].as_str().unwrap())),
            &env.owner_token,
            Some(&b),
            Some(json!({
                "client_answer_id": "a1", "expected_version": qfull["version"], "body": "this one",
                "acknowledged_context_hash": qfull["context_hash"]
            })),
        )
        .await;
    assert_eq!(s, 200, "{ans}");
    let for_a = notices(&env, &a, "").await;
    assert_eq!(kinds(&for_a), ["question_answered"]);
    assert_eq!(for_a[0]["source_id"], q["id"]);

    // A user reassigns the task from A to B -> A is told it moved away.
    let (_, task) = env.json(Method::GET, &env.url(&format!("/tasks/{tid}")), &env.owner_token, None, None).await;
    let (s, claimed) = env
        .json(
            Method::POST,
            &env.url(&format!("/tasks/{tid}/takeovers")),
            &env.owner_token,
            Some(&a),
            Some(json!({
                "client_takeover_id": "claim1", "expected_task_version": task["version"],
                "expected_owner_session_id": null, "reason": "mine"
            })),
        )
        .await;
    assert_eq!(s, 201, "{claimed}");
    let (_, task) = env.json(Method::GET, &env.url(&format!("/tasks/{tid}")), &env.owner_token, None, None).await;
    assert_eq!(task["owner_session_id"], a.id);
    let (s, r) = env
        .json(
            Method::POST,
            &env.url(&format!("/tasks/{tid}/reassign")),
            &env.owner_token,
            None,
            Some(json!({
                "client_takeover_id": "re1", "expected_task_version": task["version"],
                "expected_owner_session_id": a.id, "to_session_id": b.id, "reason": "rebalance"
            })),
        )
        .await;
    assert_eq!(s, 201, "{r}");
    let for_a = notices(&env, &a, "").await;
    assert_eq!(for_a.len(), 2);
    assert!(for_a.iter().any(|n| n["kind"] == "question_answered" && n["source_id"] == q["id"]));
    let reassigned = for_a.iter().find(|n| n["kind"] == "task_reassigned_away").unwrap();
    assert_eq!(reassigned["source_id"], r["takeover"]["id"]);
    assert_eq!(reassigned["hint"]["to_session_id"], b.id);
}

#[tokio::test]
async fn n10_notices_survive_restarts_are_paged_without_duplicates_and_acks_touch_only_the_notice() {
    let env = Env::new().await;
    let a = env.session(&env.owner_token, "A").await;
    let b = env.session(&env.owner_token, "B").await;
    let tid = new_task(&env, &a).await;
    let (_, h) = env
        .json(Method::POST, &env.url(&format!("/tasks/{tid}/handoffs")), &env.owner_token, Some(&a), Some(handoff_body("h", &b.id)))
        .await;
    for i in 0..4 {
        send(&env, &tid, &a, &format!("m{i}"), json!({"target_session_id": b.id})).await;
    }

    // A fresh server instance over the same database still has all five.
    let base = env.restart().await;
    let url = format!("{base}/api/v1/repos/{}/{}/collaboration/notices?limit=2", env.owner, env.repo);
    let mut seen = Vec::new();
    let mut cursor: Option<String> = None;
    loop {
        let mut u = url.clone();
        if let Some(c) = &cursor {
            u.push_str(&format!("&cursor={c}"));
        }
        let (s, page) = raw(Method::GET, u, env.owner_token.clone(), Some(b.clone()), None).await;
        assert_eq!(s, 200, "{page}");
        seen.extend(page["items"].as_array().unwrap().iter().map(|n| n["id"].as_str().unwrap().to_string()));
        match page["next_cursor"].as_str() {
            Some(c) => cursor = Some(c.to_string()),
            None => break,
        }
    }
    assert_eq!(seen.len(), 5);
    assert_eq!(seen.iter().collect::<std::collections::BTreeSet<_>>().len(), 5, "no duplicates");
    // A repeated pull returns the same ids: the consumer de-duplicates by id.
    let again: Vec<String> = notices(&env, &b, "?limit=100").await.iter().map(|n| n["id"].as_str().unwrap().to_string()).collect();
    assert_eq!(again.len(), 5);
    assert_eq!(again.iter().collect::<std::collections::BTreeSet<_>>(), seen.iter().collect());

    // A cursor of another listing is rejected.
    let (s, _) = raw(Method::GET, format!("{url}&cursor=garbage"), env.owner_token.clone(), Some(b.clone()), None).await;
    assert_eq!(s, 400);

    // Ack changes the notice only.
    let all = notices(&env, &b, "?limit=100").await;
    assert!(all.windows(2).all(|w| {
        (w[0]["created_at"].as_str().unwrap(), w[0]["id"].as_str().unwrap())
            <= (w[1]["created_at"].as_str().unwrap(), w[1]["id"].as_str().unwrap())
    }), "notices follow the documented created_at/id order");
    let n = all.iter().find(|n| n["kind"] == "handoff_received").unwrap();
    assert!(again.iter().any(|id| Some(id.as_str()) == n["id"].as_str()));
    let (s, acked) = env
        .json(Method::POST, &env.url(&format!("/notices/{}/ack", n["id"].as_str().unwrap())), &env.owner_token, Some(&b), None)
        .await;
    assert_eq!(s, 200, "{acked}");
    assert_eq!(acked["newly_acked"], true);
    assert_eq!(acked["notice"]["state"], "acknowledged");
    let (_, ho) = env
        .json(Method::GET, &env.url(&format!("/tasks/{tid}/handoffs")), &env.owner_token, None, None)
        .await;
    assert_eq!(ho["items"][0]["status"], "pending", "ack did not accept the handoff");
    assert_eq!(ho["items"][0]["id"], h["id"]);
    assert_eq!(notices(&env, &b, "").await.len(), 4, "acked notices leave the pending view");
    assert_eq!(notices(&env, &b, "?state=all").await.len(), 5, "but stay readable");
    let (s, _) = env.json(Method::GET, &env.url("/notices?state=bogus"), &env.owner_token, Some(&b), None).await;
    assert_eq!(s, 400);
}

#[tokio::test]
async fn n10_racing_acks_change_the_notice_once() {
    let env = Env::new().await;
    let a = env.session(&env.owner_token, "A").await;
    let b = env.session(&env.owner_token, "B").await;
    let tid = new_task(&env, &a).await;
    send(&env, &tid, &a, "m", json!({"target_session_id": b.id})).await;
    let n = notices(&env, &b, "").await.remove(0);
    let url = env.url(&format!("/notices/{}/ack", n["id"].as_str().unwrap()));
    let results = parallel(10, |_| raw(Method::POST, url.clone(), env.owner_token.clone(), Some(b.clone()), None)).await;
    assert!(results.iter().all(|(s, _)| *s == 200), "{results:?}");
    assert_eq!(results.iter().filter(|(_, b)| b["newly_acked"] == true).count(), 1, "{results:?}");
    let stamps: std::collections::BTreeSet<_> = results.iter().map(|(_, b)| b["notice"]["acked_at"].to_string()).collect();
    assert_eq!(stamps.len(), 1, "every caller sees the winner's acknowledgement time");
}

#[tokio::test]
async fn n10_barrier_duplicate_requests_emit_exactly_one_notice() {
    let env = Env::new().await;
    let a = env.session(&env.owner_token, "A").await;
    let b = env.session(&env.owner_token, "B").await;
    let tid = new_task(&env, &a).await;

    // Same client id, same body, eight at once.
    let murl = env.url(&format!("/tasks/{tid}/messages"));
    let mbody = json!({"client_message_id": "dup", "kind": "note", "body": "x", "target_session_id": b.id});
    let results = parallel(8, |_| raw(Method::POST, murl.clone(), env.owner_token.clone(), Some(a.clone()), Some(mbody.clone()))).await;
    assert!(results.iter().all(|(s, _)| *s == 200 || *s == 201), "{results:?}");
    assert_eq!(results.iter().filter(|(s, _)| *s == 201).count(), 1);

    let hurl = env.url(&format!("/tasks/{tid}/handoffs"));
    let hbody = handoff_body("hdup", &b.id);
    let results = parallel(8, |_| raw(Method::POST, hurl.clone(), env.owner_token.clone(), Some(a.clone()), Some(hbody.clone()))).await;
    assert!(results.iter().all(|(s, _)| *s == 200 || *s == 201), "{results:?}");

    let qurl = env.url(&format!("/tasks/{tid}/questions"));
    let qbody = json!({"client_question_id": "qdup", "recipient": {"kind": "session", "id": b.id}, "title": "t", "body": "b"});
    let results = parallel(8, |_| raw(Method::POST, qurl.clone(), env.owner_token.clone(), Some(a.clone()), Some(qbody.clone()))).await;
    assert!(results.iter().all(|(s, _)| *s == 200 || *s == 201), "{results:?}");

    let mut got = kinds(&notices(&env, &b, "?limit=100").await);
    got.sort();
    assert_eq!(got, ["handoff_received", "message_targeted", "question_asked"], "one notice per event");
}

#[tokio::test]
async fn n10_racing_answers_notify_the_asker_once() {
    let env = Env::new().await;
    let a = env.session(&env.owner_token, "A").await;
    let tid = new_task(&env, &a).await;
    let (_, q) = env
        .json(
            Method::POST,
            &env.url(&format!("/tasks/{tid}/questions")),
            &env.owner_token,
            Some(&a),
            Some(json!({"client_question_id": "q", "recipient": {"kind": "repo_writers"}, "title": "t", "body": "b"})),
        )
        .await;
    let qid = q["id"].as_str().unwrap();
    let url = env.url(&format!("/questions/{qid}/answer"));
    let results = parallel(6, |i| {
        raw(
            Method::POST,
            url.clone(),
            env.owner_token.clone(),
            None,
            Some(json!({
                "client_answer_id": format!("a{i}"), "expected_version": q["version"], "body": "yes",
                "acknowledged_context_hash": q["context_hash"]
            })),
        )
    })
    .await;
    assert_eq!(results.iter().filter(|(s, _)| *s == 200).count(), 1, "{results:?}");
    assert_eq!(kinds(&notices(&env, &a, "?state=all").await), ["question_answered"]);
}

#[tokio::test]
async fn n10_a_failed_business_write_leaves_no_notice() {
    let env = Env::new().await;
    let a = env.session(&env.owner_token, "A").await;
    let b = env.session(&env.owner_token, "B").await;
    let tid = new_task(&env, &a).await;
    send(&env, &tid, &a, "k", json!({"target_session_id": b.id, "body": "one"})).await;
    // Same key, different body: refused, no second notice.
    let (s, body) = send(&env, &tid, &a, "k", json!({"target_session_id": b.id, "body": "two"})).await;
    assert_eq!(s, 409, "{body}");
    // Unknown target: refused, no notice anywhere.
    let ghost = uuid::Uuid::new_v4().to_string();
    let (s, _) = send(&env, &tid, &a, "k2", json!({"target_session_id": ghost})).await;
    assert_eq!(s, 400);
    assert_eq!(notices(&env, &b, "?state=all").await.len(), 1);
}

#[tokio::test]
async fn n10_notices_are_private_to_the_recipient_and_need_access() {
    let env = Env::new().await;
    let a = env.session(&env.owner_token, "A").await;
    let b = env.session(&env.owner_token, "B").await;
    let c = env.session(&env.owner_token, "C").await;
    let tid = new_task(&env, &a).await;
    send(&env, &tid, &a, "m", json!({"target_session_id": b.id})).await;
    let n = notices(&env, &b, "").await.remove(0);
    let ack_url = env.url(&format!("/notices/{}/ack", n["id"].as_str().unwrap()));

    // A sibling session sees nothing and cannot ack it.
    assert!(notices(&env, &c, "?state=all").await.is_empty());
    let (s, _) = env.json(Method::POST, &ack_url, &env.owner_token, Some(&c), None).await;
    assert_eq!(s, 404);
    assert_eq!(notices(&env, &b, "").await.len(), 1, "still pending");

    // No session credentials, no token, no access.
    let (s, body) = env.json(Method::GET, &env.url("/notices"), &env.owner_token, None, None).await;
    assert_eq!(s, 400);
    assert_eq!(body["error"]["code"], "SESSION_CREDENTIALS_REQUIRED");
    let resp = env.send(Method::GET, &env.url("/notices"), None, Some(&b), None).await;
    assert_eq!(resp.status(), 401);
    let (stranger, _) = env.user("stranger").await;
    let (s, _) = env.json(Method::GET, &env.url("/notices"), &stranger, Some(&b), None).await;
    assert_eq!(s, 403);

    // A collaborator whose access is revoked loses the notices at once.
    let (token, uid) = env.user("collab").await;
    env.add_collaborator(uid, "write").await;
    let cs = env.session(&token, "CS").await;
    let ctid = new_task(&env, &a).await;
    send(&env, &ctid, &a, "m-cs", json!({"target_session_id": cs.id})).await;
    let (s, body) = env.json(Method::GET, &env.url("/notices"), &token, Some(&cs), None).await;
    assert_eq!(s, 200, "{body}");
    assert_eq!(body["items"].as_array().unwrap().len(), 1);
    let resp = env
        .send(Method::DELETE, &format!("{}/api/v1/repos/{}/{}/collaborators/{uid}", env.base, env.owner, env.repo), Some(&env.owner_token), None, None)
        .await;
    assert!(resp.status().is_success(), "{}", resp.status());
    let (s, _) = env.json(Method::GET, &env.url("/notices"), &token, Some(&cs), None).await;
    assert_eq!(s, 403);
}

#[tokio::test]
async fn n10_ended_or_expired_sessions_get_no_notices_and_pass_none_on() {
    let env = Env::new().await;
    let a = env.session(&env.owner_token, "A").await;
    let b = env.session(&env.owner_token, "B").await;
    let b2 = env.session(&env.owner_token, "B-successor").await;
    let tid = new_task(&env, &a).await;
    send(&env, &tid, &a, "before", json!({"target_session_id": b.id})).await;
    expire(&env, &b.id).await;

    // The expired session cannot read; its successor sees nothing of it.
    let (s, body) = env.json(Method::GET, &env.url("/notices"), &env.owner_token, Some(&b), None).await;
    assert_eq!(s, 409, "{body}");
    assert_eq!(body["error"]["code"], "SESSION_EXPIRED");
    assert!(notices(&env, &b2, "?state=all").await.is_empty());

    // Nothing new is written for the expired target.
    send(&env, &tid, &a, "after", json!({"target_session_id": b.id})).await;
    use rg_db::entities::collab_notice;
    let rows = collab_notice::Entity::find().all(&env.db).await.unwrap();
    let for_b: Vec<_> = rows.iter().filter(|r| r.recipient_session_id == b.id).collect();
    assert_eq!(for_b.len(), 1, "only the notice that predates the expiry exists");
    assert!(rows.iter().all(|r| r.recipient_session_id != b2.id));
}

// ------------------------------------------------------------------ N09

#[tokio::test]
async fn n09_expired_target_cancel_then_new_handoff_leaves_an_explainable_trail() {
    let env = Env::new().await;
    let a = env.session(&env.owner_token, "A").await;
    let b = env.session(&env.owner_token, "B").await;
    let c = env.session(&env.owner_token, "C").await;
    let tid = new_task(&env, &a).await;
    let hurl = env.url(&format!("/tasks/{tid}/handoffs"));
    let (s, h1) = env.json(Method::POST, &hurl, &env.owner_token, Some(&a), Some(handoff_body("h1", &b.id))).await;
    assert_eq!(s, 201, "{h1}");
    expire(&env, &b.id).await;

    // The target can no longer accept; the creator cancels.
    let (s, body) = env
        .json(Method::POST, &env.url(&format!("/handoffs/{}/accept", h1["id"].as_str().unwrap())), &env.owner_token, Some(&b), Some(json!({"expected_version": 1})))
        .await;
    assert_eq!(s, 409, "{body}");
    let (s, cancelled) = env
        .json(Method::POST, &env.url(&format!("/handoffs/{}/cancel", h1["id"].as_str().unwrap())), &env.owner_token, Some(&a), Some(json!({"expected_version": 1})))
        .await;
    assert_eq!(s, 200, "{cancelled}");

    // Between the two steps the list shows exactly one cancelled handoff and no
    // replacement: the interruption is visible, not hidden as an atomic move.
    let (_, list) = env.json(Method::GET, &hurl, &env.owner_token, None, None).await;
    let items = list["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["status"], "cancelled");

    // The new handoff goes to C, who gets a notice; the dead target gets none.
    let (s, h2) = env.json(Method::POST, &hurl, &env.owner_token, Some(&a), Some(handoff_body("h2", &c.id))).await;
    assert_eq!(s, 201, "{h2}");
    assert_eq!(kinds(&notices(&env, &c, "").await), ["handoff_received"]);
    let (_, list) = env.json(Method::GET, &hurl, &env.owner_token, None, None).await;
    let items = list["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    assert!(items.iter().any(|item| item["id"] == h1["id"] && item["status"] == "cancelled"));
    assert!(items.iter().any(|item| item["id"] == h2["id"] && item["status"] == "pending"));
}
