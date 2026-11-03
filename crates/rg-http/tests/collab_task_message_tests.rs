//! C03 acceptance: tasks, messages, versions, idempotency, ordering and
//! cross-repository isolation (T04, T05, T06, T10).

mod common;
mod collab_common;

use collab_common::{Env, Sess};
use reqwest::Method;
use serde_json::{json, Value};

async fn new_task(env: &Env, token: &str, s: &Sess, title: &str) -> Value {
    let (status, body) = env
        .json(Method::POST, &env.url("/tasks"), token, Some(s), Some(json!({"title": title})))
        .await;
    assert_eq!(status, 201, "{body}");
    body
}

fn msg(client_id: &str, body: &str) -> Value {
    json!({"client_message_id": client_id, "kind": "note", "body": body})
}

async fn send(env: &Env, task: &str, s: &Sess, payload: Value) -> (u16, Value) {
    env.json(
        Method::POST,
        &env.url(&format!("/tasks/{task}/messages")),
        &env.owner_token,
        Some(s),
        Some(payload),
    )
    .await
}

async fn messages(env: &Env, task: &str, after: i64) -> Value {
    let (status, body) = env
        .json(
            Method::GET,
            &env.url(&format!("/tasks/{task}/messages?after_seq={after}&limit=100")),
            &env.owner_token,
            None,
            None,
        )
        .await;
    assert_eq!(status, 200, "{body}");
    body
}

