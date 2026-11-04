//! Honest error semantics for issues, milestones, labels, boards, time
//! tracking and attachments.
//!
//! Every endpoint family pins the same contract:
//!   invalid input -> 400, missing -> 404, forbidden -> 403, conflict -> 409,
//!   and an unexpected internal failure -> a sanitised 500 that never echoes
//!   the cause (SQL / driver text / table names) to the client.
//!
//! Internal failures are provoked by dropping a table underneath the running
//! app through the shared `DatabaseConnection` handle.

mod common;

use common::{create_issue, create_repo, register_user, spawn_test_app_with_db};
use reqwest::multipart::{Form, Part};
use reqwest::{Method, StatusCode};
use sea_orm::ConnectionTrait;
use serde_json::{json, Value};

const PW: &str = "Qz7$wRtm";

struct World {
    base: String,
    db: rg_db::DatabaseConnection,
    owner: String,
    owner_token: String,
    other_token: String,
    repo: String,
}

async fn world() -> World {
    let (base, db) = spawn_test_app_with_db().await;
    let sfx = uuid::Uuid::new_v4().simple().to_string()[..8].to_string();
    let owner = format!("own{sfx}");
    let other = format!("oth{sfx}");
    let owner_token = register_user(&base, &owner, &format!("{owner}@example.com"), PW).await;
    let other_token = register_user(&base, &other, &format!("{other}@example.com"), PW).await;
    let repo = "proj".to_string();
    create_repo(&base, &owner_token, &repo).await;
    World {
        base,
        db,
        owner,
        owner_token,
        other_token,
        repo,
    }
}

impl World {
    fn url(&self, path: &str) -> String {
        format!(
            "{}/api/v1/repos/{}/{}{}",
            self.base, self.owner, self.repo, path
        )
    }

    fn url_in(&self, repo: &str, path: &str) -> String {
        format!("{}/api/v1/repos/{}/{}{}", self.base, self.owner, repo, path)
    }

    async fn drop_table(&self, table: &str) {
        self.db
            .execute_unprepared(&format!("DROP TABLE {table}"))
            .await
            .unwrap_or_else(|e| panic!("drop {table}: {e}"));
    }
}

/// Send a request; returns (status, parsed JSON or Null, raw body text).
async fn call(
    method: Method,
    url: String,
    token: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Value, String) {
    let client = reqwest::Client::new();
    let mut req = client.request(method, url);
    if let Some(t) = token {
        req = req.bearer_auth(t);
    }
    if let Some(b) = body {
        req = req.json(&b);
    }
    let resp = req.send().await.unwrap();
    let status = resp.status();
    let text = resp.text().await.unwrap();
    let json = serde_json::from_str(&text).unwrap_or(Value::Null);
    (status, json, text)
}

async fn get(url: String, token: Option<&str>) -> (StatusCode, Value, String) {
    call(Method::GET, url, token, None).await
}

async fn post(url: String, token: Option<&str>, body: Value) -> (StatusCode, Value, String) {
    call(Method::POST, url, token, Some(body)).await
}

async fn patch(url: String, token: Option<&str>, body: Value) -> (StatusCode, Value, String) {
    call(Method::PATCH, url, token, Some(body)).await
}

async fn delete(url: String, token: Option<&str>) -> (StatusCode, Value, String) {
    call(Method::DELETE, url, token, None).await
}

/// A 500 must be the generic message and must not leak what failed.
fn assert_sanitised_500(res: &(StatusCode, Value, String)) {
    let (status, json, text) = res;
    assert_eq!(*status, StatusCode::INTERNAL_SERVER_ERROR, "body: {text}");
    assert_eq!(json["error"]["message"], "Internal server error", "{text}");
    let lower = text.to_lowercase();
    for needle in ["no such table", "sqlite", "select ", "insert ", "db:", "constraint"] {
        assert!(!lower.contains(needle), "500 body leaks '{needle}': {text}");
    }
}

fn assert_status(res: &(StatusCode, Value, String), expected: StatusCode) {
    assert_eq!(res.0, expected, "unexpected status, body: {}", res.2);
}

// ── Issues ──────────────────────────────────────────────────────────────

