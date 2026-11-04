//! C04 acceptance: handoff state machine and read watermarks (T07, T08, T10).

mod collab_common;
mod common;

use collab_common::{Env, Sess};
use reqwest::Method;
use rg_db::sea_orm::{ActiveModelTrait, EntityTrait, Set};
use serde_json::{json, Value};

struct Pair {
    env: Env,
    a: Sess,
    b: Sess,
    task_id: String,
}

async fn pair() -> Pair {
    let env = Env::new().await;
    let a = env.session(&env.owner_token, "A").await;
    let b = env.session(&env.owner_token, "B").await;
    let (_, task) = env
        .json(
            Method::POST,
            &env.url("/tasks"),
            &env.owner_token,
            Some(&a),
            Some(json!({"title": "handoff task"})),
        )
        .await;
    let task_id = task["id"].as_str().unwrap().to_string();
    Pair { env, a, b, task_id }
}

fn handoff_body(client_id: &str, to: &str) -> Value {
    json!({
        "client_handoff_id": client_id, "to_session_id": to,
        "summary": "tests fail on the parser", "completed": "reproduced", "next_steps": "fix tokenizer",
        "blockers": "none", "evidence": ["ci:pipeline/42", "https://example.invalid/log"]
    })
}

impl Pair {
    async fn create(&self, from: &Sess, body: Value) -> (u16, Value) {
        self.env
            .json(
                Method::POST,
                &self.env.url(&format!("/tasks/{}/handoffs", self.task_id)),
                &self.env.owner_token,
                Some(from),
                Some(body),
            )
            .await
    }

    async fn act(&self, who: &Sess, handoff: &str, verb: &str, version: i64) -> (u16, Value) {
        self.env
            .json(
                Method::POST,
                &self.env.url(&format!("/handoffs/{handoff}/{verb}")),
                &self.env.owner_token,
                Some(who),
                Some(json!({"expected_version": version})),
            )
            .await
    }

    async fn send(&self, who: &Sess, client_id: &str) -> (u16, Value) {
        self.env
            .json(
                Method::POST,
                &self.env.url(&format!("/tasks/{}/messages", self.task_id)),
                &self.env.owner_token,
                Some(who),
                Some(json!({"client_message_id": client_id, "kind": "note", "body": client_id})),
            )
            .await
    }

    async fn watermark(&self, who: &Sess, seq: i64) -> (u16, Value) {
        self.env
            .json(
                Method::PUT,
                &self.env.url(&format!("/tasks/{}/read", self.task_id)),
                &self.env.owner_token,
                Some(who),
                Some(json!({"last_read_seq": seq})),
            )
            .await
    }
}

#[tokio::test]
async fn handoff_lifecycle_and_who_may_do_what() {
    let p = pair().await;
    let c = p.env.session(&p.env.owner_token, "C").await;
    let (status, h) = p.create(&p.a, handoff_body("h1", &p.b.id)).await;
    assert_eq!(status, 201, "{h}");
    assert_eq!(
        (h["status"].as_str(), h["version"].as_i64()),
        (Some("pending"), Some(1))
    );
    assert_eq!(h["origin"], "user_content");
    assert_eq!(
        h["evidence"],
        json!(["ci:pipeline/42", "https://example.invalid/log"])
    );
    let id = h["id"].as_str().unwrap();

    // Wrong actors: the creator cannot accept, the target cannot cancel, third parties neither.
    for (who, verb) in [
        (&p.a, "accept"),
        (&p.b, "cancel"),
        (&c, "accept"),
        (&c, "cancel"),
    ] {
        let (status, body) = p.act(who, id, verb, 1).await;
        assert_eq!(status, 403, "{verb}: {body}");
        assert_eq!(body["error"]["code"], "FORBIDDEN");
    }
    // Stale version while still pending.
    let (status, body) = p.act(&p.b, id, "accept", 9).await;
    assert_eq!(
        (status, body["error"]["code"].as_str()),
        (409, Some("VERSION_CONFLICT"))
    );
    assert_eq!(body["error"]["details"]["current_version"], 1);

    // Nothing so far changed the handoff.
    let (_, list) = p
        .env
        .json(
            Method::GET,
            &p.env.url(&format!("/tasks/{}/handoffs", p.task_id)),
            &p.env.owner_token,
            None,
            None,
        )
        .await;
    assert_eq!(list["items"][0]["status"], "pending");

    let (status, done) = p.act(&p.b, id, "accept", 1).await;
    assert_eq!(status, 200, "{done}");
    assert_eq!(
        (done["status"].as_str(), done["version"].as_i64()),
        (Some("accepted"), Some(2))
    );
    assert!(done["accepted_at"].is_string());

    // Terminal: neither repeat nor cancel works.
    for (who, verb, v) in [
        (&p.b, "accept", 2),
        (&p.a, "cancel", 2),
        (&p.a, "cancel", 1),
        (&p.b, "accept", 1),
    ] {
        let (status, body) = p.act(who, id, verb, v).await;
        assert_eq!(
            (status, body["error"]["code"].as_str()),
            (409, Some("CONFLICT")),
            "{verb} v{v}: {body}"
        );
    }

    // The handoff alone moved no other state: the task is untouched and no message appeared.
    let (_, task) = p
        .env
        .json(
            Method::GET,
            &p.env.url(&format!("/tasks/{}", p.task_id)),
            &p.env.owner_token,
            None,
            None,
        )
        .await;
    assert_eq!(
        (task["version"].as_i64(), task["last_message_seq"].as_i64()),
        (Some(1), Some(0))
    );
    assert!(task["owner_session_id"].is_null());
}