#[tokio::test]
async fn task_crud_partial_update_and_clearing() {
    let env = Env::new().await;
    let s = env.session(&env.owner_token, "A").await;
    let (status, task) = env
        .json(
            Method::POST,
            &env.url("/tasks"),
            &env.owner_token,
            Some(&s),
            Some(json!({
                "title": "Fix flaky test", "description": "see CI", "branch": "fix/flaky",
                "paths": ["src/lib.rs", "tests/**"], "owner_session_id": s.id
            })),
        )
        .await;
    assert_eq!(status, 201, "{task}");
    assert_eq!(task["status"], "open");
    assert_eq!(task["version"], 1);
    assert_eq!(task["last_message_seq"], 0);
    assert_eq!(task["origin"], "user_content");
    assert_eq!(task["paths"], json!(["src/lib.rs", "tests/**"]));
    let id = task["id"].as_str().unwrap();
    let url = env.url(&format!("/tasks/{id}"));

    // Omitted fields unchanged.
    let (status, t2) = env
        .json(Method::PATCH, &url, &env.owner_token, Some(&s), Some(json!({"expected_version": 1, "status": "in_progress"})))
        .await;
    assert_eq!(status, 200, "{t2}");
    assert_eq!((t2["version"].as_i64(), t2["status"].as_str()), (Some(2), Some("in_progress")));
    assert_eq!(t2["title"], "Fix flaky test");
    assert_eq!(t2["branch"], "fix/flaky");
    assert_eq!(t2["owner_session_id"], s.id.as_str());

    // Explicit null clears; other fields stay.
    let (status, t3) = env
        .json(
            Method::PATCH,
            &url,
            &env.owner_token,
            Some(&s),
            Some(json!({"expected_version": 2, "branch": null, "owner_session_id": null, "description": null})),
        )
        .await;
    assert_eq!(status, 200, "{t3}");
    assert!(t3["branch"].is_null() && t3["owner_session_id"].is_null() && t3["description"].is_null());
    assert_eq!(t3["status"], "in_progress");
    assert_eq!(t3["paths"], json!(["src/lib.rs", "tests/**"]));

    // Done can be reopened (any status to any status).
    for (v, st) in [(3, "done"), (4, "open"), (5, "blocked"), (6, "waiting")] {
        let (status, t) = env
            .json(Method::PATCH, &url, &env.owner_token, Some(&s), Some(json!({"expected_version": v, "status": st})))
            .await;
        assert_eq!((status, t["status"].as_str()), (200, Some(st)), "{t}");
    }

    // Validation.
    for body in [
        json!({"expected_version": 7}),
        json!({"expected_version": 7, "status": "merged"}),
        json!({"expected_version": 7, "title": ""}),
        json!({"expected_version": 7, "paths": ["../x"]}),
        json!({"expected_version": 7, "branch": "bad branch"}),
        json!({"expected_version": 7, "issue_number": 999}),
        json!({"expected_version": 7, "owner_session_id": uuid::Uuid::new_v4().to_string()}),
    ] {
        let (status, resp) = env.json(Method::PATCH, &url, &env.owner_token, Some(&s), Some(body.clone())).await;
        assert_eq!(status, 400, "{body} -> {resp}");
    }
    // Rejected updates did not consume the version.
    let (_, current) = env.json(Method::GET, &url, &env.owner_token, None, None).await;
    assert_eq!(current["version"], 7);

    // Listing with status filter.
    new_task(&env, &env.owner_token, &s, "other").await;
    let (_, waiting) = env.json(Method::GET, &env.url("/tasks?status=waiting"), &env.owner_token, None, None).await;
    assert_eq!(waiting["items"].as_array().unwrap().len(), 1);
    let (status, _) = env.json(Method::GET, &env.url("/tasks?status=bogus"), &env.owner_token, None, None).await;
    assert_eq!(status, 400);
    let (_, all) = env.json(Method::GET, &env.url("/tasks"), &env.owner_token, None, None).await;
    assert_eq!(all["items"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn task_list_paginates_without_duplicates() {
    let env = Env::new().await;
    let s = env.session(&env.owner_token, "A").await;
    for i in 0..5 {
        new_task(&env, &env.owner_token, &s, &format!("t{i}")).await;
    }
    let mut seen = std::collections::BTreeSet::new();
    let mut cursor: Option<String> = None;
    loop {
        let mut url = env.url("/tasks?limit=2&status=open");
        if let Some(c) = &cursor {
            url.push_str(&format!("&cursor={c}"));
        }
        let (status, body) = env.json(Method::GET, &url, &env.owner_token, None, None).await;
        assert_eq!(status, 200, "{body}");
        for t in body["items"].as_array().unwrap() {
            assert!(seen.insert(t["id"].as_str().unwrap().to_string()));
        }
        match body["next_cursor"].as_str() {
            Some(c) => cursor = Some(c.to_string()),
            None => {
                // A cursor minted for one filter is refused under another.
                let (status, _) = env
                    .json(Method::GET, &env.url(&format!("/tasks?status=done&cursor={}", cursor.as_deref().unwrap_or("x"))), &env.owner_token, None, None)
                    .await;
                assert_eq!(status, 400);
                break;
            }
        }
    }
    assert_eq!(seen.len(), 5);
}

/// T04: two sessions patch the same version at once.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn t04_concurrent_patches_of_one_version_have_a_single_winner() {
    let env = Env::new().await;
    let (a, b) = (
        env.session(&env.owner_token, "A").await,
        env.session(&env.owner_token, "B").await,
    );
    for round in 0..4 {
        let task = new_task(&env, &env.owner_token, &a, &format!("race-{round}")).await;
        let id = task["id"].as_str().unwrap().to_string();
        let url = env.url(&format!("/tasks/{id}"));
        let (ra, rb) = tokio::join!(
            env.json(Method::PATCH, &url, &env.owner_token, Some(&a), Some(json!({"expected_version": 1, "title": "from A", "status": "in_progress"}))),
            env.json(Method::PATCH, &url, &env.owner_token, Some(&b), Some(json!({"expected_version": 1, "title": "from B", "status": "blocked"}))),
        );
        let (win, lose) = match (ra.0, rb.0) {
            (200, 409) => (ra, rb),
            (409, 200) => (rb, ra),
            other => panic!("expected exactly one winner, got {other:?}: {ra:?} {rb:?}"),
        };
        assert_eq!(lose.1["error"]["code"], "VERSION_CONFLICT");
        assert_eq!(lose.1["error"]["details"]["current_version"], 2);
        // The loser's fields did not leak into the stored task.
        let (_, stored) = env.json(Method::GET, &url, &env.owner_token, None, None).await;
        assert_eq!(stored["version"], 2);
        assert_eq!(stored["title"], win.1["title"]);
        assert_eq!(stored["status"], win.1["status"]);

        // Retrying with the reported version succeeds.
        let (status, _) = env
            .json(Method::PATCH, &url, &env.owner_token, Some(&b), Some(json!({"expected_version": 2, "status": "done"})))
            .await;
        assert_eq!(status, 200);
    }
}

/// T05: idempotent sends, lost responses, key reuse.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn t05_concurrent_and_retried_sends_create_one_message() {
    let env = Env::new().await;
    let a = env.session(&env.owner_token, "A").await;
    let b = env.session(&env.owner_token, "B").await;
    let task = new_task(&env, &env.owner_token, &a, "t").await;
    let tid = task["id"].as_str().unwrap();

    // Six simultaneous identical sends.
    let (r0, r1, r2, r3, r4, r5) = tokio::join!(
        send(&env, tid, &a, msg("dup-1", "same body")),
        send(&env, tid, &a, msg("dup-1", "same body")),
        send(&env, tid, &a, msg("dup-1", "same body")),
        send(&env, tid, &a, msg("dup-1", "same body")),
        send(&env, tid, &a, msg("dup-1", "same body")),
        send(&env, tid, &a, msg("dup-1", "same body")),
    );
    let results = vec![r0, r1, r2, r3, r4, r5];
    let created: Vec<_> = results.iter().filter(|r| r.0 == 201).collect();
    let replayed: Vec<_> = results.iter().filter(|r| r.0 == 200).collect();
    assert_eq!((created.len(), replayed.len()), (1, 5), "{results:?}");
    let id = created[0].1["id"].clone();
    assert!(replayed.iter().all(|r| r.1["id"] == id && r.1["seq"] == 1));
    assert_eq!(messages(&env, tid, 0).await["items"].as_array().unwrap().len(), 1);

    // "Response lost": the client never saw the reply and retries later.
    let (status, again) = send(&env, tid, &a, msg("dup-1", "same body")).await;
    assert_eq!((status, &again["id"]), (200, &id));

    // Same key, different body: conflict, nothing stored.
    let (status, body) = send(&env, tid, &a, msg("dup-1", "DIFFERENT")).await;
    assert_eq!((status, body["error"]["code"].as_str()), (409, Some("IDEMPOTENCY_KEY_REUSED")));
    // Different kind with same body is also a different payload.
    let (status, _) = send(&env, tid, &a, json!({"client_message_id": "dup-1", "kind": "help", "body": "same body"})).await;
    assert_eq!(status, 409);
    assert_eq!(messages(&env, tid, 0).await["items"].as_array().unwrap().len(), 1);

    // Keys are per session.
    let (status, other) = send(&env, tid, &b, msg("dup-1", "from B")).await;
    assert_eq!(status, 201, "{other}");
    assert_eq!(other["seq"], 2);

    // A rolled-back duplicate leaves no hole in the sequence.
    let (status, next) = send(&env, tid, &a, msg("dup-2", "next")).await;
    assert_eq!((status, &next["seq"]), (201, &json!(3)));
}

/// T06 (HTTP): sequence order = visible order under concurrent writers, with a
/// concurrent poller, and the cursor survives a service restart.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn t06_pollers_see_every_message_once_in_order_across_a_restart() {
    let env = std::sync::Arc::new(Env::new().await);
    let mut sessions = Vec::new();
    for i in 0..3 {
        sessions.push(env.session(&env.owner_token, &format!("w{i}")).await);
    }
    let task = new_task(&env, &env.owner_token, &sessions[0], "stream").await;
    let tid = task["id"].as_str().unwrap().to_string();
    let per_writer = 8;
    let total = (sessions.len() * per_writer) as i64;

    let mut writers = Vec::new();
    for (w, s) in sessions.iter().enumerate() {
        let (env, s, tid) = (env.clone(), s.clone(), tid.clone());
        writers.push(tokio::spawn(async move {
            for n in 0..per_writer {
                let (status, body) = send(&env, &tid, &s, msg(&format!("w{w}-{n}"), &format!("{w}:{n}"))).await;
                assert_eq!(status, 201, "{body}");
            }
        }));
    }

    // Poll while writers run; restart the server halfway through.
    let mut base = env.base.clone();
    let mut cursor = 0i64;
    let mut seen = Vec::new();
    let mut restarted = false;
    let mut idle = 0;
    while cursor < total {
        let resp = reqwest::Client::new()
            .get(format!(
                "{base}/api/v1/repos/{}/{}/collaboration/tasks/{tid}/messages?after_seq={cursor}&limit=5",
                env.owner, env.repo
            ))
            .bearer_auth(&env.owner_token)
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let page: Value = resp.json().await.unwrap();
        let items = page["items"].as_array().unwrap();
        if items.is_empty() {
            assert_eq!(page["next_after_seq"], cursor, "empty page keeps the cursor");
            idle += 1;
            assert!(idle < 5000, "poller starved");
            tokio::task::yield_now().await;
            continue;
        }
        for (i, m) in items.iter().enumerate() {
            assert_eq!(m["seq"].as_i64().unwrap(), cursor + 1 + i as i64, "gap or reorder");
            seen.push(m["seq"].as_i64().unwrap());
        }
        cursor = page["next_after_seq"].as_i64().unwrap();
        if !restarted && cursor >= 5 {
            // Only the stored cursor survives the "restart".
            base = env.restart().await;
            restarted = true;
        }
    }
    for w in writers {
        w.await.unwrap();
    }
    assert!(restarted);
    assert_eq!(seen, (1..=total).collect::<Vec<_>>());
    let (_, t) = env.json(Method::GET, &env.url(&format!("/tasks/{tid}")), &env.owner_token, None, None).await;
    assert_eq!(t["last_message_seq"], total);
}

