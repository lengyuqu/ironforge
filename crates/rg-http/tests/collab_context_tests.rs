//! U05 acceptance (N11–N12): the byte-budgeted task context pack.
//!
//! Real HTTP server and SQLite database; with `IRONFORGE_TEST_DATABASE_URL` the
//! same tests run on PostgreSQL or MySQL.

mod collab_common;
mod common;

use collab_common::{Env, Sess};
use reqwest::Method;
use serde_json::{json, Value};

async fn new_task(env: &Env, s: &Sess, extra: Value) -> String {
    let mut body = json!({"title": "Port the cache", "owner_session_id": s.id});
    body.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    let (code, task) = env
        .json(
            Method::POST,
            &env.url("/tasks"),
            &env.owner_token,
            Some(s),
            Some(body),
        )
        .await;
    assert_eq!(code, 201, "{task}");
    task["id"].as_str().unwrap().to_string()
}

async fn say(env: &Env, tid: &str, s: &Sess, key: &str, body: &str) {
    let (code, msg) = env
        .json(
            Method::POST,
            &env.url(&format!("/tasks/{tid}/messages")),
            &env.owner_token,
            Some(s),
            Some(json!({"client_message_id": key, "kind": "note", "body": body})),
        )
        .await;
    assert_eq!(code, 201, "{msg}");
}

async fn context(env: &Env, token: &str, tid: &str, query: &str) -> (u16, Value) {
    env.json(
        Method::GET,
        &env.url(&format!("/tasks/{tid}/context{query}")),
        token,
        None,
        None,
    )
    .await
}

fn section<'a>(pack: &'a Value, name: &str) -> &'a Value {
    pack["sections"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == name)
        .unwrap_or_else(|| panic!("no section {name}: {pack}"))
}

fn serialized_len(pack: &Value) -> usize {
    serde_json::to_vec(pack).unwrap().len()
}

#[tokio::test]
async fn u05_default_pack_lists_sections_in_priority_order_and_labels_its_sources() {
    let env = Env::new().await;
    let a = env.session(&env.owner_token, "A").await;
    let tid = new_task(&env, &a, json!({})).await;
    let (status, pack) = context(&env, &env.owner_token, &tid, "").await;
    assert_eq!(status, 200, "{pack}");
    let names: Vec<&str> = pack["sections"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        [
            "task",
            "questions",
            "decisions",
            "handoffs",
            "work_packages",
            "dependencies",
            "overlaps",
            "messages"
        ]
    );
    assert_eq!(pack["task_id"], tid);
    assert_eq!(pack["task_version"], 1);
    assert_eq!(pack["max_bytes"], 32768);
    assert_eq!(
        pack["consistency"],
        json!({"db": "per_section_live_query", "git": "independent"})
    );
    assert_eq!(pack["git"]["status"], "no_branch");
    assert_eq!(pack["truncation"], json!({"sections": [], "fields": []}));
    let task = &section(&pack, "task")["items"][0];
    assert_eq!(task["id"], tid);
    assert_eq!(task["origin"], "user_content");
    assert_eq!(task["locator"], Value::Null, "the task is its own locator");
    for s in pack["sections"].as_array().unwrap() {
        assert_eq!(s["source"], "db_live_query");
        assert_eq!(s["returned_count"], s["items"].as_array().unwrap().len());
    }
}

