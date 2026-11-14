//! Who may decide a pull request review.
//!
//! Until T06 (#19, review finding N12) `submit_review` only required read
//! access, so anyone who could see a repository could cast an `approve` vote —
//! and those votes are exactly what `count_current_human_approvals` feeds into
//! the protected-branch merge gate. These tests pin the tightened contract:
//!
//! - `approve` / `request_changes` need write access **or** a reviewer seat on
//!   the PR (an explicit requested reviewer, including CODEOWNERS);
//! - `comment` still needs nothing beyond read access;
//! - the PR author still cannot decide their own PR;
//! - a refused approval never becomes a recorded vote.

mod common;

use common::{build_test_app_state, register_full, setup_test_db};
use reqwest::Method;
use serde_json::{json, Value};

struct App {
    base: String,
    _db: rg_db::DatabaseConnection,
    _dir: tempfile::TempDir,
}

/// The permission cache is process-local and keyed by `(repo_id, user_id)`,
/// while every test here has its own database with the same small ids. Without
/// this, a `false` cached by one test would be read by another. Disabling it
/// makes every check hit the database, which is what these tests assert on.
fn disable_perm_cache() {
    std::env::set_var("IRONFORGE_DISABLE_PERM_CACHE", "1");
}

async fn start() -> App {
    let (db, dir) = setup_test_db().await;
    let repo_root = dir.path().join("repos");
    std::fs::create_dir_all(&repo_root).unwrap();
    let app = rg_http::create_router_for_test(build_test_app_state(db.clone(), repo_root));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    App {
        base,
        _db: db,
        _dir: dir,
    }
}

async fn call(
    app: &App,
    method: Method,
    path: &str,
    token: &str,
    body: Option<Value>,
) -> (u16, String) {
    let mut request = reqwest::Client::new()
        .request(method, format!("{}/api/v1{}", app.base, path))
        .bearer_auth(token);
    if let Some(body) = body {
        request = request.json(&body);
    }
    let response = request.send().await.unwrap();
    let status = response.status().as_u16();
    (status, response.text().await.unwrap())
}

#[track_caller]
fn expect(result: &(u16, String), status: u16) {
    assert_eq!(result.0, status, "unexpected response: {}", result.1);
}

/// An initialised repository (`main` exists) plus a `feature` branch carrying
/// one commit, returned as the open PR's number.
async fn repo_with_pr(app: &App, token: &str, owner: &str, name: &str) -> i64 {
    let created = call(
        app,
        Method::POST,
        "/repos",
        token,
        Some(json!({"name": name, "auto_init": true})),
    )
    .await;
    expect(&created, 201);

    let wrote = call(
        app,
        Method::POST,
        &format!("/repos/{owner}/{name}/contents/note.txt"),
        token,
        Some(json!({"content": "aGFja2Vk", "message": "seed", "branch": "feature"})),
    )
    .await;
    expect(&wrote, 200);

    let created = call(
        app,
        Method::POST,
        &format!("/repos/{owner}/{name}/pulls"),
        token,
        Some(json!({"title": "A change", "head": "feature", "base": "main"})),
    )
    .await;
    expect(&created, 201);
    serde_json::from_str::<Value>(&created.1).unwrap()["number"]
        .as_i64()
        .unwrap()
}

async fn review(app: &App, token: &str, slug: &str, pr: i64, action: &str) -> (u16, String) {
    call(
        app,
        Method::POST,
        &format!("/repos/{slug}/pulls/{pr}/reviews"),
        token,
        Some(json!({"action": action, "body": "looks fine"})),
    )
    .await
}

/// A reader with no seat cannot vote, but can still comment. This is the
/// behaviour change: before, both returned 201 and the approval counted.
#[tokio::test]
async fn a_reader_may_comment_but_not_approve() {
    disable_perm_cache();
    let app = start().await;
    let (owner, _) = register_full(&app.base, "rv-owner", "rv-owner@example.com").await;
    let (reader, _) = register_full(&app.base, "rv-reader", "rv-reader@example.com").await;
    let pr = repo_with_pr(&app, &owner, "rv-owner", "rv").await;
    let slug = "rv-owner/rv";

    let denied = review(&app, &reader, slug, pr, "approve").await;
    expect(&denied, 403);
    let body: Value = serde_json::from_str(&denied.1).unwrap();
    assert_eq!(
        body["error"]["code"], "REVIEW_DECISION_REQUIRES_REVIEWER",
        "the refusal must be machine-readable: {}",
        denied.1
    );

    expect(
        &review(&app, &reader, slug, pr, "request_changes").await,
        403,
    );
    expect(&review(&app, &reader, slug, pr, "comment").await, 201);
}

