mod common;
use common::{register_bootstrap_admin, register_full, spawn_test_app_with_root};

#[tokio::test]
async fn settings_require_admin_and_guard_against_stale_writes() {
    let (base, _db, root) = spawn_test_app_with_root().await;
    register_bootstrap_admin(&base).await;
    let (owner, _) = register_full(&base, "settings_owner", "settings_owner@example.com").await;
    let (writer, writer_id) =
        register_full(&base, "settings_writer", "settings_writer@example.com").await;
    let client = reqwest::Client::new();
    let create = client
        .post(format!("{base}/api/v1/repos"))
        .bearer_auth(&owner)
        .json(&serde_json::json!({"name":"settings", "auto_init":true}))
        .send()
        .await
        .unwrap();
    assert_eq!(create.status(), 201);
    let url = format!("{base}/api/v1/repos/settings_owner/settings");
    assert_eq!(
        client
            .post(format!("{url}/collaborators"))
            .bearer_auth(&owner)
            .json(&serde_json::json!({"user_id":writer_id, "permission":"write"}))
            .send()
            .await
            .unwrap()
            .status(),
        201
    );
    let original: serde_json::Value = client
        .get(&url)
        .bearer_auth(&owner)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let version = original["settings_version"].as_str().unwrap();
    let body =
        serde_json::json!({"expected_version":version, "description":"updated", "is_private":true});
    assert_eq!(
        client
            .patch(&url)
            .bearer_auth(&writer)
            .json(&body)
            .send()
            .await
            .unwrap()
            .status(),
        403
    );
    let changed = client
        .patch(&url)
        .bearer_auth(&owner)
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(changed.status(), 200);
    let changed: serde_json::Value = changed.json().await.unwrap();
    assert_eq!(changed["description"], "updated");
    assert_eq!(changed["is_private"], true);
    assert_ne!(changed["settings_version"], version);
    assert_eq!(
        client
            .patch(&url)
            .bearer_auth(&owner)
            .json(&body)
            .send()
            .await
            .unwrap()
            .status(),
        409
    );
    assert_eq!(client.get(&url).send().await.unwrap().status(), 401);
    let version = changed["settings_version"].as_str().unwrap();
    let missing = client
        .patch(&url)
        .bearer_auth(&owner)
        .json(&serde_json::json!({"expected_version":version, "default_branch":"missing"}))
        .send()
        .await
        .unwrap();
    assert_eq!(missing.status(), 400);
    let bare = root.join("settings_owner/settings.git");
    let git = rg_git::cli_gateway::global_gateway().as_ref().unwrap();
    git.run(
        &[
            "--git-dir",
            bare.to_str().unwrap(),
            "update-ref",
            "refs/heads/develop",
            "refs/heads/main",
        ],
        None,
    )
    .unwrap()
    .ensure_success()
    .unwrap();
    let changed = client.patch(&url).bearer_auth(&owner).json(&serde_json::json!({"expected_version":version, "default_branch":"develop", "description":null})).send().await.unwrap();
    assert_eq!(changed.status(), 200);
    let changed: serde_json::Value = changed.json().await.unwrap();
    assert_eq!(changed["default_branch"], "develop");
    assert!(changed["description"].is_null());
    assert_eq!(
        std::fs::read_to_string(bare.join("HEAD")).unwrap().trim(),
        "ref: refs/heads/develop"
    );
}

#[tokio::test]
async fn simultaneous_settings_updates_have_one_winner() {
    let (base, _db, _) = common::spawn_test_app_on_env_db_with_root().await;
    let (owner, _) = register_full(&base, "settings_race_owner", "settings_race@example.com").await;
    common::create_repo(&base, &owner, "race").await;
    let client = reqwest::Client::new();
    let url = format!("{base}/api/v1/repos/settings_race_owner/race");
    let state: serde_json::Value = client
        .get(&url)
        .bearer_auth(&owner)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let version = state["settings_version"].as_str().unwrap();
    let a = client
        .patch(&url)
        .bearer_auth(&owner)
        .json(&serde_json::json!({"expected_version":version, "description":"one"}))
        .send();
    let b = client
        .patch(&url)
        .bearer_auth(&owner)
        .json(&serde_json::json!({"expected_version":version, "description":"two"}))
        .send();
    let (a, b) = tokio::join!(a, b);
    let mut statuses = [a.unwrap().status().as_u16(), b.unwrap().status().as_u16()];
    statuses.sort();
    assert_eq!(statuses, [200, 409]);
}
