//! Audit entries for repository delete / fork / transfer must record who acted.
//!
//! These handlers passed `claims.sub` - the numeric user id as a string - where
//! the audit log expects the username, so entries read `username = "3"` and a
//! fork's resource name was `3/repo` instead of `alice/repo`.

mod common;

use common::{register_full, spawn_test_app_with_db};
use rg_db::sea_orm::{ColumnTrait, EntityTrait, QueryFilter};

async fn entry(
    db: &rg_db::DatabaseConnection,
    action: &str,
) -> rg_db::entities::audit_log::Model {
    rg_db::entities::audit_log::Entity::find()
        .filter(rg_db::entities::audit_log::Column::Action.eq(action))
        .one(db)
        .await
        .unwrap()
        .unwrap_or_else(|| panic!("no audit entry for {action}"))
}

#[tokio::test]
async fn repo_delete_fork_and_transfer_audit_the_username_not_the_id() {
    let (base, db) = spawn_test_app_with_db().await;
    let (alice, alice_id) = register_full(&base, "audit-alice", "audit-alice@example.com").await;
    let (bob, bob_id) = register_full(&base, "audit-bob", "audit-bob@example.com").await;
    let client = reqwest::Client::new();

    for name in ["src", "doomed", "moving"] {
        let r = client
            .post(format!("{base}/api/v1/repos"))
            .bearer_auth(&alice)
            .json(&serde_json::json!({ "name": name, "is_private": false }))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 201);
    }

    // bob forks alice's public repo.
    let fork = client
        .post(format!("{base}/api/v1/repos/audit-alice/src/fork"))
        .bearer_auth(&bob)
        .json(&serde_json::json!({}))
        .send()
        .await
        .unwrap();
    assert!(fork.status().is_success(), "{}", fork.status());
    let forked = entry(&db, "repo.fork").await;
    assert_eq!(forked.user_id, Some(bob_id));
    assert_eq!(forked.username.as_deref(), Some("audit-bob"), "{forked:?}");
    assert_eq!(forked.resource_name.as_deref(), Some("audit-bob/src"), "{forked:?}");
    assert_eq!(forked.details.as_deref().map(|d| d.contains("audit-bob")), Some(true), "{forked:?}");

    // alice deletes one repo.
    let del = client
        .delete(format!("{base}/api/v1/repos/audit-alice/doomed"))
        .bearer_auth(&alice)
        .send()
        .await
        .unwrap();
    assert_eq!(del.status(), 200);
    let deleted = entry(&db, "repo.delete").await;
    assert_eq!(deleted.user_id, Some(alice_id));
    assert_eq!(deleted.username.as_deref(), Some("audit-alice"), "{deleted:?}");

    // alice creates an org and transfers a repo into it.
    let org = client
        .post(format!("{base}/api/v1/orgs"))
        .bearer_auth(&alice)
        .json(&serde_json::json!({ "name": "audit-org", "display_name": "o" }))
        .send()
        .await
        .unwrap();
    assert!(org.status().is_success());
    let moved = client
        .post(format!("{base}/api/v1/repos/audit-alice/moving/transfer"))
        .bearer_auth(&alice)
        .json(&serde_json::json!({ "new_owner": "audit-org" }))
        .send()
        .await
        .unwrap();
    assert_eq!(moved.status(), 200);
    let transferred = entry(&db, "repo.transfer").await;
    assert_eq!(transferred.user_id, Some(alice_id));
    assert_eq!(transferred.username.as_deref(), Some("audit-alice"), "{transferred:?}");
}
