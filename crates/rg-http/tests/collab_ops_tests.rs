//! Operability acceptance: task templates (O06), capability declarations (O07),
//! budgets and usage (O08).
//!
//! Real HTTP server and SQLite database; with `IRONFORGE_TEST_DATABASE_URL` the
//! same tests run on PostgreSQL or MySQL (not run locally).

mod common;
mod collab_common;

use collab_common::{Env, Sess};
use reqwest::Method;
use serde_json::{json, Value};

// ------------------------------------------------------------------ O06

fn tpl_body(key: &str, name: &str, goal: &str, checklist: &[&str]) -> Value {
    json!({
        "client_template_id": key,
        "name": name,
        "title_pattern": "Fix {area} bug",
        "goal": goal,
        "deliverables": ["a patch", "a regression test"],
        "checklist": checklist.iter().map(|t| json!({"text": t})).collect::<Vec<_>>(),
    })
}

fn ver_body(key: &str, expected: i64, goal: &str, checklist: &[&str]) -> Value {
    json!({
        "client_version_id": key,
        "expected_version": expected,
        "title_pattern": "Fix {area} bug (v2)",
        "goal": goal,
        "deliverables": ["a patch"],
        "checklist": checklist.iter().map(|t| json!({"text": t})).collect::<Vec<_>>(),
    })
}

async fn make_template(env: &Env, key: &str, name: &str, goal: &str, checklist: &[&str]) -> Value {
    let (status, body) = env
        .json(
            Method::POST,
            &env.url("/templates"),
            &env.owner_token,
            None,
            Some(tpl_body(key, name, goal, checklist)),
        )
        .await;
    assert_eq!(status, 201, "{body}");
    body
}

async fn from_template(env: &Env, s: &Sess, extra: Value) -> (u16, Value) {
    env.json(
        Method::POST,
        &env.url("/tasks/from-template"),
        &env.owner_token,
        Some(s),
        Some(extra),
    )
    .await
}

fn from_body(key: &str, template_id: &str, area: &str) -> Value {
    json!({
        "client_task_id": key,
        "template_id": template_id,
        "params": {"area": area},
    })
}

#[tokio::test]
async fn o06_template_edit_never_changes_old_tasks() {
    let env = Env::new().await;
    let s = env.session(&env.owner_token, "A").await;
    let t = make_template(&env, "t1", "Bugfix", "Fix the bug.", &["tests added", "no new warnings"]).await;
    let tid = t["id"].as_str().unwrap();
    assert_eq!(t["latest_version"], 1);
    assert_eq!(t["version"]["version"], 1);
    assert_eq!(t["origin"], "user_content");

    let (status, first) = from_template(&env, &s, from_body("k1", tid, "auth")).await;
    assert_eq!(status, 201, "{first}");
    assert_eq!(first["task"]["title"], "Fix auth bug");
    let desc1 = first["task"]["description"].as_str().unwrap().to_string();
    assert!(desc1.contains("- [required] tests added"), "{desc1}");
    assert!(desc1.contains("Fix the bug."));
    assert_eq!(first["template_link"]["template_version"], 1);
    assert_eq!(first["template_link"]["snapshot"]["checklist"][0]["text"], "tests added");
    let task1 = first["task"]["id"].as_str().unwrap().to_string();

    // Edit: version 2 with a different goal and checklist.
    let (status, v2) = env
        .json(
            Method::POST,
            &env.url(&format!("/templates/{tid}/versions")),
            &env.owner_token,
            None,
            Some(ver_body("v2", 1, "A different goal.", &["only this"])),
        )
        .await;
    assert_eq!(status, 201, "{v2}");
    assert_eq!(v2["latest_version"], 2);
    assert_eq!(v2["version"]["version"], 2);

    // The old task is byte-for-byte what it was; its link says a newer exists.
    let (_, task_now) = env
        .json(Method::GET, &env.url(&format!("/tasks/{task1}")), &env.owner_token, None, None)
        .await;
    assert_eq!(task_now["description"], desc1.as_str());
    assert_eq!(task_now["title"], "Fix auth bug");
    assert_eq!(task_now["version"], first["task"]["version"]);
    let (status, link) = env
        .json(Method::GET, &env.url(&format!("/tasks/{task1}/template")), &env.owner_token, None, None)
        .await;
    assert_eq!(status, 200, "{link}");
    assert_eq!(link["template_version"], 1);
    assert_eq!(link["snapshot"]["goal"], "Fix the bug.");
    assert_eq!(link["snapshot"]["checklist"].as_array().unwrap().len(), 2);
    assert_eq!(link["template_latest_version"], 2);
    assert_eq!(link["newer_version_available"], true);

    // Version 1 is still readable and unchanged; the default is the latest.
    let (_, old) = env
        .json(Method::GET, &env.url(&format!("/templates/{tid}?version=1")), &env.owner_token, None, None)
        .await;
    assert_eq!(old["version"]["goal"], "Fix the bug.");
    let (_, latest) = env
        .json(Method::GET, &env.url(&format!("/templates/{tid}")), &env.owner_token, None, None)
        .await;
    assert_eq!(latest["version"]["goal"], "A different goal.");
    let (status, _) = env
        .json(Method::GET, &env.url(&format!("/templates/{tid}?version=3")), &env.owner_token, None, None)
        .await;
    assert_eq!(status, 404);

    // A new task takes the latest version; an explicit version pins the old one.
    let (_, second) = from_template(&env, &s, from_body("k2", tid, "ui")).await;
    assert_eq!(second["template_link"]["template_version"], 2);
    assert_eq!(second["task"]["title"], "Fix ui bug (v2)");
    let mut pinned = from_body("k3", tid, "db");
    pinned["version"] = json!(1);
    let (status, third) = from_template(&env, &s, pinned).await;
    assert_eq!(status, 201, "{third}");
    assert_eq!(third["template_link"]["template_version"], 1);
    assert_eq!(third["task"]["title"], "Fix db bug");
}

