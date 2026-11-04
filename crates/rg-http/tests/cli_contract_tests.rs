//! Contract tests for the `ifg` CLI's expectations.
//!
//! The CLI (`lengyuqu/ifg`) depends on specific response shapes and a few
//! routes; when one of them changes, `ifg`'s own e2e suite only notices at
//! the next release. These tests pin the same surface here, where the break
//! is introduced.
//!
//! Covered:
//!  * `GET /users/{username}` — public profile, no email
//!  * `GET /releases/tags/{tag}` — by-tag lookup plus the 404 the CLI falls
//!    back on
//!  * `GET /orgs/{name}/members` — rows carry `username`/`display_name`
//!  * `PATCH /labels/{id}` — a rename actually persists (regression: the
//!    SeaORM ActiveModel used to issue an UPDATE without SET)
//!  * `GET /openapi.json` — `info.version` is the crate version
//!  * PAT: `Authorization: Bearer <pat>` works on HTTP and on the job-log
//!    WebSocket handshake
//!  * Pull request detail carries the fields `ifg pr checkout`/`status` read,
//!    and review comments expose `path`/`line`/`body`

mod common;

use chrono::Utc;
use common::{create_repo, register_full, spawn_test_app_with_db};
use sea_orm::ActiveValue::Set;
use serde_json::{json, Value};

/// 2xx check for the helpers' plain `u16` statuses.
fn ok(status: u16) -> bool {
    (200..300).contains(&status)
}

fn client() -> reqwest::Client {
    reqwest::Client::new()
}

async fn get_json(base: &str, path: &str, token: Option<&str>) -> (u16, Value) {
    let mut request = client().get(format!("{base}/api/v1{path}"));
    if let Some(token) = token {
        request = request.bearer_auth(token);
    }
    let response = request.send().await.expect("request");
    let status = response.status().as_u16();
    let body: Value = response.json().await.unwrap_or(Value::Null);
    (status, body)
}

async fn send_json(
    base: &str,
    method: reqwest::Method,
    path: &str,
    token: &str,
    body: &Value,
) -> (u16, Value) {
    let response = client()
        .request(method, format!("{base}/api/v1{path}"))
        .bearer_auth(token)
        .json(body)
        .send()
        .await
        .expect("request");
    let status = response.status().as_u16();
    let body: Value = response.json().await.unwrap_or(Value::Null);
    (status, body)
}

#[tokio::test]
async fn users_lookup_returns_public_profile() {
    let (base, _db) = spawn_test_app_with_db().await;
    let (token, user_id) = register_full(&base, "alice", "alice@example.com").await;

    let (status, body) = get_json(&base, "/users/alice", Some(&token)).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["id"], user_id);
    assert_eq!(body["username"], "alice");
    // The CLI resolves assignees with this route; an email here would leak
    // more than the CLI needs.
    assert!(
        body.get("email").is_none(),
        "no email in the public profile"
    );

    let (status, _) = get_json(&base, "/users/nobody", Some(&token)).await;
    assert_eq!(status, 404, "missing user must 404 for name resolution");
}

#[tokio::test]
async fn release_by_tag_route_and_404_fallback() {
    let (base, _db) = spawn_test_app_with_db().await;
    let (token, _) = register_full(&base, "releaser", "releaser@example.com").await;
    create_repo(&base, &token, "ship").await;

    let (status, created) = send_json(
        &base,
        reqwest::Method::POST,
        "/repos/releaser/ship/releases",
        &token,
        &json!({"tag_name": "v1.0.0", "title": "First", "body": "notes"}),
    )
    .await;
    assert!(ok(status), "create release: {status} {created}");

    let (status, body) = get_json(
        &base,
        "/repos/releaser/ship/releases/tags/v1.0.0",
        Some(&token),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["tag_name"], "v1.0.0");
    assert_eq!(body["title"], "First");

    // `ifg` only falls back to paging the listing when the by-tag route says
    // 404 — a different status would look like a real error.
    let (status, _) = get_json(
        &base,
        "/repos/releaser/ship/releases/tags/v9.9.9",
        Some(&token),
    )
    .await;
    assert_eq!(status, 404, "unknown tag must 404");
}