#[tokio::test]
async fn cancel_then_reassign_to_a_new_session() {
    let p = pair().await;
    // Target expires before accepting: the handoff just stays pending.
    let (_, h) = p.create(&p.a, handoff_body("h1", &p.b.id)).await;
    let id = h["id"].as_str().unwrap().to_string();
    let row = rg_db::entities::agent_session::Entity::find_by_id(p.b.id.clone())
        .one(&p.env.db)
        .await
        .unwrap()
        .unwrap();
    let mut active: rg_db::entities::agent_session::ActiveModel = row.into();
    active.expires_at = Set(chrono::Utc::now() - chrono::Duration::minutes(1));
    active.update(&p.env.db).await.unwrap();

    let (status, body) = p.act(&p.b, &id, "accept", 1).await;
    assert_eq!(
        (status, body["error"]["code"].as_str()),
        (409, Some("SESSION_EXPIRED")),
        "an expired target cannot accept"
    );
    let (_, list) = p
        .env
        .json(
            Method::GET,
            &p.env.url(&format!("/tasks/{}/handoffs", p.task_id)),
            &p.env.owner_token,
            None,
            None,
        )
        .await;
    assert_eq!(list["items"][0]["status"], "pending");

    // Creator cancels and hands over to a fresh session.
    let (status, cancelled) = p.act(&p.a, &id, "cancel", 1).await;
    assert_eq!(
        (status, cancelled["status"].as_str()),
        (200, Some("cancelled"))
    );
    assert!(cancelled["accepted_at"].is_null());
    let d = p.env.session(&p.env.owner_token, "D").await;
    let (status, h2) = p.create(&p.a, handoff_body("h2", &d.id)).await;
    assert_eq!(status, 201, "{h2}");
    let (status, ok) = p.act(&d, h2["id"].as_str().unwrap(), "accept", 1).await;
    assert_eq!((status, ok["status"].as_str()), (200, Some("accepted")));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn idempotent_handoff_creation() {
    let p = pair().await;
    let (r0, r1, r2, r3) = tokio::join!(
        p.create(&p.a, handoff_body("same", &p.b.id)),
        p.create(&p.a, handoff_body("same", &p.b.id)),
        p.create(&p.a, handoff_body("same", &p.b.id)),
        p.create(&p.a, handoff_body("same", &p.b.id)),
    );
    let all = [r0, r1, r2, r3];
    assert_eq!(all.iter().filter(|r| r.0 == 201).count(), 1, "{all:?}");
    assert_eq!(all.iter().filter(|r| r.0 == 200).count(), 3, "{all:?}");
    assert!(all.iter().all(|r| r.1["id"] == all[0].1["id"]));

    let mut changed = handoff_body("same", &p.b.id);
    changed["summary"] = json!("something else");
    let (status, body) = p.create(&p.a, changed).await;
    assert_eq!(
        (status, body["error"]["code"].as_str()),
        (409, Some("IDEMPOTENCY_KEY_REUSED"))
    );
    let (_, list) = p
        .env
        .json(
            Method::GET,
            &p.env.url(&format!("/tasks/{}/handoffs", p.task_id)),
            &p.env.owner_token,
            None,
            None,
        )
        .await;
    assert_eq!(list["items"].as_array().unwrap().len(), 1);
}

/// T07: accept and cancel race on one pending handoff.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn t07_concurrent_accept_and_cancel_have_exactly_one_winner() {
    let p = pair().await;
    for round in 0..5 {
        let (_, h) = p
            .create(&p.a, handoff_body(&format!("race-{round}"), &p.b.id))
            .await;
        let id = h["id"].as_str().unwrap().to_string();
        let (accept, cancel) =
            tokio::join!(p.act(&p.b, &id, "accept", 1), p.act(&p.a, &id, "cancel", 1));
        let (winner, loser, winner_status) = match (accept.0, cancel.0) {
            (200, 409) => (&accept, &cancel, "accepted"),
            (409, 200) => (&cancel, &accept, "cancelled"),
            other => {
                panic!("round {round}: expected one winner, got {other:?}: {accept:?} {cancel:?}")
            }
        };
        assert_eq!(winner.1["status"], winner_status);
        assert_eq!(loser.1["error"]["code"], "CONFLICT");

        // Stored state equals the winner's and the version moved exactly once.
        let (_, list) = p
            .env
            .json(
                Method::GET,
                &p.env
                    .url(&format!("/tasks/{}/handoffs?limit=100", p.task_id)),
                &p.env.owner_token,
                None,
                None,
            )
            .await;
        let stored = list["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|x| x["id"] == id.as_str())
            .unwrap()
            .clone();
        assert_eq!(
            (stored["status"].as_str(), stored["version"].as_i64()),
            (Some(winner_status), Some(2))
        );
    }
}

/// T08: read watermarks.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn t08_watermarks_are_monotonic_bounded_independent_and_durable() {
    let p = pair().await;
    for n in 1..=5 {
        let (status, m) = p.send(&p.a, &format!("m{n}")).await;
        assert_eq!((status, m["seq"].as_i64()), (201, Some(n)));
    }
    // Out of order and repeated updates.
    assert_eq!(
        p.watermark(&p.b, 3).await,
        (200, json!({"last_read_seq": 3}))
    );
    assert_eq!(
        p.watermark(&p.b, 1).await,
        (200, json!({"last_read_seq": 3})),
        "never moves back"
    );
    assert_eq!(
        p.watermark(&p.b, 0).await,
        (200, json!({"last_read_seq": 3})),
        "0 reads the current value"
    );
    // Beyond the last committed message / negative.
    let (status, body) = p.watermark(&p.b, 6).await;
    assert_eq!(
        (status, body["error"]["code"].as_str()),
        (400, Some("BAD_REQUEST")),
        "{body}"
    );
    assert_eq!(p.watermark(&p.b, -1).await.0, 400);
    assert_eq!(
        p.watermark(&p.b, 3).await.1["last_read_seq"],
        3,
        "a rejected request changed nothing"
    );
    // Independent per session.
    assert_eq!(p.watermark(&p.a, 5).await.1["last_read_seq"], 5);
    assert_eq!(p.watermark(&p.b, 0).await.1["last_read_seq"], 3);

    // Concurrent updates converge on the maximum.
    let (r1, r2, r3, r4, r5) = tokio::join!(
        p.watermark(&p.b, 4),
        p.watermark(&p.b, 2),
        p.watermark(&p.b, 5),
        p.watermark(&p.b, 1),
        p.watermark(&p.b, 4)
    );
    for r in [&r1, &r2, &r3, &r4, &r5] {
        assert_eq!(r.0, 200);
        assert!(r.1["last_read_seq"].as_i64().unwrap() >= 3);
    }
    assert_eq!(p.watermark(&p.b, 0).await.1["last_read_seq"], 5);

    // A new message raises the ceiling; unread is simply `seq > watermark`.
    p.send(&p.a, "m6").await;
    assert_eq!(p.watermark(&p.b, 6).await.1["last_read_seq"], 6);

    // Durable across a restart: only the database remembers it.
    let restarted = p.env.restart().await;
    let resp = reqwest::Client::new()
        .put(format!(
            "{restarted}/api/v1/repos/{}/{}/collaboration/tasks/{}/read",
            p.env.owner, p.env.repo, p.task_id
        ))
        .bearer_auth(&p.env.owner_token)
        .header(collab_common::HDR_SESSION, &p.b.id)
        .header(collab_common::HDR_SECRET, &p.b.secret)
        .json(&json!({"last_read_seq": 0}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.json::<Value>().await.unwrap()["last_read_seq"], 6);

    // Another session's credentials cannot move B's watermark; it can only move its own.
    let (status, _) = p
        .env
        .json(
            Method::PUT,
            &p.env.url(&format!("/tasks/{}/read", p.task_id)),
            &p.env.owner_token,
            Some(&Sess {
                id: p.b.id.clone(),
                secret: p.a.secret.clone(),
            }),
            Some(json!({"last_read_seq": 6})),
        )
        .await;
    assert_eq!(status, 403);
}

#[tokio::test]
async fn watermarks_ignore_handoff_state_and_foreign_tasks() {
    let p = pair().await;
    p.send(&p.a, "m1").await;
    let (_, h) = p.create(&p.a, handoff_body("h", &p.b.id)).await;
    p.act(&p.b, h["id"].as_str().unwrap(), "accept", 1).await;
    assert_eq!(
        p.watermark(&p.b, 0).await.1["last_read_seq"],
        0,
        "accepting a handoff reads nothing"
    );
    assert_eq!(p.watermark(&p.b, 1).await.1["last_read_seq"], 1);

    common::create_repo(&p.env.base, &p.env.owner_token, "second-repo").await;
    let (status, _) = p
        .env
        .json(
            Method::PUT,
            &p.env
                .url_in("second-repo", &format!("/tasks/{}/read", p.task_id)),
            &p.env.owner_token,
            Some(&p.b),
            Some(json!({"last_read_seq": 1})),
        )
        .await;
    assert_eq!(status, 403, "session of repo 1 is not valid in repo 2");
}

/// T10 (handoffs).
#[tokio::test]
async fn t10_invalid_and_cross_repository_handoffs_are_rejected_without_writes() {
    let p = pair().await;
    common::create_repo(&p.env.base, &p.env.owner_token, "second-repo").await;
    let (status, fs) = p
        .env
        .json(
            Method::POST,
            &p.env.url_in("second-repo", "/sessions"),
            &p.env.owner_token,
            None,
            Some(json!({"display_name": "F", "client_kind": "t"})),
        )
        .await;
    assert_eq!(status, 201);
    let foreign = Sess {
        id: fs["session"]["id"].as_str().unwrap().into(),
        secret: fs["session_secret"].as_str().unwrap().into(),
    };
    let (_, ft) = p
        .env
        .json(
            Method::POST,
            &p.env.url_in("second-repo", "/tasks"),
            &p.env.owner_token,
            Some(&foreign),
            Some(json!({"title": "foreign"})),
        )
        .await;
    let ftid = ft["id"].as_str().unwrap();
    let (_, other_h) = p
        .env
        .json(
            Method::POST,
            &p.env
                .url_in("second-repo", &format!("/tasks/{ftid}/handoffs")),
            &p.env.owner_token,
            Some(&foreign),
            Some(handoff_body("fh", &foreign.id)),
        )
        .await;
    // (self handoffs are refused, so the foreign repo has none; create a real one.)
    assert_eq!(other_h["error"]["code"], "BAD_REQUEST");
    let foreign2 = {
        let (_, v) = p
            .env
            .json(
                Method::POST,
                &p.env.url_in("second-repo", "/sessions"),
                &p.env.owner_token,
                None,
                Some(json!({"display_name": "F2", "client_kind": "t"})),
            )
            .await;
        v["session"]["id"].as_str().unwrap().to_string()
    };
    let (status, fh) = p
        .env
        .json(
            Method::POST,
            &p.env
                .url_in("second-repo", &format!("/tasks/{ftid}/handoffs")),
            &p.env.owner_token,
            Some(&foreign),
            Some(handoff_body("fh2", &foreign2)),
        )
        .await;
    assert_eq!(status, 201, "{fh}");

    let mut oversize = handoff_body("o1", &p.b.id);
    oversize["next_steps"] = json!("n".repeat(16 * 1024 + 1));
    let mut many_evidence = handoff_body("o2", &p.b.id);
    many_evidence["evidence"] = json!(vec!["e"; 21]);
    let mut multiline_evidence = handoff_body("o3", &p.b.id);
    multiline_evidence["evidence"] = json!(["line1\nline2"]);
    let mut bad_sha = handoff_body("o4", &p.b.id);
    bad_sha["commit_sha"] = json!("a".repeat(40));
    let mut empty_summary = handoff_body("o5", &p.b.id);
    empty_summary["summary"] = json!("");
    let cases = vec![
        ("foreign target", handoff_body("t1", &foreign.id)),
        (
            "unknown target",
            handoff_body("t2", &uuid::Uuid::new_v4().to_string()),
        ),
        ("garbage target", handoff_body("t3", "nope")),
        ("self target", handoff_body("t4", &p.a.id)),
        ("oversize section", oversize),
        ("too much evidence", many_evidence),
        ("multiline evidence", multiline_evidence),
        ("nonexistent commit", bad_sha),
        ("empty summary", empty_summary),
        ("bad key", handoff_body("has space", &p.b.id)),
    ];
    for (what, body) in cases {
        let (status, resp) = p.create(&p.a, body).await;
        assert_eq!(status, 400, "{what}: {resp}");
        assert!(
            !resp.to_string().contains("foreign"),
            "{what} leaked foreign data"
        );
    }
    // Foreign task and foreign handoff ids are plain 404s here.
    let (status, _) = p
        .env
        .json(
            Method::POST,
            &p.env.url(&format!("/tasks/{ftid}/handoffs")),
            &p.env.owner_token,
            Some(&p.a),
            Some(handoff_body("x", &p.b.id)),
        )
        .await;
    assert_eq!(status, 404);
    let (status, _) = p.act(&p.b, fh["id"].as_str().unwrap(), "accept", 1).await;
    assert_eq!(status, 404);
    let (status, _) = p
        .env
        .json(
            Method::GET,
            &p.env.url(&format!("/tasks/{ftid}/handoffs")),
            &p.env.owner_token,
            None,
            None,
        )
        .await;
    assert_eq!(status, 404);

    // No partial writes.
    let (_, list) = p
        .env
        .json(
            Method::GET,
            &p.env.url(&format!("/tasks/{}/handoffs", p.task_id)),
            &p.env.owner_token,
            None,
            None,
        )
        .await;
    assert!(list["items"].as_array().unwrap().is_empty());
    // The untouched foreign handoff is still pending.
    let (_, flist) = p
        .env
        .json(
            Method::GET,
            &p.env
                .url_in("second-repo", &format!("/tasks/{ftid}/handoffs")),
            &p.env.owner_token,
            None,
            None,
        )
        .await;
    assert_eq!(flist["items"][0]["status"], "pending");
}

#[tokio::test]
async fn handoff_list_paginates() {
    let p = pair().await;
    for i in 0..5 {
        let (status, _) = p
            .create(&p.a, handoff_body(&format!("h{i}"), &p.b.id))
            .await;
        assert_eq!(status, 201);
    }
    let mut seen = Vec::new();
    let mut cursor: Option<String> = None;
    loop {
        let mut url = p.env.url(&format!("/tasks/{}/handoffs?limit=2", p.task_id));
        if let Some(c) = &cursor {
            url.push_str(&format!("&cursor={c}"));
        }
        let (status, body) = p
            .env
            .json(Method::GET, &url, &p.env.owner_token, None, None)
            .await;
        assert_eq!(status, 200, "{body}");
        seen.extend(
            body["items"]
                .as_array()
                .unwrap()
                .iter()
                .map(|h| h["client_handoff_id"].as_str().unwrap().to_string()),
        );
        match body["next_cursor"].as_str() {
            Some(c) => cursor = Some(c.to_string()),
            None => break,
        }
    }
    assert_eq!(seen.len(), 5);
    let unique: std::collections::BTreeSet<_> = seen.iter().collect();
    assert_eq!(unique.len(), 5, "no duplicates across pages");
}
