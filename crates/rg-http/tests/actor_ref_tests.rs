//! T26: API rows say whether an account is a person or an agent, and for an
//! agent whose it is, without a second request to `/agents`.
//!
//! Four places carry it, each as a NEW sibling key so nothing existing changes:
//! PR detail and list (`author_ref`), the review list (`reviewer_ref`), issue
//! comments (`author_ref`) and `/admin/audit/logs` rows (`user_ref`).
//! SQLite only (no new SQL beyond one `IN (...)` lookup).

mod common;
use common::{register_full, register_user, spawn_test_app_with_db};

use reqwest::{Client, Method, StatusCode};
use sea_orm::{ActiveValue, Set};
use serde_json::{json, Value};

const PASSWORD: &str = "Qz7$wRtm";

/// The permission cache is process-wide and keyed by ids, which repeat across
/// the per-test databases: one test at a time.
static ONE_AT_A_TIME: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

struct World {
    _exclusive: tokio::sync::MutexGuard<'static, ()>,
    base: String,
    db: rg_db::DatabaseConnection,
    admin: String,
    alice: String,
    alice_id: i64,
    agent: String,
    agent_id: i64,
}

async fn call(
    w: &World,
    method: Method,
    path: &str,
    auth: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut req = Client::new()
        .request(method, format!("{}/api/v1{path}", w.base))
        .bearer_auth(auth);
    if let Some(body) = body {
        req = req.json(&body);
    }
    let resp = req.send().await.unwrap();
    let status = resp.status();
    (status, resp.json().await.unwrap_or(Value::Null))
}

/// root_admin (administrator), alice (person, owns `alice/r`) and her agent
/// `alice-agent-ci`, which has write access to the repository.
async fn world() -> World {
    let exclusive = ONE_AT_A_TIME.lock().await;
    rg_core::repo::service::invalidate_perm_cache_all();
    let (base, db) = spawn_test_app_with_db().await;
    let admin = register_user(&base, "root_admin", "root_admin@example.com", PASSWORD).await;
    let (alice, alice_id) = register_full(&base, "alice", "alice@example.com").await;
    let mut w = World {
        _exclusive: exclusive,
        base,
        db,
        admin,
        alice,
        alice_id,
        agent: String::new(),
        agent_id: 0,
    };
    let (status, created) = call(
        &w,
        Method::POST,
        "/agents",
        &w.alice,
        Some(json!({"name": "ci"})),
    )
    .await;
    assert_eq!(status, 201, "{created}");
    let agent_name = created["username"].as_str().unwrap().to_string();
    w.agent_id = created["id"].as_i64().unwrap();
    let (status, token) = call(
        &w,
        Method::POST,
        &format!("/agents/{agent_name}/tokens"),
        &w.alice,
        Some(json!({"name": "t"})),
    )
    .await;
    assert_eq!(status, 201, "{token}");
    w.agent = token["token"].as_str().unwrap().to_string();
    let (status, repo) = call(
        &w,
        Method::POST,
        "/repos",
        &w.alice,
        Some(json!({"name": "r", "auto_init": true})),
    )
    .await;
    assert_eq!(status, 201, "{repo}");
    let (status, grant) = call(
        &w,
        Method::POST,
        "/repos/alice/r/collaborators",
        &w.alice,
        Some(json!({"username": agent_name, "permission": "write"})),
    )
    .await;
    assert!(status.is_success(), "{grant}");
    w
}

/// Commit a file to a new branch and open a PR from it; returns the number.
async fn open_pr(w: &World, auth: &str, branch: &str) -> i64 {
    let (status, wrote) = call(
        w,
        Method::POST,
        &format!("/repos/alice/r/contents/{branch}.txt"),
        auth,
        Some(json!({"content": "aGk=", "message": "seed", "branch": branch})),
    )
    .await;
    assert!(status.is_success(), "commit on {branch}: {wrote}");
    let (status, pr) = call(
        w,
        Method::POST,
        "/repos/alice/r/pulls",
        auth,
        Some(json!({"title": branch, "head": branch, "base": "main"})),
    )
    .await;
    assert_eq!(status, 201, "open pr: {pr}");
    pr["number"].as_i64().unwrap()
}

fn human(id: i64, name: &str) -> Value {
    json!({"id": id, "username": name, "kind": "human", "owner_id": null, "owner_username": null})
}

fn agent_of_alice(w: &World) -> Value {
    json!({
        "id": w.agent_id, "username": "alice-agent-ci", "kind": "agent",
        "owner_id": w.alice_id, "owner_username": "alice",
    })
}