#[tokio::test]
async fn org_members_carry_usernames() {
    let (base, _db) = spawn_test_app_with_db().await;
    let (token, _) = register_full(&base, "owner", "owner@example.com").await;
    let (_, member_id) = register_full(&base, "member", "member@example.com").await;

    let (status, org) = send_json(
        &base,
        reqwest::Method::POST,
        "/orgs",
        &token,
        &json!({"name": "acme", "display_name": "Acme", "visibility": "public"}),
    )
    .await;
    assert!(ok(status), "create org: {status} {org}");

    let (status, _) = send_json(
        &base,
        reqwest::Method::POST,
        "/orgs/acme/members",
        &token,
        &json!({"user_id": member_id, "role": "member"}),
    )
    .await;
    assert!(ok(status), "add member: {status}");

    let (status, members) = get_json(&base, "/orgs/acme/members", Some(&token)).await;
    assert_eq!(status, 200, "{members}");
    let rows = members.as_array().expect("array of members");
    assert!(rows.len() >= 2, "owner and member: {members}");
    // `ifg org members` prints this column; without it the CLI can only show
    // numeric ids.
    let member = rows
        .iter()
        .find(|row| row["user_id"] == member_id)
        .expect("member row");
    assert_eq!(member["username"], "member");
    assert_eq!(member["role"], "member");
}

#[tokio::test]
async fn label_patch_actually_persists() {
    let (base, _db) = spawn_test_app_with_db().await;
    let (token, _) = register_full(&base, "labeler", "labeler@example.com").await;
    create_repo(&base, &token, "tags").await;

    let (status, created) = send_json(
        &base,
        reqwest::Method::POST,
        "/repos/labeler/tags/labels",
        &token,
        &json!({"name": "bug", "color": "#ff0000"}),
    )
    .await;
    assert!(ok(status), "create label: {status} {created}");
    let id = created["id"].as_i64().expect("label id");

    let (status, updated) = send_json(
        &base,
        reqwest::Method::PATCH,
        &format!("/repos/labeler/tags/labels/{id}"),
        &token,
        &json!({"name": "defect", "description": "renamed"}),
    )
    .await;
    assert!(ok(status), "patch label: {status} {updated}");
    assert_eq!(updated["name"], "defect", "PATCH must apply the rename");

    let (_, fetched) = get_json(
        &base,
        &format!("/repos/labeler/tags/labels/{id}"),
        Some(&token),
    )
    .await;
    assert_eq!(
        fetched["name"], "defect",
        "the rename must be persisted, not just echoed"
    );
}

#[tokio::test]
async fn openapi_version_is_the_crate_version() {
    let (base, _db) = spawn_test_app_with_db().await;
    // The route sits behind authentication (the CLI sends its PAT).
    let (token, _) = register_full(&base, "docs", "docs@example.com").await;
    let response = client()
        .get(format!("{base}/api-docs/openapi.json"))
        .bearer_auth(&token)
        .send()
        .await
        .expect("openapi request");
    assert!(
        response.status().is_success(),
        "openapi: {}",
        response.status()
    );
    let spec: Value = response.json().await.expect("json");
    assert_eq!(
        spec["info"]["version"],
        env!("CARGO_PKG_VERSION"),
        "'ifg' pins its contract snapshot to this version"
    );
}

#[tokio::test]
async fn pat_authenticates_http_and_websocket() {
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;

    let (base, _db) = spawn_test_app_with_db().await;
    let (jwt, _) = register_full(&base, "patty", "patty@example.com").await;

    // Mint a PAT with the session token, exactly like `ifg` documents.
    let (status, created) = send_json(
        &base,
        reqwest::Method::POST,
        "/users/tokens",
        &jwt,
        &json!({"name": "cli", "scopes": "repo,user"}),
    )
    .await;
    assert!(ok(status), "mint pat: {status} {created}");
    let pat = created["token"]
        .as_str()
        .expect("pat in response")
        .to_string();

    // HTTP: the CLI sends `Authorization: Bearer <pat>`.
    let (status, me) = get_json(&base, "/users/me", Some(&pat)).await;
    assert_eq!(status, 200, "pat on http: {me}");
    assert_eq!(me["username"], "patty");

    // WebSocket: a PAT must authenticate the handshake. A missing job then
    // answers 404; a rejected token would answer 401, which `ifg ci logs`
    // would (wrongly) treat as "no streaming support".
    let ws_base = base.replacen("http://", "ws://", 1);
    let url = format!("{ws_base}/api/v1/ws/job/999999?since=0");
    let mut request = url.as_str().into_client_request().expect("ws url");
    request.headers_mut().insert(
        "authorization",
        format!("Bearer {pat}").parse().expect("header value"),
    );
    let error = tokio_tungstenite::connect_async(request)
        .await
        .expect_err("no such job");
    assert!(
        error.to_string().contains("404"),
        "expected 404 for a missing job, got: {error}"
    );
}

