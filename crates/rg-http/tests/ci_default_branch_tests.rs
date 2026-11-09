mod common;

use sea_orm::{ActiveModelTrait, IntoActiveModel, Set};

fn git(args: &[&str], dir: Option<&std::path::Path>) {
    rg_git::cli_gateway::global_gateway()
        .as_ref()
        .unwrap()
        .run(args, dir)
        .unwrap()
        .ensure_success()
        .unwrap();
}

#[tokio::test]
async fn omitted_ref_uses_configured_default_branch() {
    let (base, db, root) = common::spawn_test_app_with_root().await;
    let (token, owner_id) =
        common::register_full(&base, "ci-default-owner", "ci-default@example.com").await;
    let client = reqwest::Client::new();
    let response = client
        .post(format!("{base}/api/v1/repos"))
        .bearer_auth(&token)
        .json(&serde_json::json!({"name":"default-test"}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 201);
    let repo = rg_db::ops::repo_ops::find_by_owner_and_name(&db, owner_id, "default-test")
        .await
        .unwrap()
        .unwrap();
    let mut active = repo.into_active_model();
    active.default_branch = Set("develop".into());
    active.update(&db).await.unwrap();

    let work = tempfile::tempdir().unwrap();
    let path = work.path();
    git(
        &["init", "--initial-branch=develop", path.to_str().unwrap()],
        None,
    );
    git(&["config", "user.email", "test@example.com"], Some(path));
    git(&["config", "user.name", "Test"], Some(path));
    std::fs::write(path.join("file"), "test").unwrap();
    git(&["add", "."], Some(path));
    git(&["commit", "-m", "test"], Some(path));
    let bare = root.join("ci-default-owner/default-test.git");
    git(&["push", bare.to_str().unwrap(), "develop"], Some(path));

    let url = format!("{base}/api/v1/repos/ci-default-owner/default-test/pipelines");
    let response = client
        .post(&url)
        .bearer_auth(&token)
        .json(&serde_json::json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 400);
    let body: serde_json::Value = response.json().await.unwrap();
    // The test CI engine reports no config. Reaching that check proves the
    // configured branch resolved; hardcoded main instead fails resolution.
    assert_eq!(body["error"]["message"], "no .ironforge-ci.yml found");
    let response = client
        .post(&url)
        .bearer_auth(&token)
        .json(&serde_json::json!({"ref_name":"main"}))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = response.json().await.unwrap();
    assert_eq!(
        body["error"]["message"],
        "cannot resolve commit SHA for ref"
    );
}