/// T10: cross-repository references, invalid inputs, oversize content.
#[tokio::test]
async fn t10_invalid_and_cross_repository_input_is_rejected_without_partial_writes() {
    let env = Env::new().await;
    common::create_repo(&env.base, &env.owner_token, "second-repo").await;
    let a = env.session(&env.owner_token, "A").await;
    let task = new_task(&env, &env.owner_token, &a, "main task").await;
    let tid = task["id"].as_str().unwrap();

    // The other repository's objects.
    let other = |suffix: &str| env.url_in("second-repo", suffix);
    let (status, foreign_session) = env
        .json(Method::POST, &other("/sessions"), &env.owner_token, None, Some(json!({"display_name": "F", "client_kind": "t"})))
        .await;
    assert_eq!(status, 201);
    let fs = Sess {
        id: foreign_session["session"]["id"].as_str().unwrap().into(),
        secret: foreign_session["session_secret"].as_str().unwrap().into(),
    };
    let (_, foreign_task) = env
        .json(Method::POST, &other("/tasks"), &env.owner_token, Some(&fs), Some(json!({"title": "foreign"})))
        .await;
    let ftid = foreign_task["id"].as_str().unwrap();
    let (status, foreign_msg) = env
        .json(Method::POST, &other(&format!("/tasks/{ftid}/messages")), &env.owner_token, Some(&fs), Some(msg("f1", "secret planning")))
        .await;
    assert_eq!(status, 201);

    // Foreign task id under this repository's URL: plain 404 for every verb.
    for (method, suffix) in [
        (Method::GET, format!("/tasks/{ftid}")),
        (Method::GET, format!("/tasks/{ftid}/messages")),
        (Method::PATCH, format!("/tasks/{ftid}")),
    ] {
        let (status, body) = env
            .json(method.clone(), &env.url(&suffix), &env.owner_token, Some(&a), Some(json!({"expected_version": 1, "status": "done"})))
            .await;
        assert_eq!(status, 404, "{method} {suffix}: {body}");
        assert!(!body.to_string().contains("foreign"), "no foreign data in errors");
    }
    let (status, _) = send(&env, ftid, &a, msg("x1", "hi")).await;
    assert_eq!(status, 404);
    // The foreign session's credentials are not valid in this repository either.
    let (status, _) = env
        .json(Method::POST, &env.url("/tasks"), &env.owner_token, Some(&fs), Some(json!({"title": "x"})))
        .await;
    assert_eq!(status, 403);

    let rejects: Vec<(&str, Value)> = vec![
        ("foreign reply_to", json!({"client_message_id": "r1", "kind": "note", "body": "x", "reply_to_id": foreign_msg["id"]})),
        ("unknown reply_to", json!({"client_message_id": "r2", "kind": "note", "body": "x", "reply_to_id": uuid::Uuid::new_v4().to_string()})),
        ("garbage reply_to", json!({"client_message_id": "r3", "kind": "note", "body": "x", "reply_to_id": "nope"})),
        ("foreign target", json!({"client_message_id": "r4", "kind": "note", "body": "x", "target_session_id": fs.id})),
        ("bad kind", json!({"client_message_id": "r5", "kind": "shout", "body": "x"})),
        ("empty body", json!({"client_message_id": "r6", "kind": "note", "body": ""})),
        ("oversize body", json!({"client_message_id": "r7", "kind": "note", "body": "x".repeat(32 * 1024 + 1)})),
        ("nul in body", json!({"client_message_id": "r8", "kind": "note", "body": "a\u{0}b"})),
        ("bad client id", json!({"client_message_id": "has space", "kind": "note", "body": "x"})),
        ("short sha", json!({"client_message_id": "r9", "kind": "note", "body": "x", "commit_sha": "abc123"})),
        ("unknown sha", json!({"client_message_id": "r10", "kind": "note", "body": "x", "commit_sha": "a".repeat(40)})),
    ];
    for (what, body) in rejects {
        let (status, resp) = send(&env, tid, &a, body).await;
        assert_eq!(status, 400, "{what}: {resp}");
        assert!(!resp.to_string().contains("secret planning"), "{what} leaked foreign content");
    }
    // The foreign message id and foreign session id are indistinguishable from nonexistent ones.
    let (_, e1) = send(&env, tid, &a, json!({"client_message_id": "c1", "kind": "note", "body": "x", "reply_to_id": foreign_msg["id"]})).await;
    let (_, e2) = send(&env, tid, &a, json!({"client_message_id": "c2", "kind": "note", "body": "x", "reply_to_id": uuid::Uuid::new_v4().to_string()})).await;
    assert_eq!(e1["error"]["message"], e2["error"]["message"]);

    // Task-level rejects.
    for (what, body) in [
        ("oversize title", json!({"title": "t".repeat(201)})),
        ("oversize description", json!({"title": "t", "description": "d".repeat(16 * 1024 + 1)})),
        ("absolute path", json!({"title": "t", "paths": ["/etc/passwd"]})),
        ("dotdot path", json!({"title": "t", "paths": ["a/../../b"]})),
        ("too many paths", json!({"title": "t", "paths": vec!["p"; 101]})),
        ("long path", json!({"title": "t", "paths": ["p".repeat(513)]})),
        ("missing issue", json!({"title": "t", "issue_number": 12345})),
        ("missing pr", json!({"title": "t", "pr_number": 12345})),
        ("foreign owner", json!({"title": "t", "owner_session_id": fs.id})),
    ] {
        let (status, resp) = env.json(Method::POST, &env.url("/tasks"), &env.owner_token, Some(&a), Some(body)).await;
        assert_eq!(status, 400, "{what}: {resp}");
    }

    // Nothing was written by any rejected request.
    assert_eq!(messages(&env, tid, 0).await["items"].as_array().unwrap().len(), 0);
    let (_, t) = env.json(Method::GET, &env.url(&format!("/tasks/{tid}")), &env.owner_token, None, None).await;
    assert_eq!((t["last_message_seq"].as_i64(), t["version"].as_i64()), (Some(0), Some(1)));
    let (_, all) = env.json(Method::GET, &env.url("/tasks"), &env.owner_token, None, None).await;
    assert_eq!(all["items"].as_array().unwrap().len(), 1, "no task from rejected creates");
    // The other repository is untouched too.
    assert_eq!(
        env.json(Method::GET, &other(&format!("/tasks/{ftid}")), &env.owner_token, None, None).await.1["last_message_seq"],
        1
    );
}

