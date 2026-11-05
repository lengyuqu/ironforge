//! O04 acceptance: user-defined completion conditions, manual confirmations
//! and evaluation snapshots. Display only: nothing here blocks a task.
//!
//! Real HTTP server and SQLite database; with `IRONFORGE_TEST_DATABASE_URL` the
//! same tests run on PostgreSQL or MySQL.

mod collab_common;
mod common;

use chrono::Utc;
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

async fn put(env: &Env, tid: &str, body: Value) -> (u16, Value) {
    env.json(
        Method::PUT,
        &env.url(&format!("/tasks/{tid}/completion-conditions")),
        &env.owner_token,
        None,
        Some(body),
    )
    .await
}

fn def(key: &str, expected: i64, conditions: Value) -> Value {
    json!({"client_request_id": key, "expected_version": expected, "conditions": conditions})
}

async fn status(env: &Env, tid: &str) -> Value {
    let (code, body) = env
        .json(
            Method::GET,
            &env.url(&format!("/tasks/{tid}/completion-status")),
            &env.owner_token,
            None,
            None,
        )
        .await;
    assert_eq!(code, 200, "{body}");
    body
}

fn defined<'a>(body: &'a Value, id: &str) -> &'a Value {
    body["defined"]["conditions"]
        .as_array()
        .unwrap_or_else(|| panic!("no defined conditions: {body}"))
        .iter()
        .find(|c| c["id"] == id)
        .unwrap_or_else(|| panic!("no condition {id}: {body}"))
}

