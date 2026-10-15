//! Regression: an org team used to grant access to **every** repository in the
//! organization.
//!
//! `can_write_repo` consulted only `teams.permission`, so any `write` team made
//! its members writers on all org repos. Teams can now be mapped to specific
//! repositories; a team with no mapping keeps the historical org-wide access.

mod common;

use common::{register_full, spawn_test_app_with_db};
use sea_orm::{ActiveModelTrait, EntityTrait};

#[tokio::test]
async fn mapped_team_grants_access_only_to_listed_repositories() {
    let (base, db) = spawn_test_app_with_db().await;
    let (owner_token, owner_id) =
        register_full(&base, "teamrepo_owner", "teamrepo_owner@example.com").await;
    let (member_token, member_id) =
        register_full(&base, "teamrepo_member", "teamrepo_member@example.com").await;

    // A public org so the member can be added without extra visibility rules.
    let org = rg_db::ops::org_ops::create_org(&db, "teamrepo-org", None, None, owner_id, "public")
        .await
        .unwrap();

    // Two org repositories: one mapped, one not.
    let mapped_repo = create_org_repo(&base, &owner_token, "mapped-repo").await;
    let unmapped_repo = create_org_repo(&base, &owner_token, "unmapped-repo").await;

    // Attach both to the org directly (the REST create path uses the user owner).
    attach_repo_to_org(&db, mapped_repo, org.id).await;
    attach_repo_to_org(&db, unmapped_repo, org.id).await;

    // Org membership is the outer gate; the team mapping narrows it further.
    rg_db::ops::org_ops::add_org_member(&db, org.id, member_id, "member")
        .await
        .unwrap();
    let team = rg_db::ops::org_ops::create_team(&db, org.id, "writers", None, "write")
        .await
        .unwrap();
    rg_db::ops::org_ops::add_team_member(&db, team.id, member_id, "member")
        .await
        .unwrap();

    let mapped = rg_db::entities::repository::Entity::find_by_id(mapped_repo)
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    let unmapped = rg_db::entities::repository::Entity::find_by_id(unmapped_repo)
        .one(&db)
        .await
        .unwrap()
        .unwrap();

    // With no mapping the team is org-wide: the historical behaviour.
    assert!(
        rg_core::repo::service::can_write_repo(&db, &unmapped, Some(member_id))
            .await
            .unwrap(),
        "an unmapped write team keeps org-wide access"
    );

    // Restrict the team to the mapped repository.
    rg_core::org::set_team_repositories(&db, team.id, owner_id, &[mapped_repo])
        .await
        .unwrap();

    assert!(
        rg_core::repo::service::can_write_repo(&db, &mapped, Some(member_id))
            .await
            .unwrap(),
        "the team must keep access to its mapped repository"
    );
    assert!(
        !rg_core::repo::service::can_write_repo(&db, &unmapped, Some(member_id))
            .await
            .unwrap(),
        "a mapped team must NOT reach an unmapped repository"
    );

    // Clearing the mapping restores org-wide access.
    rg_core::org::set_team_repositories(&db, team.id, owner_id, &[])
        .await
        .unwrap();
    assert!(
        rg_core::repo::service::can_write_repo(&db, &unmapped, Some(member_id))
            .await
            .unwrap(),
        "clearing the mapping must restore org-wide access"
    );

    // The member token is never used to write; it exists so the account is real.
    let _ = &member_token;
}

/// A team may not be pointed at a repository from another organization.
#[tokio::test]
async fn team_mapping_rejects_foreign_repository() {
    let (base, db) = spawn_test_app_with_db().await;
    let (owner_token, owner_id) =
        register_full(&base, "teamrepo_owner2", "teamrepo_owner2@example.com").await;

    let org = rg_db::ops::org_ops::create_org(&db, "teamrepo-org2", None, None, owner_id, "public")
        .await
        .unwrap();
    let other_org =
        rg_db::ops::org_ops::create_org(&db, "teamrepo-org3", None, None, owner_id, "public")
            .await
            .unwrap();
    let team = rg_db::ops::org_ops::create_team(&db, org.id, "t", None, "read")
        .await
        .unwrap();

    // A repository that belongs to a different organization.
    let foreign_repo = create_org_repo(&base, &owner_token, "foreign-repo").await;
    attach_repo_to_org(&db, foreign_repo, other_org.id).await;

    let error = rg_core::org::set_team_repositories(&db, team.id, owner_id, &[foreign_repo])
        .await
        .expect_err("cross-organization mapping must be refused");
    assert!(
        matches!(error, rg_core::error::CoreError::InvalidInput(_)),
        "expected InvalidInput, got {error:?}"
    );
}

async fn create_org_repo(base: &str, token: &str, name: &str) -> i64 {
    let response = reqwest::Client::new()
        .post(format!("{base}/api/v1/repos"))
        .bearer_auth(token)
        .json(&serde_json::json!({"name": name, "is_private": true}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 201, "create repo failed");
    response.json::<serde_json::Value>().await.unwrap()["id"]
        .as_i64()
        .unwrap()
}

/// Point an existing repository at an organization.
async fn attach_repo_to_org(db: &sea_orm::DatabaseConnection, repo_id: i64, org_id: i64) {
    use sea_orm::{EntityTrait, Set};
    let repo = rg_db::entities::repository::Entity::find_by_id(repo_id)
        .one(db)
        .await
        .unwrap()
        .unwrap();
    let mut active: rg_db::entities::repository::ActiveModel = repo.into();
    active.org_id = Set(Some(org_id));
    active.update(db).await.unwrap();
    rg_core::repo::service::invalidate_perm_cache_all();
}