/// `ifg pr checkout` reads `head_branch`/`head_sha`/`head_repo_id` from the
/// PR detail, and `ifg pr view --comments` reads `path`/`line`/`body` from the
/// inline comment list.
#[tokio::test]
async fn pull_request_detail_and_review_comments() {
    let (base, db) = spawn_test_app_with_db().await;
    let (token, author_id) = register_full(&base, "dev", "dev@example.com").await;
    let repo_id = create_repo(&base, &token, "flow").await;

    let pr = rg_db::ops::pull_request_ops::create(
        &db,
        rg_db::entities::pull_request::ActiveModel {
            id: sea_orm::NotSet,
            repo_id: Set(repo_id),
            number: Set(1),
            title: Set("Add the widget".to_string()),
            body: Set(Some("body".to_string())),
            state: Set("open".to_string()),
            is_draft: Set(false),
            auto_merge_enabled: Set(false),
            auto_merge_strategy: Set(None),
            auto_merge_enabled_by_id: Set(None),
            auto_merge_enabled_at: Set(None),
            author_id: Set(author_id),
            reviewer_id: Set(None),
            head_branch: Set("feature".to_string()),
            base_branch: Set("main".to_string()),
            head_sha: Set(Some("a1b2c3".to_string())),
            merge_strategy: Set(None),
            merge_commit_sha: Set(None),
            head_repo_id: Set(None),
            milestone_id: Set(None),
            labels: Set(None),
            created_at: Set(Utc::now()),
            updated_at: Set(Utc::now()),
            closed_at: Set(None),
            merged_at: Set(None),
        },
    )
    .await
    .expect("seed pr");

    let (status, detail) = get_json(&base, "/repos/dev/flow/pulls/1", Some(&token)).await;
    assert_eq!(status, 200, "{detail}");
    assert_eq!(detail["head_branch"], "feature");
    assert_eq!(detail["base_branch"], "main");
    assert_eq!(detail["head_sha"], "a1b2c3");
    // Null means "same repository" to the CLI (the fork path needs a value).
    assert!(detail["head_repo_id"].is_null(), "same-repo head");

    let review = rg_db::ops::pr_review_ops::create(
        &db,
        rg_db::entities::pr_review::ActiveModel {
            id: sea_orm::NotSet,
            pr_id: Set(pr.id),
            repo_id: Set(repo_id),
            reviewer_id: Set(author_id),
            action: Set("comment".to_string()),
            body: Set(Some("looks good".to_string())),
            commit_id: Set(Some("a1b2c3".to_string())),
            created_at: Set(Utc::now()),
        },
    )
    .await
    .expect("seed review");

    rg_db::ops::review_comment_ops::create(
        &db,
        rg_db::entities::review_comment::ActiveModel {
            id: sea_orm::NotSet,
            review_id: Set(review.id),
            pr_id: Set(pr.id),
            author_id: Set(author_id),
            path: Set("src/lib.rs".to_string()),
            position: Set(None),
            line: Set(Some(42)),
            start_line: Set(None),
            side: Set(Some("RIGHT".to_string())),
            start_side: Set(None),
            body: Set("why this shape?".to_string()),
            suggestion: Set(None),
            suggestion_applied_at: Set(None),
            suggestion_applied_by_id: Set(None),
            suggestion_commit_sha: Set(None),
            commit_id: Set(Some("a1b2c3".to_string())),
            reply_to_id: Set(None),
            resolved_at: Set(None),
            resolved_by_id: Set(None),
            created_at: Set(Utc::now()),
            updated_at: Set(Utc::now()),
        },
    )
    .await
    .expect("seed review comment");

    let (status, comments) =
        get_json(&base, "/repos/dev/flow/pulls/1/comments", Some(&token)).await;
    assert_eq!(status, 200, "{comments}");
    let rows = comments.as_array().expect("comment array");
    assert_eq!(rows.len(), 1, "{comments}");
    assert_eq!(rows[0]["path"], "src/lib.rs");
    assert_eq!(rows[0]["line"], 42);
    assert_eq!(rows[0]["body"], "why this shape?");
}