#[tokio::test]
async fn u05_pack_carries_open_questions_messages_and_the_observed_branch_head() {
    let env = Env::new().await;
    let sha = env.push_commit(&env.repo, "first");
    let a = env.session(&env.owner_token, "A").await;
    let tid = new_task(&env, &a, json!({"branch": "main"})).await;
    let (code, q) = env
        .json(
            Method::POST,
            &env.url(&format!("/tasks/{tid}/questions")),
            &env.owner_token,
            Some(&a),
            Some(json!({
                "client_question_id": "q-1",
                "recipient": {"kind": "repo_writers"},
                "title": "Which database?",
                "body": "SQLite or PostgreSQL?",
            })),
        )
        .await;
    assert_eq!(code, 201, "{q}");
    say(&env, &tid, &a, "m-1", "hello there").await;

    let (status, pack) = context(&env, &env.owner_token, &tid, "").await;
    assert_eq!(status, 200, "{pack}");
    assert_eq!(pack["git"]["status"], "observed");
    assert_eq!(pack["git"]["branch"], "main");
    assert_eq!(pack["head_sha"], sha);
    let questions = section(&pack, "questions");
    assert_eq!(questions["returned_count"], 1);
    let item = &questions["items"][0];
    assert_eq!(item["id"], q["id"]);
    assert_eq!(
        item["locator"],
        format!("/tasks/{tid}/questions/{}", q["id"].as_str().unwrap())
    );
    assert_eq!(item["origin"], "user_content");
    assert_eq!(
        questions["continue_with"],
        format!("/tasks/{tid}/questions?status=open")
    );
    let messages = section(&pack, "messages");
    assert_eq!(messages["items"][0]["body"], "hello there");
    assert_eq!(messages["has_more"], false);
}

#[tokio::test]
async fn u05_a_declared_branch_that_cannot_be_read_is_unavailable_not_guessed() {
    let env = Env::new().await;
    let a = env.session(&env.owner_token, "A").await;
    let tid = new_task(&env, &a, json!({"branch": "no-such-branch"})).await;
    let (status, pack) = context(&env, &env.owner_token, &tid, "").await;
    assert_eq!(status, 200, "{pack}");
    assert_eq!(pack["git"]["status"], "unavailable");
    assert_eq!(pack["head_sha"], Value::Null);
}

