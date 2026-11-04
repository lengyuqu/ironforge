//! Integration tests for multi-assignee issues (ISSUE-105).

mod common;

use common::{create_issue, create_repo, register_full, spawn_test_app_with_db};

#[tokio::test]
async fn assignees_round_trip_dedup_and_primary_mirror() {
    let (base, _db) = spawn_test_app_with_db().await;
    let (owner_token, _owner_id) =
        register_full(&base, "assign_owner", "assign_owner@example.com").await;
    let (_helper_token, _helper_id) =
        register_full(&base, "assign_helper", "assign_helper@example.com").await;
    let (_other_token, _other_id) =
        register_full(&base, "assign_other", "assign_other@example.com").await;
    let client = reqwest::Client::new();

    let _repo_id = create_repo(&base, &owner_token, "assign-repo").await;
    let (_issue_id, number) =
        create_issue(&base, &owner_token, "assign_owner", "assign-repo", "task").await;
    let url = format!("{base}/api/v1/repos/assign_owner/assign-repo/issues/{number}/assignees");

    // Initially empty.
    let empty = client.get(&url).send().await.unwrap();
    assert_eq!(empty.status(), 200);
    let body = empty.json::<serde_json::Value>().await.unwrap();
    assert_eq!(body["assignees"].as_array().map(Vec::len), Some(0));

    // Set two assignees (duplicates deduplicated).
    let set = client
        .put(&url)
        .bearer_auth(&owner_token)
        .json(&serde_json::json!({"assignees": ["assign_helper", "assign_other", "assign_helper"]}))
        .send()
        .await
        .unwrap();
    assert_eq!(set.status(), 200);
    let body = set.json::<serde_json::Value>().await.unwrap();
    assert_eq!(
        body["assignees"],
        serde_json::json!(["assign_helper", "assign_other"])
    );

    // Primary assignee is mirrored into the legacy issue column.
    let issue = client
        .get(format!("{base}/api/v1/repos/assign_owner/assign-repo/issues/{number}"))
        .send()
        .await
        .unwrap()
        .json::<serde_json::Value>()
        .await
        .unwrap();
    assert_eq!(issue["assignees"], serde_json::json!(["assign_helper", "assign_other"]));
    assert!(
        issue["assignee_id"].is_number(),
        "legacy assignee_id should mirror the primary assignee"
    );

    // Unknown username → 400 with the offending name.
    let unknown = client
        .put(&url)
        .bearer_auth(&owner_token)
        .json(&serde_json::json!({"assignees": ["no_such_user"]}))
        .send()
        .await
        .unwrap();
    assert_eq!(unknown.status(), 400);

    // Clearing works.
    let cleared = client
        .put(&url)
        .bearer_auth(&owner_token)
        .json(&serde_json::json!({"assignees": []}))
        .send()
        .await
        .unwrap();
    assert_eq!(cleared.status(), 200);
    let body = cleared.json::<serde_json::Value>().await.unwrap();
    assert_eq!(body["assignees"].as_array().map(Vec::len), Some(0));

    // Issue response reflects the cleared state, legacy column is null.
    let issue = client
        .get(format!("{base}/api/v1/repos/assign_owner/assign-repo/issues/{number}"))
        .send()
        .await
        .unwrap()
        .json::<serde_json::Value>()
        .await
        .unwrap();
    assert_eq!(issue["assignees"].as_array().map(Vec::len), Some(0));
    assert!(issue["assignee_id"].is_null());

    // Unknown issue → 404.
    let missing = client
        .put(format!("{base}/api/v1/repos/assign_owner/assign-repo/issues/999/assignees"))
        .bearer_auth(&owner_token)
        .json(&serde_json::json!({"assignees": ["assign_helper"]}))
        .send()
        .await
        .unwrap();
    assert_eq!(missing.status(), 404);
}

#[tokio::test]
async fn assignee_write_requires_write_access() {
    let (base, _db) = spawn_test_app_with_db().await;
    let (owner_token, _owner_id) =
        register_full(&base, "perm_owner", "perm_owner@example.com").await;
    let (stranger_token, _stranger_id) =
        register_full(&base, "perm_stranger", "perm_stranger@example.com").await;
    let client = reqwest::Client::new();

    let _repo_id = create_repo(&base, &owner_token, "perm-repo").await;
    let (_issue_id, number) =
        create_issue(&base, &owner_token, "perm_owner", "perm-repo", "task").await;
    let url = format!("{base}/api/v1/repos/perm_owner/perm-repo/issues/{number}/assignees");

    // Anonymous read is fine on a public repo.
    let anon_read = client.get(&url).send().await.unwrap();
    assert_eq!(anon_read.status(), 200);

    // Anonymous write → 401.
    let anon_write = client
        .put(&url)
        .json(&serde_json::json!({"assignees": ["perm_owner"]}))
        .send()
        .await
        .unwrap();
    assert_eq!(anon_write.status(), 401);

    // Stranger (no write access) → 403.
    let stranger_write = client
        .put(&url)
        .bearer_auth(&stranger_token)
        .json(&serde_json::json!({"assignees": ["perm_stranger"]}))
        .send()
        .await
        .unwrap();
    assert_eq!(stranger_write.status(), 403);
}