#[tokio::test]
async fn o06_only_repo_admins_edit_templates_everyone_in_the_group_reads() {
    let env = Env::new().await;
    let t = make_template(&env, "t1", "Bugfix", "Fix.", &["x"]).await;
    let tid = t["id"].as_str().unwrap();

    let (writer, writer_id) = env.user("writer").await;
    env.add_collaborator(writer_id, "write").await;
    let (reader, reader_id) = env.user("reader").await;
    env.add_collaborator(reader_id, "read").await;
    let (admin, admin_id) = env.user("admin").await;
    env.add_collaborator(admin_id, "admin").await;
    let (stranger, _) = env.user("stranger").await;

    for (token, label) in [(&writer, "writer"), (&reader, "reader"), (&stranger, "stranger")] {
        let (status, body) = env
            .json(Method::POST, &env.url("/templates"), token, None, Some(tpl_body("zz", "Other", "g", &[])))
            .await;
        assert_eq!(status, 403, "{label} must not create: {body}");
        let (status, body) = env
            .json(
                Method::POST,
                &env.url(&format!("/templates/{tid}/versions")),
                token,
                None,
                Some(ver_body("zz", 1, "hijack", &[])),
            )
            .await;
        assert_eq!(status, 403, "{label} must not edit: {body}");
    }
    let (status, _) = env
        .json(Method::POST, &env.url("/templates"), "", None, Some(tpl_body("zz", "Other", "g", &[])))
        .await;
    assert_eq!(status, 401);

    // Nothing was created or changed by the refused attempts.
    let (_, list) = env.json(Method::GET, &env.url("/templates"), &env.owner_token, None, None).await;
    assert_eq!(list["items"].as_array().unwrap().len(), 1);
    assert_eq!(list["items"][0]["latest_version"], 1);

    // A repository admin collaborator may edit.
    let (status, body) = env
        .json(
            Method::POST,
            &env.url(&format!("/templates/{tid}/versions")),
            &admin,
            None,
            Some(ver_body("a1", 1, "edited by admin", &[])),
        )
        .await;
    assert_eq!(status, 201, "{body}");

    // Collaboration data is readable by the working group only.
    for token in [&writer, &reader, &admin] {
        let (status, _) = env.json(Method::GET, &env.url("/templates"), token, None, None).await;
        assert_eq!(status, 200);
        let (status, _) = env.json(Method::GET, &env.url(&format!("/templates/{tid}")), token, None, None).await;
        assert_eq!(status, 200);
    }
    let (status, _) = env.json(Method::GET, &env.url("/templates"), &stranger, None, None).await;
    assert_eq!(status, 403);
    let (status, _) = env.json(Method::GET, &env.url(&format!("/templates/{tid}")), &stranger, None, None).await;
    assert_eq!(status, 403);

    // A reader cannot create tasks, a writer can (with a session).
    let rs = env.session(&env.owner_token, "S").await;
    let (status, _) = env
        .json(Method::POST, &env.url("/tasks/from-template"), &reader, Some(&rs), Some(from_body("r1", tid, "x")))
        .await;
    assert_eq!(status, 403);
    let (status, body) = env
        .json(Method::POST, &env.url("/tasks/from-template"), &env.owner_token, None, Some(from_body("r1", tid, "x")))
        .await;
    assert_eq!(status, 400, "session credentials are required: {body}");
    assert_eq!(body["error"]["code"], "SESSION_CREDENTIALS_REQUIRED");
}