#[tokio::test]
async fn issue_endpoints_map_errors_to_honest_statuses() {
    let w = world().await;
    let o = Some(w.owner_token.as_str());
    let (_id, number) = create_issue(&w.base, &w.owner_token, &w.owner, &w.repo, "first").await;

    // 400: validation failures
    assert_status(
        &post(w.url("/issues"), o, json!({"title": "   "})).await,
        StatusCode::BAD_REQUEST,
    );
    // A dangling milestone id is a client error, not a foreign-key 500.
    let res = post(
        w.url("/issues"),
        o,
        json!({"title": "x", "milestone_id": 99999}),
    )
    .await;
    assert_status(&res, StatusCode::BAD_REQUEST);
    assert!(!res.2.to_lowercase().contains("constraint"), "{}", res.2);
    assert_status(
        &patch(
            w.url(&format!("/issues/{number}")),
            o,
            json!({"state": "bogus"}),
        )
        .await,
        StatusCode::BAD_REQUEST,
    );
    assert_status(
        &patch(
            w.url(&format!("/issues/{number}")),
            o,
            json!({"milestone_id": 99999}),
        )
        .await,
        StatusCode::BAD_REQUEST,
    );
    assert_status(
        &patch(
            w.url(&format!("/issues/{number}")),
            o,
            json!({"assignee_id": 99999}),
        )
        .await,
        StatusCode::BAD_REQUEST,
    );
    assert_status(
        &post(
            w.url(&format!("/issues/{number}/comments")),
            o,
            json!({"body": "  "}),
        )
        .await,
        StatusCode::BAD_REQUEST,
    );
    assert_status(
        &call(
            Method::PUT,
            w.url(&format!("/issues/{number}/assignees")),
            o,
            Some(json!({"assignees": ["nobody-here"]})),
        )
        .await,
        StatusCode::BAD_REQUEST,
    );
    assert_status(
        &get(w.url("/issues?milestone=abc"), o).await,
        StatusCode::BAD_REQUEST,
    );

    // 404: missing issue / repo
    assert_status(&get(w.url("/issues/9999"), o).await, StatusCode::NOT_FOUND);
    assert_status(
        &get(w.url_in("nope", "/issues/1"), o).await,
        StatusCode::NOT_FOUND,
    );
    assert_status(
        &patch(w.url("/issues/9999"), o, json!({"title": "t"})).await,
        StatusCode::NOT_FOUND,
    );
    // used to be 400 (comment on a missing issue)
    assert_status(
        &post(w.url("/issues/9999/comments"), o, json!({"body": "hi"})).await,
        StatusCode::NOT_FOUND,
    );
    // used to be 500
    assert_status(
        &get(w.url("/issues/9999/comments"), o).await,
        StatusCode::NOT_FOUND,
    );
    assert_status(
        &get(w.url("/issues/9999/assignees"), o).await,
        StatusCode::NOT_FOUND,
    );
    assert_status(
        &call(
            Method::PUT,
            w.url("/issues/9999/assignees"),
            o,
            Some(json!({"assignees": []})),
        )
        .await,
        StatusCode::NOT_FOUND,
    );
    assert_status(
        &get(w.url("/issues?assignee=nobody-here"), o).await,
        StatusCode::NOT_FOUND,
    );
    assert_status(
        &get(w.url("/issues/9999/labels"), o).await,
        StatusCode::NOT_FOUND,
    );

    // 403: a reader that is neither author nor writer
    let other = Some(w.other_token.as_str());
    assert_status(
        &patch(
            w.url(&format!("/issues/{number}")),
            other,
            json!({"title": "hijack"}),
        )
        .await,
        StatusCode::FORBIDDEN,
    );
    assert_status(
        &call(
            Method::PUT,
            w.url(&format!("/issues/{number}/assignees")),
            other,
            Some(json!({"assignees": []})),
        )
        .await,
        StatusCode::FORBIDDEN,
    );
    // 401: anonymous write
    assert_status(
        &patch(w.url(&format!("/issues/{number}")), None, json!({"title": "t"})).await,
        StatusCode::UNAUTHORIZED,
    );
}

