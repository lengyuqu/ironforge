//! `repo_events.dispatched_at` — when an event was handed to webhook delivery
//! (#15, step 7: the minimal outbox).
//!
//! `NULL` means "the delivery system never took this row", the only state a
//! fallback sweep can act on. It is **not** a delivery *result*: success and
//! failure still live in `webhook_deliveries`, and the durable retry queue is
//! unchanged. The column answers exactly one question: does anybody still owe
//! this event a first delivery attempt?
//!
//! # Existing rows are back-filled with their `created_at`
//!
//! Every row written before this column existed was written by a binary whose
//! `trigger_event` delivered synchronously right after the commit — the sweep
//! must never treat them as missed. Leaving them `NULL` would replay the whole
//! retained event log (7 days) to every hook of every repository the first time
//! the worker ran. Back-filling is the safe default because it depends on no
//! runtime state: "it existed before the column did, so it is not our business".
//!
//! The back-fill runs **only in the pass that adds the column**. A re-run on a
//! schema that already has it (a rolled-back and re-applied migration, or a
//! partially applied one) must not mark rows the sweep still owes a delivery.
//!
//! # Index
//!
//! The sweep asks for `dispatched_at IS NULL AND created_at BETWEEN ? AND ?`,
//! so `(dispatched_at, created_at)` is the useful key: the `NULL` prefix is
//! selective (almost every row is non-null) and the timestamp column then
//! range-scans inside it. The existing `idx_repo_events_created_at` stays — the
//! retention sweep (`delete_older_than`) only knows `created_at`.
//!
//! `up` is idempotent: the column and the index are each guarded.

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261117_000001_add_repo_events_dispatched_at"
    }
}

const TABLE: &str = "repo_events";
const COLUMN: &str = "dispatched_at";
const IX_DISPATCHED: &str = "idx_repo_events_dispatched_at";

// Explicit table name: a derived `Iden` would produce the singular
// `repo_event` (CLAUDE.md pitfall 13).
#[derive(DeriveIden)]
enum RepoEvents {
    #[sea_orm(iden = "repo_events")]
    Table,
    DispatchedAt,
    CreatedAt,
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        if !manager.has_table(TABLE).await? {
            return Ok(());
        }

        if !manager.has_column(TABLE, COLUMN).await? {
            manager
                .alter_table(
                    Table::alter()
                        .table(RepoEvents::Table)
                        .add_column(
                            ColumnDef::new(RepoEvents::DispatchedAt)
                                .timestamp_with_time_zone()
                                .null(),
                        )
                        .to_owned(),
                )
                .await?;

            // Fresh column: every row predates it, so none of them is a missed
            // delivery. Back-fill before the worker can ever see a `NULL`.
            manager
                .get_connection()
                .execute_unprepared(
                    "UPDATE repo_events SET dispatched_at = created_at \
                     WHERE dispatched_at IS NULL",
                )
                .await?;
        }

        if !manager.has_index(TABLE, IX_DISPATCHED).await? {
            manager
                .create_index(
                    Index::create()
                        .if_not_exists()
                        .name(IX_DISPATCHED)
                        .table(RepoEvents::Table)
                        .col(RepoEvents::DispatchedAt)
                        .col(RepoEvents::CreatedAt)
                        .to_owned(),
                )
                .await?;
        }

