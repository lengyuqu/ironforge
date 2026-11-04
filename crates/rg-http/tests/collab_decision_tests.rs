//! P1 decision traceability (U02): acceptance N04 and N05.
//!
//! Real HTTP server and SQLite database; with `IRONFORGE_TEST_DATABASE_URL` the
//! same tests run on PostgreSQL or MySQL.

mod collab_common;
mod common;

use collab_common::{Env, Sess};
use reqwest::Method;
use serde_json::{json, Value};

type Fut<'a> = std::pin::Pin<Box<dyn std::future::Future<Output = (u16, Value)> + 'a>>;

async fn new_task(env: &Env, s: &Sess, extra: Value) -> Value {
    let mut body = json!({"title": "task"});
    body.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    let (status, task) = env
        .json(
            Method::POST,
            &env.url("/tasks"),
            &env.owner_token,
            Some(s),
            Some(body),
        )
        .await;
    assert_eq!(status, 201, "{task}");
    task
}

async fn ask(env: &Env, tid: &str, extra: Value) -> Value {
    let mut body = json!({
        "client_question_id": uuid::Uuid::new_v4().to_string(),
        "recipient": {"kind": "repo_writers"},
        "title": "Which database?",
        "body": "SQLite or PostgreSQL?",
    });
    body.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    let (status, q) = env
        .json(
            Method::POST,
            &env.url(&format!("/tasks/{tid}/questions")),
            &env.owner_token,
            None,
            Some(body),
        )
        .await;
    assert_eq!(status, 201, "{q}");
    q
}

/// Ask a question and answer it; returns `(question_id, answer_id)`.
async fn answered_question(env: &Env, tid: &str) -> (String, String) {
    let q = ask(env, tid, json!({})).await;
    let qid = q["id"].as_str().unwrap().to_string();
    let (status, answered) = env
        .json(
            Method::POST,
            &env.url(&format!("/questions/{qid}/answer")),
            &env.owner_token,
            None,
            Some(json!({
                "client_answer_id": uuid::Uuid::new_v4().to_string(),
                "expected_version": q["version"],
                "body": "PostgreSQL",
                "acknowledged_context_hash": q["context_hash"],
            })),
        )
        .await;
    assert_eq!(status, 200, "{answered}");
    (qid, answered["answer"]["id"].as_str().unwrap().to_string())
}

async fn decide(env: &Env, tid: &str, who: Option<&Sess>, key: &str, extra: Value) -> (u16, Value) {
    let mut body = json!({
        "client_decision_id": key,
        "title": "Use PostgreSQL",
        "rationale": "Needs concurrent writers.",
    });
    body.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    env.json(
        Method::POST,
        &env.url(&format!("/tasks/{tid}/decisions")),
        &env.owner_token,
        who,
        Some(body),
    )
    .await
}

async fn decide_ok(env: &Env, tid: &str, who: Option<&Sess>, key: &str) -> Value {
    let (status, d) = decide(env, tid, who, key, json!({})).await;
    assert_eq!(status, 201, "{d}");
    d
}

async fn change(env: &Env, did: &str, who: Option<&Sess>, body: Value) -> (u16, Value) {
    env.json(
        Method::POST,
        &env.url(&format!("/decisions/{did}/changes")),
        &env.owner_token,
        who,
        Some(body),
    )
    .await
}

fn supersede(key: &str, by: &str) -> Value {
    json!({"client_change_id": key, "kind": "superseded", "replacement_decision_id": by, "reason": "better option"})
}

fn revoke(key: &str) -> Value {
    json!({"client_change_id": key, "kind": "revoked", "reason": "no longer valid"})
}

async fn get_decision(env: &Env, id: &str) -> Value {
    let (status, d) = env
        .json(
            Method::GET,
            &env.url(&format!("/decisions/{id}")),
            &env.owner_token,
            None,
            None,
        )
        .await;
    assert_eq!(status, 200, "{d}");
    d
}

