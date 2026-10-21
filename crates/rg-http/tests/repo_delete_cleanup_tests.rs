//! Deleting a repository must free its on-disk path.
//!
//! `delete_repo` used to soft-delete only the row, leaving `<owner>/<name>.git`
//! in place. Creating the same name again then failed (`gix init` refuses an
//! existing directory) with a 400 that also leaked the server path; a reused
//! directory would have handed the old repository's history to the new one.
//!
//! These tests pin the whole lifecycle: delete moves the data to a tombstone,
//! the name is reusable and starts empty, legacy leftovers are quarantined
//! rather than deleted, and the purge pass removes tombstones after retention.

mod common;

use chrono::Duration;
use common::{build_test_app_state, register_full, setup_test_db};
use std::path::PathBuf;

struct App {
    base: String,
    db: rg_db::DatabaseConnection,
    repo_root: PathBuf,
}

async fn spawn() -> App {
    let (db, dir) = setup_test_db().await;
    let repo_root = dir.path().join("repos");
    std::fs::create_dir_all(&repo_root).unwrap();
    let state = build_test_app_state(db.clone(), repo_root.clone());
    let app = rg_http::create_router_for_test(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        let _dir = dir;
        axum::serve(listener, app).await.unwrap();
    });
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    App { base, db, repo_root }
}

async fn create(app: &App, token: &str, name: &str, auto_init: bool) -> (reqwest::StatusCode, serde_json::Value) {
    let response = reqwest::Client::new()
        .post(format!("{}/api/v1/repos", app.base))
        .bearer_auth(token)
        .json(&serde_json::json!({
            "name": name,
            "is_private": true,
            "auto_init": auto_init,
            "readme": "default"
        }))
        .send()
        .await
        .unwrap();
    let status = response.status();
    (status, response.json().await.unwrap_or_default())
}