#[tokio::test]
async fn a_pull_request_names_whether_a_person_or_an_agent_opened_it() {
    let w = world().await;
    let by_alice = open_pr(&w, &w.alice, "from-alice").await;
    let by_agent = open_pr(&w, &w.agent, "from-agent").await;

    let (status, detail) = call(
        &w,
        Method::GET,
        &format!("/repos/alice/r/pulls/{by_alice}"),
        &w.alice,
        None,
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(detail["author_ref"], human(w.alice_id, "alice"));
    // What was there is still there, and no existing key changed meaning.
    assert_eq!(detail["author_id"], w.alice_id);
    assert_eq!(detail["title"], "from-alice");
    assert!(
        detail.get("author").is_none(),
        "`author` is a string on the client; not reused"
    );

    let (_, detail) = call(
        &w,
        Method::GET,
        &format!("/repos/alice/r/pulls/{by_agent}"),
        &w.alice,
        None,
    )
    .await;
    assert_eq!(detail["author_ref"], agent_of_alice(&w));
    assert_eq!(detail["author_id"], w.agent_id);
}

#[tokio::test]
async fn the_pull_request_list_resolves_every_author_at_once() {
    let w = world().await;
    let by_alice = open_pr(&w, &w.alice, "from-alice").await;
    let by_agent = open_pr(&w, &w.agent, "from-agent").await;

    let (status, list) = call(
        &w,
        Method::GET,
        "/repos/alice/r/pulls?state=open",
        &w.alice,
        None,
    )
    .await;
    assert_eq!(status, 200);
    let rows = list["data"].as_array().unwrap();
    assert_eq!(rows.len(), 2, "{list}");
    assert!(
        list["pagination"]["total"].is_number(),
        "paging envelope unchanged"
    );
    for row in rows {
        let (want_id, want) = if row["number"] == by_alice {
            (w.alice_id, human(w.alice_id, "alice"))
        } else {
            assert_eq!(row["number"], by_agent);
            (w.agent_id, agent_of_alice(&w))
        };
        assert_eq!(row["author_id"], want_id);
        assert_eq!(row["author_ref"], want);
    }
}

#[tokio::test]
async fn a_pull_request_whose_author_account_is_gone_has_a_null_ref_not_an_error() {
    let w = world().await;
    let by_agent = open_pr(&w, &w.agent, "from-agent").await;
    // Owner removed behind the agent's back (the API refuses this; the row can
    // still vanish through the database).
    rg_db::ops::user_ops::delete_by_id(&w.db, w.agent_id)
        .await
        .unwrap();

    let (status, detail) = call(
        &w,
        Method::GET,
        &format!("/repos/alice/r/pulls/{by_agent}"),
        &w.alice,
        None,
    )
    .await;
    assert_eq!(status, 200, "{detail}");
    assert_eq!(detail["author_id"], w.agent_id);
    assert_eq!(
        detail.get("author_ref"),
        Some(&Value::Null),
        "present, and null"
    );
}

#[tokio::test]
async fn the_review_list_says_who_reviewed_person_or_agent() {
    let w = world().await;
    let pr = open_pr(&w, &w.alice, "feature").await;
    for (auth, body) in [(&w.alice, "by a person"), (&w.agent, "by an agent")] {
        let (status, review) = call(
            &w,
            Method::POST,
            &format!("/repos/alice/r/pulls/{pr}/reviews"),
            auth,
            Some(json!({"action": "comment", "body": body})),
        )
        .await;
        assert_eq!(status, 201, "{review}");
    }

    let (status, reviews) = call(
        &w,
        Method::GET,
        &format!("/repos/alice/r/pulls/{pr}/reviews"),
        &w.alice,
        None,
    )
    .await;
    assert_eq!(status, 200);
    let reviews = reviews.as_array().expect("still a bare array");
    assert_eq!(reviews.len(), 2);
    for review in reviews {
        assert!(review["reviewer_id"].is_number(), "existing field kept");
        let want = if review["reviewer_id"] == w.alice_id {
            human(w.alice_id, "alice")
        } else {
            agent_of_alice(&w)
        };
        assert_eq!(review["reviewer_ref"], want);
    }
}

#[tokio::test]
async fn issue_comments_say_who_wrote_them_in_the_list_and_when_created() {
    let w = world().await;
    let (_, issue_number) = common::create_issue(&w.base, &w.alice, "alice", "r", "bug").await;
    let path = format!("/repos/alice/r/issues/{issue_number}/comments");
    let mut created = Vec::new();
    for (auth, body) in [(&w.alice, "mine"), (&w.agent, "the agent's")] {
        let (status, comment) =
            call(&w, Method::POST, &path, auth, Some(json!({"body": body}))).await;
        assert_eq!(status, 201, "{comment}");
        created.push(comment);
    }
    // The create response carries it too.
    assert_eq!(created[0]["author_ref"], human(w.alice_id, "alice"));
    assert_eq!(created[1]["author_ref"], agent_of_alice(&w));

    let (status, list) = call(&w, Method::GET, &path, &w.alice, None).await;
    assert_eq!(status, 200);
    let rows = list
        .as_array()
        .or_else(|| list["data"].as_array())
        .unwrap_or_else(|| panic!("{list}"));
    assert_eq!(rows.len(), 2);
    for row in rows {
        // `author` stays the plain username string the client already reads.
        let name = row["author"].as_str().expect("author unchanged");
        assert_eq!(row["author_ref"]["username"], name);
        let kind = if name == "alice" { "human" } else { "agent" };
        assert_eq!(row["author_ref"]["kind"], kind);
        if kind == "agent" {
            assert_eq!(row["author_ref"]["owner_username"], "alice");
            assert_eq!(row["author_ref"]["owner_id"], w.alice_id);
        }
    }
}

async fn seed_audit(w: &World, user_id: Option<i64>, action: &str) -> i64 {
    rg_db::ops::audit_log_ops::insert(
        &w.db,
        rg_db::entities::audit_log::ActiveModel {
            id: ActiveValue::NotSet,
            user_id: Set(user_id),
            username: Set(Some("recorded-name".into())),
            on_behalf_of_user_id: Set(None),
            action: Set(action.into()),
            resource_type: Set(None),
            resource_id: Set(None),
            resource_name: Set(None),
            ip_address: Set(None),
            user_agent: Set(None),
            details: Set(None),
            created_at: Set(chrono::Utc::now()),
        },
    )
    .await
    .unwrap()
    .id
}

#[tokio::test]
async fn audit_rows_carry_the_live_identity_beside_the_recorded_one() {
    let w = world().await;
    let of_person = seed_audit(&w, Some(w.alice_id), "t26.person").await;
    let of_agent = seed_audit(&w, Some(w.agent_id), "t26.agent").await;
    let of_nobody = seed_audit(&w, None, "t26.system").await;
    let of_deleted = seed_audit(&w, Some(987_654), "t26.deleted").await;

    let (status, list) = call(
        &w,
        Method::GET,
        "/admin/audit/logs?per_page=100",
        &w.admin,
        None,
    )
    .await;
    assert_eq!(status, 200, "{list}");
    let row = |id: i64| -> &Value {
        list["logs"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["id"] == id)
            .unwrap()
    };
    assert_eq!(row(of_person)["user_ref"], human(w.alice_id, "alice"));
    assert_eq!(row(of_agent)["user_ref"], agent_of_alice(&w));
    assert!(row(of_nobody)["user_ref"].is_null());
    assert!(row(of_deleted)["user_ref"].is_null());
    // Existing fields untouched: id, the name recorded at write time, on-behalf-of.
    assert_eq!(row(of_agent)["user_id"], w.agent_id);
    assert_eq!(row(of_agent)["username"], "recorded-name");
    assert!(row(of_agent).get("on_behalf_of_user_id").is_some());

    // Same on the single-row endpoint.
    let (status, one) = call(
        &w,
        Method::GET,
        &format!("/admin/audit/logs/{of_agent}"),
        &w.admin,
        None,
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(one["user_ref"], agent_of_alice(&w));
}

#[tokio::test]
async fn the_capability_flag_is_advertised() {
    let w = world().await;
    let (status, caps) = call(
        &w,
        Method::GET,
        "/collaboration/capabilities",
        &w.admin,
        None,
    )
    .await;
    assert_eq!(status, 200, "{caps}");
    let flags: Vec<&str> = caps["api_capabilities"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert!(flags.contains(&"actors.ref.v1"), "{flags:?}");
}

#[tokio::test]
async fn the_openapi_document_describes_the_audit_user_ref() {
    let doc: Value = serde_json::from_str(&rg_http::openapi::openapi_spec()).unwrap();
    let schemas = &doc["components"]["schemas"];
    for field in ["id", "username", "kind", "owner_id", "owner_username"] {
        assert!(
            schemas["ActorRefSchema"]["properties"].get(field).is_some(),
            "ActorRefSchema.{field}"
        );
    }
    assert!(schemas["AuditLogEntry"]["properties"]
        .get("user_ref")
        .is_some());
}
