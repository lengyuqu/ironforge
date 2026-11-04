//! Integration tests for issue / issue-comment reactions (ISSUE-104).

mod common;

use common::{create_issue, create_repo, register_full, spawn_test_app_with_db};

#[tokio::test]
async fn issue_reactions_round_trip_uniqueness_and_aggregation() {
    let (base, _db) = spawn_test_app_with_db().await;
    let (owner_token, _owner_id) =
        register_full(&base, "react_owner", "react_owner@example.com").await;
    let (stranger_token, _stranger_id) =
        register_full(&base, "react_other", "react_other@example.com").await;
    let client = reqwest::Client::new();

    let _repo_id = create_repo(&base, &owner_token, "react-repo").await;
    let (_issue_id, number) = create_issue(&base, &owner_token, "react_owner", "react-repo", "bug")
        .await;
    let reactions_url = format!("{base}/api/v1/repos/react_owner/react-repo/issues/{number}/reactions");

    // Anonymous can read reactions on a public repo.
    let anon_list = client.get(&reactions_url).send().await.unwrap();
    assert_eq!(anon_list.status(), 200);
    assert_eq!(anon_list.json::<serde_json::Value>().await.unwrap().as_array().map(Vec::len), Some(0));

    // Anonymous cannot react.
    let anon_react = client
        .post(&reactions_url)
        .json(&serde_json::json!({"content": "+1"}))
        .send()
        .await
        .unwrap();
    assert_eq!(anon_react.status(), 401);

    // Invalid content is rejected.
    let invalid = client
        .post(&reactions_url)
        .bearer_auth(&owner_token)
        .json(&serde_json::json!({"content": "party_parrot"}))
        .send()
        .await
        .unwrap();
    assert_eq!(invalid.status(), 400);

    // Owner and stranger both react +1.
    for token in [&owner_token, &stranger_token] {
        let resp = client
            .post(&reactions_url)
            .bearer_auth(token)
            .json(&serde_json::json!({"content": "+1"}))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 201, "first +1 for each user must succeed");
    }

    // Duplicate reaction is rejected with 409.
    let duplicate = client
        .post(&reactions_url)
        .bearer_auth(&owner_token)
        .json(&serde_json::json!({"content": "+1"}))
        .send()
        .await
        .unwrap();
    assert_eq!(duplicate.status(), 409);

    // Owner adds a heart too — aggregation keeps contents separate.
    let heart = client
        .post(&reactions_url)
        .bearer_auth(&owner_token)
        .json(&serde_json::json!({"content": "heart"}))
        .send()
        .await
        .unwrap();
    assert_eq!(heart.status(), 201);
    let body = heart.json::<serde_json::Value>().await.unwrap();
    let plus_one = body
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["content"] == "+1")
        .unwrap();
    assert_eq!(plus_one["count"], 2);
    assert_eq!(plus_one["reacted_by_me"], true);
    assert!(
        body.as_array()
            .unwrap()
            .iter()
            .any(|s| s["content"] == "heart" && s["count"] == 1)
    );

    // Stranger viewer sees reacted_by_me only for their own +1.
    let stranger_view = client
        .get(&reactions_url)
        .bearer_auth(&stranger_token)
        .send()
        .await
        .unwrap()
        .json::<serde_json::Value>()
        .await
        .unwrap();
    let stranger_plus_one = stranger_view
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["content"] == "+1")
        .unwrap();
    assert_eq!(stranger_plus_one["count"], 2);
    assert_eq!(stranger_plus_one["reacted_by_me"], true);
    let stranger_heart = stranger_view
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["content"] == "heart")
        .unwrap();
    assert_eq!(stranger_heart["reacted_by_me"], false);

    // Removing the owner's +1 leaves the stranger's.
    let removed = client
        .delete(&reactions_url)
        .bearer_auth(&owner_token)
        .json(&serde_json::json!({"content": "+1"}))
        .send()
        .await
        .unwrap();
    assert_eq!(removed.status(), 200);
    let body = removed.json::<serde_json::Value>().await.unwrap();
    let plus_one = body
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["content"] == "+1")
        .unwrap();
    assert_eq!(plus_one["count"], 1);
    assert_eq!(plus_one["reacted_by_me"], false);

    // Removing again is idempotent.
    let removed_again = client
        .delete(&reactions_url)
        .bearer_auth(&owner_token)
        .json(&serde_json::json!({"content": "+1"}))
        .send()
        .await
        .unwrap();
    assert_eq!(removed_again.status(), 200);

    // Unknown issue returns 404.
    let missing = client
        .get(format!("{base}/api/v1/repos/react_owner/react-repo/issues/999/reactions"))
        .send()
        .await
        .unwrap();
    assert_eq!(missing.status(), 404);
}

