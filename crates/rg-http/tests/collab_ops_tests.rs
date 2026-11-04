//! Operability acceptance: task templates (O06), capability declarations (O07),
//! budgets and usage (O08).
//!
//! Real HTTP server and SQLite database; with `IRONFORGE_TEST_DATABASE_URL` the
//! same tests run on PostgreSQL or MySQL (not run locally).

mod collab_common;
mod common;

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
    let t = make_template(
        &env,
        "t1",
        "Bugfix",
        "Fix the bug.",
        &["tests added", "no new warnings"],
    )
    .await;
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
    assert_eq!(
        first["template_link"]["snapshot"]["checklist"][0]["text"],
        "tests added"
    );
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
        .json(
            Method::GET,
            &env.url(&format!("/tasks/{task1}")),
            &env.owner_token,
            None,
            None,
        )
        .await;
    assert_eq!(task_now["description"], desc1.as_str());
    assert_eq!(task_now["title"], "Fix auth bug");
    assert_eq!(task_now["version"], first["task"]["version"]);
    let (status, link) = env
        .json(
            Method::GET,
            &env.url(&format!("/tasks/{task1}/template")),
            &env.owner_token,
            None,
            None,
        )
        .await;
    assert_eq!(status, 200, "{link}");
    assert_eq!(link["template_version"], 1);
    assert_eq!(link["snapshot"]["goal"], "Fix the bug.");
    assert_eq!(link["snapshot"]["checklist"].as_array().unwrap().len(), 2);
    assert_eq!(link["template_latest_version"], 2);
    assert_eq!(link["newer_version_available"], true);

    // Version 1 is still readable and unchanged; the default is the latest.
    let (_, old) = env
        .json(
            Method::GET,
            &env.url(&format!("/templates/{tid}?version=1")),
            &env.owner_token,
            None,
            None,
        )
        .await;
    assert_eq!(old["version"]["goal"], "Fix the bug.");
    let (_, latest) = env
        .json(
            Method::GET,
            &env.url(&format!("/templates/{tid}")),
            &env.owner_token,
            None,
            None,
        )
        .await;
    assert_eq!(latest["version"]["goal"], "A different goal.");
    let (status, _) = env
        .json(
            Method::GET,
            &env.url(&format!("/templates/{tid}?version=3")),
            &env.owner_token,
            None,
            None,
        )
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

    for (token, label) in [
        (&writer, "writer"),
        (&reader, "reader"),
        (&stranger, "stranger"),
    ] {
        let (status, body) = env
            .json(
                Method::POST,
                &env.url("/templates"),
                token,
                None,
                Some(tpl_body("zz", "Other", "g", &[])),
            )
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
        .json(
            Method::POST,
            &env.url("/templates"),
            "",
            None,
            Some(tpl_body("zz", "Other", "g", &[])),
        )
        .await;
    assert_eq!(status, 401);

    // Nothing was created or changed by the refused attempts.
    let (_, list) = env
        .json(
            Method::GET,
            &env.url("/templates"),
            &env.owner_token,
            None,
            None,
        )
        .await;
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
        let (status, _) = env
            .json(Method::GET, &env.url("/templates"), token, None, None)
            .await;
        assert_eq!(status, 200);
        let (status, _) = env
            .json(
                Method::GET,
                &env.url(&format!("/templates/{tid}")),
                token,
                None,
                None,
            )
            .await;
        assert_eq!(status, 200);
    }
    let (status, _) = env
        .json(Method::GET, &env.url("/templates"), &stranger, None, None)
        .await;
    assert_eq!(status, 403);
    let (status, _) = env
        .json(
            Method::GET,
            &env.url(&format!("/templates/{tid}")),
            &stranger,
            None,
            None,
        )
        .await;
    assert_eq!(status, 403);

    // A reader cannot create tasks, a writer can (with a session).
    let rs = env.session(&env.owner_token, "S").await;
    let (status, _) = env
        .json(
            Method::POST,
            &env.url("/tasks/from-template"),
            &reader,
            Some(&rs),
            Some(from_body("r1", tid, "x")),
        )
        .await;
    assert_eq!(status, 403);
    let (status, body) = env
        .json(
            Method::POST,
            &env.url("/tasks/from-template"),
            &env.owner_token,
            None,
            Some(from_body("r1", tid, "x")),
        )
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
        .json(
            Method::POST,
            &env.url("/templates"),
            &env.owner_token,
            None,
            Some(tpl_body("t1", "Bugfix", "Fix.", &["a"])),
        )
        .await;
    assert_eq!(status, 200, "{again}");
    assert_eq!(again["id"], t["id"]);
    let (status, body) = env
        .json(
            Method::POST,
            &env.url("/templates"),
            &env.owner_token,
            None,
            Some(tpl_body("t1", "Bugfix", "Other.", &["a"])),
        )
        .await;
    assert_eq!(
        (status, body["error"]["code"].as_str()),
        (409, Some("IDEMPOTENCY_KEY_REUSED")),
        "{body}"
    );
    // A new key with a taken name is its own code.
    let (status, body) = env
        .json(
            Method::POST,
            &env.url("/templates"),
            &env.owner_token,
            None,
            Some(tpl_body("t2", "Bugfix", "Fix.", &["a"])),
        )
        .await;
    assert_eq!(
        (status, body["error"]["code"].as_str()),
        (409, Some("TEMPLATE_NAME_TAKEN")),
        "{body}"
    );

    // Versions: replay, reuse, stale expectation.
    let url = env.url(&format!("/templates/{tid}/versions"));
    let (status, v2) = env
        .json(
            Method::POST,
            &url,
            &env.owner_token,
            None,
            Some(ver_body("v2", 1, "g2", &[])),
        )
        .await;
    assert_eq!(status, 201, "{v2}");
    let (status, replay) = env
        .json(
            Method::POST,
            &url,
            &env.owner_token,
            None,
            Some(ver_body("v2", 1, "g2", &[])),
        )
        .await;
    assert_eq!(status, 200, "{replay}");
    assert_eq!(replay["version"]["version"], 2);
    assert_eq!(replay["latest_version"], 2);
    let (status, body) = env
        .json(
            Method::POST,
            &url,
            &env.owner_token,
            None,
            Some(ver_body("v2", 1, "g2-changed", &[])),
        )
        .await;
    assert_eq!(
        (status, body["error"]["code"].as_str()),
        (409, Some("IDEMPOTENCY_KEY_REUSED")),
        "{body}"
    );
    let (status, body) = env
        .json(
            Method::POST,
            &url,
            &env.owner_token,
            None,
            Some(ver_body("v3", 1, "g3", &[])),
        )
        .await;
    assert_eq!(
        (status, body["error"]["code"].as_str()),
        (409, Some("VERSION_CONFLICT")),
        "{body}"
    );
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
    assert_eq!(
        (status, body["error"]["code"].as_str()),
        (409, Some("IDEMPOTENCY_KEY_REUSED")),
        "{body}"
    );
    let (_, tasks) = env
        .json(
            Method::GET,
            &env.url("/tasks"),
            &env.owner_token,
            None,
            None,
        )
        .await;
    assert_eq!(
        tasks["items"].as_array().unwrap().len(),
        1,
        "a refused retry creates nothing"
    );
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
    assert_eq!(
        statuses.iter().filter(|s| **s == 201).count(),
        1,
        "{statuses:?}"
    );
    assert_eq!(
        statuses.iter().filter(|s| **s == 409).count(),
        5,
        "{statuses:?}"
    );
    let (_, now) = env
        .json(
            Method::GET,
            &env.url(&format!("/templates/{tid}")),
            &env.owner_token,
            None,
            None,
        )
        .await;
    assert_eq!(now["latest_version"], 2);
    let (status, _) = env
        .json(
            Method::GET,
            &env.url(&format!("/templates/{tid}?version=3")),
            &env.owner_token,
            None,
            None,
        )
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
    let (_, tasks) = env
        .json(
            Method::GET,
            &env.url("/tasks"),
            &env.owner_token,
            None,
            None,
        )
        .await;
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
        (
            json!({"checklist": vec![json!({"text": "x"}); 31]}),
            "too many checklist items",
        ),
        (
            json!({"deliverables": vec!["x"; 21]}),
            "too many deliverables",
        ),
        (
            json!({"checklist": [{"text": "two\nlines"}]}),
            "multi-line item",
        ),
        (json!({"name": ""}), "empty name"),
    ] {
        let mut body = tpl_body("v1", "Other", "g", &[]);
        body.as_object_mut()
            .unwrap()
            .extend(patch.as_object().unwrap().clone());
        let (status, resp) = env
            .json(
                Method::POST,
                &env.url("/templates"),
                &env.owner_token,
                None,
                Some(body),
            )
            .await;
        assert_eq!(status, 400, "{why}: {resp}");
    }

    // Params must match the pattern exactly.
    let (status, _) = from_template(
        &env,
        &s,
        json!({"client_task_id": "p1", "template_id": tid}),
    )
    .await;
    assert_eq!(status, 400, "missing param");
    let (status, _) = from_template(
        &env,
        &s,
        json!({"client_task_id": "p2", "template_id": tid, "params": {"area": "a", "extra": "b"}}),
    )
    .await;
    assert_eq!(status, 400, "unknown param");

    // Unknown template, unknown version, malformed id.
    let (status, _) = from_template(
        &env,
        &s,
        from_body("p3", &uuid::Uuid::new_v4().to_string(), "a"),
    )
    .await;
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
        .json(
            Method::GET,
            &env.url_in(other, &format!("/templates/{tid}")),
            &env.owner_token,
            None,
            None,
        )
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
        .json(
            Method::POST,
            &env.url("/tasks"),
            &env.owner_token,
            Some(&s),
            Some(json!({"title": "plain"})),
        )
        .await;
    let (status, _) = env
        .json(
            Method::GET,
            &env.url(&format!(
                "/tasks/{}/template",
                plain["id"].as_str().unwrap()
            )),
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
            &env.url(&format!(
                "/tasks/{}/template",
                got["task"]["id"].as_str().unwrap()
            )),
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
    let (_, tasks) = env
        .json(
            Method::GET,
            &env.url("/tasks"),
            &env.owner_token,
            None,
            None,
        )
        .await;
    assert_eq!(
        tasks["items"].as_array().unwrap().len(),
        2,
        "a refused request creates no task"
    );
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
    env.json(
        Method::GET,
        &env.url(&format!("/sessions/{id}/declaration{query}")),
        token,
        None,
        None,
    )
    .await
}