#[tokio::test]
async fn o06_idempotency_conflicts_and_stable_codes() {
    let env = Env::new().await;
    let t = make_template(&env, "t1", "Bugfix", "Fix.", &["a"]).await;
    let tid = t["id"].as_str().unwrap();

    // Same key and body: replay, same template. Different body: reused.
    let (status, again) = env
        .json(Method::POST, &env.url("/templates"), &env.owner_token, None, Some(tpl_body("t1", "Bugfix", "Fix.", &["a"])))
        .await;
    assert_eq!(status, 200, "{again}");
    assert_eq!(again["id"], t["id"]);
    let (status, body) = env
        .json(Method::POST, &env.url("/templates"), &env.owner_token, None, Some(tpl_body("t1", "Bugfix", "Other.", &["a"])))
        .await;
    assert_eq!((status, body["error"]["code"].as_str()), (409, Some("IDEMPOTENCY_KEY_REUSED")), "{body}");
    // A new key with a taken name is its own code.
    let (status, body) = env
        .json(Method::POST, &env.url("/templates"), &env.owner_token, None, Some(tpl_body("t2", "Bugfix", "Fix.", &["a"])))
        .await;
    assert_eq!((status, body["error"]["code"].as_str()), (409, Some("TEMPLATE_NAME_TAKEN")), "{body}");

    // Versions: replay, reuse, stale expectation.
    let url = env.url(&format!("/templates/{tid}/versions"));
    let (status, v2) = env
        .json(Method::POST, &url, &env.owner_token, None, Some(ver_body("v2", 1, "g2", &[])))
        .await;
    assert_eq!(status, 201, "{v2}");
    let (status, replay) = env
        .json(Method::POST, &url, &env.owner_token, None, Some(ver_body("v2", 1, "g2", &[])))
        .await;
    assert_eq!(status, 200, "{replay}");
    assert_eq!(replay["version"]["version"], 2);
    assert_eq!(replay["latest_version"], 2);
    let (status, body) = env
        .json(Method::POST, &url, &env.owner_token, None, Some(ver_body("v2", 1, "g2-changed", &[])))
        .await;
    assert_eq!((status, body["error"]["code"].as_str()), (409, Some("IDEMPOTENCY_KEY_REUSED")), "{body}");
    let (status, body) = env
        .json(Method::POST, &url, &env.owner_token, None, Some(ver_body("v3", 1, "g3", &[])))
        .await;
    assert_eq!((status, body["error"]["code"].as_str()), (409, Some("VERSION_CONFLICT")), "{body}");
    assert_eq!(body["error"]["details"]["current_version"], 2);

    // Tasks: replay returns the same task even after the template moved on;
    // a different body under the same key is refused.
    let s = env.session(&env.owner_token, "A").await;
    let mut first_req = from_body("k1", tid, "auth");
    first_req["version"] = json!(1);
    let (status, first) = from_template(&env, &s, first_req.clone()).await;
    assert_eq!(status, 201, "{first}");
    let (status, replay) = from_template(&env, &s, first_req).await;
    assert_eq!(status, 200, "{replay}");
    assert_eq!(replay["replayed"], true);
    assert_eq!(replay["task"]["id"], first["task"]["id"]);
    let (status, body) = from_template(&env, &s, from_body("k1", tid, "other")).await;
    assert_eq!((status, body["error"]["code"].as_str()), (409, Some("IDEMPOTENCY_KEY_REUSED")), "{body}");
    let (_, tasks) = env.json(Method::GET, &env.url("/tasks"), &env.owner_token, None, None).await;
    assert_eq!(tasks["items"].as_array().unwrap().len(), 1, "a refused retry creates nothing");
}