#[tokio::test]
async fn comment_reactions_round_trip_and_cleanup() {
    let (base, db) = spawn_test_app_with_db().await;
    let (owner_token, _owner_id) =
        register_full(&base, "creact_owner", "creact_owner@example.com").await;
    let (stranger_token, _stranger_id) =
        register_full(&base, "creact_other", "creact_other@example.com").await;
    let client = reqwest::Client::new();

    let _repo_id = create_repo(&base, &owner_token, "creact-repo").await;
    let (_issue_id, number) = create_issue(&base, &owner_token, "creact_owner", "creact-repo", "bug")
        .await;

    let comment = client
        .post(format!(
            "{base}/api/v1/repos/creact_owner/creact-repo/issues/{number}/comments"
        ))
        .bearer_auth(&owner_token)
        .json(&serde_json::json!({"body": "looks fine to me"}))
        .send()
        .await
        .unwrap();
    assert_eq!(comment.status(), 201);
    let comment_id = comment.json::<serde_json::Value>().await.unwrap()["id"]
        .as_i64()
        .unwrap();

    let url = format!(
        "{base}/api/v1/repos/creact_owner/creact-repo/issues/comments/{comment_id}/reactions"
    );

    // Stranger reacts to the owner's comment.
    let react = client
        .post(&url)
        .bearer_auth(&stranger_token)
        .json(&serde_json::json!({"content": "hooray"}))
        .send()
        .await
        .unwrap();
    assert_eq!(react.status(), 201);
    let body = react.json::<serde_json::Value>().await.unwrap();
    assert_eq!(body[0]["content"], "hooray");
    assert_eq!(body[0]["count"], 1);
    assert_eq!(body[0]["reacted_by_me"], true);

    // Duplicate is rejected.
    let duplicate = client
        .post(&url)
        .bearer_auth(&stranger_token)
        .json(&serde_json::json!({"content": "hooray"}))
        .send()
        .await
        .unwrap();
    assert_eq!(duplicate.status(), 409);

    // Unknown comment returns 404.
    let missing = client
        .get(format!("{base}/api/v1/repos/creact_owner/creact-repo/issues/comments/999999/reactions"))
        .send()
        .await
        .unwrap();
    assert_eq!(missing.status(), 404);

    // Deleting the comment also removes its reactions (transactional cleanup).
    rg_core::issue::delete_comment(&db, comment_id).await.unwrap();
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
    let rows = rg_db::entities::reactions::Entity::find()
        .filter(rg_db::entities::reactions::Column::CommentId.eq(comment_id))
        .all(&db)
        .await
        .unwrap();
    assert_eq!(rows.len(), 0, "comment reactions must be deleted with the comment");
}

#[tokio::test]
async fn private_repo_reactions_require_read_access() {
    let (base, _db) = spawn_test_app_with_db().await;
    let (owner_token, _owner_id) =
        register_full(&base, "priv_react_owner", "priv_react_owner@example.com").await;
    let (stranger_token, _stranger_id) =
        register_full(&base, "priv_react_other", "priv_react_other@example.com").await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{base}/api/v1/repos"))
        .bearer_auth(&owner_token)
        .json(&serde_json::json!({"name": "priv-react-repo", "is_private": true}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);
    let (_issue_id, number) =
        create_issue(&base, &owner_token, "priv_react_owner", "priv-react-repo", "secret").await;
    let url = format!(
        "{base}/api/v1/repos/priv_react_owner/priv-react-repo/issues/{number}/reactions"
    );

    // Anonymous listing a private repo requires authentication.
    let anon = client.get(&url).send().await.unwrap();
    assert_eq!(anon.status(), 401);

    // Authenticated stranger without access is forbidden.
    let stranger = client.get(&url).bearer_auth(&stranger_token).send().await.unwrap();
    assert_eq!(stranger.status(), 403);

    // Stranger cannot react either.
    let stranger_react = client
        .post(&url)
        .bearer_auth(&stranger_token)
        .json(&serde_json::json!({"content": "+1"}))
        .send()
        .await
        .unwrap();
    assert_eq!(stranger_react.status(), 403);

    // Owner can react.
    let owner_react = client
        .post(&url)
        .bearer_auth(&owner_token)
        .json(&serde_json::json!({"content": "+1"}))
        .send()
        .await
        .unwrap();
    assert_eq!(owner_react.status(), 201);
}

#[tokio::test]
async fn reactions_notify_author_but_not_the_reactor() {
    let (base, db) = spawn_test_app_with_db().await;
    let (owner_token, owner_id) =
        register_full(&base, "notify_react_owner", "notify_react_owner@example.com").await;
    let (stranger_token, stranger_id) =
        register_full(&base, "notify_react_other", "notify_react_other@example.com").await;
    let client = reqwest::Client::new();

    let _repo_id = create_repo(&base, &owner_token, "notify-react-repo").await;
    let (_issue_id, number) =
        create_issue(&base, &owner_token, "notify_react_owner", "notify-react-repo", "bug").await;
    let url = format!(
        "{base}/api/v1/repos/notify_react_owner/notify-react-repo/issues/{number}/reactions"
    );

    // Stranger reacts → owner gets a notification, stranger does not.
    let react = client
        .post(&url)
        .bearer_auth(&stranger_token)
        .json(&serde_json::json!({"content": "+1"}))
        .send()
        .await
        .unwrap();
    assert_eq!(react.status(), 201);

    let owner_notifications = rg_db::ops::notification_ops::list_notifications(
        &db, owner_id, false,
    )
    .await
    .unwrap();
    assert!(
        owner_notifications
            .iter()
            .any(|n| n.event_type == "issue_reaction"),
        "issue author must be notified about the reaction"
    );

    let stranger_notifications = rg_db::ops::notification_ops::list_notifications(
        &db, stranger_id, false,
    )
    .await
    .unwrap();
    assert!(
        !stranger_notifications
            .iter()
            .any(|n| n.event_type == "issue_reaction"),
        "the reactor must not be notified about their own reaction"
    );

    // Owner reacting to their own issue must not notify themselves.
    let self_react = client
        .post(&url)
        .bearer_auth(&owner_token)
        .json(&serde_json::json!({"content": "heart"}))
        .send()
        .await
        .unwrap();
    assert_eq!(self_react.status(), 201);
    let owner_notifications = rg_db::ops::notification_ops::list_notifications(
        &db, owner_id, false,
    )
    .await
    .unwrap();
    assert_eq!(
        owner_notifications
            .iter()
            .filter(|n| n.event_type == "issue_reaction")
            .count(),
        1,
        "self-reaction must not create another notification"
    );
}
