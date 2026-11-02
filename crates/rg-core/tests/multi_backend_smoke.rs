//! Runtime smoke for PostgreSQL/MySQL CI service containers.
//!
//! Run with:
//! `IRONFORGE_TEST_DATABASE_URL=... cargo test -p rg-core --test multi_backend_smoke -- --ignored`

use sea_orm::{NotSet, Set};

/// The tests in this file share one database and run in parallel. Preparing a
/// fresh database is not safe to do from two connections at once: both try to
/// record the same migration (and, on SQLite, to create the same file). The lock
/// serialises connecting and migrating; once the first caller is done, the
/// others find nothing left to apply.
static SETUP_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

async fn connect_and_migrate() -> sea_orm::DatabaseConnection {
    let database_url = std::env::var("IRONFORGE_TEST_DATABASE_URL")
        .expect("IRONFORGE_TEST_DATABASE_URL must be set");
    let _guard = SETUP_LOCK.lock().await;
    let db = rg_db::connect_with_pool(&database_url, 15, 60, 2)
        .await
        .expect("connect to test database");
    rg_db::run_migrations(&db).await.expect("run migrations");
    db
}

#[tokio::test]
#[ignore = "requires IRONFORGE_TEST_DATABASE_URL pointing at a disposable database"]
async fn migrations_crud_counters_and_fts_work_on_server_database() {
    let db = connect_and_migrate().await;

    let suffix = uuid::Uuid::new_v4().simple().to_string();
    let suffix = &suffix[..10];
    let username = format!("dbsmoke{suffix}");
    let repo_name = format!("crossbackendrepo{suffix}");
    let wiki_term = format!("crossbackendneedle{suffix}");

    let user = rg_db::ops::user_ops::create_user(
        &db,
        &username,
        &format!("{username}@example.invalid"),
        "unused",
        "Database Smoke",
    )
    .await
    .expect("create user");

    let (first, second, third, fourth, fifth) = tokio::join!(
        rg_db::ops::user_ops::record_failed_login(&db, user.id, 5),
        rg_db::ops::user_ops::record_failed_login(&db, user.id, 5),
        rg_db::ops::user_ops::record_failed_login(&db, user.id, 5),
        rg_db::ops::user_ops::record_failed_login(&db, user.id, 5),
        rg_db::ops::user_ops::record_failed_login(&db, user.id, 5),
    );
    for result in [first, second, third, fourth, fifth] {
        result.expect("atomically record concurrent failed login");
    }
    let locked_user = rg_db::ops::user_ops::find_by_id(&db, user.id)
        .await
        .expect("read locked user")
        .expect("locked user exists");
    assert_eq!(locked_user.login_attempts, 5);
    assert!(locked_user.locked_until.is_some());
    rg_db::ops::user_ops::reset_login_failures(&db, user.id)
        .await
        .expect("reset failed logins");

    let now = chrono::Utc::now();
    let repo = rg_db::ops::repo_ops::create(
        &db,
        rg_db::entities::repository::ActiveModel {
            id: NotSet,
            owner_id: Set(user.id),
            name: Set(repo_name.clone()),
            description: Set(Some("cross backend repository search".to_string())),
            is_private: Set(false),
            default_branch: Set("main".to_string()),
            fork_id: Set(None),
            stars_count: Set(0),
            forks_count: Set(0),
            org_id: Set(None),
            created_at: Set(now),
            updated_at: Set(now),
            deleted_at: Set(None),
            origin_repo_id: Set(None),
        },
    )
    .await
    .expect("create repository");

    assert!(
        rg_db::ops::repo_star_ops::toggle_star(&db, user.id, repo.id)
            .await
            .expect("create star")
    );
    rg_db::ops::repo_ops::update_stars_count(&db, repo.id)
        .await
        .expect("update star counter with backend-specific placeholders");
    let counted_repo = rg_db::ops::repo_ops::find_by_owner_and_name(&db, user.id, &repo_name)
        .await
        .expect("read repository")
        .expect("repository exists");
    assert_eq!(counted_repo.stars_count, 1);

    let page = rg_core::wiki::service::create_page(
        &db,
        repo.id,
        "Home",
        &format!("This page contains {wiki_term} for full text search."),
        Some("initial page"),
        Some(user.id),
    )
    .await
    .expect("create wiki page and synchronize FTS");

    let (wiki_results, wiki_total) = rg_core::search::service::search(
        &db,
        &format!("{wiki_term} repo:{username}/{repo_name}"),
        "wiki",
        None,
        1,
        20,
    )
    .await
    .expect("search wiki FTS with repository filter");
    assert_eq!(wiki_total, 1);
    assert_eq!(wiki_results.first().map(|result| result.id), Some(page.id));

    let (repo_results, repo_total) = rg_core::search::service::search(
        &db,
        &format!("{repo_name} author:{username}"),
        "repos",
        None,
        1,
        20,
    )
    .await
    .expect("search repository FTS with owner filter");
    assert_eq!(repo_total, 1);
    assert_eq!(repo_results.first().map(|result| result.id), Some(repo.id));

    rg_core::wiki::service::delete_page(&db, repo.id, "Home")
        .await
        .expect("delete wiki page and FTS row");
    let (_, wiki_total_after_delete) = rg_core::search::service::search(
        &db,
        &format!("{wiki_term} repo:{username}/{repo_name}"),
        "wiki",
        None,
        1,
        20,
    )
    .await
    .expect("verify wiki FTS deletion");
    assert_eq!(wiki_total_after_delete, 0);

    assert!(
        !rg_db::ops::repo_star_ops::toggle_star(&db, user.id, repo.id)
            .await
            .expect("remove smoke-test star")
    );
    rg_db::ops::repo_ops::delete_by_id(&db, repo.id)
        .await
        .expect("delete smoke-test repository");
    rg_db::ops::user_ops::delete_by_id(&db, user.id)
        .await
        .expect("delete smoke-test user");
    assert!(rg_db::ops::user_ops::find_by_id(&db, user.id)
        .await
        .expect("verify smoke-test user cleanup")
        .is_none());
}

