//! Per-commit completeness counters on `symbol_commits` (#63 step 4, T31).
//!
//! The writer already knows, for every file it parses, whether the extractor
//! reported a parse error (`Outline::error`) or hit the item cap
//! (`Outline::truncated`). Until this migration it threw that knowledge away,
//! so the reader could only answer `partial: true` for *every* commit — a
//! statement that is honest but carries no information.
//!
//! Three counters record what one indexing run saw:
//!
//! * `files_total` — files this run actually read and offered to the extractor;
//! * `files_failed` — of those, how many came back with a parse error;
//! * `files_truncated` — of those, how many hit the item cap.
//!
//! # Rows that predate the counters
//!
//! `NOT NULL DEFAULT 0` would make every row written by an older release read
//! as "0 files, 0 failures" — a *claim of completeness* that release never
//! made and cannot support. `up` therefore backfills `files_total = -1` on the
//! rows that exist at migration time, the sentinel
//! [`crate::entities::symbol_commit::UNKNOWN_FILES_TOTAL`]. A sentinel was
//! preferred over a `stats_known` boolean: one column instead of two, and
//! `files_total` has no other negative value, so "unknown" cannot be confused
//! with a measurement. The reader (`rg-core`'s `symbol_query`) treats it as
//! `partial: true`.
//!
//! `up` is idempotent: each column is guarded by `has_column`, and the backfill
//! runs only in the same branch that adds `files_total` — re-running `up` on a
//! database that already has the column never rewrites measured statistics.
//! `down` drops the three columns in reverse order and is likewise guarded, so
//! it can run on SQLite (3.35+), PostgreSQL and MySQL alike.

use sea_orm_migration::prelude::*;

use crate::entities::symbol_commit::UNKNOWN_FILES_TOTAL;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261118_000002_symbol_commit_completeness"
    }
}

const SYMBOL_COMMITS: &str = "symbol_commits";

#[derive(DeriveIden)]
enum SymbolCommits {
    #[sea_orm(iden = "symbol_commits")]
    Table,
    FilesTotal,
    FilesFailed,
    FilesTruncated,
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        if !manager.has_column(SYMBOL_COMMITS, "files_total").await? {
            manager
                .alter_table(
                    Table::alter()
                        .table(SymbolCommits::Table)
                        .add_column(
                            ColumnDef::new(SymbolCommits::FilesTotal)
                                .integer()
                                .not_null()
                                .default(0)
                                .to_owned(),
                        )
                        .to_owned(),
                )
                .await?;
            // Every row that exists at this point was written by a release that
            // did not count anything: mark it unknown instead of complete. The
            // column was just added, so `0` is only ever the synthetic default
            // here, never a measurement.
            manager
                .get_connection()
                .execute_unprepared(&format!(
                    "UPDATE {SYMBOL_COMMITS} SET files_total = {UNKNOWN_FILES_TOTAL}"
                ))
                .await?;
        }