#[tokio::test]
async fn t10_real_issue_and_real_commit_references_are_accepted_and_kept_separate_from_proof() {
    let env = Env::new().await;
    let (_, number) = common::create_issue(&env.base, &env.owner_token, &env.owner, &env.repo, "bug").await;
    let sha = env.push_commit(&env.repo, "first");
    let a = env.session(&env.owner_token, "A").await;

    let (status, task) = env
        .json(Method::POST, &env.url("/tasks"), &env.owner_token, Some(&a), Some(json!({"title": "t", "issue_number": number})))
        .await;
    assert_eq!(status, 201, "{task}");
    assert_eq!(task["issue_number"], number);
    let tid = task["id"].as_str().unwrap();

    // Uppercase is normalised; the stored value is lowercase.
    let (status, m) = send(&env, tid, &a, json!({"client_message_id": "c", "kind": "note", "body": "done in this commit", "commit_sha": sha.to_uppercase()})).await;
    assert_eq!(status, 201, "{m}");
    assert_eq!(m["commit_sha"], sha.as_str());
    assert_eq!(m["origin"], "user_content");

    // A tree or blob id is not a commit.
    let tree = env.rev_parse(&env.repo, &format!("{sha}^{{tree}}"));
    let (status, _) = send(&env, tid, &a, json!({"client_message_id": "t", "kind": "note", "body": "x", "commit_sha": tree})).await;
    assert_eq!(status, 400);
}