#[tokio::test]
async fn n11_the_byte_budget_is_exact_utf8_safe_and_every_drop_is_listed() {
    let env = Env::new().await;
    let a = env.session(&env.owner_token, "A").await;
    let tid = new_task(&env, &a, json!({})).await;
    // 15 messages of ~1.5 KiB Chinese text (3 bytes per character).
    for i in 0..15 {
        say(&env, &tid, &a, &format!("m-{i}"), &"语".repeat(500)).await;
    }
    let (status, full) = context(&env, &env.owner_token, &tid, "?max_bytes=262144").await;
    assert_eq!(status, 200, "{full}");
    assert_eq!(section(&full, "messages")["returned_count"], 15);

    let (status, small) = context(&env, &env.owner_token, &tid, "?max_bytes=8192").await;
    assert_eq!(status, 200, "{small}");
    assert!(
        serialized_len(&small) <= 8192,
        "response is {} bytes",
        serialized_len(&small)
    );
    let messages = section(&small, "messages");
    let kept = messages["returned_count"].as_u64().unwrap();
    assert!(kept > 0 && kept < 15, "kept {kept}");
    assert_eq!(messages["has_more"], true);
    // The oldest messages go first: what stays is the newest tail.
    let seqs: Vec<i64> = messages["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["seq"].as_i64().unwrap())
        .collect();
    assert_eq!(*seqs.last().unwrap(), 15, "newest message is kept");
    assert!(seqs.windows(2).all(|w| w[1] == w[0] + 1), "{seqs:?}");
    let cut = small["truncation"]["sections"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["section"] == "messages")
        .expect("the drop is listed");
    assert_eq!(cut["omitted_items"], 15 - kept);
    assert_eq!(cut["reason"], "byte_budget");
    assert_eq!(
        messages["continue_with"],
        format!("/tasks/{tid}/messages?after_seq=0")
    );
}

#[tokio::test]
async fn n11_long_fields_are_cut_on_a_character_boundary_and_listed() {
    let env = Env::new().await;
    let a = env.session(&env.owner_token, "A").await;
    let tid = new_task(&env, &a, json!({})).await;
    say(&env, &tid, &a, "long", &"语".repeat(5000)).await; // 15000 bytes
    let (status, pack) = context(&env, &env.owner_token, &tid, "?sections=messages").await;
    assert_eq!(status, 200, "{pack}");
    let item = &section(&pack, "messages")["items"][0];
    let body = item["body"].as_str().unwrap();
    assert!(
        body.len() <= 2048 && body.chars().all(|c| c == '语'),
        "{}",
        body.len()
    );
    let cuts = pack["truncation"]["fields"].as_array().unwrap();
    assert_eq!(cuts.len(), 1, "{cuts:?}");
    assert_eq!(cuts[0]["section"], "messages");
    assert_eq!(cuts[0]["object_id"], item["id"]);
    assert_eq!(cuts[0]["field"], "body");
    assert_eq!(cuts[0]["original_bytes"], 15000);
    assert_eq!(cuts[0]["kept_bytes"], body.len());
}

#[tokio::test]
async fn n11_a_budget_that_cannot_hold_the_task_is_refused_by_name() {
    let env = Env::new().await;
    let a = env.session(&env.owner_token, "A").await;
    // 100 paths of 400 bytes: the task alone is about 40 KB.
    let paths: Vec<String> = (0..100)
        .map(|i| format!("{i:03}{}", "d/".repeat(199)))
        .collect();
    let tid = new_task(&env, &a, json!({"paths": paths})).await;
    let (status, body) = context(&env, &env.owner_token, &tid, "?max_bytes=4096").await;
    assert_eq!(status, 400, "{body}");
    assert_eq!(body["error"]["code"], "BUDGET_TOO_SMALL");
    assert!(body["error"]["details"]["needed_bytes"].as_u64().unwrap() > 4096);
    // The same task is readable with a budget that holds it.
    let (status, pack) = context(&env, &env.owner_token, &tid, "?max_bytes=65536").await;
    assert_eq!(status, 200, "{pack}");
}

#[tokio::test]
async fn n11_requested_sections_are_validated_and_always_in_fixed_order() {
    let env = Env::new().await;
    let a = env.session(&env.owner_token, "A").await;
    let tid = new_task(&env, &a, json!({})).await;
    let (status, pack) =
        context(&env, &env.owner_token, &tid, "?sections=messages,questions").await;
    assert_eq!(status, 200, "{pack}");
    let names: Vec<&str> = pack["sections"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["task", "questions", "messages"]);
    for bad in [
        "?sections=nope",
        "?sections=task",
        "?max_bytes=100",
        "?max_bytes=300000",
        "?max_bytes=abc",
    ] {
        let (status, body) = context(&env, &env.owner_token, &tid, bad).await;
        assert_eq!(status, 400, "{bad}: {body}");
    }
}

#[tokio::test]
async fn n12_the_pack_follows_the_task_between_reads_and_names_its_version() {
    let env = Env::new().await;
    let a = env.session(&env.owner_token, "A").await;
    let tid = new_task(&env, &a, json!({})).await;
    let (_, before) = context(&env, &env.owner_token, &tid, "").await;
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
    let (_, after) = context(&env, &env.owner_token, &tid, "").await;
    assert_eq!(before["task_version"], 1);
    assert_eq!(after["task_version"], 2);
    assert_eq!(section(&after, "task")["items"][0]["status"], "in_progress");
    assert_ne!(before["as_of"], after["as_of"]);
}

#[tokio::test]
async fn n07_reading_a_pack_needs_repository_access_and_a_task_of_this_repository() {
    let env = Env::with_repo(json!({"name": "private-repo", "is_private": true})).await;
    let a = env.session(&env.owner_token, "A").await;
    let tid = new_task(&env, &a, json!({})).await;
    let (stranger, _) = env.user("stranger").await;
    let (status, _) = context(&env, &stranger, &tid, "").await;
    assert!(status == 403 || status == 404, "{status}");
    let (status, _) = context(&env, "", &tid, "").await;
    assert!(status == 401 || status == 403 || status == 404, "{status}");
    let (status, _) = context(
        &env,
        &env.owner_token,
        "00000000-0000-4000-8000-000000000000",
        "",
    )
    .await;
    assert_eq!(status, 404);
}
