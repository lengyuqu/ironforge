//! Who may transfer a repository, and where.
//!
//! `transfer_repo` authorized on `repo.owner_id` and put the repository wherever
//! the caller named. For an organization repository `owner_id` is merely the
//! member who created it, so a plain member could move the organization's code
//! into their own account; and anyone could drop a repository into any
//! organization, or into another person's account, without that side agreeing.

mod common;

use common::{build_test_app_state, register_full, setup_test_db};
use std::path::PathBuf;

struct App {
    base: String,
    repo_root: PathBuf,
}

async fn spawn() -> App {
    let (db, dir) = setup_test_db().await;
    let repo_root = dir.path().join("repos");
    std::fs::create_dir_all(&repo_root).unwrap();
    let state = build_test_app_state(db, repo_root.clone());
    let app = rg_http::create_router_for_test(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        let _dir = dir;
        axum::serve(listener, app).await.unwrap();
    });
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    App { base, repo_root }
}

async fn make_org(app: &App, token: &str, name: &str) {
    let r = reqwest::Client::new()
        .post(format!("{}/api/v1/orgs", app.base))
        .bearer_auth(token)
        .json(&serde_json::json!({ "name": name, "display_name": name }))
        .send()
        .await
        .unwrap();
    assert!(r.status().is_success(), "create org: {}", r.status());
}

async fn add_member(app: &App, token: &str, org: &str, user_id: i64, role: &str) {
    let r = reqwest::Client::new()
        .post(format!("{}/api/v1/orgs/{org}/members", app.base))
        .bearer_auth(token)
        .json(&serde_json::json!({ "user_id": user_id, "role": role }))
        .send()
        .await
        .unwrap();
    assert!(r.status().is_success(), "add member: {}", r.status());
}

async fn create(app: &App, token: &str, name: &str, org: Option<&str>) {
    let r = reqwest::Client::new()
        .post(format!("{}/api/v1/repos", app.base))
        .bearer_auth(token)
        .json(&serde_json::json!({ "name": name, "org": org, "is_private": false }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 201, "create {name}");
}

async fn transfer(app: &App, token: &str, owner: &str, name: &str, to: &str) -> reqwest::StatusCode {
    reqwest::Client::new()
        .post(format!("{}/api/v1/repos/{owner}/{name}/transfer", app.base))
        .bearer_auth(token)
        .json(&serde_json::json!({ "new_owner": to }))
        .send()
        .await
        .unwrap()
        .status()
}

fn dir(app: &App, owner: &str, name: &str) -> PathBuf {
    app.repo_root.join(format!("{owner}/{name}.git"))
}

#[tokio::test]
async fn cannot_transfer_into_an_organization_you_do_not_belong_to() {
    let app = spawn().await;
    let (alice, _) = register_full(&app.base, "tr-alice", "tr-alice@example.com").await;
    let (bob, bob_id) = register_full(&app.base, "tr-bob", "tr-bob@example.com").await;
    make_org(&app, &alice, "tr-acme").await;
    create(&app, &bob, "mine", None).await;

    let status = transfer(&app, &bob, "tr-bob", "mine", "tr-acme").await;
    assert_eq!(status, 403, "a non-member must not drop a repo into the org");
    assert!(dir(&app, "tr-bob", "mine").exists(), "refused transfer must not move data");
    assert!(!dir(&app, "tr-acme", "mine").exists());

    // Once they belong to the organization the same request succeeds.
    add_member(&app, &alice, "tr-acme", bob_id, "member").await;
    assert_eq!(transfer(&app, &bob, "tr-bob", "mine", "tr-acme").await, 200);
    assert!(dir(&app, "tr-acme", "mine").exists());
}

#[tokio::test]
async fn cannot_push_a_repository_into_another_users_account() {
    let app = spawn().await;
    let (alice, _) = register_full(&app.base, "tu-alice", "tu-alice@example.com").await;
    let (_bob, _) = register_full(&app.base, "tu-bob", "tu-bob@example.com").await;
    create(&app, &alice, "gift", None).await;

    assert_eq!(transfer(&app, &alice, "tu-alice", "gift", "tu-bob").await, 403);
    assert!(dir(&app, "tu-alice", "gift").exists());
    assert!(!dir(&app, "tu-bob", "gift").exists());
}

#[tokio::test]
async fn only_org_admins_can_move_an_org_repository_out() {
    let app = spawn().await;
    let (owner, _) = register_full(&app.base, "to-owner", "to-owner@example.com").await;
    let (carol, carol_id) = register_full(&app.base, "to-carol", "to-carol@example.com").await;
    let (dave, dave_id) = register_full(&app.base, "to-dave", "to-dave@example.com").await;
    let (eve, _) = register_full(&app.base, "to-eve", "to-eve@example.com").await;
    make_org(&app, &owner, "to-acme").await;
    add_member(&app, &owner, "to-acme", carol_id, "member").await;
    add_member(&app, &owner, "to-acme", dave_id, "admin").await;

    // carol (a plain member) creates the org's repo, so `owner_id == carol`.
    create(&app, &carol, "crown", Some("to-acme")).await;
    assert!(dir(&app, "to-acme", "crown").exists());

    // Creating a repo does not entitle her to take it home...
    let status = transfer(&app, &carol, "to-acme", "crown", "to-carol").await;
    assert!(status == 403 || status == 404, "creator-member must be refused, got {status}");
    // ...and an outsider certainly cannot.
    let status = transfer(&app, &eve, "to-acme", "crown", "to-eve").await;
    assert!(status == 403 || status == 404, "outsider must be refused, got {status}");
    assert!(dir(&app, "to-acme", "crown").exists(), "org repo must stay where it is");
    assert!(!dir(&app, "to-carol", "crown").exists());
    assert!(!dir(&app, "to-eve", "crown").exists());

    // An organization admin can.
    assert_eq!(transfer(&app, &dave, "to-acme", "crown", "to-dave").await, 200);
    assert!(dir(&app, "to-dave", "crown").exists());
    assert!(!dir(&app, "to-acme", "crown").exists());
}
