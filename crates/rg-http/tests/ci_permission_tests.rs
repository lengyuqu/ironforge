mod common;

use common::{register_full, spawn_test_app_with_db};

async fn create_private_repo(base: &str, token: &str, name: &str) -> i64 {
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/api/v1/repos", base))
        .bearer_auth(token)
        .json(&serde_json::json!({
            "name": name,
            "is_private": true
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201, "create private repo failed");
    resp.json::<serde_json::Value>().await.unwrap()["id"]
        .as_i64()
        .unwrap()
}

#[tokio::test]
async fn private_pipeline_list_requires_read_access() {
    let (base, db) = spawn_test_app_with_db().await;
    let client = reqwest::Client::new();
    let (owner_token, _owner_id) = register_full(&base, "ci_owner", "ci_owner@example.com").await;
    let (other_token, _other_id) = register_full(&base, "ci_other", "ci_other@example.com").await;
    let repo_id = create_private_repo(&base, &owner_token, "private-ci").await;

    rg_db::ops::pipeline_ops::create_pipeline(
        &db,
        repo_id,
        "0123456789012345678901234567890123456789",
        "refs/heads/main",
        "manual",
        None,
    )
    .await
    .unwrap();

    let anon_resp = client
        .get(format!(
            "{}/api/v1/repos/ci_owner/private-ci/pipelines",
            base
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(anon_resp.status(), 401);

    let other_resp = client
        .get(format!(
            "{}/api/v1/repos/ci_owner/private-ci/pipelines",
            base
        ))
        .bearer_auth(&other_token)
        .send()
        .await
        .unwrap();
    assert_eq!(other_resp.status(), 403);

    let owner_resp = client
        .get(format!(
            "{}/api/v1/repos/ci_owner/private-ci/pipelines",
            base
        ))
        .bearer_auth(&owner_token)
        .send()
        .await
        .unwrap();
    assert_eq!(owner_resp.status(), 200);
    let body: serde_json::Value = owner_resp.json().await.unwrap();
    assert_eq!(body["data"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn cancel_pipeline_requires_write_access() {
    let (base, db) = spawn_test_app_with_db().await;
    let client = reqwest::Client::new();
    let (owner_token, _owner_id) =
        register_full(&base, "ci_cancel_owner", "ci_cancel_owner@example.com").await;
    let (other_token, _other_id) =
        register_full(&base, "ci_cancel_other", "ci_cancel_other@example.com").await;
    let repo_id = create_private_repo(&base, &owner_token, "private-cancel").await;

    let pipeline = rg_db::ops::pipeline_ops::create_pipeline(
        &db,
        repo_id,
        "abcdefabcdefabcdefabcdefabcdefabcdefabcd",
        "refs/heads/main",
        "manual",
        None,
    )
    .await
    .unwrap();

    let other_resp = client
        .post(format!(
            "{}/api/v1/repos/ci_cancel_owner/private-cancel/pipelines/{}/cancel",
            base, pipeline.id
        ))
        .bearer_auth(&other_token)
        .send()
        .await
        .unwrap();
    assert_eq!(other_resp.status(), 403);

    let owner_resp = client
        .post(format!(
            "{}/api/v1/repos/ci_cancel_owner/private-cancel/pipelines/{}/cancel",
            base, pipeline.id
        ))
        .bearer_auth(&owner_token)
        .send()
        .await
        .unwrap();
    assert_eq!(owner_resp.status(), 200);
}
