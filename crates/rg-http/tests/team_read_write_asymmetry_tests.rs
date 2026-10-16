//! Pins the read/write asymmetry that the team→repository mapping relies on.
//!
//! Team→repository mappings restrict **write** only. Read access is
//! organization-wide: every member of the owning org can read its private
//! repositories, so team `permission = 'read'` has no effect. This asymmetry is
//! intentional (see `can_read_repo` / `can_write_repo`), and the test exists so a
//! future change cannot silently flip either half.

mod common;

use common::{register_full, spawn_test_app_with_db};
use sea_orm::{ActiveModelTrait, EntityTrait, Set};

async fn create_user(db: &rg_db::DatabaseConnection, name: &str) -> i64 {
    rg_db::entities::user::ActiveModel {
        username: Set(name.to_string()),
        email: Set(format!("{name}@example.com")),
        password_hash: Set("unused".to_string()),
        display_name: Set(Some(name.to_string())),
        is_admin: Set(false),
        is_active: Set(true),
        created_at: Set(chrono::Utc::now()),
        updated_at: Set(chrono::Utc::now()),
        ..Default::default()
    }
    .insert(db)
    .await
    .expect("insert user")
    .id
}

async fn private_repo_in_org(
    db: &rg_db::DatabaseConnection,
    owner_id: i64,
    org_id: i64,
    name: &str,
) -> rg_db::entities::repository::Model {
    rg_db::entities::repository::ActiveModel {
        owner_id: Set(owner_id),
        name: Set(name.to_string()),
        is_private: Set(true),
        default_branch: Set("main".to_string()),
        stars_count: Set(0),
        forks_count: Set(0),
        org_id: Set(Some(org_id)),
        created_at: Set(chrono::Utc::now()),
        updated_at: Set(chrono::Utc::now()),
        ..Default::default()
    }
    .insert(db)
    .await
    .expect("insert repo")
}

#[tokio::test]
async fn team_mapping_restricts_write_but_not_read() {
    let (base, db) = spawn_test_app_with_db().await;
    // Real users so the accounts exist; the assertions use the service layer.
    let (owner_token, owner_id) =
        register_full(&base, "asym-owner", "asym-owner@example.com").await;
    let (_, member_id) = register_full(&base, "asym-member", "asym-member@example.com").await;
    let _ = owner_token;

    let org = rg_db::ops::org_ops::create_org(
        &db,
        "asym-org",
        None,
        None,
        owner_id,
        "public",
    )
    .await
    .expect("create org");
    rg_db::ops::org_ops::add_org_member(&db, org.id, member_id, "member")
        .await
        .expect("add org member");
    let team = rg_db::ops::org_ops::create_team(&db, org.id, "readers", None, "read")
        .await
        .expect("create team");
    rg_db::ops::org_ops::add_team_member(&db, team.id, member_id, "member")
        .await
        .expect("add team member");

    let mapped = private_repo_in_org(&db, owner_id, org.id, "mapped").await;
    let unmapped = private_repo_in_org(&db, owner_id, org.id, "unmapped").await;

    // Restrict the team to `mapped` only.
    rg_db::ops::org_ops::set_team_repositories(&db, team.id, &[mapped.id])
        .await
        .expect("set mapping");
    rg_core::repo::service::invalidate_perm_cache_all();

    // READ stays org-wide, mapping or not.
    assert!(
        rg_core::repo::service::can_read_repo(&db, &mapped, Some(member_id))
            .await
            .expect("read mapped"),
        "an org member can read a mapped private repo"
    );
    assert!(
        rg_core::repo::service::can_read_repo(&db, &unmapped, Some(member_id))
            .await
            .expect("read unmapped"),
        "read access stays org-wide even for an unmapped repo"
    );

    // WRITE needs a write/admin team, so a read team grants none.
    assert!(
        !rg_core::repo::service::can_write_repo(&db, &mapped, Some(member_id))
            .await
            .expect("write mapped"),
        "a read team must not grant write"
    );
    assert!(
        !rg_core::repo::service::can_write_repo(&db, &unmapped, Some(member_id))
            .await
            .expect("write unmapped"),
        "a read team must not grant write"
    );

    // A non-member and an anonymous caller get neither.
    let outsider = create_user(&db, "asym-outsider").await;
    assert!(
        !rg_core::repo::service::can_read_repo(&db, &mapped, Some(outsider))
            .await
            .expect("outsider read")
    );
    assert!(!rg_core::repo::service::can_read_repo(&db, &mapped, None)
        .await
        .expect("anon read"));

    // Sanity: the repository rows really are private and org-scoped.
    let reloaded = rg_db::entities::repository::Entity::find_by_id(mapped.id)
        .one(&db)
        .await
        .expect("reload")
        .expect("mapped repo exists");
    assert!(reloaded.is_private);
    assert_eq!(reloaded.org_id, Some(org.id));
}