#[tokio::test]
async fn issue_cannot_reference_a_milestone_of_another_repository() {
    let w = world().await;
    let o = Some(w.owner_token.as_str());
    create_repo(&w.base, &w.owner_token, "second").await;
    let (status, ms, text) = post(
        w.url_in("second", "/milestones"),
        o,
        json!({"title": "v1"}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{text}");
    let ms_id = ms["id"].as_i64().unwrap();

    assert_status(
        &post(
            w.url("/issues"),
            o,
            json!({"title": "x", "milestone_id": ms_id}),
        )
        .await,
        StatusCode::BAD_REQUEST,
    );

    // The same milestone is fine inside its own repository.
    assert_status(
        &post(
            w.url_in("second", "/issues"),
            o,
            json!({"title": "x", "milestone_id": ms_id}),
        )
        .await,
        StatusCode::CREATED,
    );
}

#[tokio::test]
async fn milestone_reads_and_issue_labels_respect_private_repository_access() {
    let w = world().await;
    let o = Some(w.owner_token.as_str());
    let (status, _, text) = post(
        format!("{}/api/v1/repos", w.base),
        o,
        json!({"name": "secret", "is_private": true}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{text}");
    let (_, number) = create_issue(&w.base, &w.owner_token, &w.owner, "secret", "hidden").await;
    let (status, ms, text) = post(
        w.url_in("secret", "/milestones"),
        o,
        json!({"title": "internal"}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{text}");
    let ms_id = ms["id"].as_i64().unwrap();

    let paths = [
        "/milestones".to_string(),
        format!("/milestones/{ms_id}"),
        format!("/issues/{number}/labels"),
    ];
    for path in &paths {
        assert_status(
            &get(w.url_in("secret", path), None).await,
            StatusCode::UNAUTHORIZED,
        );
        assert_status(
            &get(w.url_in("secret", path), Some(&w.other_token)).await,
            StatusCode::FORBIDDEN,
        );
        assert_status(&get(w.url_in("secret", path), o).await, StatusCode::OK);
    }
}

#[tokio::test]
async fn milestone_writes_map_errors_and_hide_internal_failures() {
    let w = world().await;
    let o = Some(w.owner_token.as_str());
    assert_status(
        &get(w.url("/milestones/9999"), o).await,
        StatusCode::NOT_FOUND,
    );
    assert_status(
        &post(w.url("/milestones"), Some(&w.other_token), json!({"title": "t"})).await,
        StatusCode::FORBIDDEN,
    );
    assert_status(
        &post(w.url("/milestones"), None, json!({"title": "t"})).await,
        StatusCode::UNAUTHORIZED,
    );

    // Internal failure: the database refuses the write. Used to be a 400
    // carrying the driver message.
    w.drop_table("milestones").await;
    assert_sanitised_500(&post(w.url("/milestones"), o, json!({"title": "t"})).await);
}

#[tokio::test]
async fn issue_config_distinguishes_bad_content_from_internal_failure() {
    let w = world().await;
    let o = Some(w.owner_token.as_str());

    // Empty repository: defaults, no error.
    assert_status(&get(w.url("/issue_config"), o).await, StatusCode::OK);

    // A malformed config committed to the repo is a content problem: 400 on
    // read, and a `valid: false` verdict on validate.
    let (status, _, text) = post(
        w.url("/contents/.gitea/ISSUE_TEMPLATE/config.yml"),
        o,
        json!({"content": "contact_links:\n  - name: ''\n    url: nope\n    about: ''\n", "message": "bad config"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{text}");

    let res = get(w.url("/issue_config"), o).await;
    assert_status(&res, StatusCode::BAD_REQUEST);
    assert!(res.1["error"]["message"]
        .as_str()
        .unwrap()
        .contains("requires name and about"));

    let res = get(w.url("/issue_config/validate"), o).await;
    assert_status(&res, StatusCode::OK);
    assert_eq!(res.1["valid"], false);
    assert!(res.1["message"]
        .as_str()
        .unwrap()
        .contains("requires name and about"));
}

#[tokio::test]
async fn issue_config_internal_read_failure_is_not_echoed() {
    // Own app so the test knows where the bare repositories live.
    let (db, dir) = common::setup_test_db().await;
    let repo_root = dir.path().join("repos");
    std::fs::create_dir_all(&repo_root).unwrap();
    let state = common::build_test_app_state(db, repo_root.clone());
    let app = rg_http::create_router_for_test(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let owner = "cfgowner";
    let token = register_user(&base, owner, "cfgowner@example.com", PW).await;
    create_repo(&base, &token, "proj").await;
    let url = |p: &str| format!("{base}/api/v1/repos/{owner}/proj{p}");
    let t = Some(token.as_str());
    let (status, _, text) = post(
        url("/contents/.github/ISSUE_TEMPLATE/config.yml"),
        t,
        json!({"content": "blank_issues_enabled: true\n", "message": "cfg"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{text}");
    assert_status(&get(url("/issue_config"), t).await, StatusCode::OK);

    // Destroy the object database: the repository can no longer be read.
    let git_dir = repo_root.join(owner).join("proj.git");
    std::fs::remove_dir_all(git_dir.join("objects")).unwrap();

    // An unreadable repository is an internal failure, not "invalid config":
    // sanitised 500 on read, and an error (not a `valid: false` verdict with
    // a message) on validate. Neither body may carry filesystem paths.
    let read = get(url("/issue_config"), t).await;
    assert_sanitised_500(&read);
    assert!(!read.2.contains("proj.git"), "{}", read.2);
    let validate = get(url("/issue_config/validate"), t).await;
    assert_sanitised_500(&validate);
    assert!(!validate.2.contains("proj.git"), "{}", validate.2);
    drop(dir);
}

// ── Labels ──────────────────────────────────────────────────────────────

#[tokio::test]
async fn label_endpoints_map_errors_to_honest_statuses() {
    let w = world().await;
    let o = Some(w.owner_token.as_str());

    let (status, bug, text) = post(
        w.url("/labels"),
        o,
        json!({"name": "bug", "color": "#ff0000"}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{text}");
    let bug_id = bug["id"].as_i64().unwrap();
    let (status, feat, text) = post(
        w.url("/labels"),
        o,
        json!({"name": "feature", "color": "#00ff00"}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{text}");
    let feat_id = feat["id"].as_i64().unwrap();

    // 400
    assert_status(
        &post(w.url("/labels"), o, json!({"name": "x", "color": "red"})).await,
        StatusCode::BAD_REQUEST,
    );
    assert_status(
        &post(w.url("/labels"), o, json!({"name": " ", "color": "#ffffff"})).await,
        StatusCode::BAD_REQUEST,
    );
    assert_status(
        &patch(w.url(&format!("/labels/{bug_id}")), o, json!({"color": "nope"})).await,
        StatusCode::BAD_REQUEST,
    );

    // 409: duplicate name on create (used to be 400) and on rename
    let res = post(
        w.url("/labels"),
        o,
        json!({"name": "bug", "color": "#123456"}),
    )
    .await;
    assert_status(&res, StatusCode::CONFLICT);
    assert!(!res.2.to_lowercase().contains("unique"), "{}", res.2);
    assert_status(
        &patch(
            w.url(&format!("/labels/{feat_id}")),
            o,
            json!({"name": "bug"}),
        )
        .await,
        StatusCode::CONFLICT,
    );
    // Renaming a label to its own name is not a conflict.
    assert_status(
        &patch(
            w.url(&format!("/labels/{feat_id}")),
            o,
            json!({"name": "feature"}),
        )
        .await,
        StatusCode::OK,
    );

    // 404: used to be 400 (update) / 204 (delete)
    assert_status(
        &get(w.url("/labels/9999"), o).await,
        StatusCode::NOT_FOUND,
    );
    assert_status(
        &patch(w.url("/labels/9999"), o, json!({"name": "z"})).await,
        StatusCode::NOT_FOUND,
    );
    assert_status(&delete(w.url("/labels/9999"), o).await, StatusCode::NOT_FOUND);

    // 403 / 401
    let other = Some(w.other_token.as_str());
    assert_status(
        &post(w.url("/labels"), other, json!({"name": "q", "color": "#000000"})).await,
        StatusCode::FORBIDDEN,
    );
    assert_status(
        &delete(w.url(&format!("/labels/{bug_id}")), other).await,
        StatusCode::FORBIDDEN,
    );
    assert_status(
        &delete(w.url(&format!("/labels/{bug_id}")), None).await,
        StatusCode::UNAUTHORIZED,
    );

    // 204 on a real delete
    assert_status(
        &delete(w.url(&format!("/labels/{bug_id}")), o).await,
        StatusCode::NO_CONTENT,
    );
}

#[tokio::test]
async fn label_id_from_another_repository_is_not_found() {
    let w = world().await;
    let o = Some(w.owner_token.as_str());
    create_repo(&w.base, &w.owner_token, "second").await;
    let (status, label, text) = post(
        w.url_in("second", "/labels"),
        o,
        json!({"name": "private-label", "color": "#abcdef"}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{text}");
    let id = label["id"].as_i64().unwrap();

    // Addressed through the wrong repository the label does not exist ...
    assert_status(
        &patch(w.url(&format!("/labels/{id}")), o, json!({"name": "hijacked"})).await,
        StatusCode::NOT_FOUND,
    );
    assert_status(
        &delete(w.url(&format!("/labels/{id}")), o).await,
        StatusCode::NOT_FOUND,
    );
    // ... and was neither renamed nor deleted.
    let (status, label, _) = get(w.url_in("second", &format!("/labels/{id}")), o).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(label["name"], "private-label");
}

#[tokio::test]
async fn label_database_failure_is_a_sanitised_500() {
    let w = world().await;
    let o = Some(w.owner_token.as_str());
    w.drop_table("labels").await;
    // used to be 400
    assert_sanitised_500(
        &post(w.url("/labels"), o, json!({"name": "bug", "color": "#ff0000"})).await,
    );
    assert_sanitised_500(&get(w.url("/labels"), o).await);
}

// ── Time tracking ───────────────────────────────────────────────────────

#[tokio::test]
async fn time_tracking_maps_errors_to_honest_statuses() {
    let w = world().await;
    let o = Some(w.owner_token.as_str());
    let (_, n1) = create_issue(&w.base, &w.owner_token, &w.owner, &w.repo, "one").await;
    let (_, n2) = create_issue(&w.base, &w.owner_token, &w.owner, &w.repo, "two").await;

    // 400
    assert_status(
        &post(
            w.url(&format!("/issues/{n1}/time")),
            o,
            json!({"duration_minutes": 0}),
        )
        .await,
        StatusCode::BAD_REQUEST,
    );
    // 404
    assert_status(
        &post(w.url("/issues/9999/time"), o, json!({"duration_minutes": 5})).await,
        StatusCode::NOT_FOUND,
    );
    assert_status(&get(w.url("/issues/9999/time"), o).await, StatusCode::NOT_FOUND);
    // 403
    assert_status(
        &post(
            w.url(&format!("/issues/{n1}/time")),
            Some(&w.other_token),
            json!({"duration_minutes": 5}),
        )
        .await,
        StatusCode::FORBIDDEN,
    );

    let (status, entry, text) = post(
        w.url(&format!("/issues/{n1}/time")),
        o,
        json!({"duration_minutes": 30}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{text}");
    let entry_id = entry["id"].as_i64().unwrap();

    // An entry of issue #1 cannot be addressed through issue #2 (404), and
    // is still there afterwards. Missing entries are 404 too (used to be 204).
    assert_status(
        &delete(w.url(&format!("/issues/{n2}/time/{entry_id}")), o).await,
        StatusCode::NOT_FOUND,
    );
    assert_status(
        &delete(w.url(&format!("/issues/{n1}/time/9999")), o).await,
        StatusCode::NOT_FOUND,
    );
    assert_status(
        &delete(w.url("/issues/9999/time/1"), o).await,
        StatusCode::NOT_FOUND,
    );
    let (_, total, _) = get(w.url(&format!("/issues/{n1}/time/total")), o).await;
    assert_eq!(total["total_minutes"], 30);
    assert_status(
        &delete(w.url(&format!("/issues/{n1}/time/{entry_id}")), o).await,
        StatusCode::NO_CONTENT,
    );
}

#[tokio::test]
async fn time_tracking_database_failure_is_a_sanitised_500() {
    let w = world().await;
    let o = Some(w.owner_token.as_str());
    let (_, n) = create_issue(&w.base, &w.owner_token, &w.owner, &w.repo, "one").await;
    w.drop_table("time_entries").await;
    // used to be 400
    assert_sanitised_500(
        &post(
            w.url(&format!("/issues/{n}/time")),
            o,
            json!({"duration_minutes": 5}),
        )
        .await,
    );
    assert_sanitised_500(&get(w.url(&format!("/issues/{n}/time/total")), o).await);
}

// ── Boards ──────────────────────────────────────────────────────────────

async fn new_board(w: &World, repo: &str, token: &str) -> (i64, Vec<i64>) {
    let (status, board, text) = post(
        w.url_in(repo, "/boards"),
        Some(token),
        json!({"name": "kanban"}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{text}");
    let id = board["id"].as_i64().unwrap();
    let (_, full, _) = get(w.url_in(repo, &format!("/boards/{id}")), Some(token)).await;
    let cols = full["columns"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["column"]["id"].as_i64().unwrap())
        .collect();
    (id, cols)
}

#[tokio::test]
async fn board_endpoints_map_errors_to_honest_statuses() {
    let w = world().await;
    let o = Some(w.owner_token.as_str());
    let (board, cols) = new_board(&w, &w.repo, &w.owner_token).await;

    // 400
    assert_status(
        &post(w.url("/boards"), o, json!({"name": "  "})).await,
        StatusCode::BAD_REQUEST,
    );
    assert_status(
        &post(w.url(&format!("/boards/{board}/columns")), o, json!({"name": ""})).await,
        StatusCode::BAD_REQUEST,
    );
    assert_status(
        &patch(w.url(&format!("/boards/{board}")), o, json!({"name": ""})).await,
        StatusCode::BAD_REQUEST,
    );

    // 404 (used to be 400 for the PATCH/POST ones and 204 for DELETE)
    assert_status(&get(w.url("/boards/9999"), o).await, StatusCode::NOT_FOUND);
    assert_status(
        &patch(w.url("/boards/9999"), o, json!({"name": "x"})).await,
        StatusCode::NOT_FOUND,
    );
    assert_status(&delete(w.url("/boards/9999"), o).await, StatusCode::NOT_FOUND);
    assert_status(
        &post(w.url("/boards/9999/columns"), o, json!({"name": "x"})).await,
        StatusCode::NOT_FOUND,
    );
    assert_status(
        &patch(
            w.url(&format!("/boards/{board}/columns/9999")),
            o,
            json!({"name": "x"}),
        )
        .await,
        StatusCode::NOT_FOUND,
    );
    assert_status(
        &delete(w.url(&format!("/boards/{board}/columns/9999")), o).await,
        StatusCode::NOT_FOUND,
    );
    assert_status(
        &post(
            w.url(&format!("/boards/{board}/columns/9999/cards")),
            o,
            json!({"note": "n"}),
        )
        .await,
        StatusCode::NOT_FOUND,
    );
    assert_status(
        &patch(
            w.url(&format!("/boards/{board}/cards/9999")),
            o,
            json!({"note": "n"}),
        )
        .await,
        StatusCode::NOT_FOUND,
    );
    assert_status(
        &delete(w.url(&format!("/boards/{board}/cards/9999")), o).await,
        StatusCode::NOT_FOUND,
    );

    // Body references that do not resolve are client errors (400).
    let (status, card, text) = post(
        w.url(&format!("/boards/{board}/columns/{}/cards", cols[0])),
        o,
        json!({"note": "n"}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{text}");
    let card_id = card["id"].as_i64().unwrap();
    assert_status(
        &post(
            w.url(&format!("/boards/{board}/cards/{card_id}/move")),
            o,
            json!({"column_id": 99999, "position": 0}),
        )
        .await,
        StatusCode::BAD_REQUEST,
    );
    assert_status(
        &post(
            w.url(&format!("/boards/{board}/cards/reorder")),
            o,
            json!({"positions": [[99999, 0]]}),
        )
        .await,
        StatusCode::BAD_REQUEST,
    );
    assert_status(
        &post(
            w.url(&format!("/boards/{board}/columns/{}/cards", cols[0])),
            o,
            json!({"issue_id": 99999}),
        )
        .await,
        StatusCode::BAD_REQUEST,
    );

    // 403 / 401
    assert_status(
        &post(w.url("/boards"), Some(&w.other_token), json!({"name": "x"})).await,
        StatusCode::FORBIDDEN,
    );
    assert_status(
        &delete(w.url(&format!("/boards/{board}")), Some(&w.other_token)).await,
        StatusCode::FORBIDDEN,
    );
    assert_status(
        &post(w.url("/boards"), None, json!({"name": "x"})).await,
        StatusCode::UNAUTHORIZED,
    );

    // Happy path still works: move within the board, then delete.
    assert_status(
        &post(
            w.url(&format!("/boards/{board}/cards/{card_id}/move")),
            o,
            json!({"column_id": cols[1], "position": 0}),
        )
        .await,
        StatusCode::OK,
    );
    assert_status(
        &delete(w.url(&format!("/boards/{board}")), o).await,
        StatusCode::NO_CONTENT,
    );
}

#[tokio::test]
async fn board_ids_from_another_repository_are_not_found_and_untouched() {
    let w = world().await;
    let o = Some(w.owner_token.as_str());
    // A second owner with their own repository and board.
    let sfx = uuid::Uuid::new_v4().simple().to_string()[..8].to_string();
    let mallory = format!("mal{sfx}");
    let mtoken = register_user(&w.base, &mallory, &format!("{mallory}@example.com"), PW).await;
    create_repo(&w.base, &mtoken, "mine").await;
    let murl = |path: &str| format!("{}/api/v1/repos/{}/mine{}", w.base, mallory, path);

    // The victim's board with a card.
    let (vboard, vcols) = new_board(&w, &w.repo, &w.owner_token).await;
    let (status, card, text) = post(
        w.url(&format!("/boards/{vboard}/columns/{}/cards", vcols[0])),
        o,
        json!({"note": "victim card"}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{text}");
    let vcard = card["id"].as_i64().unwrap();

    // Mallory has write access to "mine" only.
    let (mboard, mcols) = {
        let (status, board, text) = post(murl("/boards"), Some(&mtoken), json!({"name": "m"})).await;
        assert_eq!(status, StatusCode::CREATED, "{text}");
        let id = board["id"].as_i64().unwrap();
        let (_, full, _) = get(murl(&format!("/boards/{id}")), Some(&mtoken)).await;
        let cols: Vec<i64> = full["columns"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["column"]["id"].as_i64().unwrap())
            .collect();
        (id, cols)
    };
    let m = Some(mtoken.as_str());

    // Victim board addressed through Mallory's repository.
    assert_status(&get(murl(&format!("/boards/{vboard}")), m).await, StatusCode::NOT_FOUND);
    assert_status(
        &patch(murl(&format!("/boards/{vboard}")), m, json!({"name": "pwned"})).await,
        StatusCode::NOT_FOUND,
    );
    assert_status(&delete(murl(&format!("/boards/{vboard}")), m).await, StatusCode::NOT_FOUND);
    assert_status(
        &patch(
            murl(&format!("/boards/{vboard}/columns/{}", vcols[0])),
            m,
            json!({"name": "pwned"}),
        )
        .await,
        StatusCode::NOT_FOUND,
    );
    // Victim column / card addressed through Mallory's own board.
    assert_status(
        &patch(
            murl(&format!("/boards/{mboard}/columns/{}", vcols[0])),
            m,
            json!({"name": "pwned"}),
        )
        .await,
        StatusCode::NOT_FOUND,
    );
    assert_status(
        &delete(murl(&format!("/boards/{mboard}/columns/{}", vcols[0])), m).await,
        StatusCode::NOT_FOUND,
    );
    assert_status(
        &delete(murl(&format!("/boards/{mboard}/cards/{vcard}")), m).await,
        StatusCode::NOT_FOUND,
    );
    assert_status(
        &patch(
            murl(&format!("/boards/{mboard}/cards/{vcard}")),
            m,
            json!({"note": "pwned"}),
        )
        .await,
        StatusCode::NOT_FOUND,
    );
    assert_status(
        &post(
            murl(&format!("/boards/{mboard}/columns/{}/cards", vcols[0])),
            m,
            json!({"note": "planted"}),
        )
        .await,
        StatusCode::NOT_FOUND,
    );
    // Moving Mallory's own card into the victim's column, or reordering the
    // victim's card, is a bad reference.
    let (status, mcard, text) = post(
        murl(&format!("/boards/{mboard}/columns/{}/cards", mcols[0])),
        m,
        json!({"note": "mine"}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{text}");
    let mcard = mcard["id"].as_i64().unwrap();
    assert_status(
        &post(
            murl(&format!("/boards/{mboard}/cards/{mcard}/move")),
            m,
            json!({"column_id": vcols[0], "position": 0}),
        )
        .await,
        StatusCode::BAD_REQUEST,
    );
    assert_status(
        &post(
            murl(&format!("/boards/{mboard}/cards/reorder")),
            m,
            json!({"positions": [[vcard, 7]]}),
        )
        .await,
        StatusCode::BAD_REQUEST,
    );

    // Nothing of the victim's changed.
    let (status, full, _) = get(w.url(&format!("/boards/{vboard}")), o).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(full["board"]["name"], "kanban");
    let column0 = &full["columns"][0];
    assert_eq!(column0["column"]["name"], "To Do");
    let cards = column0["cards"].as_array().unwrap();
    assert_eq!(cards.len(), 1);
    assert_eq!(cards[0]["note"], "victim card");
    assert_eq!(cards[0]["position"], 0);
}

#[tokio::test]
async fn board_card_cannot_link_an_issue_of_another_repository() {
    let w = world().await;
    let o = Some(w.owner_token.as_str());
    create_repo(&w.base, &w.owner_token, "second").await;
    let (issue_id, _) = create_issue(&w.base, &w.owner_token, &w.owner, "second", "elsewhere").await;
    let (own_issue, _) = create_issue(&w.base, &w.owner_token, &w.owner, &w.repo, "here").await;
    let (board, cols) = new_board(&w, &w.repo, &w.owner_token).await;

    assert_status(
        &post(
            w.url(&format!("/boards/{board}/columns/{}/cards", cols[0])),
            o,
            json!({"issue_id": issue_id}),
        )
        .await,
        StatusCode::BAD_REQUEST,
    );
    assert_status(
        &post(
            w.url(&format!("/boards/{board}/columns/{}/cards", cols[0])),
            o,
            json!({"issue_id": own_issue}),
        )
        .await,
        StatusCode::CREATED,
    );
}

#[tokio::test]
async fn board_database_failure_is_a_sanitised_500() {
    let w = world().await;
    let o = Some(w.owner_token.as_str());
    // The default columns are written in the same transaction as the board.
    w.drop_table("board_columns").await;
    // used to be 400
    assert_sanitised_500(&post(w.url("/boards"), o, json!({"name": "kanban"})).await);
}

// ── Attachments ─────────────────────────────────────────────────────────

fn upload_form(name: &str, bytes: &[u8]) -> Form {
    Form::new().part(
        "attachment",
        Part::bytes(bytes.to_vec())
            .file_name(name.to_string())
            .mime_str("text/plain")
            .unwrap(),
    )
}

async fn upload(url: String, token: &str, form: Form) -> (StatusCode, Value, String) {
    let resp = reqwest::Client::new()
        .post(url)
        .bearer_auth(token)
        .multipart(form)
        .send()
        .await
        .unwrap();
    let status = resp.status();
    let text = resp.text().await.unwrap();
    let json = serde_json::from_str(&text).unwrap_or(Value::Null);
    (status, json, text)
}

#[tokio::test]
async fn attachment_endpoints_map_errors_to_honest_statuses() {
    let w = world().await;
    let o = Some(w.owner_token.as_str());
    let (_, n) = create_issue(&w.base, &w.owner_token, &w.owner, &w.repo, "files").await;
    let assets = w.url(&format!("/issues/{n}/assets"));

    // 400: empty file, disallowed type, missing field
    assert_status(
        &upload(assets.clone(), &w.owner_token, upload_form("empty.txt", b"")).await,
        StatusCode::BAD_REQUEST,
    );
    assert_status(
        &upload(assets.clone(), &w.owner_token, upload_form("run.exe", b"x")).await,
        StatusCode::BAD_REQUEST,
    );
    assert_status(
        &upload(
            assets.clone(),
            &w.owner_token,
            Form::new().text("other", "value"),
        )
        .await,
        StatusCode::BAD_REQUEST,
    );
    // 404
    assert_status(
        &upload(
            w.url("/issues/9999/assets"),
            &w.owner_token,
            upload_form("a.txt", b"x"),
        )
        .await,
        StatusCode::NOT_FOUND,
    );
    assert_status(&get(format!("{assets}/9999"), o).await, StatusCode::NOT_FOUND);
    assert_status(&delete(format!("{assets}/9999"), o).await, StatusCode::NOT_FOUND);
    // 403: reader who is neither the issue author nor a writer
    assert_status(
        &upload(assets.clone(), &w.other_token, upload_form("a.txt", b"x")).await,
        StatusCode::FORBIDDEN,
    );
    // 201 happy path
    let res = upload(assets.clone(), &w.owner_token, upload_form("a.txt", b"x")).await;
    assert_status(&res, StatusCode::CREATED);
    let id = res.1["id"].as_i64().unwrap();
    assert_status(&get(format!("{assets}/{id}"), o).await, StatusCode::OK);
}

#[tokio::test]
async fn attachment_database_failure_is_a_sanitised_500() {
    let w = world().await;
    let (_, n) = create_issue(&w.base, &w.owner_token, &w.owner, &w.repo, "files").await;
    let assets = w.url(&format!("/issues/{n}/assets"));
    w.drop_table("attachments").await;
    // Used to be a 400 (quota lookup failure reported as bad input).
    assert_sanitised_500(&upload(assets.clone(), &w.owner_token, upload_form("a.txt", b"x")).await);
    assert_sanitised_500(&get(assets, Some(&w.owner_token)).await);
}
