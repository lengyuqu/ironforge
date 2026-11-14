//! The push path must leave a secret-scan audit trail (T04's gap, taken over by
//! T05).
//!
//! `rg-git` scans a pushed pack before the refs move but has no database, so it
//! reports through a sink. `rg-http`'s post-push hook now collects those events
//! for the receive-pack task that produced them and writes `audit_log` rows:
//! `repo.secret_scan_rejected` / `_findings` / `_skipped`.
//!
//! This test does a real `git push` over HTTP as an agent with a blocking secret
//! in the pack and asserts the audit row lands on the right repository, carries
//! the rule and path, and never carries the matched text.

mod common;

use reqwest::Client;
use rg_db::sea_orm::EntityTrait;
use serde_json::{json, Value};

const PASSWORD: &str = "Qz7$wRtm";

fn git(args: &[&str], dir: Option<&std::path::Path>) -> rg_git::cli_gateway::GitOutput {
    rg_git::cli_gateway::global_gateway()
        .as_ref()
        .unwrap()
        .run(args, dir)
        .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_rejected_push_writes_a_secret_scan_audit_row() {
    let (base, db) = common::spawn_test_app_with_db().await;
    let client = Client::new();

    // The first account is the administrator; it owns the repository.
    let owner =
        common::register_user(&base, "scan_owner", "scan_owner@example.com", PASSWORD).await;
    let me: Value = client
        .get(format!("{base}/api/v1/users/me"))
        .bearer_auth(&owner)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let owner_id = me["id"].as_i64().expect("owner id");

    let created: Value = client
        .post(format!("{base}/api/v1/repos"))
        .bearer_auth(&owner)
        .json(&json!({ "name": "scan", "auto_init": true, "is_private": true }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(created["name"], "scan");
    let repo_id = rg_db::ops::repo_ops::find_by_owner_and_name(&db, owner_id, "scan")
        .await
        .unwrap()
        .expect("repository row")
        .id;

    // An agent of that owner, with a repo-scoped token and write access.
    let agent: Value = client
        .post(format!("{base}/api/v1/agents"))
        .bearer_auth(&owner)
        .json(&json!({ "name": "scanner" }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let agent_name = agent["username"].as_str().unwrap().to_string();
    let token: Value = client
        .post(format!("{base}/api/v1/agents/{agent_name}/tokens"))
        .bearer_auth(&owner)
        .json(&json!({ "name": "push" }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let agent_token = token["token"].as_str().unwrap().to_string();
    let granted = client
        .post(format!("{base}/api/v1/repos/scan_owner/scan/collaborators"))
        .bearer_auth(&owner)
        .json(&json!({ "username": agent_name, "permission": "write" }))
        .send()
        .await
        .unwrap();
    assert!(granted.status().is_success());

    // A real git push with a blocking cloud access key in a normal source file.
    let authenticated = base.replacen("http://", &format!("http://{agent_name}:{agent_token}@"), 1);
    let remote = format!("{authenticated}/scan_owner/scan.git");
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path();
    git(&["clone", &remote, path.to_string_lossy().as_ref()], None);
    git(&["config", "user.name", "Agent"], Some(path));
    git(&["config", "user.email", "agent@example.org"], Some(path));
    git(&["config", "commit.gpgsign", "false"], Some(path));
    std::fs::write(
        path.join("config.rs"),
        "pub const KEY: &str = \"AKIAIOSFODNN7EXAMPLE\";\n",
    )
    .unwrap();
    git(&["add", "."], Some(path));
    git(&["commit", "-m", "add key"], Some(path));
    let pushed = git(&["push", "origin", "HEAD:main"], Some(path));
    assert!(!pushed.success(), "a blocking secret must refuse the push");

    // The audit row is written by the spawned post-push hook.
    let mut row = None;
    for _ in 0..100 {
        let rows = rg_db::entities::audit_log::Entity::find()
            .all(&db)
            .await
            .unwrap();
        row = rows.into_iter().find(|row| {
            row.action == "repo.secret_scan_rejected" && row.resource_id == Some(repo_id)
        });
        if row.is_some() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    let row = row.expect("a rejected push must write repo.secret_scan_rejected");

    assert_eq!(row.resource_type.as_deref(), Some("repo"));
    assert_eq!(row.resource_name.as_deref(), Some("scan_owner/scan"));
    let details: Value =
        serde_json::from_str(row.details.as_deref().expect("details")).expect("details json");
    assert_eq!(details["via"], "git_push");
    assert_eq!(details["path"], "config.rs");
    assert_eq!(details["rule"], "cloud_access_key");
    assert_eq!(details["ref"], "refs/heads/main");
    let raw = row.details.clone().unwrap();
    assert!(
        !raw.contains("AKIAIOSFODNN7EXAMPLE"),
        "audit details must never carry the matched text: {raw}"
    );
}
