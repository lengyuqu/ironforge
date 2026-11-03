//! C01: the collaboration migration on a fresh database, as an upgrade of an
//! existing one, and through a rollback.
//!
//! The rollback mutates the schema, so this test never touches a shared
//! database: it is ignored by default and needs a dedicated one.
//!
//! ```text
//! IRONFORGE_TEST_DATABASE_URL=postgres://.../dedicated_db \
//!   cargo test -p rg-db --test collab_migration -- --ignored
//! ```
//! Without the variable it uses a throw-away SQLite file.

use rg_db::migrations::Migrator;
use rg_db::sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
use sea_orm_migration::MigratorTrait;

const TABLES: [&str; 5] = [
    "agent_sessions",
    "collab_tasks",
    "collab_messages",
    "collab_handoffs",
    "collab_reads",
];

async fn existing_tables(db: &rg_db::DatabaseConnection) -> Vec<String> {
    let backend = db.get_database_backend();
    let sql = match backend {
        DatabaseBackend::Sqlite => "SELECT name FROM sqlite_master WHERE type = 'table'",
        DatabaseBackend::Postgres => "SELECT table_name FROM information_schema.tables WHERE table_schema = current_schema()",
        DatabaseBackend::MySql => "SELECT table_name FROM information_schema.tables WHERE table_schema = DATABASE()",
    };
    db.query_all(Statement::from_string(backend, sql.to_string()))
        .await
        .unwrap()
        .iter()
        .filter_map(|row| row.try_get_by_index::<String>(0).ok())
        .collect()
}

async fn present(db: &rg_db::DatabaseConnection) -> Vec<bool> {
    let tables = existing_tables(db).await;
    TABLES.iter().map(|t| tables.iter().any(|x| x.eq_ignore_ascii_case(t))).collect()
}

#[tokio::test]
#[ignore = "rolls the schema back; run against a dedicated database"]
async fn upgrade_rollback_and_reapply_on_the_configured_backend() {
    let url = std::env::var("IRONFORGE_TEST_DATABASE_URL").unwrap_or_else(|_| {
        let path = std::env::temp_dir().join(format!("ironforge_collab_migration_{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        format!("sqlite://{}?mode=rwc", path.display())
    });
    let db = rg_db::connect_with_pool(&url, 20, 60, 4).await.expect("connect");

    // Bring the database to the state just before this migration: the
    // "existing deployment" an upgrade starts from.
    rg_db::run_migrations(&db).await.expect("migrate");
    let all = Migrator::migrations().len();
    Migrator::down(&db, Some(1)).await.expect("roll back the newest migration");
    assert_eq!(present(&db).await, vec![false; 5], "rollback removes every collaboration table");
    let applied: Vec<_> = Migrator::get_applied_migrations(&db).await.unwrap();
    assert_eq!(applied.len(), all - 1);

    // Upgrade: applying the pending migration creates the tables over existing data.
    Migrator::up(&db, None).await.expect("upgrade");
    assert_eq!(present(&db).await, vec![true; 5]);
    // Applying again is a no-op.
    Migrator::up(&db, None).await.expect("second up");
    rg_db::run_migrations(&db).await.expect("run_migrations is idempotent");
    assert_eq!(present(&db).await, vec![true; 5]);

    // And the schema is usable after the round trip: constraints still bite.
    let backend = db.get_database_backend();
    let bad = db
        .execute(Statement::from_string(
            backend,
            "INSERT INTO collab_tasks (id, repo_id, title, status, paths_json, version, next_seq, created_at, updated_at) \
             VALUES ('00000000-0000-4000-8000-000000000001', 999999999, 't', 'open', '[]', 1, 0, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)"
                .to_string(),
        ))
        .await;
    assert!(bad.is_err(), "a task for a nonexistent repository must violate the foreign key");
}
