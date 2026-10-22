//! Honest error semantics for pull requests, reviews, branch protection and
//! collaborators.
//!
//! These endpoints used to map every service failure to `400` (or `500`) with
//! the raw cause as the message. The cases pinned here are the contract:
//! invalid input -> 400, missing -> 404, forbidden -> 403, state/duplicate
//! conflict -> 409, and an unexpected I/O or database failure -> a sanitised
//! 500 that never echoes the cause (paths, SQL, driver text).

mod common;

use common::{build_test_app_state, register_full, setup_test_db};
use reqwest::Method;
use sea_orm::ConnectionTrait;
use serde_json::{json, Value};

struct App {
    base: String,
    db: rg_db::DatabaseConnection,
    repo_root: std::path::PathBuf,
    _dir: tempfile::TempDir,
}

async fn start() -> App {
    let (db, dir) = setup_test_db().await;
    let repo_root = dir.path().join("repos");
    std::fs::create_dir_all(&repo_root).unwrap();
    let app = rg_http::create_router_for_test(build_test_app_state(db.clone(), repo_root.clone()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    App {
        base,
        db,
        repo_root,
        _dir: dir,
    }
}

/// Send a request and return `(status, body text)`.
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

/// A 500 must be the generic message only.
#[track_caller]
fn expect_sanitised_500(app: &App, result: &(u16, String)) {
    expect(result, 500);
    let body: Value = serde_json::from_str(&result.1).unwrap();
    assert_eq!(body["error"]["message"], "Internal server error");
    let root = app.repo_root.to_string_lossy().to_string();
    for leak in [
        root.as_str(),
        ".git",
        "no such table",
        "sqlite",
        "SQL",
        "pr_reviews",
        "protected_branches",
        "repo_collaborators",
        "does not exist",
    ] {
        assert!(
            !result.1.contains(leak),
            "500 response leaked `{leak}`: {}",
            result.1
        );
    }
}

/// Create an auto-initialised repo (so `main` exists) and a `feature` branch.
async fn repo_with_feature_branch(app: &App, token: &str, name: &str) {
    let created = call(
        app,
        Method::POST,
        "/repos",
        token,
        Some(json!({"name": name, "auto_init": true})),
    )
    .await;
    expect(&created, 201);
}

async fn write_on_branch(app: &App, token: &str, owner: &str, repo: &str, branch: &str) {
    let wrote = call(
        app,
        Method::POST,
        &format!("/repos/{owner}/{repo}/contents/note.txt"),
        token,
        Some(json!({"content": "aGFja2Vk", "message": "seed", "branch": branch})),
    )
    .await;
    expect(&wrote, 200);
}

async fn open_pr(app: &App, token: &str, owner: &str, repo: &str, head: &str, draft: bool) -> i64 {
    let created = call(
        app,
        Method::POST,
        &format!("/repos/{owner}/{repo}/pulls"),
        token,
        Some(json!({"title": "A change", "head": head, "base": "main", "draft": draft})),
    )
    .await;
    expect(&created, 201);
    serde_json::from_str::<Value>(&created.1).unwrap()["number"]
        .as_i64()
        .unwrap()
}

// ── Pull requests ───────────────────────────────────────────────────────

#[tokio::test]
async fn pull_request_endpoints_classify_errors() {
    let app = start().await;
    let (owner, _) = register_full(&app.base, "pre-owner", "pre-owner@example.com").await;
    let (outsider, _) = register_full(&app.base, "pre-outsider", "pre-outsider@example.com").await;
    repo_with_feature_branch(&app, &owner, "r").await;
    write_on_branch(&app, &owner, "pre-owner", "r", "feature").await;
    let pr = open_pr(&app, &owner, "pre-owner", "r", "feature", false).await;
    let pulls = "/repos/pre-owner/r/pulls";

    // Missing -> 404.
    expect(
        &call(&app, Method::GET, &format!("{pulls}/99"), &owner, None).await,
        404,
    );
    expect(
        &call(&app, Method::GET, &format!("{pulls}/99/diff"), &owner, None).await,
        404,
    );
    expect(
        &call(
            &app,
            Method::PATCH,
            &format!("{pulls}/99"),
            &owner,
            Some(json!({"title": "x"})),
        )
        .await,
        404,
    );
    expect(
        &call(
            &app,
            Method::POST,
            &format!("{pulls}/99/merge"),
            &owner,
            Some(json!({"strategy": "merge"})),
        )
        .await,
        404,
    );
    expect(
        &call(
            &app,
            Method::DELETE,
            &format!("{pulls}/{pr}/merge-queue"),
            &owner,
            None,
        )
        .await,
        404,
    );

    // Invalid input -> 400.
    for body in [
        json!({"title": "t", "head": "", "base": "main"}),
        json!({"title": "t", "head": "main", "base": "main"}),
        json!({"title": "t", "head": "feature", "base": "main", "milestone_id": 9999}),
    ] {
        expect(
            &call(&app, Method::POST, pulls, &owner, Some(body)).await,
            400,
        );
    }
    for body in [json!({"state": "bogus"}), json!({"state": "merged"})] {
        expect(
            &call(
                &app,
                Method::PATCH,
                &format!("{pulls}/{pr}"),
                &owner,
                Some(body),
            )
            .await,
            400,
        );
    }
    expect(
        &call(
            &app,
            Method::POST,
            &format!("{pulls}/{pr}/merge"),
            &owner,
            Some(json!({"strategy": "fast-forward-ish"})),
        )
        .await,
        400,
    );

    // Forbidden -> 403.
    expect(
        &call(
            &app,
            Method::PATCH,
            &format!("{pulls}/{pr}"),
            &outsider,
            Some(json!({"title": "hijack"})),
        )
        .await,
        403,
    );
    expect(
        &call(
            &app,
            Method::POST,
            &format!("{pulls}/{pr}/merge"),
            &outsider,
            Some(json!({"strategy": "merge"})),
        )
        .await,
        403,
    );

    // State conflicts -> 409: draft and closed PRs cannot be merged, queued or
    // set to auto-merge.
    let draft = open_pr(&app, &owner, "pre-owner", "r", "feature", true).await;
    let closed = open_pr(&app, &owner, "pre-owner", "r", "feature", false).await;
    expect(
        &call(
            &app,
            Method::PATCH,
            &format!("{pulls}/{closed}"),
            &owner,
            Some(json!({"state": "closed"})),
        )
        .await,
        200,
    );
    for number in [draft, closed] {
        for (method, suffix, body) in [
            (Method::POST, "merge", json!({"strategy": "merge"})),
            (Method::PUT, "auto-merge", json!({"strategy": "merge"})),
            (Method::PUT, "merge-queue", json!({"strategy": "merge"})),
        ] {
            expect(
                &call(
                    &app,
                    method,
                    &format!("{pulls}/{number}/{suffix}"),
                    &owner,
                    Some(body),
                )
                .await,
                409,
            );
        }
    }
}

#[tokio::test]
async fn merging_a_pr_whose_head_branch_is_gone_is_a_conflict() {
    let app = start().await;
    let (owner, _) = register_full(&app.base, "gone-owner", "gone-owner@example.com").await;
    repo_with_feature_branch(&app, &owner, "r").await;
    // The head branch never exists (equivalent to it having been deleted).
    let pr = open_pr(&app, &owner, "gone-owner", "r", "ghost", false).await;

    for strategy in ["merge", "squash", "rebase"] {
        let merged = call(
            &app,
            Method::POST,
            &format!("/repos/gone-owner/r/pulls/{pr}/merge"),
            &owner,
            Some(json!({"strategy": strategy})),
        )
        .await;
        expect(&merged, 409);
        assert!(merged.1.contains("ghost"), "{}", merged.1);
    }
    // The failed attempts must leave the PR open and mergeable later.
    let fetched = call(
        &app,
        Method::GET,
        &format!("/repos/gone-owner/r/pulls/{pr}"),
        &owner,
        None,
    )
    .await;
    expect(&fetched, 200);
    assert_eq!(
        serde_json::from_str::<Value>(&fetched.1).unwrap()["state"],
        "open"
    );

    // Its diff points at a missing ref: 404, not a server error.
    expect(
        &call(
            &app,
            Method::GET,
            &format!("/repos/gone-owner/r/pulls/{pr}/diff"),
            &owner,
            None,
        )
        .await,
        404,
    );
}

#[tokio::test]
async fn pull_request_git_failure_is_a_sanitised_500() {
    let app = start().await;
    let (owner, _) = register_full(&app.base, "vanish-owner", "vanish-owner@example.com").await;
    repo_with_feature_branch(&app, &owner, "r").await;
    write_on_branch(&app, &owner, "vanish-owner", "r", "feature").await;
    let pr = open_pr(&app, &owner, "vanish-owner", "r", "feature", false).await;

    // The repository directory disappears from disk.
    std::fs::remove_dir_all(app.repo_root.join("vanish-owner/r.git")).unwrap();

    let diff = call(
        &app,
        Method::GET,
        &format!("/repos/vanish-owner/r/pulls/{pr}/diff"),
        &owner,
        None,
    )
    .await;
    expect_sanitised_500(&app, &diff);
    for strategy in ["merge", "squash", "rebase"] {
        let merged = call(
            &app,
            Method::POST,
            &format!("/repos/vanish-owner/r/pulls/{pr}/merge"),
            &owner,
            Some(json!({"strategy": strategy})),
        )
        .await;
        expect_sanitised_500(&app, &merged);
    }
    // A failed merge releases its claim.
    let fetched = call(
        &app,
        Method::GET,
        &format!("/repos/vanish-owner/r/pulls/{pr}"),
        &owner,
        None,
    )
    .await;
    assert_eq!(
        serde_json::from_str::<Value>(&fetched.1).unwrap()["state"],
        "open"
    );
}

// ── Reviews ─────────────────────────────────────────────────────────────

#[tokio::test]
async fn review_endpoints_classify_errors() {
    let app = start().await;
    let (owner, _) = register_full(&app.base, "rev-owner", "rev-owner@example.com").await;
    let (reviewer, _) = register_full(&app.base, "rev-reviewer", "rev-reviewer@example.com").await;
    let (outsider_with_no_write, _) =
        register_full(&app.base, "rev-reader", "rev-reader@example.com").await;
    repo_with_feature_branch(&app, &owner, "r").await;
    write_on_branch(&app, &owner, "rev-owner", "r", "feature").await;
    let pr = open_pr(&app, &owner, "rev-owner", "r", "feature", false).await;
    let base = format!("/repos/rev-owner/r/pulls/{pr}");

    // Missing PR / review / comment / thread -> 404.
    expect(
        &call(
            &app,
            Method::POST,
            "/repos/rev-owner/r/pulls/99/reviews",
            &reviewer,
            Some(json!({"action": "comment"})),
        )
        .await,
        404,
    );
    expect(
        &call(
            &app,
            Method::GET,
            "/repos/rev-owner/r/pulls/99/reviews",
            &reviewer,
            None,
        )
        .await,
        404,
    );
    expect(
        &call(
            &app,
            Method::GET,
            &format!("{base}/reviews/9999"),
            &reviewer,
            None,
        )
        .await,
        404,
    );
    expect(
        &call(
            &app,
            Method::POST,
            &format!("{base}/reviews/9999/dismiss"),
            &owner,
            Some(json!({"message": "no"})),
        )
        .await,
        404,
    );
    expect(
        &call(
            &app,
            Method::POST,
            &format!("{base}/comments/9999/suggestion/apply"),
            &owner,
            None,
        )
        .await,
        404,
    );
    expect(
        &call(
            &app,
            Method::PATCH,
            &format!("{base}/comments/9999/resolution"),
            &owner,
            Some(json!({"resolved": true})),
        )
        .await,
        404,
    );
    expect(
        &call(
            &app,
            Method::POST,
            &format!("{base}/reviewers"),
            &owner,
            Some(json!({"username": "nobody-here"})),
        )
        .await,
        404,
    );

    // Invalid input -> 400.
    expect(
        &call(
            &app,
            Method::POST,
            &format!("{base}/reviews"),
            &reviewer,
            Some(json!({"action": "rubber-stamp"})),
        )
        .await,
        400,
    );
    expect(
        &call(
            &app,
            Method::POST,
            &format!("{base}/reviews"),
            &owner,
            Some(json!({"action": "approve"})),
        )
        .await,
        400,
    );
    expect(
        &call(
            &app,
            Method::POST,
            &format!("{base}/comments"),
            &reviewer,
            Some(json!({"path": "note.txt", "line": 1, "side": "RIGHT", "body": "   "})),
        )
        .await,
        400,
    );
    expect(
        &call(
            &app,
            Method::POST,
            &format!("{base}/comments"),
            &reviewer,
            Some(json!({
                "path": "note.txt", "line": 1, "side": "RIGHT",
                "body": "reply", "reply_to_id": 9999
            })),
        )
        .await,
        400,
    );
    expect(
        &call(
            &app,
            Method::POST,
            &format!("{base}/suggestions/apply"),
            &owner,
            Some(json!({"comment_ids": []})),
        )
        .await,
        400,
    );

    // Forbidden -> 403: a plain reader cannot dismiss reviews or apply suggestions.
    expect(
        &call(
            &app,
            Method::POST,
            &format!("{base}/suggestions/apply"),
            &outsider_with_no_write,
            Some(json!({"comment_ids": [1]})),
        )
        .await,
        403,
    );

    // Conflict -> 409: reviewing a closed PR; duplicate reviewer request.
    expect(
        &call(
            &app,
            Method::POST,
            &format!("{base}/reviewers"),
            &owner,
            Some(json!({"username": "rev-reviewer"})),
        )
        .await,
        201,
    );
    expect(
        &call(
            &app,
            Method::POST,
            &format!("{base}/reviewers"),
            &owner,
            Some(json!({"username": "rev-reviewer"})),
        )
        .await,
        409,
    );
    expect(
        &call(
            &app,
            Method::PATCH,
            &base,
            &owner,
            Some(json!({"state": "closed"})),
        )
        .await,
        200,
    );
    expect(
        &call(
            &app,
            Method::POST,
            &format!("{base}/reviews"),
            &reviewer,
            Some(json!({"action": "comment"})),
        )
        .await,
        409,
    );
}

#[tokio::test]
async fn review_database_failure_is_a_sanitised_500() {
    let app = start().await;
    let (owner, _) = register_full(&app.base, "revdb-owner", "revdb-owner@example.com").await;
    repo_with_feature_branch(&app, &owner, "r").await;
    let pr = open_pr(&app, &owner, "revdb-owner", "r", "ghost", false).await;

    app.db
        .execute_unprepared("DROP TABLE pr_reviews")
        .await
        .unwrap();

    let listed = call(
        &app,
        Method::GET,
        &format!("/repos/revdb-owner/r/pulls/{pr}/reviews"),
        &owner,
        None,
    )
    .await;
    expect_sanitised_500(&app, &listed);
    let submitted = call(
        &app,
        Method::POST,
        &format!("/repos/revdb-owner/r/pulls/{pr}/reviews"),
        &owner,
        Some(json!({"action": "comment"})),
    )
    .await;
    expect_sanitised_500(&app, &submitted);
}

// ── Branch protection ───────────────────────────────────────────────────

#[tokio::test]
async fn branch_protection_endpoints_classify_errors() {
    let app = start().await;
    let (owner, _) = register_full(&app.base, "bp-owner", "bp-owner@example.com").await;
    let (other, _) = register_full(&app.base, "bp-other", "bp-other@example.com").await;
    repo_with_feature_branch(&app, &owner, "r").await;
    repo_with_feature_branch(&app, &other, "elsewhere").await;
    let url = "/repos/bp-owner/r/branches/protection";

    let created = call(
        &app,
        Method::POST,
        url,
        &owner,
        Some(json!({"branch_name": "main"})),
    )
    .await;
    expect(&created, 201);
    let id = serde_json::from_str::<Value>(&created.1).unwrap()["id"]
        .as_i64()
        .unwrap();
    let foreign = call(
        &app,
        Method::POST,
        "/repos/bp-other/elsewhere/branches/protection",
        &other,
        Some(json!({"branch_name": "main"})),
    )
    .await;
    expect(&foreign, 201);
    let foreign_id = serde_json::from_str::<Value>(&foreign.1).unwrap()["id"]
        .as_i64()
        .unwrap();

    // Invalid input -> 400.
    expect(
        &call(
            &app,
            Method::POST,
            url,
            &owner,
            Some(json!({"branch_name": "  "})),
        )
        .await,
        400,
    );
    expect(
        &call(
            &app,
            Method::POST,
            url,
            &owner,
            Some(json!({"branch_name": "dev", "required_approvals": -1})),
        )
        .await,
        400,
    );
    expect(
        &call(
            &app,
            Method::PATCH,
            &format!("{url}/{id}"),
            &owner,
            Some(json!({"required_approvals": -2})),
        )
        .await,
        400,
    );

    // Duplicate -> 409.
    expect(
        &call(
            &app,
            Method::POST,
            url,
            &owner,
            Some(json!({"branch_name": "main"})),
        )
        .await,
        409,
    );

    // Missing, or belonging to another repository -> 404.
    for missing in [9999, foreign_id] {
        expect(
            &call(&app, Method::GET, &format!("{url}/{missing}"), &owner, None).await,
            404,
        );
        expect(
            &call(
                &app,
                Method::PATCH,
                &format!("{url}/{missing}"),
                &owner,
                Some(json!({"require_pr": true})),
            )
            .await,
            404,
        );
        expect(
            &call(
                &app,
                Method::DELETE,
                &format!("{url}/{missing}"),
                &owner,
                None,
            )
            .await,
            404,
        );
    }
    // The foreign rule was left alone.
    expect(
        &call(
            &app,
            Method::GET,
            &format!("/repos/bp-other/elsewhere/branches/protection/{foreign_id}"),
            &other,
            None,
        )
        .await,
        200,
    );

    // Forbidden -> 403: only repository admins manage rules.
    expect(
        &call(
            &app,
            Method::POST,
            url,
            &other,
            Some(json!({"branch_name": "dev"})),
        )
        .await,
        403,
    );
    expect(
        &call(&app, Method::DELETE, &format!("{url}/{id}"), &other, None).await,
        403,
    );
}

#[tokio::test]
async fn branch_protection_database_failure_is_a_sanitised_500() {
    let app = start().await;
    let (owner, _) = register_full(&app.base, "bpdb-owner", "bpdb-owner@example.com").await;
    repo_with_feature_branch(&app, &owner, "r").await;
    app.db
        .execute_unprepared("DROP TABLE protected_branches")
        .await
        .unwrap();

    let url = "/repos/bpdb-owner/r/branches/protection";
    expect_sanitised_500(&app, &call(&app, Method::GET, url, &owner, None).await);
    expect_sanitised_500(
        &app,
        &call(
            &app,
            Method::POST,
            url,
            &owner,
            Some(json!({"branch_name": "main"})),
        )
        .await,
    );
    expect_sanitised_500(
        &app,
        &call(&app, Method::GET, &format!("{url}/1"), &owner, None).await,
    );
    expect_sanitised_500(
        &app,
        &call(&app, Method::DELETE, &format!("{url}/1"), &owner, None).await,
    );
}

// ── Collaborators ───────────────────────────────────────────────────────

#[tokio::test]
async fn collaborator_endpoints_classify_errors() {
    let app = start().await;
    let (owner, _) = register_full(&app.base, "col-owner", "col-owner@example.com").await;
    let (stranger, _) = register_full(&app.base, "col-stranger", "col-stranger@example.com").await;
    let (_, alice_id) = register_full(&app.base, "col-alice", "col-alice@example.com").await;
    repo_with_feature_branch(&app, &owner, "r").await;
    repo_with_feature_branch(&app, &stranger, "theirs").await;
    let url = "/repos/col-owner/r/collaborators";

    let added = call(
        &app,
        Method::POST,
        url,
        &owner,
        Some(json!({"username": "col-alice", "permission": "read"})),
    )
    .await;
    expect(&added, 201);
    let collab_id = serde_json::from_str::<Value>(&added.1).unwrap()["id"]
        .as_i64()
        .unwrap();

    // The stranger's repo has its own collaborator row.
    let theirs = call(
        &app,
        Method::POST,
        "/repos/col-stranger/theirs/collaborators",
        &stranger,
        Some(json!({"user_id": alice_id, "permission": "read"})),
    )
    .await;
    expect(&theirs, 201);
    let theirs_id = serde_json::from_str::<Value>(&theirs.1).unwrap()["id"]
        .as_i64()
        .unwrap();

    // Invalid input -> 400.
    for body in [
        json!({"username": "col-alice", "permission": "owner"}),
        json!({"permission": "read"}),
        json!({"user_id": 0, "permission": "read"}),
    ] {
        expect(
            &call(&app, Method::POST, url, &owner, Some(body)).await,
            400,
        );
    }
    expect(
        &call(
            &app,
            Method::PATCH,
            &format!("{url}/{collab_id}"),
            &owner,
            Some(json!({"permission": "superuser"})),
        )
        .await,
        400,
    );

    // Unknown user / collaborator -> 404.
    for body in [
        json!({"username": "no-such-user"}),
        json!({"email": "nobody@example.com"}),
        json!({"user_id": 987654}),
    ] {
        expect(
            &call(&app, Method::POST, url, &owner, Some(body)).await,
            404,
        );
    }
    expect(
        &call(
            &app,
            Method::PATCH,
            &format!("{url}/987654"),
            &owner,
            Some(json!({"permission": "write"})),
        )
        .await,
        404,
    );
    // A collaborator row of another repository is not addressable here, and
    // must not be modified through this route.
    expect(
        &call(
            &app,
            Method::PATCH,
            &format!("{url}/{theirs_id}"),
            &owner,
            Some(json!({"permission": "admin"})),
        )
        .await,
        404,
    );
    let theirs_list = call(
        &app,
        Method::GET,
        "/repos/col-stranger/theirs/collaborators",
        &stranger,
        None,
    )
    .await;
    assert_eq!(
        serde_json::from_str::<Vec<Value>>(&theirs_list.1).unwrap()[0]["permission"],
        "read"
    );

    // Duplicate -> 409.
    expect(
        &call(
            &app,
            Method::POST,
            url,
            &owner,
            Some(json!({"username": "col-alice", "permission": "write"})),
        )
        .await,
        409,
    );

    // Forbidden -> 403.
    expect(
        &call(
            &app,
            Method::POST,
            url,
            &stranger,
            Some(json!({"username": "col-alice"})),
        )
        .await,
        403,
    );
    expect(
        &call(
            &app,
            Method::DELETE,
            &format!("{url}/{alice_id}"),
            &stranger,
            None,
        )
        .await,
        403,
    );
}

#[tokio::test]
async fn collaborator_database_failure_is_a_sanitised_500() {
    let app = start().await;
    let (owner, _) = register_full(&app.base, "coldb-owner", "coldb-owner@example.com").await;
    register_full(&app.base, "coldb-alice", "coldb-alice@example.com").await;
    repo_with_feature_branch(&app, &owner, "r").await;
    app.db
        .execute_unprepared("DROP TABLE repo_collaborators")
        .await
        .unwrap();

    let url = "/repos/coldb-owner/r/collaborators";
    expect_sanitised_500(&app, &call(&app, Method::GET, url, &owner, None).await);
    expect_sanitised_500(
        &app,
        &call(
            &app,
            Method::POST,
            url,
            &owner,
            Some(json!({"username": "coldb-alice"})),
        )
        .await,
    );
}
