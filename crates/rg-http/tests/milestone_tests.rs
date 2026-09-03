//! Integration tests for Milestone M2-2 backend enrichment (A1/A2/A3/A5).
//!
//! Guards added on top of `issue_tests.rs` milestone coverage:
//!   A1  GET /repos/:o/:r/issues?milestone={id|none|*}   — issue list milestone filter
//!   A2  GET /repos/:o/:r/milestones (+ GET one)          — open_issues/closed_issues counts
//!   A3  GET /repos/:o/:r/issues[/:n]                     — milestone_title backfill
//!   A5  DELETE /repos/:o/:r/milestones/:id               — cascade detach + cross-repo guard
//!
//! Existing coverage (issue_tests.rs): milestone create/list/update/delete basics.

mod common;

use common::{create_repo, register_user, spawn_test_app};

const PW: &str = "Qz7$wRtm";

async fn setup(suffix: &str) -> (String, String, String, String) {
    let base = spawn_test_app().await;
    let owner = format!("msuser{suffix}");
    let token = register_user(&base, &owner, &format!("msuser{suffix}@example.com"), PW).await;
    let repo = format!("msrepo{suffix}");
    create_repo(&base, &token, &repo).await;
    (base, token, owner, repo)
}

/// Create a milestone, return (id, title).
async fn create_milestone(
    base: &str,
    token: &str,
    owner: &str,
    repo: &str,
    title: &str,
) -> (i64, String) {
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/api/v1/repos/{}/{}/milestones", base, owner, repo))
        .bearer_auth(token)
        .json(&serde_json::json!({"title": title}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201, "create milestone '{title}' failed");
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["open_issues"], 0, "fresh milestone open_issues");
    assert_eq!(body["closed_issues"], 0, "fresh milestone closed_issues");
    (body["id"].as_i64().unwrap(), body["title"].as_str().unwrap().to_string())
}

/// Create an issue on a milestone (or none when `milestone_id` is None).
async fn create_issue_with_milestone(
    base: &str,
    token: &str,
    owner: &str,
    repo: &str,
    title: &str,
    milestone_id: Option<i64>,
) -> serde_json::Value {
    let client = reqwest::Client::new();
    let mut payload = serde_json::json!({"title": title});
    if let Some(id) = milestone_id {
        payload["milestone_id"] = serde_json::json!(id);
    }
    let resp = client
        .post(format!("{}/api/v1/repos/{}/{}/issues", base, owner, repo))
        .bearer_auth(token)
        .json(&payload)
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        201,
        "create issue '{title}' (milestone={milestone_id:?}) failed"
    );
    resp.json().await.unwrap()
}

async fn list_issues(
    base: &str,
    token: &str,
    owner: &str,
    repo: &str,
    query: &str,
) -> serde_json::Value {
    let client = reqwest::Client::new();
    let resp = client
        .get(format!(
            "{}/api/v1/repos/{}/{}/issues{}",
            base, owner, repo, query
        ))
        .bearer_auth(token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200, "list issues {query}");
    resp.json().await.unwrap()
}