async fn list_decisions(env: &Env, tid: &str, query: &str) -> Value {
    let (status, page) = env
        .json(
            Method::GET,
            &env.url(&format!("/tasks/{tid}/decisions{query}")),
            &env.owner_token,
            None,
            None,
        )
        .await;
    assert_eq!(status, 200, "{page}");
    page
}

async fn count(env: &Env, tid: &str) -> usize {
    list_decisions(env, tid, "").await["items"]
        .as_array()
        .unwrap()
        .len()
}

fn code(body: &Value) -> &str {
    body["error"]["code"].as_str().unwrap_or_default()
}

// ---------------------------------------------------------------------- N04

#[tokio::test]
async fn n04_a_decision_cites_a_question_and_its_answer_and_stays_traceable() {
    let env = Env::new().await;
    let sha = env.push_tree(&env.repo, "feature", &[("a.txt", "1")], &[]);
    let a = env.session(&env.owner_token, "A").await;
    let task = new_task(&env, &a, json!({"branch": "feature"})).await;
    let tid = task["id"].as_str().unwrap();
    let (qid, aid) = answered_question(&env, tid).await;

    let (status, d) = decide(
        &env,
        tid,
        Some(&a),
        "d1",
        json!({"question_id": qid, "answer_id": aid, "context": {"head_sha": sha}}),
    )
    .await;
    assert_eq!(status, 201, "{d}");
    assert_eq!(d["question_id"], qid.as_str());
    assert_eq!(d["answer_id"], aid.as_str());
    assert_eq!(d["status"], "active");
    assert_eq!(d["sequence"], 1);
    assert_eq!(d["context"]["head_sha"], sha.as_str());
    assert_eq!(d["context"]["task_version"], task["version"]);
    // Source is traceable and marked as untrusted content.
    assert_eq!(d["actor_kind"], "session");
    assert_eq!(d["actor_id"], a.id.as_str());
    assert_eq!(d["origin"], "user_content");
    assert_eq!(d["context_freshness"], "fresh");
    assert!(d["change"].is_null());

    // The same decision through the user door (no session headers).
    let (status, by_user) = decide(&env, tid, None, "d2", json!({"question_id": qid})).await;
    assert_eq!(status, 201, "{by_user}");
    assert_eq!(by_user["actor_kind"], "user");
    assert_eq!(by_user["actor_id"], env.owner_id.to_string());
    assert!(by_user["answer_id"].is_null());

    let read = get_decision(&env, d["id"].as_str().unwrap()).await;
    assert_eq!(read["rationale"], "Needs concurrent writers.");
    assert_eq!(read["supersedes"], json!([]));
    let page = list_decisions(&env, tid, "").await;
    let ids: Vec<&str> = page["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["id"].as_str().unwrap())
        .collect();
    assert_eq!(
        ids,
        [d["id"].as_str().unwrap(), by_user["id"].as_str().unwrap()]
    );
}

#[tokio::test]
async fn n04_mismatched_question_or_answer_links_are_refused_without_writing() {
    let env = Env::new().await;
    let a = env.session(&env.owner_token, "A").await;
    let task = new_task(&env, &a, json!({})).await;
    let other = new_task(&env, &a, json!({})).await;
    let tid = task["id"].as_str().unwrap();
    let other_id = other["id"].as_str().unwrap();
    let (q1, a1) = answered_question(&env, tid).await;
    let (q2, a2) = answered_question(&env, tid).await;
    let (q_other, a_other) = answered_question(&env, other_id).await;
    let unknown = uuid::Uuid::new_v4().to_string();

    let cases = [
        (
            "answer of another question of the task",
            json!({"question_id": q1, "answer_id": a2}),
        ),
        ("question of another task", json!({"question_id": q_other})),
        (
            "answer of another task's question",
            json!({"question_id": q1, "answer_id": a_other}),
        ),
        ("answer without question", json!({"answer_id": a1})),
        ("unknown question", json!({"question_id": unknown})),
        (
            "unknown answer",
            json!({"question_id": q1, "answer_id": unknown}),
        ),
        ("not an id", json!({"question_id": "nope"})),
    ];
    for (i, (what, extra)) in cases.iter().enumerate() {
        let (status, body) = decide(&env, tid, Some(&a), &format!("bad{i}"), extra.clone()).await;
        assert_eq!(status, 400, "{what}: {body}");
    }
    // An answer of an unanswered question cannot exist; its question can be cited.
    let open = ask(&env, tid, json!({})).await;
    let (status, body) = decide(
        &env,
        tid,
        Some(&a),
        "open-q",
        json!({"question_id": open["id"], "answer_id": a1}),
    )
    .await;
    assert_eq!(status, 400, "{body}");
    assert_eq!(
        count(&env, tid).await,
        0,
        "rejected requests left no record"
    );
    assert_eq!(count(&env, other_id).await, 0);

    // The valid pairs of both questions still work.
    for (i, (q, a_id)) in [(&q1, &a1), (&q2, &a2)].into_iter().enumerate() {
        let (status, body) = decide(
            &env,
            tid,
            Some(&a),
            &format!("ok{i}"),
            json!({"question_id": q, "answer_id": a_id}),
        )
        .await;
        assert_eq!(status, 201, "{body}");
    }
    assert_eq!(count(&env, tid).await, 2);
}

#[tokio::test]
async fn n04_a_question_of_another_repository_reads_like_any_mismatch() {
    let env = Env::new().await;
    let a = env.session(&env.owner_token, "A").await;
    let task = new_task(&env, &a, json!({})).await;
    let tid = task["id"].as_str().unwrap();
    common::create_repo(&env.base, &env.owner_token, "other-repo").await;
    let a2 = {
        let (status, body) = env
            .json(
                Method::POST,
                &env.url_in("other-repo", "/sessions"),
                &env.owner_token,
                None,
                Some(json!({"display_name": "A2", "client_kind": "t"})),
            )
            .await;
        assert_eq!(status, 201, "{body}");
        Sess {
            id: body["session"]["id"].as_str().unwrap().to_string(),
            secret: body["session_secret"].as_str().unwrap().to_string(),
        }
    };
    let (status, foreign_task) = env
        .json(
            Method::POST,
            &env.url_in("other-repo", "/tasks"),
            &env.owner_token,
            Some(&a2),
            Some(json!({"title": "t"})),
        )
        .await;
    assert_eq!(status, 201, "{foreign_task}");
    let (status, foreign_q) = env
        .json(
            Method::POST,
            &env.url_in("other-repo", &format!("/tasks/{}/questions", foreign_task["id"].as_str().unwrap())),
            &env.owner_token,
            None,
            Some(json!({"client_question_id": "fq", "recipient": {"kind": "repo_writers"}, "title": "q", "body": "b"})),
        )
        .await;
    assert_eq!(status, 201, "{foreign_q}");

    let (status, body) = decide(
        &env,
        tid,
        Some(&a),
        "x",
        json!({"question_id": foreign_q["id"]}),
    )
    .await;
    assert_eq!(status, 400, "{body}");
    assert_eq!(count(&env, tid).await, 0);
}

#[tokio::test]
async fn n04_a_stale_task_version_or_unknown_commit_is_refused_and_replays_are_idempotent() {
    let env = Env::new().await;
    let a = env.session(&env.owner_token, "A").await;
    let task = new_task(&env, &a, json!({})).await;
    let tid = task["id"].as_str().unwrap();

    let (status, first) = decide(
        &env,
        tid,
        Some(&a),
        "v1",
        json!({"context": {"task_version": 1}}),
    )
    .await;
    assert_eq!(status, 201, "{first}");
    assert_eq!(first["context"]["task_version"], 1);

    // The task moves on.
    let (status, _) = env
        .json(
            Method::PATCH,
            &env.url(&format!("/tasks/{tid}")),
            &env.owner_token,
            Some(&a),
            Some(json!({"expected_version": 1, "status": "blocked"})),
        )
        .await;
    assert_eq!(status, 200);

    let (status, body) = decide(
        &env,
        tid,
        Some(&a),
        "v2",
        json!({"context": {"task_version": 1}}),
    )
    .await;
    assert_eq!(status, 409, "{body}");
    assert_eq!(code(&body), "VERSION_CONFLICT");
    assert_eq!(body["error"]["details"]["current_version"], 2);
    let (status, body) = decide(
        &env,
        tid,
        Some(&a),
        "v3",
        json!({"context": {"head_sha": "0".repeat(40)}}),
    )
    .await;
    assert_eq!(status, 400, "{body}");
    let (status, body) = decide(&env, tid, Some(&a), "v4", json!({"title": "   "})).await;
    assert_eq!(status, 400, "{body}");
    assert_eq!(count(&env, tid).await, 1, "only the valid decision exists");

    // Replay: same key, same body → 200 and the same record, even though the
    // task version it named is no longer current. A different body is refused.
    let (status, replay) = decide(
        &env,
        tid,
        Some(&a),
        "v1",
        json!({"context": {"task_version": 1}}),
    )
    .await;
    assert_eq!(status, 200, "{replay}");
    assert_eq!(replay["id"], first["id"]);
    let (status, reused) = decide(
        &env,
        tid,
        Some(&a),
        "v1",
        json!({"title": "Another title", "context": {"task_version": 1}}),
    )
    .await;
    assert_eq!(status, 409, "{reused}");
    assert_eq!(code(&reused), "IDEMPOTENCY_KEY_REUSED");

    // An omitted version records the current one.
    let current = decide_ok(&env, tid, Some(&a), "v5").await;
    assert_eq!(current["context"]["task_version"], 2);
    assert_eq!(current["sequence"], 2);
    assert_eq!(count(&env, tid).await, 2);
}

#[tokio::test]
async fn n04_access_is_limited_to_the_working_group_and_writers() {
    let env = Env::new().await;
    let a = env.session(&env.owner_token, "A").await;
    let task = new_task(&env, &a, json!({})).await;
    let tid = task["id"].as_str().unwrap();
    let d = decide_ok(&env, tid, Some(&a), "d").await;
    let did = d["id"].as_str().unwrap();

    let (stranger, _) = env.user("stranger").await;
    let (reader, reader_id) = env.user("reader").await;
    env.add_collaborator(reader_id, "read").await;
    for (path, method, body) in [
        (format!("/tasks/{tid}/decisions"), Method::GET, None),
        (format!("/decisions/{did}"), Method::GET, None),
        (
            format!("/tasks/{tid}/decisions"),
            Method::POST,
            Some(json!({"client_decision_id": "s", "title": "t", "rationale": "r"})),
        ),
        (
            format!("/decisions/{did}/changes"),
            Method::POST,
            Some(revoke("s")),
        ),
    ] {
        let (status, _) = env
            .json(
                method.clone(),
                &env.url(&path),
                &stranger,
                None,
                body.clone(),
            )
            .await;
        assert_eq!(status, 403, "stranger {method} {path}");
    }
    // A read collaborator may read but not write.
    let (status, _) = env
        .json(
            Method::GET,
            &env.url(&format!("/decisions/{did}")),
            &reader,
            None,
            None,
        )
        .await;
    assert_eq!(status, 200);
    let (status, _) = env
        .json(
            Method::POST,
            &env.url(&format!("/decisions/{did}/changes")),
            &reader,
            None,
            Some(revoke("r")),
        )
        .await;
    assert_eq!(status, 403);
    let anonymous = env
        .send(
            Method::GET,
            &env.url(&format!("/decisions/{did}")),
            None,
            None,
            None,
        )
        .await;
    assert_eq!(anonymous.status().as_u16(), 401);
    assert!(
        get_decision(&env, did).await["change"].is_null(),
        "nothing was changed"
    );
}

// ---------------------------------------------------------------------- N05

#[tokio::test]
async fn n05_racing_revoke_and_supersede_leave_exactly_one_terminal_event() {
    let env = Env::new().await;
    let a = env.session(&env.owner_token, "A").await;
    let b = env.session(&env.owner_token, "B").await;
    let task = new_task(&env, &a, json!({})).await;
    let tid = task["id"].as_str().unwrap();

    for round in 0..5 {
        let old = decide_ok(&env, tid, Some(&a), &format!("old{round}")).await;
        let new = decide_ok(&env, tid, Some(&a), &format!("new{round}")).await;
        let oid = old["id"].as_str().unwrap();
        let results: Vec<(u16, Value)> = futures::future::join_all(vec![
            Box::pin(change(&env, oid, Some(&a), revoke(&format!("r{round}")))) as Fut,
            Box::pin(change(
                &env,
                oid,
                Some(&b),
                supersede(&format!("s{round}"), new["id"].as_str().unwrap()),
            )),
            Box::pin(change(&env, oid, None, revoke(&format!("u{round}")))),
        ])
        .await;
        let winners: Vec<_> = results.iter().filter(|(s, _)| *s == 201).collect();
        assert_eq!(
            winners.len(),
            1,
            "exactly one terminal change wins: {results:?}"
        );
        for (status, body) in results.iter().filter(|(s, _)| *s != 201) {
            assert_eq!(*status, 409, "{body}");
            assert_eq!(code(body), "DECISION_ALREADY_CHANGED");
            // The loser is told who won.
            assert_eq!(
                body["error"]["details"]["kind"],
                winners[0].1["change"]["kind"]
            );
            assert_eq!(
                body["error"]["details"]["change_id"],
                winners[0].1["change"]["id"]
            );
        }
        let read = get_decision(&env, oid).await;
        assert_eq!(read["change"]["id"], winners[0].1["change"]["id"]);
        assert_eq!(read["status"], winners[0].1["change"]["kind"]);
        // The other decision is untouched.
        assert!(get_decision(&env, new["id"].as_str().unwrap()).await["change"].is_null());
    }
}

#[tokio::test]
async fn n05_a_decision_takes_one_change_only_and_history_is_preserved() {
    let env = Env::new().await;
    let a = env.session(&env.owner_token, "A").await;
    let task = new_task(&env, &a, json!({})).await;
    let tid = task["id"].as_str().unwrap();
    let d1 = decide_ok(&env, tid, Some(&a), "d1").await;
    let d2 = decide_ok(&env, tid, Some(&a), "d2").await;
    let (id1, id2) = (d1["id"].as_str().unwrap(), d2["id"].as_str().unwrap());

    let (status, done) = change(&env, id1, Some(&a), supersede("c1", id2)).await;
    assert_eq!(status, 201, "{done}");
    assert_eq!(done["status"], "superseded");
    assert_eq!(done["change"]["replacement_decision_id"], id2);
    assert_eq!(done["change"]["reason"], "better option");
    assert_eq!(done["change"]["actor_id"], a.id.as_str());
    assert_eq!(done["change"]["origin"], "user_content");

    // Replay → 200 with the same event; same key, other body → 409.
    let (status, replay) = change(&env, id1, Some(&a), supersede("c1", id2)).await;
    assert_eq!(status, 200, "{replay}");
    assert_eq!(replay["change"]["id"], done["change"]["id"]);
    let (status, reused) = change(&env, id1, Some(&a), revoke("c1")).await;
    assert_eq!(status, 409, "{reused}");
    assert_eq!(code(&reused), "IDEMPOTENCY_KEY_REUSED");

    // A second change — of any kind, by anyone — is refused.
    for (who, body) in [(Some(&a), revoke("c2")), (None, supersede("c3", id2))] {
        let (status, body) = change(&env, id1, who, body).await;
        assert_eq!(status, 409, "{body}");
        assert_eq!(code(&body), "DECISION_ALREADY_CHANGED");
        assert_eq!(body["error"]["details"]["kind"], "superseded");
    }

    // History: the original text is intact, the replacement knows what it replaced,
    // and the listing shows both decisions with the event.
    let read1 = get_decision(&env, id1).await;
    assert_eq!(read1["title"], d1["title"]);
    assert_eq!(read1["rationale"], d1["rationale"]);
    assert_eq!(read1["context"], d1["context"]);
    assert_eq!(read1["created_at"], d1["created_at"]);
    assert_eq!(read1["change"]["id"], done["change"]["id"]);
    let read2 = get_decision(&env, id2).await;
    assert_eq!(read2["supersedes"], json!([id1]));
    assert_eq!(read2["status"], "active");
    let page = list_decisions(&env, tid, "").await;
    let statuses: Vec<&str> = page["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["status"].as_str().unwrap())
        .collect();
    assert_eq!(statuses, ["superseded", "active"]);

    // A revoke event keeps the decision readable too, and is recorded as revoked.
    let (status, revoked) = change(&env, id2, None, revoke("c4")).await;
    assert_eq!(status, 201, "{revoked}");
    assert_eq!(revoked["status"], "revoked");
    assert_eq!(revoked["change"]["actor_kind"], "user");
    assert!(revoked["change"]["replacement_decision_id"].is_null());
    assert_eq!(
        get_decision(&env, id2).await["supersedes"],
        json!([id1]),
        "history survives the revoke"
    );

    // Malformed changes write nothing.
    let d3 = decide_ok(&env, tid, Some(&a), "d3").await;
    let id3 = d3["id"].as_str().unwrap();
    for (what, body) in [
        (
            "supersede without replacement",
            json!({"client_change_id": "m1", "kind": "superseded", "reason": "r"}),
        ),
        (
            "revoke with replacement",
            json!({"client_change_id": "m2", "kind": "revoked", "replacement_decision_id": id2, "reason": "r"}),
        ),
        (
            "unknown kind",
            json!({"client_change_id": "m3", "kind": "deleted", "reason": "r"}),
        ),
        (
            "empty reason",
            json!({"client_change_id": "m4", "kind": "revoked", "reason": ""}),
        ),
        ("self replacement", supersede("m5", id3)),
        (
            "unknown replacement",
            supersede("m6", &uuid::Uuid::new_v4().to_string()),
        ),
        ("not an id", supersede("m7", "nope")),
    ] {
        let (status, body) = change(&env, id3, Some(&a), body).await;
        assert_eq!(status, 400, "{what}: {body}");
    }
    assert!(get_decision(&env, id3).await["change"].is_null());
    let (status, _) = change(
        &env,
        &uuid::Uuid::new_v4().to_string(),
        Some(&a),
        revoke("m8"),
    )
    .await;
    assert_eq!(status, 404);
}

#[tokio::test]
async fn n05_supersede_cycles_and_foreign_replacements_are_rejected() {
    let env = Env::new().await;
    let a = env.session(&env.owner_token, "A").await;
    let task = new_task(&env, &a, json!({})).await;
    let other_task = new_task(&env, &a, json!({})).await;
    let tid = task["id"].as_str().unwrap();
    let mut ids = Vec::new();
    for i in 0..4 {
        ids.push(
            decide_ok(&env, tid, Some(&a), &format!("d{i}")).await["id"]
                .as_str()
                .unwrap()
                .to_string(),
        );
    }
    let foreign = decide_ok(&env, other_task["id"].as_str().unwrap(), Some(&a), "f").await;

    // d0 → d1 → d2 is a fine chain.
    for (i, (from, to)) in [(0, 1), (1, 2)].into_iter().enumerate() {
        let (status, body) = change(
            &env,
            &ids[from],
            Some(&a),
            supersede(&format!("c{i}"), &ids[to]),
        )
        .await;
        assert_eq!(status, 201, "{body}");
    }
    // d2 → d0 and d2 → d1 would close a cycle; they are named as such.
    for (i, target) in [&ids[0], &ids[1]].into_iter().enumerate() {
        let (status, body) = change(
            &env,
            &ids[2],
            Some(&a),
            supersede(&format!("cy{i}"), target),
        )
        .await;
        assert_eq!(status, 409, "{body}");
        assert_eq!(code(&body), "DECISION_CYCLE");
    }
    // d3 → d0: no cycle (d0 is superseded, not pointing at d3) but not a later decision.
    let (status, body) = change(&env, &ids[3], Some(&a), supersede("older", &ids[0])).await;
    assert_eq!(status, 409, "{body}");
    assert_eq!(code(&body), "REPLACEMENT_NOT_NEWER");
    // A decision of another task cannot be a replacement.
    let (status, body) = change(
        &env,
        &ids[3],
        Some(&a),
        supersede("foreign", foreign["id"].as_str().unwrap()),
    )
    .await;
    assert_eq!(status, 400, "{body}");

    // The refused changes left d2 and d3 without any event.
    for id in [&ids[2], &ids[3]] {
        assert!(get_decision(&env, id).await["change"].is_null());
    }
    assert_eq!(
        get_decision(&env, &ids[2]).await["supersedes"],
        json!([ids[1]])
    );
}

#[tokio::test]
async fn n05_opposite_supersedes_racing_cannot_form_a_cycle() {
    let env = Env::new().await;
    let a = env.session(&env.owner_token, "A").await;
    let task = new_task(&env, &a, json!({})).await;
    let tid = task["id"].as_str().unwrap();
    for round in 0..5 {
        let x = decide_ok(&env, tid, Some(&a), &format!("x{round}")).await;
        let y = decide_ok(&env, tid, Some(&a), &format!("y{round}")).await;
        let (xid, yid) = (x["id"].as_str().unwrap(), y["id"].as_str().unwrap());
        let results: Vec<(u16, Value)> = futures::future::join_all(vec![
            Box::pin(change(
                &env,
                xid,
                Some(&a),
                supersede(&format!("a{round}"), yid),
            )) as Fut,
            Box::pin(change(
                &env,
                yid,
                None,
                supersede(&format!("b{round}"), xid),
            )),
        ])
        .await;
        // Only x → y (a later replacement) can ever succeed; y → x never does.
        assert_eq!(results[0].0, 201, "{results:?}");
        assert_eq!(results[1].0, 409, "{results:?}");
        assert!(
            matches!(
                code(&results[1].1),
                "DECISION_CYCLE" | "REPLACEMENT_NOT_NEWER"
            ),
            "{results:?}"
        );
        assert_eq!(get_decision(&env, xid).await["status"], "superseded");
        assert_eq!(get_decision(&env, yid).await["status"], "active");
    }
}

#[tokio::test]
async fn n05_freshness_tells_fresh_from_stale_from_unknown_and_never_revokes() {
    let env = Env::new().await;
    let first = env.push_tree(&env.repo, "feature", &[("a.txt", "1")], &[]);
    let a = env.session(&env.owner_token, "A").await;
    let on_branch = new_task(&env, &a, json!({"branch": "feature"})).await;
    let no_branch = new_task(&env, &a, json!({})).await;
    let (bid, nid) = (
        on_branch["id"].as_str().unwrap(),
        no_branch["id"].as_str().unwrap(),
    );

    let at_head = decide(
        &env,
        bid,
        Some(&a),
        "h",
        json!({"context": {"head_sha": first}}),
    )
    .await
    .1;
    let version_only = decide_ok(&env, bid, Some(&a), "v").await;
    let no_declared_branch = decide(
        &env,
        nid,
        Some(&a),
        "n",
        json!({"context": {"head_sha": first}}),
    )
    .await
    .1;
    for d in [&at_head, &version_only] {
        assert_eq!(
            get_decision(&env, d["id"].as_str().unwrap()).await["context_freshness"],
            "fresh"
        );
    }
    // A commit can only be judged against a declared branch.
    assert_eq!(
        get_decision(&env, no_declared_branch["id"].as_str().unwrap()).await["context_freshness"],
        "unknown"
    );

    // The branch moves on: only the decision tied to the old head is stale.
    env.push_tree(&env.repo, "feature", &[("a.txt", "2")], &[]);
    assert_eq!(
        get_decision(&env, at_head["id"].as_str().unwrap()).await["context_freshness"],
        "stale"
    );
    assert_eq!(
        get_decision(&env, version_only["id"].as_str().unwrap()).await["context_freshness"],
        "fresh"
    );

    // The task changes: every decision of it is stale, the other task's is not.
    let (status, _) = env
        .json(
            Method::PATCH,
            &env.url(&format!("/tasks/{bid}")),
            &env.owner_token,
            Some(&a),
            Some(json!({"expected_version": 1, "status": "blocked"})),
        )
        .await;
    assert_eq!(status, 200);
    let page = list_decisions(&env, bid, "").await;
    for item in page["items"].as_array().unwrap() {
        assert_eq!(item["context_freshness"], "stale", "{item}");
        // Stale is reported, never acted on.
        assert_eq!(item["status"], "active");
        assert!(item["change"].is_null());
    }
    assert_eq!(
        get_decision(&env, no_declared_branch["id"].as_str().unwrap()).await["context_freshness"],
        "unknown"
    );

    // A stale decision can still be superseded or revoked explicitly.
    let fresh = decide_ok(&env, bid, Some(&a), "after").await;
    assert_eq!(fresh["context"]["task_version"], 2);
    let (status, body) = change(
        &env,
        at_head["id"].as_str().unwrap(),
        Some(&a),
        supersede("s", fresh["id"].as_str().unwrap()),
    )
    .await;
    assert_eq!(status, 201, "{body}");
    assert_eq!(
        body["context_freshness"], "stale",
        "freshness is independent of the change"
    );
}

#[tokio::test]
async fn n05_concurrent_creates_get_distinct_sequence_numbers_and_pages_follow_them() {
    let env = Env::new().await;
    let a = env.session(&env.owner_token, "A").await;
    let task = new_task(&env, &a, json!({})).await;
    let tid = task["id"].as_str().unwrap();

    let keys: Vec<String> = (0..6).map(|i| format!("par{i}")).collect();
    let results: Vec<(u16, Value)> = futures::future::join_all(
        keys.iter()
            .map(|key| Box::pin(decide(&env, tid, Some(&a), key, json!({}))) as Fut),
    )
    .await;
    assert!(results.iter().all(|(s, _)| *s == 201), "{results:?}");
    let mut seqs: Vec<i64> = results
        .iter()
        .map(|(_, d)| d["sequence"].as_i64().unwrap())
        .collect();
    seqs.sort_unstable();
    assert_eq!(seqs, [1, 2, 3, 4, 5, 6]);

    // Listing is in sequence order across pages, with no gap or repeat.
    let mut seen = Vec::new();
    let mut cursor = String::new();
    loop {
        let page = list_decisions(&env, tid, &format!("?limit=4{cursor}")).await;
        seen.extend(
            page["items"]
                .as_array()
                .unwrap()
                .iter()
                .map(|i| i["sequence"].as_i64().unwrap()),
        );
        match page["next_cursor"].as_str() {
            Some(next) => cursor = format!("&cursor={next}"),
            None => break,
        }
    }
    assert_eq!(seen, [1, 2, 3, 4, 5, 6]);

    // A racing retry of one key creates one decision.
    let results: Vec<(u16, Value)> = futures::future::join_all(
        (0..4).map(|_| Box::pin(decide(&env, tid, Some(&a), "same-key", json!({}))) as Fut),
    )
    .await;
    assert!(
        results
            .iter()
            .all(|(s, d)| matches!(*s, 200 | 201) && d["id"] == results[0].1["id"]),
        "{results:?}"
    );
    assert_eq!(count(&env, tid).await, 7);
}