async fn age_declaration(env: &Env, session_id: &str, ago: chrono::Duration) {
    use rg_db::entities::collab_session_declaration as d;
    use rg_db::sea_orm::{ActiveModelTrait, EntityTrait, Set};
    let row = d::Entity::find_by_id(session_id.to_string())
        .one(&env.db)
        .await
        .unwrap()
        .unwrap();
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
    assert!(none["meaning"]
        .as_str()
        .unwrap()
        .contains("authorizes nothing"));

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
    assert_eq!(
        (status, conflict["error"]["code"].as_str()),
        (409, Some("VERSION_CONFLICT")),
        "{conflict}"
    );
    assert_eq!(conflict["error"]["details"]["current_version"], 1);
    let (_, still) = get_decl(&env, &env.owner_token, &a.id, "").await;
    assert_eq!(
        still["declaration"]["can_run_tests"], "yes",
        "a refused replace changes nothing"
    );

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
    assert_eq!(
        (status, body["error"]["code"].as_str()),
        (403, Some("SESSION_CREDENTIALS_INVALID")),
        "{body}"
    );
    let (status, body) = env
        .json(
            Method::PUT,
            &env.url(&format!("/sessions/{}/declaration", a.id)),
            &env.owner_token,
            None,
            Some(decl_body(2, "yes", "no session")),
        )
        .await;
    assert_eq!(
        (status, body["error"]["code"].as_str()),
        (400, Some("SESSION_CREDENTIALS_REQUIRED")),
        "{body}"
    );
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
        body.as_object_mut()
            .unwrap()
            .extend(patch.as_object().unwrap().clone());
        let (status, resp) = put_decl(&env, &a, body).await;
        assert_eq!(status, 400, "{patch}: {resp}");
    }

    // Declaring changes nothing about what the session may do: its task rights
    // are exactly what they were (here: a claim is still a claim with the usual
    // conditions, not something the declaration unlocked).
    let (_, task) = env
        .json(
            Method::POST,
            &env.url("/tasks"),
            &env.owner_token,
            Some(&b),
            Some(json!({"title": "t"})),
        )
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
    assert_eq!(
        status, 200,
        "task updates are governed by repository write access alone"
    );
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
    assert_eq!(
        old["declaration"]["can_run_tests"], "yes",
        "stale data is still shown, with its flag"
    );

    // The threshold is configurable per request, and clamped.
    let (_, lenient) = get_decl(&env, &env.owner_token, &a.id, "?stale_after_seconds=172800").await;
    assert_eq!(lenient["declaration"]["stale"], false);
    let (_, strict) = get_decl(&env, &env.owner_token, &b.id, "?stale_after_seconds=1").await;
    assert_eq!(strict["declaration"]["stale_after_seconds"], 60);
    assert_eq!(
        strict["declaration"]["stale"], false,
        "a fresh declaration is not stale at the 60s floor"
    );

    // A session that ended: its declaration is flagged although it is recent.
    let (status, _) = env
        .json(
            Method::POST,
            &env.url(&format!("/sessions/{}/end", c.id)),
            &env.owner_token,
            Some(&c),
            None,
        )
        .await;
    assert_eq!(status, 204);
    let (_, ended) = get_decl(&env, &env.owner_token, &c.id, "").await;
    assert_eq!(ended["declaration"]["stale"], true);
    assert_eq!(
        ended["declaration"]["stale_reasons"],
        json!(["session_ended"])
    );
    assert_eq!(ended["session_state"], "ended");

    // The list carries the same flags and can be joined with /sessions by id.
    let (status, list) = env
        .json(
            Method::GET,
            &env.url("/declarations"),
            &env.owner_token,
            None,
            None,
        )
        .await;
    assert_eq!(status, 200, "{list}");
    let items = list["items"].as_array().unwrap();
    assert_eq!(items.len(), 3);
    let find = |id: &str| items.iter().find(|i| i["session_id"] == id).unwrap();
    assert_eq!(find(&a.id)["stale"], true);
    assert_eq!(find(&b.id)["stale"], false);
    assert_eq!(find(&c.id)["stale_reasons"], json!(["session_ended"]));
    for item in items {
        assert_eq!(
            (item["source"].as_str(), item["authorization"].as_str()),
            (Some("user_content"), Some("none"))
        );
    }
    let (_, sessions) = env
        .json(
            Method::GET,
            &env.url("/sessions"),
            &env.owner_token,
            None,
            None,
        )
        .await;
    let session_ids: Vec<&str> = sessions["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["id"].as_str().unwrap())
        .collect();
    for item in items {
        assert!(
            session_ids.contains(&item["session_id"].as_str().unwrap()),
            "joinable by session_id"
        );
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
    let (status, _) = env
        .json(
            Method::GET,
            &env.url("/declarations"),
            &stranger,
            None,
            None,
        )
        .await;
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
    let (status, _) = get_decl(
        &env,
        &env.owner_token,
        &uuid::Uuid::new_v4().to_string(),
        "",
    )
    .await;
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
    assert_eq!(
        statuses.iter().filter(|s| **s == 200).count(),
        1,
        "{statuses:?}"
    );
    assert_eq!(
        statuses.iter().filter(|s| **s == 409).count(),
        5,
        "{statuses:?}"
    );
    let (_, now) = get_decl(&env, &env.owner_token, &a.id, "").await;
    assert_eq!(now["declaration"]["version"], 2);
}

// ------------------------------------------------------------------ O08

async fn new_task(env: &Env, s: &Sess) -> String {
    let (status, task) = env
        .json(
            Method::POST,
            &env.url("/tasks"),
            &env.owner_token,
            Some(s),
            Some(json!({"title": "budgeted"})),
        )
        .await;
    assert_eq!(status, 201, "{task}");
    task["id"].as_str().unwrap().to_string()
}

fn budget_body(expected: i64, unit: &str, source: &str, amount: Value) -> Value {
    json!({
        "expected_version": expected,
        "unit": unit,
        "source": source,
        "amount": amount,
        "sampling_period": "task_total",
    })
}

async fn put_budget(env: &Env, task: &str, body: Value) -> (u16, Value) {
    env.json(
        Method::PUT,
        &env.url(&format!("/tasks/{task}/budgets")),
        &env.owner_token,
        None,
        Some(body),
    )
    .await
}

fn usage_body(key: &str, unit: &str, source: &str, amount: Value) -> Value {
    let end = chrono::Utc::now() - chrono::Duration::minutes(1);
    json!({
        "client_usage_id": key,
        "unit": unit,
        "source": source,
        "amount": amount,
        "period_start": (end - chrono::Duration::minutes(10)).to_rfc3339(),
        "period_end": end.to_rfc3339(),
    })
}

async fn report(env: &Env, task: &str, s: Option<&Sess>, body: Value) -> (u16, Value) {
    env.json(
        Method::POST,
        &env.url(&format!("/tasks/{task}/usage")),
        &env.owner_token,
        s,
        Some(body),
    )
    .await
}

async fn budget(env: &Env, task: &str) -> Value {
    let (status, body) = env
        .json(
            Method::GET,
            &env.url(&format!("/tasks/{task}/budget")),
            &env.owner_token,
            None,
            None,
        )
        .await;
    assert_eq!(status, 200, "{body}");
    body
}

fn entry<'a>(summary: &'a Value, unit: &str, source: &str) -> &'a Value {
    summary["budgets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["unit"] == unit && b["source"] == source)
        .unwrap_or_else(|| panic!("no budget {unit}/{source} in {summary}"))
}