#[tokio::test]
async fn read_and_write_permissions_apply_to_tasks_and_messages() {
    let env = Env::new().await;
    let a = env.session(&env.owner_token, "A").await;
    let task = new_task(&env, &env.owner_token, &a, "t").await;
    let tid = task["id"].as_str().unwrap();
    send(&env, tid, &a, msg("m1", "hello")).await;
    let (reader, reader_id) = env.user("task_reader").await;
    env.add_collaborator(reader_id, "read").await;

    // Public repository, but collaboration data still needs an account.
    for suffix in ["/tasks".to_string(), format!("/tasks/{tid}"), format!("/tasks/{tid}/messages")] {
        let resp = env.send(Method::GET, &env.url(&suffix), None, None, None).await;
        assert_eq!(resp.status(), 401, "anonymous GET {suffix}");
    }
    // A reader can read…
    for suffix in ["/tasks".to_string(), format!("/tasks/{tid}"), format!("/tasks/{tid}/messages")] {
        let (status, _) = env.json(Method::GET, &env.url(&suffix), &reader, None, None).await;
        assert_eq!(status, 200, "reader GET {suffix}");
    }
    // …but not write, even holding the owner's session credentials.
    let (status, _) = env.json(Method::POST, &env.url("/tasks"), &reader, Some(&a), Some(json!({"title": "x"}))).await;
    assert_eq!(status, 403);
    let (status, _) = env
        .json(Method::POST, &env.url(&format!("/tasks/{tid}/messages")), &reader, Some(&a), Some(msg("m2", "x")))
        .await;
    assert_eq!(status, 403);
    let (status, _) = env
        .json(Method::PATCH, &env.url(&format!("/tasks/{tid}")), &reader, Some(&a), Some(json!({"expected_version": 1, "status": "done"})))
        .await;
    assert_eq!(status, 403);

    // Writes need a live session in addition to the account.
    let (status, body) = env.json(Method::POST, &env.url("/tasks"), &env.owner_token, None, Some(json!({"title": "x"}))).await;
    assert_eq!((status, body["error"]["code"].as_str()), (400, Some("SESSION_CREDENTIALS_REQUIRED")));
    env.json(Method::POST, &env.url(&format!("/sessions/{}/end", a.id)), &env.owner_token, Some(&a), None).await;
    let (status, body) = send(&env, tid, &a, msg("m3", "x")).await;
    assert_eq!((status, body["error"]["code"].as_str()), (409, Some("SESSION_EXPIRED")));
}
