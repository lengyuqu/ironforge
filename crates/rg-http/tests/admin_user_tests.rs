mod common;

use common::{register_full, spawn_test_app_with_db};

async fn promote_user_to_admin(db: &rg_db::DatabaseConnection, user_id: i64) {
    rg_db::ops::user_ops::update_by_id(db, user_id, None, None, Some(true), None)
        .await
        .expect("promote user to admin");
}

#[tokio::test]
async fn admin_users_list_requires_auth() {
    let (base, _db) = spawn_test_app_with_db().await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{}/api/v1/admin/users", base))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 403);
}

#[tokio::test]
async fn admin_users_list_requires_admin() {
    let (base, db) = spawn_test_app_with_db().await;
    let client = reqwest::Client::new();

    let (user_token, _) = register_full(&base, "nona", "nona@example.com").await;
    let user_resp = client
        .get(format!("{}/api/v1/admin/users", base))
        .bearer_auth(&user_token)
        .send()
        .await
        .unwrap();
    assert_eq!(user_resp.status(), 403);

    let (admin_token, admin_id) = register_full(&base, "adminer", "adminer@example.com").await;
    promote_user_to_admin(&db, admin_id).await;

    let admin_resp = client
        .get(format!("{}/api/v1/admin/users", base))
        .bearer_auth(&admin_token)
        .send()
        .await
        .unwrap();
    assert_eq!(admin_resp.status(), 200);
    let body: serde_json::Value = admin_resp.json().await.unwrap();
    assert_eq!(body["pagination"]["total"], 2);
}

#[tokio::test]
async fn admin_users_get_and_update() {
    let (base, db) = spawn_test_app_with_db().await;
    let client = reqwest::Client::new();

    let (admin_token, admin_id) = register_full(&base, "adminx", "adminx@example.com").await;
    let (_, target_id) = register_full(&base, "target", "target@example.com").await;
    promote_user_to_admin(&db, admin_id).await;

    let get_resp = client
        .get(format!("{}/api/v1/admin/users/{}", base, target_id))
        .bearer_auth(&admin_token)
        .send()
        .await
        .unwrap();
    assert_eq!(get_resp.status(), 200);
    let target_body: serde_json::Value = get_resp.json().await.unwrap();
    assert_eq!(target_body["id"], target_id);

    let update_resp = client
        .patch(format!("{}/api/v1/admin/users/{}", base, target_id))
        .bearer_auth(&admin_token)
        .json(&serde_json::json!({
            "display_name": "Target User",
            "bio": "Updated for admin test",
            "is_admin": true,
            "is_active": false,
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(update_resp.status(), 200);
    let updated: serde_json::Value = update_resp.json().await.unwrap();
    assert_eq!(updated["id"], target_id);
    assert_eq!(updated["display_name"], "Target User");
    assert_eq!(updated["is_admin"], true);
    assert_eq!(updated["is_active"], false);
}

#[tokio::test]
async fn admin_users_delete_block_self() {
    let (base, db) = spawn_test_app_with_db().await;
    let client = reqwest::Client::new();

    let (admin_token, admin_id) =
        register_full(&base, "admin_delete_self", "admin_delete_self@example.com").await;
    promote_user_to_admin(&db, admin_id).await;

    let del_resp = client
        .delete(format!("{}/api/v1/admin/users/{}", base, admin_id))
        .bearer_auth(&admin_token)
        .send()
        .await
        .unwrap();
    assert_eq!(del_resp.status(), 400);
}

#[tokio::test]
async fn admin_users_delete_target() {
    let (base, db) = spawn_test_app_with_db().await;
    let client = reqwest::Client::new();

    let (admin_token, admin_id) = register_full(&base, "admin_del", "admin_del@example.com").await;
    let (_, target_id) = register_full(&base, "victim", "victim@example.com").await;
    promote_user_to_admin(&db, admin_id).await;

    let del_resp = client
        .delete(format!("{}/api/v1/admin/users/{}", base, target_id))
        .bearer_auth(&admin_token)
        .send()
        .await
        .unwrap();
    assert_eq!(del_resp.status(), 200);
    let del_body: serde_json::Value = del_resp.json().await.unwrap();
    assert_eq!(del_body["deleted"], true);

    let get_after = client
        .get(format!("{}/api/v1/admin/users/{}", base, target_id))
        .bearer_auth(&admin_token)
        .send()
        .await
        .unwrap();
    assert_eq!(get_after.status(), 404);
}
