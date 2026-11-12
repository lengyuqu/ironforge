//! `audit_log.on_behalf_of_user_id`: which person an agent acted for.
//!
//! - an agent's write records the agent in `user_id` and its owner in
//!   `on_behalf_of_user_id`; a person's own write leaves the new column NULL;
//! - `GET /admin/audit/logs` can filter by the owner and returns the field;
//! - the row outlives both accounts (the column is not a foreign key).
//!
//! One test function on purpose: the first registered account is the
//! administrator, and `IRONFORGE_TEST_DATABASE_URL` shares one database between
//! the tests of a target. Run it on a fresh database for PostgreSQL / MySQL.

mod common;
use common::{register_full, register_user, spawn_test_app_on_env_db};

use reqwest::Client;
use rg_db::entities::audit_log;
use rg_db::sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use serde_json::{json, Value};

const PASSWORD: &str = "Qz7$wRtm";

#[tokio::test]
async fn an_agents_audit_rows_name_the_person_it_acted_for() {
    let (base, db) = spawn_test_app_on_env_db().await;
    let api = |path: &str| format!("{base}/api/v1{path}");
    let client = Client::new();

    let admin = register_user(&base, "obo_admin", "obo_admin@example.com", PASSWORD).await;
    let (alice, alice_id) = register_full(&base, "obo_alice", "obo_alice@example.com").await;

    let resp = client
        .post(api("/repos"))
        .bearer_auth(&alice)
        .json(&json!({ "name": "work", "is_private": true, "auto_init": true }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);

    // alice's agent, with write access to her repository.
    let resp = client
        .post(api("/agents"))
        .bearer_auth(&alice)
        .json(&json!({ "name": "ci" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);
    let created: Value = resp.json().await.unwrap();
    let agent = created["username"].as_str().unwrap().to_string();
    let agent_id = created["id"].as_i64().unwrap();
    let resp = client
        .post(api(&format!("/agents/{agent}/tokens")))
        .bearer_auth(&alice)
        .json(&json!({ "name": "t" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);
    let token = resp.json::<Value>().await.unwrap()["token"]
        .as_str()
        .unwrap()
        .to_string();
    let resp = client
        .post(api("/repos/obo_alice/work/collaborators"))
        .bearer_auth(&alice)
        .json(&json!({ "username": agent, "permission": "write" }))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success(), "{}", resp.status());

    let commit = |auth: String, path: &'static str| {
        let client = client.clone();
        let url = api(&format!("/repos/obo_alice/work/contents/{path}"));
        async move {
            let resp = client
                .post(url)
                .bearer_auth(auth)
                .json(&json!({ "content": "x\n", "message": "add" }))
                .send()
                .await
                .unwrap();
            assert!(resp.status().is_success(), "{path}: {}", resp.status());
        }
    };
    commit(token.clone(), "by-agent.txt").await;
    commit(alice.clone(), "by-person.txt").await;

    // Agent write: both columns. Person write: the new column is NULL.
    let rows = audit_log::Entity::find()
        .filter(audit_log::Column::Action.eq("repo.file_commit"))
        .all(&db)
        .await
        .unwrap();
    let by_agent = rows
        .iter()
        .find(|r| r.user_id == Some(agent_id))
        .unwrap_or_else(|| panic!("no agent row in {rows:?}"));
    assert_eq!(by_agent.username.as_deref(), Some(agent.as_str()));
    assert_eq!(
        by_agent.on_behalf_of_user_id,
        Some(alice_id),
        "{by_agent:?}"
    );
    let by_person = rows
        .iter()
        .find(|r| r.user_id == Some(alice_id))
        .unwrap_or_else(|| panic!("no person row in {rows:?}"));
    assert_eq!(by_person.on_behalf_of_user_id, None, "{by_person:?}");
    // The owner is a column, not something stuffed into details.
    assert!(!by_agent
        .details
        .clone()
        .unwrap_or_default()
        .contains("on_behalf"));

    // The admin API returns the field and filters on it.
    let list = |query: String| {
        let client = client.clone();
        let url = api(&format!("/admin/audit/logs?{query}"));
        let admin = admin.clone();
        async move {
            let resp = client.get(url).bearer_auth(admin).send().await.unwrap();
            assert_eq!(resp.status(), 200);
            resp.json::<Value>().await.unwrap()
        }
    };
    let by_owner = list(format!("on_behalf_of_user_id={alice_id}&per_page=100")).await;
    let logs = by_owner["logs"].as_array().unwrap();
    assert!(!logs.is_empty());
    for entry in logs {
        assert_eq!(entry["on_behalf_of_user_id"], json!(alice_id), "{entry}");
        assert_eq!(entry["user_id"], json!(agent_id), "{entry}");
    }
    let everything = list("per_page=100".to_string()).await;
    let person_entry = everything["logs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["id"] == json!(by_person.id))
        .unwrap();
    assert!(
        person_entry["on_behalf_of_user_id"].is_null(),
        "{person_entry}"
    );
    // Existing filters still work next to the new one.
    let agent_only = list(format!("user_id={agent_id}&per_page=100")).await;
    assert!(agent_only["logs"]
        .as_array()
        .unwrap()
        .iter()
        .all(|e| e["user_id"] == json!(agent_id)));
    let nobody = list("on_behalf_of_user_id=999999999".to_string()).await;
    assert_eq!(nobody["total"], 0);
    // A single entry carries it too.
    let one: Value = client
        .get(api(&format!("/admin/audit/logs/{}", by_agent.id)))
        .bearer_auth(&admin)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(one["on_behalf_of_user_id"], json!(alice_id));

    // Delete the agent, then its owner: the row stays, readable, with the ids.
    rg_db::ops::user_ops::delete_by_id(&db, agent_id)
        .await
        .expect("delete agent");
    rg_db::ops::user_ops::delete_by_id(&db, alice_id)
        .await
        .expect("delete owner");
    let after = audit_log::Entity::find_by_id(by_agent.id)
        .one(&db)
        .await
        .unwrap()
        .expect("audit row survives both accounts");
    assert_eq!(after.user_id, Some(agent_id));
    assert_eq!(after.on_behalf_of_user_id, Some(alice_id));
    let one: Value = client
        .get(api(&format!("/admin/audit/logs/{}", by_agent.id)))
        .bearer_auth(&admin)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(one["on_behalf_of_user_id"], json!(alice_id));
    assert_eq!(one["username"], json!(agent));
}