#[tokio::test]
async fn o08_missing_usage_is_unknown_never_zero() {
    let env = Env::new().await;
    let s = env.session(&env.owner_token, "A").await;
    let task = new_task(&env, &s).await;

    // No budget at all: an empty ledger, not a zero one.
    let empty = budget(&env, &task).await;
    assert_eq!(empty["budgets"], json!([]));
    assert_eq!(empty["unmatched_usage"], json!([]));
    assert!(empty["enforcement"].as_str().unwrap().starts_with("none"));

    let (status, set) =
        put_budget(&env, &task, budget_body(0, "USD", "invoice", json!("10"))).await;
    assert_eq!(status, 201, "{set}");
    assert_eq!(set["version"], 1);
    assert_eq!(set["amount"], "10");
    assert_eq!(set["origin"], "user_content");
    assert_eq!(set["usage"]["status"], "unknown");

    let got = budget(&env, &task).await;
    let b = entry(&got, "USD", "invoice");
    assert_eq!(b["usage"]["status"], "unknown");
    assert!(
        b["usage"]["total"].is_null(),
        "no usage is null, never \"0\": {b}"
    );
    assert_eq!(b["usage"]["record_count"], 0);
    assert!(b["usage"]["reason"]
        .as_str()
        .unwrap()
        .contains("no usage has been reported"));
    assert!(b["over_threshold"].is_null());
    assert!(b["threshold"].is_null());

    // An explicit zero IS a measurement and differs from missing.
    let (status, _) = report(
        &env,
        &task,
        Some(&s),
        usage_body("z", "USD", "invoice", json!(0)),
    )
    .await;
    assert_eq!(status, 201);
    let got = budget(&env, &task).await;
    let b = entry(&got, "USD", "invoice");
    assert_eq!(b["usage"]["status"], "reported");
    assert_eq!(b["usage"]["total"], "0");
    assert_eq!(b["over_threshold"], false);
}