async fn create_public(app: &App, token: &str, name: &str) -> i64 {
    let response = reqwest::Client::new()
        .post(format!("{}/api/v1/repos", app.base))
        .bearer_auth(token)
        .json(&serde_json::json!({
            "name": name, "is_private": false, "auto_init": true, "readme": "default"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 201);
    response.json::<serde_json::Value>().await.unwrap()["id"].as_i64().unwrap()
}

async fn delete(app: &App, token: &str, owner: &str, name: &str) -> reqwest::StatusCode {
    reqwest::Client::new()
        .delete(format!("{}/api/v1/repos/{owner}/{name}", app.base))
        .bearer_auth(token)
        .send()
        .await
        .unwrap()
        .status()
}

async fn branch_count(app: &App, token: &str, owner: &str, name: &str) -> usize {
    let body: serde_json::Value = reqwest::Client::new()
        .get(format!("{}/api/v1/repos/{owner}/{name}/branches", app.base))
        .bearer_auth(token)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    body.as_array()
        .or_else(|| body.get("data").and_then(|d| d.as_array()))
        .map(|a| a.len())
        .unwrap_or_else(|| panic!("unexpected branches payload: {body}"))
}

fn live(app: &App, owner: &str, name: &str) -> PathBuf {
    app.repo_root.join(format!("{owner}/{name}.git"))
}

fn tombstone(app: &App, owner: &str, name: &str, id: i64) -> PathBuf {
    rg_core::repo::service::tombstone_path(&app.repo_root, owner, name, id)
}

#[tokio::test]
async fn delete_frees_the_path_and_the_name_starts_empty() {
    let app = spawn().await;
    let (token, _) = register_full(&app.base, "del-owner", "del-owner@example.com").await;

    let (status, body) = create(&app, &token, "proj", true).await;
    assert_eq!(status, 201, "{body}");
    let id = body["id"].as_i64().unwrap();
    assert!(live(&app, "del-owner", "proj").join("HEAD").exists());
    assert_eq!(branch_count(&app, &token, "del-owner", "proj").await, 1);

    assert_eq!(delete(&app, &token, "del-owner", "proj").await, 200);

    // The live path is free; the data is preserved in the tombstone.
    assert!(
        !live(&app, "del-owner", "proj").exists(),
        "delete must free <owner>/<name>.git"
    );
    assert!(
        tombstone(&app, "del-owner", "proj", id).join("HEAD").exists(),
        "the deleted repository's data must be kept in a tombstone"
    );

    // The name is reusable (this was a 400 `gix init --bare failed`), and the
    // new repository must NOT inherit the old one's history.
    let (status, body) = create(&app, &token, "proj", false).await;
    assert_eq!(status, 201, "recreating a deleted name must succeed: {body}");
    assert_ne!(body["id"].as_i64().unwrap(), id);
    assert_eq!(
        branch_count(&app, &token, "del-owner", "proj").await,
        0,
        "a recreated repository must start empty, not inherit the old history"
    );
    // ...and the tombstone of the first one is untouched by the recreation.
    assert!(tombstone(&app, "del-owner", "proj", id).join("HEAD").exists());
}

#[tokio::test]
async fn leftover_directory_from_an_old_delete_is_quarantined_not_removed() {
    let app = spawn().await;
    let (token, _) = register_full(&app.base, "legacy-owner", "legacy-owner@example.com").await;

    let (status, body) = create(&app, &token, "legacy", true).await;
    assert_eq!(status, 201, "{body}");
    let id = body["id"].as_i64().unwrap();

    // Reproduce the pre-fix behaviour: row soft-deleted, directory left alone.
    rg_db::ops::repo_ops::soft_delete(&app.db, id).await.unwrap();
    assert!(live(&app, "legacy-owner", "legacy").exists());

    let (status, body) = create(&app, &token, "legacy", false).await;
    assert_eq!(status, 201, "a leftover directory must not block creation: {body}");
    assert_eq!(branch_count(&app, &token, "legacy-owner", "legacy").await, 0);

    assert!(
        tombstone(&app, "legacy-owner", "legacy", id).join("HEAD").exists(),
        "the leftover data must be moved aside, not deleted"
    );
}

#[tokio::test]
async fn purge_removes_only_expired_tombstones_and_is_idempotent() {
    let app = spawn().await;
    let (token, _) = register_full(&app.base, "purge-owner", "purge-owner@example.com").await;

    let (_, body) = create(&app, &token, "doomed", true).await;
    let id = body["id"].as_i64().unwrap();
    assert_eq!(delete(&app, &token, "purge-owner", "doomed").await, 200);
    let dead = tombstone(&app, "purge-owner", "doomed", id);
    assert!(dead.exists());

    // Inside the retention window nothing is removed.
    let kept =
        rg_core::repo::service::purge_deleted_repositories(&app.db, &app.repo_root, Duration::days(30))
            .await
            .unwrap();
    assert_eq!(kept, 0);
    assert!(dead.exists(), "data inside the retention window must be kept");

    // A live repository is never touched by a purge.
    let (_, live_body) = create(&app, &token, "survivor", true).await;
    assert!(live_body["id"].as_i64().is_some());

    let purged =
        rg_core::repo::service::purge_deleted_repositories(&app.db, &app.repo_root, Duration::zero())
            .await
            .unwrap();
    assert_eq!(purged, 1);
    assert!(!dead.exists(), "expired tombstone must be removed");
    assert!(live(&app, "purge-owner", "survivor").join("HEAD").exists());

    let again =
        rg_core::repo::service::purge_deleted_repositories(&app.db, &app.repo_root, Duration::zero())
            .await
            .unwrap();
    assert_eq!(again, 0, "purge must be idempotent");
}

#[tokio::test]
async fn zero_retention_removes_the_data_with_the_delete() {
    let app = spawn().await;
    let (token, owner_id) = register_full(&app.base, "zero-owner", "zero-owner@example.com").await;
    let (_, body) = create(&app, &token, "gone", true).await;
    let id = body["id"].as_i64().unwrap();

    rg_core::repo::service::delete_repo_with_retention(
        &app.db,
        &app.repo_root,
        id,
        Duration::zero(),
    )
    .await
    .unwrap();

    let _ = owner_id;
    assert!(!live(&app, "zero-owner", "gone").exists());
    assert!(
        !tombstone(&app, "zero-owner", "gone", id).exists(),
        "zero retention must not leave a tombstone"
    );
    let (status, _) = create(&app, &token, "gone", false).await;
    assert_eq!(status, 201);
}

async fn fork(app: &App, token: &str, owner: &str, name: &str) -> (reqwest::StatusCode, serde_json::Value) {
    let response = reqwest::Client::new()
        .post(format!("{}/api/v1/repos/{owner}/{name}/fork", app.base))
        .bearer_auth(token)
        .json(&serde_json::json!({}))
        .send()
        .await
        .unwrap();
    let status = response.status();
    (status, response.json().await.unwrap_or_default())
}

#[tokio::test]
async fn deleting_a_fork_frees_the_name_so_it_can_be_forked_again() {
    let app = spawn().await;
    let (owner_token, _) = register_full(&app.base, "fork-src", "fork-src@example.com").await;
    let (user_token, _) = register_full(&app.base, "fork-user", "fork-user@example.com").await;
    create_public(&app, &owner_token, "base").await;

    let (status, first) = fork(&app, &user_token, "fork-src", "base").await;
    assert_eq!(status, 202, "{first}");
    let first_id = first["id"].as_i64().unwrap();
    assert!(live(&app, "fork-user", "base").join("HEAD").exists());

    assert_eq!(delete(&app, &user_token, "fork-user", "base").await, 200);
    assert!(!live(&app, "fork-user", "base").exists());
    assert!(tombstone(&app, "fork-user", "base", first_id).join("HEAD").exists());

    let (status, second) = fork(&app, &user_token, "fork-src", "base").await;
    assert_eq!(status, 202, "re-forking after a delete must work: {second}");
    assert_ne!(second["id"].as_i64().unwrap(), first_id);
    assert_eq!(branch_count(&app, &user_token, "fork-user", "base").await, 1);
}

#[tokio::test]
async fn fork_quarantines_a_leftover_directory_from_an_old_delete() {
    let app = spawn().await;
    let (owner_token, _) = register_full(&app.base, "fl-src", "fl-src@example.com").await;
    let (user_token, _) = register_full(&app.base, "fl-user", "fl-user@example.com").await;
    create_public(&app, &owner_token, "base").await;

    // The user already had a repo of that name which an old build "deleted"
    // by soft-deleting the row only.
    let (_, old) = create(&app, &user_token, "base", true).await;
    let old_id = old["id"].as_i64().unwrap();
    rg_db::ops::repo_ops::soft_delete(&app.db, old_id).await.unwrap();
    assert!(live(&app, "fl-user", "base").exists());

    let (status, forked) = fork(&app, &user_token, "fl-src", "base").await;
    assert_eq!(status, 202, "a leftover directory must not block a fork: {forked}");
    assert!(
        tombstone(&app, "fl-user", "base", old_id).join("HEAD").exists(),
        "the leftover data must be moved aside, not deleted"
    );
}