/// A reviewer the repository asked for may decide, without write access. This
/// keeps CODEOWNERS-driven reviews working.
#[tokio::test]
async fn a_requested_reviewer_may_approve_without_write_access() {
    disable_perm_cache();
    let app = start().await;
    let (owner, _) = register_full(&app.base, "rv-owner2", "rv-owner2@example.com").await;
    let (reviewer, _) = register_full(&app.base, "rv-reviewer", "rv-reviewer@example.com").await;
    let pr = repo_with_pr(&app, &owner, "rv-owner2", "rv2").await;
    let slug = "rv-owner2/rv2";

    let requested = call(
        &app,
        Method::POST,
        &format!("/repos/{slug}/pulls/{pr}/reviewers"),
        &owner,
        Some(json!({"username": "rv-reviewer"})),
    )
    .await;
    expect(&requested, 201);

    expect(&review(&app, &reviewer, slug, pr, "approve").await, 201);
    expect(
        &review(&app, &reviewer, slug, pr, "request_changes").await,
        201,
    );
}

/// Write access is enough on its own.
#[tokio::test]
async fn a_writer_may_approve() {
    disable_perm_cache();
    let app = start().await;
    let (owner, _) = register_full(&app.base, "rv-owner3", "rv-owner3@example.com").await;
    let (writer, _) = register_full(&app.base, "rv-writer", "rv-writer@example.com").await;
    let pr = repo_with_pr(&app, &owner, "rv-owner3", "rv3").await;
    let slug = "rv-owner3/rv3";

    let added = call(
        &app,
        Method::POST,
        &format!("/repos/{slug}/collaborators"),
        &owner,
        Some(json!({"username": "rv-writer", "permission": "write"})),
    )
    .await;
    expect(&added, 201);

    expect(&review(&app, &writer, slug, pr, "approve").await, 201);
}

/// The author still cannot decide their own PR, even with write access.
#[tokio::test]
async fn the_author_still_cannot_decide_their_own_pr() {
    disable_perm_cache();
    let app = start().await;
    let (owner, _) = register_full(&app.base, "rv-owner4", "rv-owner4@example.com").await;
    let pr = repo_with_pr(&app, &owner, "rv-owner4", "rv4").await;
    let slug = "rv-owner4/rv4";

    expect(&review(&app, &owner, slug, pr, "approve").await, 400);
    expect(
        &review(&app, &owner, slug, pr, "request_changes").await,
        400,
    );
    expect(&review(&app, &owner, slug, pr, "comment").await, 201);
}

/// A refused approval is not a recorded vote, so it cannot satisfy the merge
/// gate. Before the fix the reader's 201 approval would have counted.
#[tokio::test]
async fn a_refused_approval_does_not_count_towards_the_merge_gate() {
    disable_perm_cache();
    let app = start().await;
    let (owner, _) = register_full(&app.base, "rv-owner5", "rv-owner5@example.com").await;
    let (reader, _) = register_full(&app.base, "rv-reader5", "rv-reader5@example.com").await;
    let pr = repo_with_pr(&app, &owner, "rv-owner5", "rv5").await;
    let slug = "rv-owner5/rv5";

    expect(&review(&app, &reader, slug, pr, "approve").await, 403);

    let listed = call(
        &app,
        Method::GET,
        &format!("/repos/{slug}/pulls/{pr}/reviews"),
        &owner,
        None,
    )
    .await;
    expect(&listed, 200);
    let reviews: Value = serde_json::from_str(&listed.1).unwrap();
    let approvals = reviews
        .as_array()
        .map(|rows| rows.iter().filter(|row| row["action"] == "approve").count())
        .unwrap_or(0);
    assert_eq!(
        approvals, 0,
        "a refused approval must not be recorded: {}",
        listed.1
    );
}