#[tokio::test]
async fn o08_units_and_sources_are_never_mixed_and_thresholds_show_their_numbers() {
    let env = Env::new().await;
    let s = env.session(&env.owner_token, "A").await;
    let task = new_task(&env, &s).await;

    let mut with_alert = budget_body(0, "USD", "invoice", json!("10"));
    with_alert["alert_at"] = json!("5");
    assert_eq!(put_budget(&env, &task, with_alert).await.0, 201);
    assert_eq!(
        put_budget(
            &env,
            &task,
            budget_body(0, "tokens", "harness", json!(1000))
        )
        .await
        .0,
        201
    );
    assert_eq!(
        put_budget(&env, &task, budget_body(0, "x", "s", json!("0.3")))
            .await
            .0,
        201
    );

    // Same unit, other source; other unit, same source: neither is the budget's.
    assert_eq!(
        report(
            &env,
            &task,
            Some(&s),
            usage_body("u1", "USD", "estimate", json!("100"))
        )
        .await
        .0,
        201
    );
    assert_eq!(
        report(
            &env,
            &task,
            Some(&s),
            usage_body("u2", "EUR", "invoice", json!("100"))
        )
        .await
        .0,
        201
    );
    let got = budget(&env, &task).await;
    let usd = entry(&got, "USD", "invoice");
    assert_eq!(
        usd["usage"]["status"], "unknown",
        "100 USD from another source is not this budget's usage"
    );
    assert!(usd["over_threshold"].is_null());
    let unmatched = got["unmatched_usage"].as_array().unwrap();
    assert_eq!(unmatched.len(), 2);
    for (unit, source) in [("USD", "estimate"), ("EUR", "invoice")] {
        let u = unmatched
            .iter()
            .find(|u| u["unit"] == unit && u["source"] == source)
            .unwrap();
        assert_eq!(u["total"], "100");
        assert_eq!(u["record_count"], 1);
    }
    assert_eq!(
        entry(&got, "tokens", "harness")["usage"]["status"],
        "unknown"
    );

    // Matching usage accumulates per exact (unit, source) only.
    assert_eq!(
        report(
            &env,
            &task,
            Some(&s),
            usage_body("u3", "USD", "invoice", json!("2.5"))
        )
        .await
        .0,
        201
    );
    let got = budget(&env, &task).await;
    let usd = entry(&got, "USD", "invoice");
    assert_eq!(usd["usage"]["total"], "2.5");
    assert_eq!(usd["over_threshold"], false);
    assert_eq!(usd["threshold"]["basis"], "alert_at");
    assert_eq!(usd["threshold"]["threshold_amount"], "5");
    assert_eq!(usd["threshold"]["used_total"], "2.5");
    assert_eq!(usd["threshold"]["unit"], "USD");
    assert_eq!(usd["threshold"]["source"], "invoice");

    assert_eq!(
        report(
            &env,
            &task,
            Some(&s),
            usage_body("u4", "USD", "invoice", json!("2.5"))
        )
        .await
        .0,
        201
    );
    let got = budget(&env, &task).await;
    let usd = entry(&got, "USD", "invoice");
    assert_eq!(usd["usage"]["total"], "5");
    assert_eq!(usd["usage"]["record_count"], 2);
    assert_eq!(
        usd["over_threshold"], true,
        "reaching alert_at raises the flag"
    );
    assert_eq!(
        usd["threshold"]["comparison"],
        "over_threshold = used_total >= threshold_amount"
    );
    assert_eq!(usd["threshold"]["used_total"], "5");
    assert_eq!(
        usd["usage"]["overlapping_periods"], true,
        "the two test records cover the same period"
    );
    assert_eq!(usd["threshold"]["total_may_double_count"], true);
    // The other ledgers did not move.
    assert_eq!(
        entry(&got, "tokens", "harness")["usage"]["status"],
        "unknown"
    );

    // Exact decimal arithmetic: 0.1 + 0.2 is 0.3, not 0.30000000000000004.
    assert_eq!(
        report(
            &env,
            &task,
            Some(&s),
            usage_body("d1", "x", "s", json!(0.1))
        )
        .await
        .0,
        201
    );
    assert_eq!(
        report(
            &env,
            &task,
            Some(&s),
            usage_body("d2", "x", "s", json!("0.2"))
        )
        .await
        .0,
        201
    );
    let got = budget(&env, &task).await;
    let x = entry(&got, "x", "s");
    assert_eq!(x["usage"]["total"], "0.3");
    assert_eq!(x["over_threshold"], true);

    // Filters on the record list are exact.
    let (_, list) = env
        .json(
            Method::GET,
            &env.url(&format!("/tasks/{task}/usage?unit=USD&source=invoice")),
            &env.owner_token,
            None,
            None,
        )
        .await;
    assert_eq!(list["items"].as_array().unwrap().len(), 2);
    let (_, list) = env
        .json(
            Method::GET,
            &env.url(&format!("/tasks/{task}/usage")),
            &env.owner_token,
            None,
            None,
        )
        .await;
    assert_eq!(list["items"].as_array().unwrap().len(), 6);
    assert_eq!(list["items"][0]["origin"], "user_content");
}