        Ok(())
    }

    /// Drops the index first (SQLite refuses to drop a column an index names),
    /// then the column. A nullable column would be harmless to older code, but
    /// dropping it keeps "column exists" ⇔ "the back-fill ran" true, so a
    /// rollback followed by a re-apply converges on the same state.
    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        if !manager.has_table(TABLE).await? {
            return Ok(());
        }
        if manager.has_index(TABLE, IX_DISPATCHED).await? {
            manager
                .drop_index(
                    Index::drop()
                        .name(IX_DISPATCHED)
                        .table(RepoEvents::Table)
                        .to_owned(),
                )
                .await?;
        }
        if manager.has_column(TABLE, COLUMN).await? {
            manager
                .alter_table(
                    Table::alter()
                        .table(RepoEvents::Table)
                        .drop_column(RepoEvents::DispatchedAt)
                        .to_owned(),
                )
                .await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::{ConnectionTrait, Database, Statement};
    use sea_orm_migration::sea_query::{
        MysqlQueryBuilder, PostgresQueryBuilder, SqliteQueryBuilder,
    };

    async fn scalar(db: &sea_orm::DatabaseConnection, sql: &str) -> i64 {
        let row = db
            .query_one(Statement::from_string(db.get_database_backend(), sql))
            .await
            .unwrap()
            .unwrap();
        row.try_get_by_index::<i64>(0).unwrap()
    }

    /// The DDL names the plural table on every backend (CLAUDE.md pitfall 13).
    #[test]
    fn the_column_and_index_use_the_plural_table_name() {
        let alter = Table::alter()
            .table(RepoEvents::Table)
            .add_column(
                ColumnDef::new(RepoEvents::DispatchedAt)
                    .timestamp_with_time_zone()
                    .null(),
            )
            .to_owned();
        let index = Index::create()
            .name(IX_DISPATCHED)
            .table(RepoEvents::Table)
            .col(RepoEvents::DispatchedAt)
            .col(RepoEvents::CreatedAt)
            .to_owned();
        for sql in [
            alter.to_string(SqliteQueryBuilder),
            alter.to_string(PostgresQueryBuilder),
            alter.to_string(MysqlQueryBuilder),
        ] {
            assert!(sql.contains(TABLE), "plural table missing: {sql}");
            assert!(sql.contains(COLUMN), "column missing: {sql}");
        }
        for sql in [
            index.to_string(SqliteQueryBuilder),
            index.to_string(PostgresQueryBuilder),
            index.to_string(MysqlQueryBuilder),
        ] {
            assert!(sql.contains(IX_DISPATCHED), "index name missing: {sql}");
            assert!(sql.contains(TABLE), "plural table missing: {sql}");
        }
    }

    /// The dangerous half: a row that existed before the column must come out
    /// with a non-`NULL` `dispatched_at`, or the sweep would replay it.
    #[tokio::test]
    async fn existing_rows_are_backfilled_with_their_created_at() {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        // Stop just before this migration: the table exists, the column does not.
        let before = crate::migrations::Migrator::migrations()
            .iter()
            .position(|migration| {
                migration.name() == "m20261117_000001_add_repo_events_dispatched_at"
            })
            .expect("this migration must be registered");
        crate::migrations::Migrator::up(&db, Some(before as u32))
            .await
            .unwrap();

        // A historical event, written by a binary that delivered it itself.
        db.execute_unprepared(
            "INSERT INTO repo_events (repo_id, event_type, payload, created_at) \
             VALUES (1, 'push', '{\"schema\":1}', '2026-01-01 00:00:00 +00:00')",
        )
        .await
        .expect("insert a pre-column row");

        crate::migrations::Migrator::up(&db, None).await.unwrap();
        assert_eq!(
            scalar(
                &db,
                "SELECT COUNT(*) FROM repo_events WHERE dispatched_at IS NULL"
            )
            .await,
            0,
            "a pre-column row must be marked as already handled"
        );
        // The value is the row's own `created_at`, not "now": the state says
        // "this predates the column", which is what the sweep reasons about.
        let row = db
            .query_one(Statement::from_string(
                db.get_database_backend(),
                "SELECT CAST(created_at AS TEXT), CAST(dispatched_at AS TEXT) \
                 FROM repo_events WHERE repo_id = 1",
            ))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            row.try_get_by_index::<String>(0).unwrap(),
            row.try_get_by_index::<String>(1).unwrap(),
            "the back-fill must copy created_at"
        );

        // A row written after the column exists stays `NULL`: it is exactly
        // what the sweep is for, so the back-fill must not touch it.
        db.execute_unprepared(
            "INSERT INTO repo_events (repo_id, event_type, payload, created_at) \
             VALUES (1, 'push', '{\"schema\":1}', '2026-01-02 00:00:00 +00:00')",
        )
        .await
        .expect("insert a post-column row");
        let manager = SchemaManager::new(&db);
        Migration.up(&manager).await.expect("a re-run converges");
        assert_eq!(
            scalar(
                &db,
                "SELECT COUNT(*) FROM repo_events WHERE dispatched_at IS NULL"
            )
            .await,
            1,
            "a re-run must not swallow the row the sweep still owes"
        );
    }

    /// `down` removes the column, and re-applying restores it and back-fills
    /// again (the rollback round trip the migration test in `lib.rs` exercises).
    #[tokio::test]
    async fn down_drops_the_column_and_up_restores_it() {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        crate::migrations::Migrator::up(&db, None).await.unwrap();
        let manager = SchemaManager::new(&db);
        assert!(manager.has_column(TABLE, COLUMN).await.unwrap());

        Migration.down(&manager).await.unwrap();
        assert!(!manager.has_column(TABLE, COLUMN).await.unwrap());
        assert!(
            !manager.has_index(TABLE, IX_DISPATCHED).await.unwrap(),
            "the index must go with the column"
        );

        Migration.up(&manager).await.unwrap();
        assert!(manager.has_column(TABLE, COLUMN).await.unwrap());
        assert!(manager.has_index(TABLE, IX_DISPATCHED).await.unwrap());
    }
}