async fn ask(env: &Env, tid: &str, s: &Sess, key: &str) -> Value {
    let (code, q) = env
        .json(
            Method::POST,
            &env.url(&format!("/tasks/{tid}/questions")),
            &env.owner_token,
            Some(s),
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

async fn answer(env: &Env, q: &Value, key: &str) {
    let (code, body) = env
        .json(
            Method::POST,
            &env.url(&format!("/questions/{}/answer", q["id"].as_str().unwrap())),
            &env.owner_token,
            None,
            Some(json!({
                "client_answer_id": key,
                "expected_version": q["version"],
                "body": "PostgreSQL",
                "option_id": null,
                "acknowledged_context_hash": q["context_hash"],
            })),
        )
        .await;
    assert_eq!(code, 200, "{body}");
}

async fn package(env: &Env, tid: &str, s: &Sess, key: &str) -> Value {
    let (code, p) = env
        .json(
            Method::POST,
            &env.url(&format!("/tasks/{tid}/work-packages")),
            &env.owner_token,
            Some(s),
            Some(json!({
                "client_package_id": key,
                "goal": "ship the cache",
                "scope": "crates/cache",
                "completed": ["wrote the cache"],
                "open_items": [],
                "next_steps": [],
            })),
        )
        .await;
    assert_eq!(code, 201, "{p}");
    p
}

async fn bump(env: &Env, tid: &str, s: &Sess, to: &str) {
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
            Some(s),
            Some(json!({"expected_version": t["version"], "status": to})),
        )
        .await;
    assert_eq!(code, 200, "{body}");
}

#[tokio::test]
async fn a_task_without_a_definition_keeps_the_derived_evaluation_only() {
    let env = Env::new().await;
    let a = env.session(&env.owner_token, "A").await;
    let tid = new_task(&env, &a, json!({})).await;
    let (code, current) = env
        .json(
            Method::GET,
            &env.url(&format!("/tasks/{tid}/completion-conditions")),
            &env.owner_token,
            None,
            None,
        )
        .await;
    assert_eq!(code, 200, "{current}");
    assert_eq!(current["version"], 0);
    assert_eq!(current["conditions"], json!([]));
    let s = status(&env, &tid).await;
    assert_eq!(s["defined"], Value::Null);
    assert!(s["conditions"].as_array().unwrap().len() == 3);
}

#[tokio::test]
async fn definitions_are_versioned_idempotent_and_conflict_instead_of_merging() {
    let env = Env::new().await;
    let a = env.session(&env.owner_token, "A").await;
    let tid = new_task(&env, &a, json!({})).await;
    let q = ask(&env, &tid, &a, "q-1").await;
    let first = def(
        "d-1",
        0,
        json!([
            {"type": "question_answered", "label": "DB decided", "params": {"question_id": q["id"]}},
            {"type": "manual_confirmation", "label": "Owner signs off"},
        ]),
    );
    let (code, v1) = put(&env, &tid, first.clone()).await;
    assert_eq!(code, 201, "{v1}");
    assert_eq!(v1["replayed"], false);
    assert_eq!(v1["set"]["version"], 1);
    let conditions = v1["set"]["conditions"].as_array().unwrap().clone();
    assert_eq!(conditions.len(), 2);
    assert_eq!(conditions[0]["introduced_in_version"], 1);
    assert_eq!(conditions[0]["created_by_kind"], "user");
    let keep = conditions[0]["id"].as_str().unwrap().to_string();

    // The same request again is a replay, not a second version.
    let (code, again) = put(&env, &tid, first.clone()).await;
    assert_eq!(code, 200, "{again}");
    assert_eq!(again["replayed"], true);
    assert_eq!(again["set"]["version"], 1);
    // The same key with another body is refused.
    let mut other = first.clone();
    other["conditions"] = json!([]);
    let (code, body) = put(&env, &tid, other).await;
    assert_eq!(code, 409, "{body}");
    assert_eq!(body["error"]["code"], "IDEMPOTENCY_KEY_REUSED");

    // A stale writer conflicts and sees the current version; nothing changes.
    let (code, body) = put(&env, &tid, def("d-stale", 0, json!([]))).await;
    assert_eq!(code, 409, "{body}");
    assert_eq!(body["error"]["code"], "VERSION_CONFLICT");
    assert_eq!(body["error"]["details"]["current_version"], 1);

    // Version 2 keeps one condition's identity (new label) and drops the other.
    let (code, v2) = put(
        &env,
        &tid,
        def(
            "d-2",
            1,
            json!([{"id": keep, "type": "question_answered", "label": "renamed", "params": {"question_id": q["id"]}}]),
        ),
    )
    .await;
    assert_eq!(code, 201, "{v2}");
    let kept = &v2["set"]["conditions"][0];
    assert_eq!(kept["id"], keep.as_str());
    assert_eq!(kept["label"], "renamed");
    assert_eq!(
        kept["introduced_in_version"], 1,
        "identity and origin are kept"
    );

    // History: both versions stay readable and unchanged.
    let (code, versions) = env
        .json(
            Method::GET,
            &env.url(&format!("/tasks/{tid}/completion-conditions/versions")),
            &env.owner_token,
            None,
            None,
        )
        .await;
    assert_eq!(code, 200, "{versions}");
    let items = versions["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0]["version"], 2);
    assert_eq!(items[1]["version"], 1);
    assert_eq!(items[1]["conditions"].as_array().unwrap().len(), 2);
    assert_eq!(items[1]["conditions"][0]["label"], "DB decided");
}

#[tokio::test]
async fn invalid_definitions_are_refused_by_name() {
    let env = Env::new().await;
    let a = env.session(&env.owner_token, "A").await;
    let tid = new_task(&env, &a, json!({})).await;
    let other = new_task(&env, &a, json!({})).await;
    let foreign_q = ask(&env, &other, &a, "q-other").await;
    let own_q = ask(&env, &tid, &a, "q-own").await;
    let cases = [
        json!([{"type": "no_such_type"}]),
        json!([{"type": "manual_confirmation", "params": {"x": 1}}]),
        json!([{"type": "question_answered", "params": {}}]),
        json!([{"type": "question_answered", "params": {"question_id": foreign_q["id"]}}]),
        json!([{"type": "question_answered", "params": {"question_id": own_q["id"], "extra": 1}}]),
        json!([{"type": "work_package_present", "params": {"work_package_id": "00000000-0000-4000-8000-000000000000"}}]),
        json!([{"type": "ci_passed_for_commit", "params": {"commit_sha": "main"}}]),
        json!([{"id": "00000000-0000-4000-8000-000000000000", "type": "manual_confirmation"}]),
        json!([{"type": "manual_confirmation", "label": ""}]),
        json!((0..21)
            .map(|_| json!({"type": "manual_confirmation"}))
            .collect::<Vec<_>>()),
    ];
    for (i, conditions) in cases.into_iter().enumerate() {
        let (code, body) = put(&env, &tid, def(&format!("bad-{i}"), 0, conditions)).await;
        assert_eq!(code, 400, "case {i}: {body}");
    }
    // Unknown fields (also nested) are rejected, not ignored.
    let (code, _) = put(
        &env,
        &tid,
        json!({"client_request_id": "x", "expected_version": 0, "conditions": [], "extra": 1}),
    )
    .await;
    assert_eq!(code, 400);
    let (code, _) = put(
        &env,
        &tid,
        def(
            "nested",
            0,
            json!([{"type": "manual_confirmation", "oops": true}]),
        ),
    )
    .await;
    assert_eq!(code, 400);
    // A condition keeps its type.
    let (_, v1) = put(
        &env,
        &tid,
        def("ok", 0, json!([{"type": "manual_confirmation"}])),
    )
    .await;
    let id = v1["set"]["conditions"][0]["id"].clone();
    let (code, body) = put(
        &env,
        &tid,
        def("retype", 1, json!([{"id": id, "type": "question_answered", "params": {"question_id": own_q["id"]}}])),
    )
    .await;
    assert_eq!(code, 400, "{body}");
    // Nothing above stored a version.
    let (_, current) = env
        .json(
            Method::GET,
            &env.url(&format!("/tasks/{tid}/completion-conditions")),
            &env.owner_token,
            None,
            None,
        )
        .await;
    assert_eq!(current["version"], 1);
}

#[tokio::test]
async fn questions_and_packages_are_judged_against_their_current_context() {
    let env = Env::new().await;
    let a = env.session(&env.owner_token, "A").await;
    let tid = new_task(&env, &a, json!({})).await;
    let q = ask(&env, &tid, &a, "q-1").await;
    let p1 = package(&env, &tid, &a, "w1").await;
    let (code, v1) = put(
        &env,
        &tid,
        def(
            "d-1",
            0,
            json!([
                {"type": "question_answered", "params": {"question_id": q["id"]}},
                {"type": "work_package_present", "params": {"work_package_id": p1["id"]}},
            ]),
        ),
    )
    .await;
    assert_eq!(code, 201, "{v1}");
    let qc = v1["set"]["conditions"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let pc = v1["set"]["conditions"][1]["id"]
        .as_str()
        .unwrap()
        .to_string();

    let s = status(&env, &tid).await;
    assert_eq!(s["defined"]["condition_set_version"], 1);
    assert_eq!(defined(&s, &qc)["status"], "unsatisfied", "still open");
    assert_eq!(defined(&s, &pc)["status"], "satisfied");
    assert_eq!(s["defined"]["overall"], "unsatisfied");

    answer(&env, &q, "a-1").await;
    let s = status(&env, &tid).await;
    assert_eq!(defined(&s, &qc)["status"], "satisfied");
    assert_eq!(s["defined"]["overall"], "all_satisfied");

    // The task moves on: the answer was given for an older task version.
    bump(&env, &tid, &a, "in_progress").await;
    let s = status(&env, &tid).await;
    assert_eq!(defined(&s, &qc)["status"], "stale");
    assert_eq!(s["defined"]["overall"], "incomplete");

    // A newer package replaces the one the condition pointed at.
    package(&env, &tid, &a, "w2").await;
    let s = status(&env, &tid).await;
    assert_eq!(defined(&s, &pc)["status"], "stale");
}

#[tokio::test]
async fn ci_conditions_bind_to_the_exact_commit_and_nothing_else() {
    let env = Env::new().await;
    let a = env.session(&env.owner_token, "A").await;
    let first = env.push_tree(&env.repo, "feature", &[("a", "1")], &[]);
    let tid = new_task(&env, &a, json!({"branch": "feature"})).await;
    let (code, v1) = put(
        &env,
        &tid,
        def(
            "d-1",
            0,
            json!([{"type": "ci_passed_for_commit", "params": {"commit_sha": first.to_uppercase()}}]),
        ),
    )
    .await;
    assert_eq!(code, 201, "{v1}");
    let id = v1["set"]["conditions"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(
        v1["set"]["conditions"][0]["params"]["commit_sha"],
        first.as_str()
    );

    // No pipeline recorded: unknown, never passed.
    assert_eq!(defined(&status(&env, &tid).await, &id)["status"], "unknown");

    let run = rg_db::ops::pipeline_ops::create_pipeline(
        &env.db,
        env.repo_id,
        &first,
        "feature",
        "push",
        None,
    )
    .await
    .unwrap();
    rg_db::ops::pipeline_ops::update_pipeline_status(
        &env.db,
        run.id,
        "success",
        None,
        Some(Utc::now().naive_utc()),
    )
    .await
    .unwrap();
    let s = status(&env, &tid).await;
    assert_eq!(defined(&s, &id)["status"], "satisfied");
    assert_eq!(defined(&s, &id)["evidence"][0]["pipeline_id"], run.id);

    // The branch moves to another commit: the old pipeline no longer counts.
    env.push_tree(&env.repo, "feature", &[("a", "2")], &[]);
    let s = status(&env, &tid).await;
    assert_eq!(defined(&s, &id)["status"], "stale");
}

#[tokio::test]
async fn manual_confirmation_is_explicit_idempotent_and_tied_to_the_branch_head() {
    let env = Env::new().await;
    let a = env.session(&env.owner_token, "A").await;
    env.push_tree(&env.repo, "feature", &[("a", "1")], &[]);
    let tid = new_task(&env, &a, json!({"branch": "feature"})).await;
    let (_, v1) = put(
        &env,
        &tid,
        def("d-1", 0, json!([{"type": "manual_confirmation", "label": "Looks right"}, {"type": "question_answered", "params": {"question_id": ask(&env, &tid, &a, "q").await["id"]}}])),
    )
    .await;
    let mc = v1["set"]["conditions"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let qc = v1["set"]["conditions"][1]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let url = env.url(&format!(
        "/tasks/{tid}/completion-conditions/{mc}/confirmations"
    ));

    assert_eq!(
        defined(&status(&env, &tid).await, &mc)["status"],
        "unsatisfied"
    );

    let (code, done) = env
        .json(
            Method::POST,
            &url,
            &env.owner_token,
            None,
            Some(json!({"client_request_id": "c-1", "note": "checked by hand"})),
        )
        .await;
    assert_eq!(code, 201, "{done}");
    assert_eq!(done["confirmation"]["note"], "checked by hand");
    assert_eq!(done["confirmation"]["origin"], "user_content");
    let (code, replay) = env
        .json(
            Method::POST,
            &url,
            &env.owner_token,
            None,
            Some(json!({"client_request_id": "c-1", "note": "checked by hand"})),
        )
        .await;
    assert_eq!(code, 200, "{replay}");
    assert_eq!(replay["replayed"], true);
    assert_eq!(replay["confirmation"]["id"], done["confirmation"]["id"]);
    let (code, _) = env
        .json(
            Method::POST,
            &url,
            &env.owner_token,
            None,
            Some(json!({"client_request_id": "c-1", "note": "different"})),
        )
        .await;
    assert_eq!(code, 409);

    let s = status(&env, &tid).await;
    assert_eq!(defined(&s, &mc)["status"], "satisfied");
    assert!(defined(&s, &mc)["detail"]
        .as_str()
        .unwrap()
        .contains("not a merge approval"));
    // The other condition is still open, so the whole is not satisfied.
    assert_eq!(s["defined"]["overall"], "unsatisfied");
    assert_eq!(defined(&s, &qc)["status"], "unsatisfied");

    // The branch moves: the confirmation was for another commit.
    env.push_tree(&env.repo, "feature", &[("a", "2")], &[]);
    assert_eq!(defined(&status(&env, &tid).await, &mc)["status"], "stale");

    // Only a manual_confirmation condition of the current definition can be confirmed.
    let url = env.url(&format!(
        "/tasks/{tid}/completion-conditions/{qc}/confirmations"
    ));
    let (code, _) = env
        .json(
            Method::POST,
            &url,
            &env.owner_token,
            None,
            Some(json!({"client_request_id": "c-2"})),
        )
        .await;
    assert_eq!(code, 404);
}

#[tokio::test]
async fn snapshots_keep_the_evaluation_as_it_was() {
    let env = Env::new().await;
    let a = env.session(&env.owner_token, "A").await;
    let tid = new_task(&env, &a, json!({})).await;
    let (_, v1) = put(
        &env,
        &tid,
        def("d-1", 0, json!([{"type": "manual_confirmation"}])),
    )
    .await;
    let mc = v1["set"]["conditions"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    // The agent says done with nothing confirmed.
    bump(&env, &tid, &a, "done").await;
    let live = status(&env, &tid).await;
    assert_eq!(live["declared_done_without_evidence"], true);

    let snaps = env.url(&format!("/tasks/{tid}/completion-status/snapshots"));
    let (code, snap) = env
        .json(Method::POST, &snaps, &env.owner_token, None, None)
        .await;
    assert_eq!(code, 201, "{snap}");
    assert_eq!(snap["immutable"], true);
    assert_eq!(snap["overall"], "unsatisfied");
    assert_eq!(snap["condition_set_version"], 1);
    assert_eq!(snap["result"]["declared_done_without_evidence"], true);
    let id = snap["id"].as_str().unwrap().to_string();

    // Later the condition is confirmed; the live status changes, the snapshot does not.
    let (code, _) = env
        .json(
            Method::POST,
            &env.url(&format!(
                "/tasks/{tid}/completion-conditions/{mc}/confirmations"
            )),
            &env.owner_token,
            None,
            Some(json!({"client_request_id": "c-1"})),
        )
        .await;
    assert_eq!(code, 201);
    let now = status(&env, &tid).await;
    assert_eq!(now["defined"]["conditions"][0]["status"], "satisfied");
    assert_eq!(now["declared_done_without_evidence"], false);
    let (code, stored) = env
        .json(
            Method::GET,
            &format!("{snaps}/{id}"),
            &env.owner_token,
            None,
            None,
        )
        .await;
    assert_eq!(code, 200, "{stored}");
    assert_eq!(stored["result"], snap["result"]);
    let (code, list) = env
        .json(Method::GET, &snaps, &env.owner_token, None, None)
        .await;
    assert_eq!(code, 200, "{list}");
    assert_eq!(list["items"].as_array().unwrap().len(), 1);
    assert_eq!(list["items"][0]["id"], id.as_str());
    assert!(
        list["items"][0].get("result").is_none(),
        "summaries carry no result"
    );
    let (code, _) = env
        .json(
            Method::GET,
            &format!("{snaps}/00000000-0000-4000-8000-000000000000"),
            &env.owner_token,
            None,
            None,
        )
        .await;
    assert_eq!(code, 404);
}

#[tokio::test]
async fn reading_and_writing_follow_repository_permissions() {
    let env = Env::with_repo(json!({"name": "private-repo", "is_private": true})).await;
    let a = env.session(&env.owner_token, "A").await;
    let tid = new_task(&env, &a, json!({})).await;
    let (stranger, _) = env.user("stranger").await;
    let (reader, reader_id) = env.user("reader").await;
    env.add_collaborator(reader_id, "read").await;
    let base = format!("/tasks/{tid}/completion-conditions");
    for token in [&stranger, &reader] {
        let (code, _) = env
            .json(
                Method::PUT,
                &env.url(&base),
                token,
                None,
                Some(def("w", 0, json!([]))),
            )
            .await;
        assert!(code == 403 || code == 404, "write {code}");
        let (code, _) = env
            .json(
                Method::POST,
                &env.url(&format!("/tasks/{tid}/completion-status/snapshots")),
                token,
                None,
                None,
            )
            .await;
        assert!(code == 403 || code == 404, "snapshot {code}");
    }
    let (code, _) = env
        .json(Method::GET, &env.url(&base), &reader, None, None)
        .await;
    assert_eq!(code, 200, "a reader may read the definition");
    let (code, _) = env
        .json(Method::GET, &env.url(&base), &stranger, None, None)
        .await;
    assert!(code == 403 || code == 404, "{code}");
    let (code, _) = env.json(Method::GET, &env.url(&base), "", None, None).await;
    assert!(code == 401 || code == 403 || code == 404, "{code}");
}

#[tokio::test]
async fn racing_definitions_with_one_expected_version_leave_exactly_one_winner() {
    let env = Env::new().await;
    let a = env.session(&env.owner_token, "A").await;
    let tid = new_task(&env, &a, json!({})).await;
    let results = futures::future::join_all((0..6).map(|i| {
        let conditions = json!([{"type": "manual_confirmation", "label": format!("writer {i}")}]);
        put(&env, &tid, def(&format!("race-{i}"), 0, conditions))
    }))
    .await;
    let created = results.iter().filter(|(code, _)| *code == 201).count();
    let conflicts: Vec<_> = results.iter().filter(|(code, _)| *code == 409).collect();
    assert_eq!(created, 1, "{results:?}");
    assert_eq!(conflicts.len(), 5, "{results:?}");
    for (_, body) in conflicts {
        assert_eq!(body["error"]["code"], "VERSION_CONFLICT");
        assert_eq!(body["error"]["details"]["current_version"], 1);
    }
    let (_, versions) = env
        .json(
            Method::GET,
            &env.url(&format!("/tasks/{tid}/completion-conditions/versions")),
            &env.owner_token,
            None,
            None,
        )
        .await;
    assert_eq!(versions["items"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn snapshots_per_task_are_capped() {
    let env = Env::new().await;
    let a = env.session(&env.owner_token, "A").await;
    let tid = new_task(&env, &a, json!({})).await;
    let snaps = env.url(&format!("/tasks/{tid}/completion-status/snapshots"));
    for i in 0..200 {
        let (code, body) = env
            .json(Method::POST, &snaps, &env.owner_token, None, None)
            .await;
        assert_eq!(code, 201, "snapshot {i}: {body}");
    }
    let (code, body) = env
        .json(Method::POST, &snaps, &env.owner_token, None, None)
        .await;
    assert_eq!(code, 409, "{body}");
    assert_eq!(body["error"]["code"], "SNAPSHOT_LIMIT_REACHED", "{body}");
    assert_eq!(body["error"]["details"]["limit"], 200);
    // Existing snapshots stay readable, and another task is unaffected.
    let (code, list) = env
        .json(Method::GET, &snaps, &env.owner_token, None, None)
        .await;
    assert_eq!(code, 200, "{list}");
    let other = new_task(&env, &a, json!({})).await;
    let (code, body) = env
        .json(
            Method::POST,
            &env.url(&format!("/tasks/{other}/completion-status/snapshots")),
            &env.owner_token,
            None,
            None,
        )
        .await;
    assert_eq!(code, 201, "{body}");
}

#[tokio::test]
async fn snapshot_requests_with_a_key_are_idempotent() {
    let env = Env::new().await;
    let a = env.session(&env.owner_token, "A").await;
    let tid = new_task(&env, &a, json!({})).await;
    let base = env.url(&format!("/tasks/{tid}/completion-status/snapshots"));
    let post = |key: &str| {
        let url = format!("{base}?client_request_id={key}");
        let env = &env;
        async move {
            env.json(Method::POST, &url, &env.owner_token, None, None)
                .await
        }
    };

    let (code, first) = post("snap-1").await;
    assert_eq!(code, 201, "{first}");
    assert_eq!(first["replayed"], false);
    let id = first["id"].as_str().unwrap().to_string();

    // A retry returns the same snapshot, stores nothing, and says so.
    let (code, again) = post("snap-1").await;
    assert_eq!(code, 200, "{again}");
    assert_eq!(again["id"], id.as_str());
    assert_eq!(again["replayed"], true);
    assert_eq!(again["result"], first["result"]);

    // Another key is another snapshot; a key-less request still works.
    let (code, second) = post("snap-2").await;
    assert_eq!(code, 201, "{second}");
    assert_ne!(second["id"], id.as_str());
    let (code, keyless) = env
        .json(Method::POST, &base, &env.owner_token, None, None)
        .await;
    assert_eq!(code, 201, "{keyless}");
    assert_eq!(keyless["replayed"], false);
    let (_, list) = env
        .json(Method::GET, &base, &env.owner_token, None, None)
        .await;
    assert_eq!(list["items"].as_array().unwrap().len(), 3, "{list}");

    // Bad keys are refused by name, and the same key on another task is independent.
    let (code, _) = post("has%20space").await;
    assert_eq!(code, 400);
    let other = new_task(&env, &a, json!({})).await;
    let (code, body) = env
        .json(
            Method::POST,
            &env.url(&format!(
                "/tasks/{other}/completion-status/snapshots?client_request_id=snap-1"
            )),
            &env.owner_token,
            None,
            None,
        )
        .await;
    assert_eq!(code, 201, "{body}");
    assert_ne!(body["id"], id.as_str());
}

#[tokio::test]
async fn racing_snapshot_requests_with_one_key_store_one_snapshot() {
    let env = Env::new().await;
    let a = env.session(&env.owner_token, "A").await;
    let tid = new_task(&env, &a, json!({})).await;
    let url = env.url(&format!(
        "/tasks/{tid}/completion-status/snapshots?client_request_id=race"
    ));
    let results = futures::future::join_all((0..6).map(|_| {
        let env = &env;
        let url = url.clone();
        async move {
            env.json(Method::POST, &url, &env.owner_token, None, None)
                .await
        }
    }))
    .await;
    let created = results.iter().filter(|(c, _)| *c == 201).count();
    let replayed = results.iter().filter(|(c, _)| *c == 200).count();
    assert_eq!(created, 1, "{results:?}");
    assert_eq!(created + replayed, 6, "{results:?}");
    let ids: std::collections::HashSet<_> = results
        .iter()
        .map(|(_, b)| b["id"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(ids.len(), 1, "every racer sees the same snapshot");
}