#[tokio::test]
async fn o08_crossing_a_threshold_stops_nothing() {
    let env = Env::new().await;
    let s = env.session(&env.owner_token, "A").await;
    let task = new_task(&env, &s).await;
    assert_eq!(
        put_budget(&env, &task, budget_body(0, "USD", "invoice", json!("1")))
            .await
            .0,
        201
    );
    assert_eq!(
        report(
            &env,
            &task,
            Some(&s),
            usage_body("big", "USD", "invoice", json!("999"))
        )
        .await
        .0,
        201
    );
    assert_eq!(
        entry(&budget(&env, &task).await, "USD", "invoice")["over_threshold"],
        true
    );

    // Work continues: the task and the session are untouched and still writable.
    let (status, t) = env
        .json(
            Method::GET,
            &env.url(&format!("/tasks/{task}")),
            &env.owner_token,
            None,
            None,
        )
        .await;
    assert_eq!(
        (status, t["status"].as_str(), t["version"].as_i64()),
        (200, Some("open"), Some(1))
    );
    let (status, _) = env
        .json(
            Method::PATCH,
            &env.url(&format!("/tasks/{task}")),
            &env.owner_token,
            Some(&s),
            Some(json!({"expected_version": 1, "status": "in_progress"})),
        )
        .await;
    assert_eq!(status, 200);
    let (status, _) = env
        .json(
            Method::POST,
            &env.url(&format!("/sessions/{}/heartbeat", s.id)),
            &env.owner_token,
            Some(&s),
            None,
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(
        report(
            &env,
            &task,
            Some(&s),
            usage_body("more", "USD", "invoice", json!("1"))
        )
        .await
        .0,
        201
    );
}

#[tokio::test]
async fn o08_validation_versions_and_idempotency() {
    let env = Env::new().await;
    let s = env.session(&env.owner_token, "A").await;
    let task = new_task(&env, &s).await;

    // unit and source are required; amounts are strict decimals.
    for (patch, why) in [
        (json!({"unit": ""}), "empty unit"),
        (json!({"source": ""}), "empty source"),
        (json!({"unit": "two words"}), "unit with a space"),
        (json!({"amount": "-1"}), "negative"),
        (json!({"amount": "1e3"}), "exponent"),
        (json!({"amount": "0"}), "zero budget"),
        (json!({"amount": "1.1234567"}), "too many fractional digits"),
        (json!({"amount": true}), "not a number"),
        (json!({"alert_at": "11"}), "alert above amount"),
        (json!({"sampling_period": ""}), "empty period"),
    ] {
        let mut body = budget_body(0, "USD", "invoice", json!("10"));
        body.as_object_mut()
            .unwrap()
            .extend(patch.as_object().unwrap().clone());
        let (status, resp) = put_budget(&env, &task, body).await;
        assert_eq!(status, 400, "{why}: {resp}");
    }
    for field in ["unit", "source", "sampling_period", "amount"] {
        let mut body = budget_body(0, "USD", "invoice", json!("10"));
        body.as_object_mut().unwrap().remove(field);
        let (status, _) = put_budget(&env, &task, body).await;
        assert!(
            status == 400 || status == 422,
            "missing {field} must be refused, got {status}"
        );
    }
    let (_, none) = (0, budget(&env, &task).await);
    assert_eq!(
        none["budgets"],
        json!([]),
        "refused requests create nothing"
    );

    // Versioned replace, with replay of the request that succeeded.
    let (status, v1) = put_budget(&env, &task, budget_body(0, "USD", "invoice", json!("10"))).await;
    assert_eq!((status, v1["version"].as_i64()), (201, Some(1)));
    let (status, replay) = put_budget(
        &env,
        &task,
        budget_body(0, "USD", "invoice", json!("10.00")),
    )
    .await;
    assert_eq!(status, 200, "{replay}");
    assert_eq!(replay["version"], 1, "10.00 normalizes to 10");
    let (status, conflict) =
        put_budget(&env, &task, budget_body(0, "USD", "invoice", json!("20"))).await;
    assert_eq!(
        (status, conflict["error"]["code"].as_str()),
        (409, Some("VERSION_CONFLICT")),
        "{conflict}"
    );
    assert_eq!(conflict["error"]["details"]["current_version"], 1);
    let (status, v2) = put_budget(&env, &task, budget_body(1, "USD", "invoice", json!("20"))).await;
    assert_eq!(
        (status, v2["version"].as_i64(), v2["amount"].as_str()),
        (200, Some(2), Some("20"))
    );
    let (status, conflict) =
        put_budget(&env, &task, budget_body(5, "USD", "invoice", json!("30"))).await;
    assert_eq!(
        (
            status,
            conflict["error"]["details"]["current_version"].as_i64()
        ),
        (409, Some(2))
    );
    let (status, conflict) =
        put_budget(&env, &task, budget_body(1, "JPY", "invoice", json!("30"))).await;
    assert_eq!(
        (
            status,
            conflict["error"]["details"]["current_version"].as_i64()
        ),
        (409, Some(0)),
        "no budget for that unit yet"
    );

    // At most 20 budgets per task.
    for i in 0..19 {
        let (status, resp) = put_budget(
            &env,
            &task,
            budget_body(0, &format!("u{i}"), "s", json!("1")),
        )
        .await;
        assert_eq!(status, 201, "{resp}");
    }
    let (status, resp) = put_budget(&env, &task, budget_body(0, "u19", "s", json!("1"))).await;
    assert_eq!(
        (status, resp["error"]["code"].as_str()),
        (409, Some("BUDGET_LIMIT_REACHED")),
        "{resp}"
    );

    // Usage: bad periods and amounts, replay, key reuse.
    let now = chrono::Utc::now();
    let mut backwards = usage_body("bw", "USD", "invoice", json!("1"));
    backwards["period_start"] = json!(now.to_rfc3339());
    backwards["period_end"] = json!((now - chrono::Duration::hours(1)).to_rfc3339());
    assert_eq!(report(&env, &task, Some(&s), backwards).await.0, 400);
    let mut future = usage_body("fu", "USD", "invoice", json!("1"));
    future["period_end"] = json!((now + chrono::Duration::hours(1)).to_rfc3339());
    assert_eq!(report(&env, &task, Some(&s), future).await.0, 400);
    assert_eq!(
        report(
            &env,
            &task,
            Some(&s),
            usage_body("neg", "USD", "invoice", json!("-5"))
        )
        .await
        .0,
        400
    );
    let mut no_unit = usage_body("nu", "USD", "invoice", json!("1"));
    no_unit.as_object_mut().unwrap().remove("unit");
    let status = report(&env, &task, Some(&s), no_unit).await.0;
    assert!(status == 400 || status == 422, "{status}");

    let body = usage_body("r1", "USD", "invoice", json!("3"));
    let (status, first) = report(&env, &task, Some(&s), body.clone()).await;
    assert_eq!(status, 201, "{first}");
    assert_eq!(first["reported_by_kind"], "session");
    assert_eq!(first["reported_by_id"], s.id.as_str());
    let (status, again) = report(&env, &task, Some(&s), body).await;
    assert_eq!(status, 200, "{again}");
    assert_eq!(again["id"], first["id"]);
    let (status, reused) = report(
        &env,
        &task,
        Some(&s),
        usage_body("r1", "USD", "invoice", json!("4")),
    )
    .await;
    assert_eq!(
        (status, reused["error"]["code"].as_str()),
        (409, Some("IDEMPOTENCY_KEY_REUSED")),
        "{reused}"
    );
    let got = budget(&env, &task).await;
    assert_eq!(entry(&got, "USD", "invoice")["usage"]["total"], "3");
    assert_eq!(entry(&got, "USD", "invoice")["usage"]["record_count"], 1);

    // The user door may report too; the record says so.
    let (status, by_user) = report(
        &env,
        &task,
        None,
        usage_body("ud", "USD", "invoice", json!("1")),
    )
    .await;
    assert_eq!(status, 201, "{by_user}");
    assert_eq!(by_user["reported_by_kind"], "user");
}

#[tokio::test]
async fn o08_access_races_and_unknown_tasks() {
    let env = Env::new().await;
    let s = env.session(&env.owner_token, "A").await;
    let task = new_task(&env, &s).await;

    let (reader, reader_id) = env.user("reader").await;
    env.add_collaborator(reader_id, "read").await;
    let (stranger, _) = env.user("stranger").await;
    assert_eq!(
        put_budget(&env, &task, budget_body(0, "USD", "invoice", json!("10")))
            .await
            .0,
        201
    );

    let get = env.url(&format!("/tasks/{task}/budget"));
    assert_eq!(
        env.json(Method::GET, &get, &reader, None, None).await.0,
        200
    );
    assert_eq!(
        env.json(Method::GET, &get, &stranger, None, None).await.0,
        403
    );
    let usage = env.url(&format!("/tasks/{task}/usage"));
    assert_eq!(
        env.json(Method::GET, &usage, &stranger, None, None).await.0,
        403
    );
    for (method, url, body) in [
        (
            Method::PUT,
            env.url(&format!("/tasks/{task}/budgets")),
            budget_body(1, "USD", "invoice", json!("99")),
        ),
        (
            Method::POST,
            usage.clone(),
            usage_body("x", "USD", "invoice", json!("1")),
        ),
    ] {
        assert_eq!(
            env.json(method.clone(), &url, &reader, None, Some(body.clone()))
                .await
                .0,
            403,
            "reader {method}"
        );
        assert_eq!(
            env.json(method, &url, &stranger, None, Some(body)).await.0,
            403,
            "stranger"
        );
    }
    let got = budget(&env, &task).await;
    assert_eq!(
        entry(&got, "USD", "invoice")["amount"],
        "10",
        "refused writes changed nothing"
    );
    assert!(got["unmatched_usage"].as_array().unwrap().is_empty());

    // Unknown and foreign tasks read as absent.
    let ghost = uuid::Uuid::new_v4().to_string();
    for (method, path) in [
        (Method::GET, format!("/tasks/{ghost}/budget")),
        (Method::GET, format!("/tasks/{ghost}/usage")),
    ] {
        assert_eq!(
            env.json(method, &env.url(&path), &env.owner_token, None, None)
                .await
                .0,
            404
        );
    }
    assert_eq!(
        report(
            &env,
            &ghost,
            Some(&s),
            usage_body("g", "USD", "invoice", json!("1"))
        )
        .await
        .0,
        404
    );
    assert_eq!(
        put_budget(&env, &ghost, budget_body(0, "USD", "invoice", json!("1")))
            .await
            .0,
        404
    );

    // Six budget writers from version 1: one winner, the rest conflict.
    let mut handles = Vec::new();
    for i in 0..6 {
        let url = env.url(&format!("/tasks/{task}/budgets"));
        let token = env.owner_token.clone();
        handles.push(tokio::spawn(async move {
            reqwest::Client::new()
                .put(url)
                .bearer_auth(token)
                .json(&budget_body(
                    1,
                    "USD",
                    "invoice",
                    json!(format!("{}", 20 + i)),
                ))
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
    assert_eq!(
        statuses.iter().filter(|s| **s == 200).count(),
        1,
        "{statuses:?}"
    );
    assert_eq!(
        statuses.iter().filter(|s| **s == 409).count(),
        5,
        "{statuses:?}"
    );
    assert_eq!(
        entry(&budget(&env, &task).await, "USD", "invoice")["version"],
        2
    );

    // Six identical usage reports: one record.
    let body = usage_body("same", "USD", "invoice", json!("1"));
    let mut handles = Vec::new();
    for _ in 0..6 {
        let url = usage.clone();
        let token = env.owner_token.clone();
        let s = s.clone();
        let body = body.clone();
        handles.push(tokio::spawn(async move {
            reqwest::Client::new()
                .post(url)
                .bearer_auth(token)
                .header(collab_common::HDR_SESSION, &s.id)
                .header(collab_common::HDR_SECRET, &s.secret)
                .json(&body)
                .send()
                .await
                .unwrap()
                .status()
                .as_u16()
        }));
    }
    let mut created = 0;
    for h in handles {
        let status = h.await.unwrap();
        assert!(status == 201 || status == 200, "{status}");
        created += (status == 201) as u32;
    }
    assert_eq!(created, 1);
    assert_eq!(
        entry(&budget(&env, &task).await, "USD", "invoice")["usage"]["record_count"],
        1
    );
}