#[tokio::test]
async fn create_issue_with_assignees_and_list_filter() {
    let (base, _db) = spawn_test_app_with_db().await;
    let (owner_token, _owner_id) =
        register_full(&base, "filter_owner", "filter_owner@example.com").await;
    let (helper_token, helper_id) =
        register_full(&base, "filter_helper", "filter_helper@example.com").await;
    let client = reqwest::Client::new();

    let _repo_id = create_repo(&base, &owner_token, "filter-repo").await;

    // Create with assignees pre-set.
    let created = client
        .post(format!("{base}/api/v1/repos/filter_owner/filter-repo/issues"))
        .bearer_auth(&owner_token)
        .json(&serde_json::json!({
            "title": "pre-assigned",
            "assignees": ["filter_helper", "filter_owner"]
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(created.status(), 201);
    let body = created.json::<serde_json::Value>().await.unwrap();
    assert_eq!(
        body["assignees"],
        serde_json::json!(["filter_helper", "filter_owner"])
    );

    // Another issue without assignees.
    let (_id, _number) =
        create_issue(&base, &owner_token, "filter_owner", "filter-repo", "unassigned").await;

    // Assignee filter returns only the assigned issue.
    let filtered = client
        .get(format!(
            "{base}/api/v1/repos/filter_owner/filter-repo/issues?assignee=filter_helper"
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(filtered.status(), 200);
    let body = filtered.json::<serde_json::Value>().await.unwrap();
    let data = body["data"].as_array().expect("paginated data array");
    assert_eq!(data.len(), 1);
    assert_eq!(data[0]["title"], "pre-assigned");
    assert_eq!(data[0]["assignees"], serde_json::json!(["filter_helper", "filter_owner"]));

    // Filtering by owner also matches (owner is second assignee).
    let by_owner = client
        .get(format!(
            "{base}/api/v1/repos/filter_owner/filter-repo/issues?assignee=filter_owner"
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(by_owner.status(), 200);
    let body = by_owner.json::<serde_json::Value>().await.unwrap();
    assert_eq!(body["data"].as_array().map(Vec::len), Some(1));

    // Unknown assignee username → 404.
    let unknown = client
        .get(format!(
            "{base}/api/v1/repos/filter_owner/filter-repo/issues?assignee=no_such_user"
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(unknown.status(), 404);

    // Assignee gets a notification (not the actor).
    let notifications = client
        .get(format!("{base}/api/v1/notifications"))
        .bearer_auth(&helper_token)
        .send()
        .await
        .unwrap();
    assert_eq!(notifications.status(), 200);
    let body = notifications.json::<serde_json::Value>().await.unwrap();
    let found = body["data"]
        .as_array()
        .map(|rows| {
            rows.iter()
                .any(|n| n["event_type"] == "issue_assigned" || n["title"] == "pre-assigned")
        })
        .unwrap_or(false);
    assert!(found, "helper ({helper_id}) should be notified about the assignment");
}

#[tokio::test]
async fn patch_assignee_id_syncs_junction_table() {
    let (base, _db) = spawn_test_app_with_db().await;
    let (owner_token, _owner_id) =
        register_full(&base, "sync_owner", "sync_owner@example.com").await;
    let (_helper_token, helper_id) =
        register_full(&base, "sync_helper", "sync_helper@example.com").await;
    let client = reqwest::Client::new();

    let _repo_id = create_repo(&base, &owner_token, "sync-repo").await;
    let (_issue_id, number) =
        create_issue(&base, &owner_token, "sync_owner", "sync-repo", "task").await;

    // PATCH with legacy single assignee_id (write access via owner).
    let patched = client
        .patch(format!("{base}/api/v1/repos/sync_owner/sync-repo/issues/{number}"))
        .bearer_auth(&owner_token)
        .json(&serde_json::json!({ "assignee_id": helper_id }))
        .send()
        .await
        .unwrap();
    assert_eq!(patched.status(), 200);

    // The assignees endpoint reflects the junction-table sync.
    let assignees = client
        .get(format!("{base}/api/v1/repos/sync_owner/sync-repo/issues/{number}/assignees"))
        .send()
        .await
        .unwrap()
        .json::<serde_json::Value>()
        .await
        .unwrap();
    assert_eq!(assignees["assignees"], serde_json::json!(["sync_helper"]));

    // Note: PATCH `{"assignee_id": null}` parses as "field absent" under the
    // existing `Option<Option<i64>>` semantics (pre-ISSUE-105 behavior), so
    // clearing goes through the dedicated assignees endpoint instead.
}