#[tokio::test]
async fn o06_concurrent_edits_and_creates_have_one_winner() {
    let env = Env::new().await;
    let t = make_template(&env, "t1", "Bugfix", "Fix.", &["a"]).await;
    let tid = t["id"].as_str().unwrap().to_string();

    // Six editors that all saw version 1: exactly one version 2.
    let mut handles = Vec::new();
    for i in 0..6 {
        let url = env.url(&format!("/templates/{tid}/versions"));
        let token = env.owner_token.clone();
        handles.push(tokio::spawn(async move {
            let resp = reqwest::Client::new()
                .post(url)
                .bearer_auth(token)
                .json(&ver_body(&format!("race{i}"), 1, &format!("goal {i}"), &[]))
                .send()
                .await
                .unwrap();
            resp.status().as_u16()
        }));
    }
    let mut statuses = Vec::new();
    for h in handles {
        statuses.push(h.await.unwrap());
    }
    assert_eq!(statuses.iter().filter(|s| **s == 201).count(), 1, "{statuses:?}");
    assert_eq!(statuses.iter().filter(|s| **s == 409).count(), 5, "{statuses:?}");
    let (_, now) = env.json(Method::GET, &env.url(&format!("/templates/{tid}")), &env.owner_token, None, None).await;
    assert_eq!(now["latest_version"], 2);
    let (status, _) = env
        .json(Method::GET, &env.url(&format!("/templates/{tid}?version=3")), &env.owner_token, None, None)
        .await;
    assert_eq!(status, 404);

    // Six identical creates with one key: one task, five replays of it.
    let s = env.session(&env.owner_token, "A").await;
    let mut handles = Vec::new();
    for _ in 0..6 {
        let url = env.url("/tasks/from-template");
        let token = env.owner_token.clone();
        let s = s.clone();
        let body = from_body("same-key", &tid, "x");
        handles.push(tokio::spawn(async move {
            let resp = reqwest::Client::new()
                .post(url)
                .bearer_auth(token)
                .header(collab_common::HDR_SESSION, &s.id)
                .header(collab_common::HDR_SECRET, &s.secret)
                .json(&body)
                .send()
                .await
                .unwrap();
            let status = resp.status().as_u16();
            let v: Value = resp.json().await.unwrap();
            (status, v)
        }));
    }
    let mut ids = std::collections::BTreeSet::new();
    let mut created = 0;
    for h in handles {
        let (status, v) = h.await.unwrap();
        assert!(status == 201 || status == 200, "{status} {v}");
        if status == 201 {
            created += 1;
        }
        ids.insert(v["task"]["id"].as_str().unwrap().to_string());
    }
    assert_eq!(created, 1);
    assert_eq!(ids.len(), 1);
    let (_, tasks) = env.json(Method::GET, &env.url("/tasks"), &env.owner_token, None, None).await;
    assert_eq!(tasks["items"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn o06_validation_and_missing_things() {
    let env = Env::new().await;
    let t = make_template(&env, "t1", "Bugfix", "Fix.", &["a"]).await;
    let tid = t["id"].as_str().unwrap();
    let s = env.session(&env.owner_token, "A").await;

    // Pattern braces, empty goal, oversized lists.
    for (patch, why) in [
        (json!({"title_pattern": "Fix {oops"}), "unbalanced brace"),
        (json!({"title_pattern": "Fix {a b}"}), "bad placeholder"),
        (json!({"goal": " "}), "empty goal"),
        (json!({"checklist": vec![json!({"text": "x"}); 31]}), "too many checklist items"),
        (json!({"deliverables": vec!["x"; 21]}), "too many deliverables"),
        (json!({"checklist": [{"text": "two\nlines"}]}), "multi-line item"),
        (json!({"name": ""}), "empty name"),
    ] {
        let mut body = tpl_body("v1", "Other", "g", &[]);
        body.as_object_mut().unwrap().extend(patch.as_object().unwrap().clone());
        let (status, resp) = env.json(Method::POST, &env.url("/templates"), &env.owner_token, None, Some(body)).await;
        assert_eq!(status, 400, "{why}: {resp}");
    }

    // Params must match the pattern exactly.
    let (status, _) = from_template(&env, &s, json!({"client_task_id": "p1", "template_id": tid})).await;
    assert_eq!(status, 400, "missing param");
    let (status, _) = from_template(
        &env,
        &s,
        json!({"client_task_id": "p2", "template_id": tid, "params": {"area": "a", "extra": "b"}}),
    )
    .await;
    assert_eq!(status, 400, "unknown param");

    // Unknown template, unknown version, malformed id.
    let (status, _) = from_template(&env, &s, from_body("p3", &uuid::Uuid::new_v4().to_string(), "a")).await;
    assert_eq!(status, 404);
    let mut v9 = from_body("p4", tid, "a");
    v9["version"] = json!(9);
    let (status, _) = from_template(&env, &s, v9).await;
    assert_eq!(status, 404);
    let (status, _) = from_template(&env, &s, from_body("p5", "not-a-uuid", "a")).await;
    assert_eq!(status, 404);

    // A template of another repository is invisible here.
    let other = "other-repo";
    let resp = reqwest::Client::new()
        .post(format!("{}/api/v1/repos", env.base))
        .bearer_auth(&env.owner_token)
        .json(&json!({"name": other}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);
    let (status, _) = env
        .json(Method::GET, &env.url_in(other, &format!("/templates/{tid}")), &env.owner_token, None, None)
        .await;
    assert_eq!(status, 404);
    let other_session = {
        let (status, body) = env
            .json(
                Method::POST,
                &env.url_in(other, "/sessions"),
                &env.owner_token,
                None,
                Some(json!({"display_name": "O", "client_kind": "t"})),
            )
            .await;
        assert_eq!(status, 201);
        Sess {
            id: body["session"]["id"].as_str().unwrap().to_string(),
            secret: body["session_secret"].as_str().unwrap().to_string(),
        }
    };
    let (status, _) = env
        .json(
            Method::POST,
            &env.url_in(other, "/tasks/from-template"),
            &env.owner_token,
            Some(&other_session),
            Some(from_body("x1", tid, "a")),
        )
        .await;
    assert_eq!(status, 404, "cross-repository template ids read as absent");

    // A task made without a template has no pin.
    let (_, plain) = env
        .json(Method::POST, &env.url("/tasks"), &env.owner_token, Some(&s), Some(json!({"title": "plain"})))
        .await;
    let (status, _) = env
        .json(
            Method::GET,
            &env.url(&format!("/tasks/{}/template", plain["id"].as_str().unwrap())),
            &env.owner_token,
            None,
            None,
        )
        .await;
    assert_eq!(status, 404);
}

#[tokio::test]
async fn o06_rules_commit_is_recorded_only_when_a_ref_is_supplied() {
    let env = Env::new().await;
    let sha = env.push_tree("collab-repo", "main", &[("AGENTS.md", "# rules\n")], &[]);
    let t = make_template(&env, "t1", "Bugfix", "Fix.", &["a"]).await;
    let tid = t["id"].as_str().unwrap();
    let s = env.session(&env.owner_token, "A").await;

    let (status, none) = from_template(&env, &s, from_body("r0", tid, "a")).await;
    assert_eq!(status, 201, "{none}");
    assert!(none["template_link"]["rules_commit"].is_null());

    let mut with_ref = from_body("r1", tid, "b");
    with_ref["rules_ref"] = json!("main");
    let (status, got) = from_template(&env, &s, with_ref.clone()).await;
    assert_eq!(status, 201, "{got}");
    assert_eq!(got["template_link"]["rules_commit"], sha.as_str());

    // The branch moves on; a retry still answers with the pinned commit, and the
    // stored link is not re-resolved.
    let newer = env.push_tree("collab-repo", "main", &[("AGENTS.md", "# rules v2\n")], &[]);
    assert_ne!(newer, sha);
    let (status, replay) = from_template(&env, &s, with_ref).await;
    assert_eq!(status, 200, "{replay}");
    assert_eq!(replay["template_link"]["rules_commit"], sha.as_str());
    let (_, link) = env
        .json(
            Method::GET,
            &env.url(&format!("/tasks/{}/template", got["task"]["id"].as_str().unwrap())),
            &env.owner_token,
            None,
            None,
        )
        .await;
    assert_eq!(link["rules_commit"], sha.as_str());

    let mut bad = from_body("r2", tid, "c");
    bad["rules_ref"] = json!("no-such-branch");
    let (status, body) = from_template(&env, &s, bad).await;
    assert_eq!(status, 400, "{body}");
    let (_, tasks) = env.json(Method::GET, &env.url("/tasks"), &env.owner_token, None, None).await;
    assert_eq!(tasks["items"].as_array().unwrap().len(), 2, "a refused request creates no task");
}

// ------------------------------------------------------------------ O07

fn decl_body(expected: i64, tests: &str, notes: &str) -> Value {
    json!({
        "expected_version": expected,
        "tools": ["cargo", "git"],
        "can_run_tests": tests,
        "network": "restricted",
        "network_notes": "crates.io only",
        "notes": notes,
    })
}

async fn put_decl(env: &Env, s: &Sess, body: Value) -> (u16, Value) {
    env.json(
        Method::PUT,
        &env.url(&format!("/sessions/{}/declaration", s.id)),
        &env.owner_token,
        Some(s),
        Some(body),
    )
    .await
}

async fn get_decl(env: &Env, token: &str, id: &str, query: &str) -> (u16, Value) {
    env.json(Method::GET, &env.url(&format!("/sessions/{id}/declaration{query}")), token, None, None)
        .await
}

async fn age_declaration(env: &Env, session_id: &str, ago: chrono::Duration) {
    use rg_db::entities::collab_session_declaration as d;
    use rg_db::sea_orm::{ActiveModelTrait, EntityTrait, Set};
    let row = d::Entity::find_by_id(session_id.to_string()).one(&env.db).await.unwrap().unwrap();
    let mut active: d::ActiveModel = row.into();
    active.declared_at = Set(chrono::Utc::now() - ago);
    active.update(&env.db).await.unwrap();
}

#[tokio::test]
async fn o07_declaration_is_self_reported_versioned_and_authorizes_nothing() {
    let env = Env::new().await;
    let a = env.session(&env.owner_token, "A").await;
    let b = env.session(&env.owner_token, "B").await;

    // Before any declaration: unknown, explicitly not "cannot".
    let (status, none) = get_decl(&env, &env.owner_token, &a.id, "").await;
    assert_eq!(status, 200, "{none}");
    assert_eq!(none["status"], "undeclared");
    assert!(none["declaration"].is_null());
    assert_eq!(none["authorization"], "none");
    assert!(none["meaning"].as_str().unwrap().contains("authorizes nothing"));

    let (status, first) = put_decl(&env, &a, decl_body(0, "yes", "first")).await;
    assert_eq!(status, 200, "{first}");
    assert_eq!(first["version"], 1);
    assert_eq!(first["source"], "user_content");
    assert_eq!(first["authorization"], "none");
    assert_eq!(first["stale"], false);
    assert_eq!(first["tools"], json!(["cargo", "git"]));
    assert_eq!(first["can_run_tests"], "yes");
    assert_eq!(first["session_state"], "online");

    // The retry of the same request is harmless; a different body at the old
    // version is a conflict that says where things stand.
    let (status, retry) = put_decl(&env, &a, decl_body(0, "yes", "first")).await;
    assert_eq!(status, 200, "{retry}");
    assert_eq!(retry["version"], 1);
    let (status, conflict) = put_decl(&env, &a, decl_body(0, "no", "other")).await;
    assert_eq!((status, conflict["error"]["code"].as_str()), (409, Some("VERSION_CONFLICT")), "{conflict}");
    assert_eq!(conflict["error"]["details"]["current_version"], 1);
    let (_, still) = get_decl(&env, &env.owner_token, &a.id, "").await;
    assert_eq!(still["declaration"]["can_run_tests"], "yes", "a refused replace changes nothing");

    let (status, second) = put_decl(&env, &a, decl_body(1, "no", "second")).await;
    assert_eq!(status, 200, "{second}");
    assert_eq!(second["version"], 2);
    assert_eq!(second["can_run_tests"], "no");

    // A session declares only for itself.
    let (status, body) = env
        .json(
            Method::PUT,
            &env.url(&format!("/sessions/{}/declaration", a.id)),
            &env.owner_token,
            Some(&b),
            Some(decl_body(2, "yes", "forged")),
        )
        .await;
    assert_eq!((status, body["error"]["code"].as_str()), (403, Some("SESSION_CREDENTIALS_INVALID")), "{body}");
    let (status, body) = env
        .json(
            Method::PUT,
            &env.url(&format!("/sessions/{}/declaration", a.id)),
            &env.owner_token,
            None,
            Some(decl_body(2, "yes", "no session")),
        )
        .await;
    assert_eq!((status, body["error"]["code"].as_str()), (400, Some("SESSION_CREDENTIALS_REQUIRED")), "{body}");
    let (_, now) = get_decl(&env, &env.owner_token, &a.id, "").await;
    assert_eq!(now["declaration"]["version"], 2);
    assert_eq!(now["declaration"]["notes"], "second");

    // Closed vocabularies and bounds.
    for patch in [
        json!({"can_run_tests": "maybe"}),
        json!({"network": "fast"}),
        json!({"tools": vec!["x"; 51]}),
        json!({"tools": [""]}),
        json!({"notes": "x".repeat(4097)}),
        json!({"expected_version": -1}),
    ] {
        let mut body = decl_body(2, "yes", "n");
        body.as_object_mut().unwrap().extend(patch.as_object().unwrap().clone());
        let (status, resp) = put_decl(&env, &a, body).await;
        assert_eq!(status, 400, "{patch}: {resp}");
    }

    // Declaring changes nothing about what the session may do: its task rights
    // are exactly what they were (here: a claim is still a claim with the usual
    // conditions, not something the declaration unlocked).
    let (_, task) = env
        .json(Method::POST, &env.url("/tasks"), &env.owner_token, Some(&b), Some(json!({"title": "t"})))
        .await;
    let (status, _) = env
        .json(
            Method::PATCH,
            &env.url(&format!("/tasks/{}", task["id"].as_str().unwrap())),
            &env.owner_token,
            Some(&a),
            Some(json!({"expected_version": 1, "status": "done"})),
        )
        .await;
    assert_eq!(status, 200, "task updates are governed by repository write access alone");
}

#[tokio::test]
async fn o07_old_or_orphaned_declarations_are_marked_stale() {
    let env = Env::new().await;
    let a = env.session(&env.owner_token, "A").await;
    let b = env.session(&env.owner_token, "B").await;
    let c = env.session(&env.owner_token, "C").await;
    put_decl(&env, &a, decl_body(0, "yes", "a")).await;
    put_decl(&env, &b, decl_body(0, "yes", "b")).await;
    put_decl(&env, &c, decl_body(0, "yes", "c")).await;

    age_declaration(&env, &a.id, chrono::Duration::hours(30)).await;
    let (_, old) = get_decl(&env, &env.owner_token, &a.id, "").await;
    assert_eq!(old["declaration"]["stale"], true);
    assert_eq!(old["declaration"]["stale_reasons"], json!(["age"]));
    assert!(old["declaration"]["age_seconds"].as_i64().unwrap() >= 30 * 3600);
    assert_eq!(old["declaration"]["stale_after_seconds"], 86400);
    assert_eq!(old["declaration"]["can_run_tests"], "yes", "stale data is still shown, with its flag");

    // The threshold is configurable per request, and clamped.
    let (_, lenient) = get_decl(&env, &env.owner_token, &a.id, "?stale_after_seconds=172800").await;
    assert_eq!(lenient["declaration"]["stale"], false);
    let (_, strict) = get_decl(&env, &env.owner_token, &b.id, "?stale_after_seconds=1").await;
    assert_eq!(strict["declaration"]["stale_after_seconds"], 60);
    assert_eq!(strict["declaration"]["stale"], false, "a fresh declaration is not stale at the 60s floor");

    // A session that ended: its declaration is flagged although it is recent.
    let (status, _) = env
        .json(Method::POST, &env.url(&format!("/sessions/{}/end", c.id)), &env.owner_token, Some(&c), None)
        .await;
    assert_eq!(status, 204);
    let (_, ended) = get_decl(&env, &env.owner_token, &c.id, "").await;
    assert_eq!(ended["declaration"]["stale"], true);
    assert_eq!(ended["declaration"]["stale_reasons"], json!(["session_ended"]));
    assert_eq!(ended["session_state"], "ended");

    // The list carries the same flags and can be joined with /sessions by id.
    let (status, list) = env.json(Method::GET, &env.url("/declarations"), &env.owner_token, None, None).await;
    assert_eq!(status, 200, "{list}");
    let items = list["items"].as_array().unwrap();
    assert_eq!(items.len(), 3);
    let find = |id: &str| items.iter().find(|i| i["session_id"] == id).unwrap();
    assert_eq!(find(&a.id)["stale"], true);
    assert_eq!(find(&b.id)["stale"], false);
    assert_eq!(find(&c.id)["stale_reasons"], json!(["session_ended"]));
    for item in items {
        assert_eq!((item["source"].as_str(), item["authorization"].as_str()), (Some("user_content"), Some("none")));
    }
    let (_, sessions) = env.json(Method::GET, &env.url("/sessions"), &env.owner_token, None, None).await;
    let session_ids: Vec<&str> = sessions["items"].as_array().unwrap().iter().map(|s| s["id"].as_str().unwrap()).collect();
    for item in items {
        assert!(session_ids.contains(&item["session_id"].as_str().unwrap()), "joinable by session_id");
    }

    // Re-declaring (same content, current version) refreshes the timestamp.
    let (status, fresh) = put_decl(&env, &a, decl_body(1, "yes", "a")).await;
    assert_eq!(status, 200, "{fresh}");
    assert_eq!(fresh["version"], 2);
    assert_eq!(fresh["stale"], false);
}

#[tokio::test]
async fn o07_access_and_races() {
    let env = Env::new().await;
    let a = env.session(&env.owner_token, "A").await;
    put_decl(&env, &a, decl_body(0, "yes", "a")).await;

    let (reader, reader_id) = env.user("reader").await;
    env.add_collaborator(reader_id, "read").await;
    let (stranger, _) = env.user("stranger").await;
    let (status, _) = get_decl(&env, &reader, &a.id, "").await;
    assert_eq!(status, 200);
    let (status, _) = get_decl(&env, &stranger, &a.id, "").await;
    assert_eq!(status, 403);
    let (status, _) = env.json(Method::GET, &env.url("/declarations"), &stranger, None, None).await;
    assert_eq!(status, 403);
    let (status, _) = env
        .json(
            Method::PUT,
            &env.url(&format!("/sessions/{}/declaration", a.id)),
            &reader,
            Some(&a),
            Some(decl_body(1, "no", "x")),
        )
        .await;
    assert_eq!(status, 403, "read access cannot declare");

    // Unknown and foreign-repository sessions read as absent.
    let (status, _) = get_decl(&env, &env.owner_token, &uuid::Uuid::new_v4().to_string(), "").await;
    assert_eq!(status, 404);
    let (status, _) = get_decl(&env, &env.owner_token, "nope", "").await;
    assert_eq!(status, 404);

    // Six declarations racing from version 1: one winner.
    let mut handles = Vec::new();
    for i in 0..6 {
        let url = env.url(&format!("/sessions/{}/declaration", a.id));
        let token = env.owner_token.clone();
        let a = a.clone();
        handles.push(tokio::spawn(async move {
            reqwest::Client::new()
                .put(url)
                .bearer_auth(token)
                .header(collab_common::HDR_SESSION, &a.id)
                .header(collab_common::HDR_SECRET, &a.secret)
                .json(&decl_body(1, "yes", &format!("racer {i}")))
                .send()
                .await
                .unwrap()
                .status()
                .as_u16()
        }));
    }
    let mut statuses = Vec::new();
    for h in handles {
        statuses.push(h.await.unwrap());
    }
    assert_eq!(statuses.iter().filter(|s| **s == 200).count(), 1, "{statuses:?}");
    assert_eq!(statuses.iter().filter(|s| **s == 409).count(), 5, "{statuses:?}");
    let (_, now) = get_decl(&env, &env.owner_token, &a.id, "").await;
    assert_eq!(now["declaration"]["version"], 2);
}