/// Insert a repository row directly, so the database (not the service-level
/// name check) is what accepts or rejects it.
async fn insert_repo(
    db: &sea_orm::DatabaseConnection,
    owner_id: i64,
    org_id: Option<i64>,
    name: &str,
) -> anyhow::Result<rg_db::entities::repository::Model> {
    let now = chrono::Utc::now();
    rg_db::ops::repo_ops::create(
        db,
        rg_db::entities::repository::ActiveModel {
            id: NotSet,
            owner_id: Set(owner_id),
            name: Set(name.to_string()),
            description: Set(None),
            is_private: Set(false),
            default_branch: Set("main".to_string()),
            fork_id: Set(None),
            stars_count: Set(0),
            forks_count: Set(0),
            org_id: Set(org_id),
            created_at: Set(now),
            updated_at: Set(now),
            deleted_at: Set(None),
            origin_repo_id: Set(None),
        },
    )
    .await
}

fn is_unique_violation(error: &anyhow::Error) -> bool {
    error
        .downcast_ref::<sea_orm::DbErr>()
        .and_then(|e| e.sql_err())
        .is_some_and(|e| matches!(e, sea_orm::SqlErr::UniqueConstraintViolation(_)))
}

/// A live repository name is unique within its namespace *in the database*,
/// on every backend: SQLite and PostgreSQL use partial unique indexes, MySQL
/// uses unique indexes over generated columns that are NULL for dead rows.
#[tokio::test]
#[ignore = "requires IRONFORGE_TEST_DATABASE_URL pointing at a disposable database"]
async fn live_repository_names_are_unique_per_namespace_in_the_database() {
    let db = connect_and_migrate().await;

    let suffix = uuid::Uuid::new_v4().simple().to_string();
    let suffix = &suffix[..10];
    let alice = rg_db::ops::user_ops::create_user(
        &db,
        &format!("uqalice{suffix}"),
        &format!("uqalice{suffix}@example.invalid"),
        "unused",
        "Alice",
    )
    .await
    .expect("create alice");
    let bob = rg_db::ops::user_ops::create_user(
        &db,
        &format!("uqbob{suffix}"),
        &format!("uqbob{suffix}@example.invalid"),
        "unused",
        "Bob",
    )
    .await
    .expect("create bob");
    let org = rg_db::ops::org_ops::create_org(
        &db,
        &format!("uqorg{suffix}"),
        None,
        None,
        alice.id,
        "public",
    )
    .await
    .expect("create organization");
    let name = format!("shared{suffix}");

    // Personal namespace: a second live row with the same name is refused.
    let first = insert_repo(&db, alice.id, None, &name)
        .await
        .expect("first personal repository");
    let duplicate = insert_repo(&db, alice.id, None, &name)
        .await
        .expect_err("a second live personal repository with the same name");
    assert!(
        is_unique_violation(&duplicate),
        "expected a unique violation, got: {duplicate:#}"
    );

    // Another user's namespace and the organization's namespace are separate,
    // even though the organization repo's `owner_id` is alice's.
    let bobs = insert_repo(&db, bob.id, None, &name)
        .await
        .expect("same name under another user");
    let orgs = insert_repo(&db, alice.id, Some(org.id), &name)
        .await
        .expect("same name in an organization owned by the same user");

    // Organization namespace: uniqueness is per organization, not per creator.
    let org_duplicate = insert_repo(&db, bob.id, Some(org.id), &name)
        .await
        .expect_err("a second live organization repository with the same name");
    assert!(
        is_unique_violation(&org_duplicate),
        "expected a unique violation, got: {org_duplicate:#}"
    );

    // Soft-deleting frees the name for reuse in that namespace only.
    rg_db::ops::repo_ops::soft_delete(&db, first.id)
        .await
        .expect("soft-delete the personal repository");
    let reused = insert_repo(&db, alice.id, None, &name)
        .await
        .expect("a soft-deleted name can be reused");
    assert_ne!(reused.id, first.id);
    let still_duplicate = insert_repo(&db, alice.id, None, &name)
        .await
        .expect_err("the reused name is live again");
    assert!(is_unique_violation(&still_duplicate));

    // Several soft-deleted rows may share one name (they are all dead).
    rg_db::ops::repo_ops::soft_delete(&db, reused.id)
        .await
        .expect("soft-delete the reused repository");
    let again = insert_repo(&db, alice.id, None, &name)
        .await
        .expect("name is free after the second delete");

    // Concurrent creators racing for one free name: exactly one wins.
    let racing = format!("racing{suffix}");
    let attempts = tokio::join!(
        insert_repo(&db, alice.id, None, &racing),
        insert_repo(&db, alice.id, None, &racing),
        insert_repo(&db, alice.id, None, &racing),
        insert_repo(&db, alice.id, None, &racing),
        insert_repo(&db, alice.id, None, &racing),
        insert_repo(&db, alice.id, None, &racing),
    );
    let results = [
        attempts.0, attempts.1, attempts.2, attempts.3, attempts.4, attempts.5,
    ];
    let winners: Vec<_> = results.iter().filter_map(|r| r.as_ref().ok()).collect();
    assert_eq!(winners.len(), 1, "exactly one concurrent insert may win");
    for result in results.iter().filter_map(|r| r.as_ref().err()) {
        assert!(
            is_unique_violation(result),
            "losers must fail with a unique violation, got: {result:#}"
        );
    }
    let racing_id = winners[0].id;

    for id in [bobs.id, orgs.id, again.id, racing_id] {
        rg_db::ops::repo_ops::delete_by_id(&db, id)
            .await
            .expect("clean up repository");
    }
    for id in [first.id, reused.id] {
        rg_db::ops::repo_ops::delete_by_id(&db, id)
            .await
            .expect("clean up soft-deleted repository");
    }
    rg_db::ops::org_ops::delete_org(&db, org.id)
        .await
        .expect("clean up organization");
    for id in [alice.id, bob.id] {
        rg_db::ops::user_ops::delete_by_id(&db, id)
            .await
            .expect("clean up user");
    }
}