        for (name, column) in [
            ("files_failed", SymbolCommits::FilesFailed),
            ("files_truncated", SymbolCommits::FilesTruncated),
        ] {
            if !manager.has_column(SYMBOL_COMMITS, name).await? {
                manager
                    .alter_table(
                        Table::alter()
                            .table(SymbolCommits::Table)
                            .add_column(
                                ColumnDef::new(column)
                                    .integer()
                                    .not_null()
                                    .default(0)
                                    .to_owned(),
                            )
                            .to_owned(),
                    )
                    .await?;
            }
        }
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Reverse of `up`. Each drop is guarded so a partially rolled back
        // database converges instead of failing on the second run.
        for (name, column) in [
            ("files_truncated", SymbolCommits::FilesTruncated),
            ("files_failed", SymbolCommits::FilesFailed),
            ("files_total", SymbolCommits::FilesTotal),
        ] {
            if manager.has_column(SYMBOL_COMMITS, name).await? {
                manager
                    .alter_table(
                        Table::alter()
                            .table(SymbolCommits::Table)
                            .drop_column(column)
                            .to_owned(),
                    )
                    .await?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::ActiveModelTrait;
    use sea_orm::ActiveValue::Set;
    use sea_orm::{ConnectionTrait, Database, DatabaseConnection, Statement};
    use sea_orm_migration::MigratorTrait;

    const SHA: &str = "0123456789abcdef0123456789abcdef01234567";

    async fn count(db: &DatabaseConnection, sql: &str) -> i64 {
        db.query_one(Statement::from_string(db.get_database_backend(), sql))
            .await
            .unwrap()
            .unwrap()
            .try_get_by_index::<i64>(0)
            .unwrap()
    }

    async fn column_exists(db: &DatabaseConnection, column: &str) -> i64 {
        count(
            db,
            &format!(
                "SELECT COUNT(*) FROM pragma_table_info('{SYMBOL_COMMITS}') WHERE name = '{column}'"
            ),
        )
        .await
    }

    async fn files_total(db: &DatabaseConnection) -> i64 {
        // `files_total` is `INTEGER` (INT4 on PostgreSQL); read it at its own
        // width instead of casting, which differs between the three backends.
        db.query_one(Statement::from_string(
            db.get_database_backend(),
            format!("SELECT files_total FROM {SYMBOL_COMMITS} LIMIT 1"),
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get_by_index::<i32>(0)
        .unwrap() as i64
    }

    /// A repository and one `symbol_commits` row, so the backfill has something
    /// to rewrite.
    async fn seed_commit(db: &DatabaseConnection) {
        let now = chrono::Utc::now();
        let owner = crate::entities::user::ActiveModel {
            username: Set("stats-owner".to_string()),
            email: Set("stats@example.com".to_string()),
            password_hash: Set("unused".to_string()),
            display_name: Set(None),
            is_admin: Set(false),
            is_active: Set(true),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap();
        let repo = crate::entities::repository::ActiveModel {
            owner_id: Set(owner.id),
            name: Set("stats".to_string()),
            default_branch: Set("main".to_string()),
            is_private: Set(false),
            stars_count: Set(0),
            forks_count: Set(0),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap();
        db.execute_unprepared(&format!(
            "INSERT INTO {SYMBOL_COMMITS} (repo_id, commit_sha, created_at) \
             VALUES ({}, '{SHA}', '{}')",
            repo.id, now
        ))
        .await
        .unwrap();
    }

    /// The whole point of the backfill: a row written before the counters must
    /// not come out of the migration claiming to be complete.
    ///
    /// The pre-counter schema is produced by rolling this migration back rather
    /// than by hardcoding a step count, so the test keeps working when more
    /// migrations are appended.
    #[tokio::test]
    async fn existing_rows_are_marked_unknown_not_complete() {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        crate::migrations::Migrator::up(&db, None).await.unwrap();
        let manager = SchemaManager::new(&db);
        Migration.down(&manager).await.unwrap();
        assert_eq!(column_exists(&db, "files_total").await, 0);

        seed_commit(&db).await;

        Migration.up(&manager).await.unwrap();
        for column in ["files_total", "files_failed", "files_truncated"] {
            assert_eq!(column_exists(&db, column).await, 1, "{column} missing");
        }
        assert_eq!(
            files_total(&db).await,
            UNKNOWN_FILES_TOTAL as i64,
            "a pre-counter row must not read as complete"
        );
        assert_eq!(
            count(
                &db,
                &format!("SELECT files_failed + files_truncated FROM {SYMBOL_COMMITS} LIMIT 1")
            )
            .await,
            0,
            "the other counters keep their neutral default"
        );
    }

    /// Re-running `up` must not touch measured statistics, and `down` must
    /// remove all three columns while keeping the rows.
    #[tokio::test]
    async fn up_is_idempotent_and_down_drops_the_columns() {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        crate::migrations::Migrator::up(&db, None).await.unwrap();
        seed_commit(&db).await;
        db.execute_unprepared(&format!(
            "UPDATE {SYMBOL_COMMITS} SET files_total = 42, files_failed = 3, files_truncated = 1"
        ))
        .await
        .unwrap();

        let manager = SchemaManager::new(&db);
        Migration.up(&manager).await.unwrap();
        assert_eq!(
            files_total(&db).await,
            42,
            "the guard must stop the backfill from rewriting a measurement"
        );

        Migration.down(&manager).await.unwrap();
        for column in ["files_total", "files_failed", "files_truncated"] {
            assert_eq!(column_exists(&db, column).await, 0, "{column} survived");
        }
        assert_eq!(
            count(&db, &format!("SELECT COUNT(*) FROM {SYMBOL_COMMITS}")).await,
            1,
            "rolling back the columns must not delete rows"
        );

        Migration.up(&manager).await.unwrap();
        assert_eq!(column_exists(&db, "files_total").await, 1);
        assert_eq!(
            files_total(&db).await,
            UNKNOWN_FILES_TOTAL as i64,
            "a row whose counters were dropped is unknown again"
        );
    }

    /// Rolling back and re-applying the whole suite, on PostgreSQL or MySQL when
    /// `IRONFORGE_TEST_DATABASE_URL` names one. SQLite is covered above.
    #[tokio::test]
    async fn round_trip_on_the_backend_from_the_environment() {
        let Ok(url) = std::env::var("IRONFORGE_TEST_DATABASE_URL") else {
            return;
        };
        let db = crate::connect_with_pool(&url, 5, 60, 4)
            .await
            .expect("connect to IRONFORGE_TEST_DATABASE_URL");
        crate::migrations::Migrator::up(&db, None).await.unwrap();
        let manager = SchemaManager::new(&db);
        assert!(manager
            .has_column(SYMBOL_COMMITS, "files_total")
            .await
            .unwrap());
        assert!(manager
            .has_column(SYMBOL_COMMITS, "files_failed")
            .await
            .unwrap());
        assert!(manager
            .has_column(SYMBOL_COMMITS, "files_truncated")
            .await
            .unwrap());

        // This migration is registered last, so one step down is this one.
        crate::migrations::Migrator::down(&db, Some(1))
            .await
            .unwrap();
        assert!(!manager
            .has_column(SYMBOL_COMMITS, "files_total")
            .await
            .unwrap());

        // Insert a row on the pre-counter schema: the backfill and the
        // `has_column` guard must work here too, not only on SQLite. The entity
        // cannot be used while the columns are missing, hence raw SQL.
        let (repo_id, user_id) = seed_repository_rows(&db).await;
        db.execute_unprepared(&format!(
            "INSERT INTO {SYMBOL_COMMITS} (repo_id, commit_sha, created_at) \
             VALUES ({repo_id}, '{SHA}', '2026-11-18 00:00:00')"
        ))
        .await
        .unwrap();

        crate::migrations::Migrator::up(&db, None).await.unwrap();
        assert!(manager
            .has_column(SYMBOL_COMMITS, "files_total")
            .await
            .unwrap());
        assert_eq!(
            db.query_one(Statement::from_string(
                db.get_database_backend(),
                format!("SELECT files_total FROM {SYMBOL_COMMITS} WHERE repo_id = {repo_id}"),
            ))
            .await
            .unwrap()
            .unwrap()
            .try_get_by_index::<i32>(0)
            .unwrap(),
            UNKNOWN_FILES_TOTAL,
            "the backfill must mark pre-counter rows unknown on this backend"
        );

        // Shared database: remove the rows this test created, children first.
        for sql in [
            format!("DELETE FROM {SYMBOL_COMMITS} WHERE repo_id = {repo_id}"),
            format!("DELETE FROM repositories WHERE id = {repo_id}"),
            format!("DELETE FROM users WHERE id = {user_id}"),
        ] {
            db.execute_unprepared(&sql).await.unwrap();
        }
    }

    /// `(repository, user)` rows with per-process unique names, so a shared
    /// environment database can be reused by concurrent runs.
    async fn seed_repository_rows(db: &DatabaseConnection) -> (i64, i64) {
        let suffix = std::process::id();
        let now = chrono::Utc::now();
        let owner = crate::entities::user::ActiveModel {
            username: Set(format!("t31-{suffix}")),
            email: Set(format!("t31-{suffix}@example.com")),
            password_hash: Set("unused".to_string()),
            display_name: Set(None),
            is_admin: Set(false),
            is_active: Set(true),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap();
        let repo = crate::entities::repository::ActiveModel {
            owner_id: Set(owner.id),
            name: Set(format!("t31-{suffix}")),
            default_branch: Set("main".to_string()),
            is_private: Set(false),
            stars_count: Set(0),
            forks_count: Set(0),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap();
        (repo.id, owner.id)
    }
}
