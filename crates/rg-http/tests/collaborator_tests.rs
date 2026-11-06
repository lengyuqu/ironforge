mod common;

use common::{
    create_repo, register_bootstrap_admin, register_full, spawn_test_app, spawn_test_app_with_db,
};

#[tokio::test]
async fn updating_permission_persists_and_invalidates_write_access() {
    let (base, db) = spawn_test_app_with_db().await;
    register_bootstrap_admin(&base).await;
    let (owner_token, _) = register_full(&base, "perm_owner", "perm_owner@example.com").await;
    let (_, user_id) = register_full(&base, "perm_user", "perm_user@example.com").await;
    create_repo(&base, &owner_token, "perm_repo").await;
    let client = reqwest::Client::new();
    let url = format!("{base}/api/v1/repos/perm_owner/perm_repo/collaborators");
    let added = client
        .post(&url)
        .bearer_auth(&owner_token)
        .json(&serde_json::json!({"user_id": user_id, "permission": "read"}))
        .send()
        .await
        .unwrap();
    assert_eq!(added.status(), 201);
    let id = added.json::<serde_json::Value>().await.unwrap()["id"]
        .as_i64()
        .unwrap();

    for (permission, can_write) in [("write", true), ("admin", true), ("read", false)] {
        // Populate the cache before every change, including before revocation.
        let before =
            rg_core::repo::service::can_write(&db, "perm_owner", "perm_repo", Some(user_id))
                .await
                .unwrap();
        let response = client
            .patch(format!("{url}/{id}"))
            .bearer_auth(&owner_token)
            .json(&serde_json::json!({"permission": permission}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        assert_eq!(
            response.json::<serde_json::Value>().await.unwrap()["permission"],
            permission
        );
        let list = client
            .get(&url)
            .bearer_auth(&owner_token)
            .send()
            .await
            .unwrap();
        let rows = list.json::<Vec<serde_json::Value>>().await.unwrap();
        assert_eq!(rows[0]["permission"], permission);
        assert_eq!(
            rg_core::repo::service::can_write(&db, "perm_owner", "perm_repo", Some(user_id))
                .await
                .unwrap(),
            can_write,
            "previous access: {before}"
        );
    }
}

#[tokio::test]
async fn add_collaborator_accepts_username_and_email() {
    let base = spawn_test_app().await;
    let (owner_token, _owner_id) =
        register_full(&base, "collab_owner", "collab_owner@example.com").await;
    let (_alice_token, alice_id) =
        register_full(&base, "collab_alice", "collab_alice@example.com").await;
    let (_bob_token, bob_id) = register_full(&base, "collab_bob", "collab_bob@example.com").await;
    create_repo(&base, &owner_token, "collab_repo").await;

    let client = reqwest::Client::new();
    let by_username = client
        .post(format!(
            "{}/api/v1/repos/collab_owner/collab_repo/collaborators",
            base
        ))
        .bearer_auth(&owner_token)
        .json(&serde_json::json!({
            "username": "collab_alice",
            "permission": "read"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(by_username.status(), 201);
    let username_body: serde_json::Value = by_username.json().await.unwrap();
    assert_eq!(username_body["user_id"], alice_id);

    let by_email = client
        .post(format!(
            "{}/api/v1/repos/collab_owner/collab_repo/collaborators",
            base
        ))
        .bearer_auth(&owner_token)
        .json(&serde_json::json!({
            "email": "collab_bob@example.com",
            "permission": "write"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(by_email.status(), 201);
    let email_body: serde_json::Value = by_email.json().await.unwrap();
    assert_eq!(email_body["user_id"], bob_id);

    let list = client
        .get(format!(
            "{}/api/v1/repos/collab_owner/collab_repo/collaborators",
            base
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(list.status(), 200);
    let collaborators: Vec<serde_json::Value> = list.json().await.unwrap();
    let ids: Vec<i64> = collaborators
        .iter()
        .filter_map(|collab| collab["user_id"].as_i64())
        .collect();
    assert!(ids.contains(&alice_id));
    assert!(ids.contains(&bob_id));
}