async fn list_milestones(base: &str, token: &str, owner: &str, repo: &str) -> Vec<serde_json::Value> {
    let client = reqwest::Client::new();
    let resp = client
        .get(format!(
            "{}/api/v1/repos/{}/{}/milestones",
            base, owner, repo
        ))
        .bearer_auth(token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    resp.json::<serde_json::Value>()
        .await
        .unwrap()
        .as_array()
        .unwrap()
        .clone()
}

// ── A2: counts ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_milestone_counts_reflect_issue_states() {
    let (base, token, owner, repo) = setup("a2").await;
    let client = reqwest::Client::new();
    let (ms_id, _title) = create_milestone(&base, &token, &owner, &repo, "v1.0").await;

    // 2 open + 1 closed on the milestone; 1 issue with no milestone.
    let a = create_issue_with_milestone(&base, &token, &owner, &repo, "ms open 1", Some(ms_id)).await;
    create_issue_with_milestone(&base, &token, &owner, &repo, "ms open 2", Some(ms_id)).await;
    create_issue_with_milestone(&base, &token, &owner, &repo, "no ms", None).await;

    // Close the first issue.
    let number = a["number"].as_i64().unwrap();
    let resp = client
        .patch(format!(
            "{}/api/v1/repos/{}/{}/issues/{}",
            base, owner, repo, number
        ))
        .bearer_auth(&token)
        .json(&serde_json::json!({"state": "closed"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let ms = list_milestones(&base, &token, &owner, &repo).await;
    assert_eq!(ms.len(), 1);
    assert_eq!(ms[0]["open_issues"], 1, "one open left");
    assert_eq!(ms[0]["closed_issues"], 1, "one closed");

    // Single-milestone GET also carries counts.
    let resp = client
        .get(format!(
            "{}/api/v1/repos/{}/{}/milestones/{}",
            base, owner, repo, ms_id
        ))
        .bearer_auth(token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let one: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(one["open_issues"], 1);
    assert_eq!(one["closed_issues"], 1);
}

// ── A1: issue-list milestone filter ───────────────────────────────────────────

#[tokio::test]
async fn test_issue_list_milestone_filter() {
    let (base, token, owner, repo) = setup("a1").await;
    let client = reqwest::Client::new();
    let (ms_id, _title) = create_milestone(&base, &token, &owner, &repo, "sprint-1").await;

    let m1 = create_issue_with_milestone(&base, &token, &owner, &repo, "in sprint 1", Some(ms_id)).await;
    let m2 = create_issue_with_milestone(&base, &token, &owner, &repo, "in sprint 2", Some(ms_id)).await;
    create_issue_with_milestone(&base, &token, &owner, &repo, "backlog", None).await;
    let n1 = m1["number"].as_i64().unwrap();
    let n2 = m2["number"].as_i64().unwrap();

    // Filter by milestone id.
    let body = list_issues(&base, &token, &owner, &repo, &format!("?milestone={ms_id}")).await;
    let data = body["data"].as_array().unwrap();
    assert_eq!(data.len(), 2, "both issues on the milestone");
    let numbers: Vec<i64> = data.iter().map(|i| i["number"].as_i64().unwrap()).collect();
    assert!(numbers.contains(&n1) && numbers.contains(&n2));

    // Combined with state filter.
    let resp = client
        .patch(format!(
            "{}/api/v1/repos/{}/{}/issues/{}",
            base, owner, repo, n1
        ))
        .bearer_auth(&token)
        .json(&serde_json::json!({"state": "closed"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body =
        list_issues(&base, &token, &owner, &repo, &format!("?state=closed&milestone={ms_id}")).await;
    let data = body["data"].as_array().unwrap();
    assert_eq!(data.len(), 1);
    assert_eq!(data[0]["number"].as_i64().unwrap(), n1);

    // `none` → issues without a milestone.
    let body = list_issues(&base, &token, &owner, &repo, "?milestone=none").await;
    let data = body["data"].as_array().unwrap();
    assert_eq!(data.len(), 1);
    assert_eq!(data[0]["title"], "backlog");

    // `*` → issues with any milestone.
    let body = list_issues(&base, &token, &owner, &repo, "?milestone=*").await;
    let data = body["data"].as_array().unwrap();
    assert_eq!(data.len(), 2);

    // Invalid filter → 400.
    let client = reqwest::Client::new();
    let resp = client
        .get(format!(
            "{}/api/v1/repos/{}/{}/issues?milestone=banana",
            base, owner, repo
        ))
        .bearer_auth(token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 400);
}

// ── A3: milestone_title backfill ──────────────────────────────────────────────

#[tokio::test]
async fn test_issue_response_has_milestone_title() {
    let (base, token, owner, repo) = setup("a3").await;
    let (ms_id, ms_title) = create_milestone(&base, &token, &owner, &repo, "release-2.0").await;

    let on_ms = create_issue_with_milestone(&base, &token, &owner, &repo, "tracked", Some(ms_id)).await;
    let number = on_ms["number"].as_i64().unwrap();

    // Single issue response.
    assert_eq!(on_ms["milestone_title"], serde_json::json!(ms_title));

    // List response backfills too.
    let body_all = list_issues(&base, &token, &owner, &repo, "").await;
    let data = body_all["data"].as_array().unwrap();
    assert_eq!(data.len(), 1);
    assert_eq!(data[0]["milestone_title"], serde_json::json!(ms_title));

    // After detaching the milestone the field disappears from the payload.
    let resp = reqwest::Client::new()
        .patch(format!(
            "{}/api/v1/repos/{}/{}/issues/{}",
            base, owner, repo, number
        ))
        .bearer_auth(&token)
        .json(&serde_json::json!({"milestone_id": null}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let updated: serde_json::Value = resp.json().await.unwrap();
    assert!(updated.get("milestone_title").is_none(), "title omitted when unset");
}

// ── A5: cascade delete + cross-repo guard ─────────────────────────────────────

#[tokio::test]
async fn test_delete_milestone_detaches_issues() {
    let (base, token, owner, repo) = setup("a5").await;
    let (ms_id, _title) = create_milestone(&base, &token, &owner, &repo, "obsolete").await;

    create_issue_with_milestone(&base, &token, &owner, &repo, "child 1", Some(ms_id)).await;
    create_issue_with_milestone(&base, &token, &owner, &repo, "child 2", Some(ms_id)).await;

    // Issues are attached before the delete.
    let body = list_issues(&base, &token, &owner, &repo, &format!("?milestone={ms_id}")).await;
    assert_eq!(body["data"].as_array().unwrap().len(), 2);

    let client = reqwest::Client::new();
    let resp = client
        .delete(format!(
            "{}/api/v1/repos/{}/{}/milestones/{}",
            base, owner, repo, ms_id
        ))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 204, "milestone deleted");

    // The milestone is gone…
    let resp = client
        .get(format!(
            "{}/api/v1/repos/{}/{}/milestones/{}",
            base, owner, repo, ms_id
        ))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);

    // …and its issues are detached, not deleted.
    let body = list_issues(&base, &token, &owner, &repo, &format!("?milestone={ms_id}")).await;
    assert_eq!(body["total"].as_i64().unwrap_or_default(), 0);
    let body = list_issues(&base, &token, &owner, &repo, "?milestone=none").await;
    assert_eq!(body["data"].as_array().unwrap().len(), 2, "issues survived as un-milestoned");
}

#[tokio::test]
async fn test_cross_repo_milestone_is_guarded() {
    let (base, token, owner, repo_a) = setup("xr").await;
    let client = reqwest::Client::new();

    // A second repo owned by the same user holds a milestone.
    let repo_b = format!("{}b", repo_a);
    create_repo(&base, &token, &repo_b).await;
    let (ms_b, _title) = create_milestone(&base, &token, &owner, &repo_b, "b-only").await;

    // Attempting to act on repo B's milestone through repo A must 404 (get + delete).
    let resp = client
        .get(format!(
            "{}/api/v1/repos/{}/{}/milestones/{}",
            base, owner, repo_a, ms_b
        ))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 404, "cross-repo get guarded");

    let resp = client
        .delete(format!(
            "{}/api/v1/repos/{}/{}/milestones/{}",
            base, owner, repo_a, ms_b
        ))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 404, "cross-repo delete guarded");

    // Repo B's milestone is untouched.
    let resp = client
        .get(format!(
            "{}/api/v1/repos/{}/{}/milestones/{}",
            base, owner, repo_b, ms_b
        ))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
}

// ── Milestone PATCH null-clear (Some(None) semantics) ─────────────────────────

#[tokio::test]
async fn test_update_milestone_null_clears_fields() {
    let (base, token, owner, repo) = setup("nc").await;
    let client = reqwest::Client::new();
    let (ms_id, _title) = create_milestone(&base, &token, &owner, &repo, "clearable").await;

    // Set description + due date.
    let resp = client
        .patch(format!(
            "{}/api/v1/repos/{}/{}/milestones/{}",
            base, owner, repo, ms_id
        ))
        .bearer_auth(&token)
        .json(&serde_json::json!({
            "description": "ship it",
            "due_date": "2026-12-31T23:59:59Z"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["description"], "ship it");
    assert_eq!(body["due_date"], "2026-12-31T23:59:59Z");

    // JSON null clears both (distinct from an absent field).
    let resp = client
        .patch(format!(
            "{}/api/v1/repos/{}/{}/milestones/{}",
            base, owner, repo, ms_id
        ))
        .bearer_auth(&token)
        .json(&serde_json::json!({"description": null, "due_date": null}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["description"], serde_json::Value::Null, "description cleared");
    assert_eq!(body["due_date"], serde_json::Value::Null, "due date cleared");
}

