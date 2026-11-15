//! `notification_preferences`: per user, per repository, per event type, per
//! channel opt-outs (#19 step 5, T12).
//!
//! **Absence of a row means the channel is on.** The table only records
//! deviations (`enabled = false`), so a new account starts fully subscribed
//! without anyone writing a row, and a new event type is subscribed by default
//! too. Reading is therefore a resolution, not a lookup:
//!
//! 1. a row for `(user, repo, event, channel)` wins — the repository override;
//! 2. otherwise a row with `repo_id IS NULL` — the global default;
//! 3. otherwise on.
//!
//! `repo_id` is nullable on purpose (`NULL` = global). The unique key
//! `(user_id, repo_id, event_type, channel)` is declared as specified, but SQL
//! treats two `NULL`s as distinct on every backend this project supports, so the
//! database alone does **not** deduplicate global rows; the ops layer
//! (`notification_preference_ops::upsert`) finds the existing row before
//! writing and the reader collapses any duplicate deterministically. Do not
//! "fix" this by inventing a sentinel `repo_id = 0`: `NULL` is the documented
//! semantics of "global".
//!
//! Both foreign keys cascade: a deleted account or repository takes its
//! preferences with it, since a preference about a repository that no longer
//! exists can never be consulted again.
//!
//! `up` is idempotent (table and indexes are each guarded).

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261115_000002_create_notification_preferences"
    }
}

const TABLE: &str = "notification_preferences";
const UQ_SCOPE: &str = "uq_notification_preferences_scope";
const IX_USER: &str = "ix_notification_preferences_user";

// Explicit table name matching the entity's `table_name` (CLAUDE.md pitfall 13):
// a derived `Iden` would produce the singular `notification_preference`.
#[derive(DeriveIden)]
enum NotificationPreferences {
    #[sea_orm(iden = "notification_preferences")]
    Table,
    Id,
    UserId,
    RepoId,
    EventType,
    Channel,
    Enabled,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum Users {
    Table,
    Id,
}

#[derive(DeriveIden)]
enum Repositories {
    Table,
    Id,
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        if manager.has_table(TABLE).await? {
            // Repair a partially applied schema: every column is guarded, so a
            // re-run after a failed create converges.
            for (name, definition) in column_defs() {
                if !manager.has_column(TABLE, name).await? {
                    manager
                        .alter_table(
                            Table::alter()
                                .table(NotificationPreferences::Table)
                                .add_column(definition)
                                .to_owned(),
                        )
                        .await?;
                }
            }
        } else {
            manager.create_table(table_create()).await?;
        }
        if !manager.has_index(TABLE, UQ_SCOPE).await? {
            manager.create_index(scope_index()).await?;
        }
        if !manager.has_index(TABLE, IX_USER).await? {
            manager.create_index(user_index()).await?;
        }
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(
                Table::drop()
                    .table(NotificationPreferences::Table)
                    .if_exists()
                    .to_owned(),
            )
            .await
    }
}

/// `(column name, definition)`, in table order.
fn column_defs() -> Vec<(&'static str, ColumnDef)> {
    vec![
        (
            "id",
            ColumnDef::new(NotificationPreferences::Id)
                .big_integer()
                .not_null()
                .auto_increment()
                .primary_key()
                .to_owned(),
        ),
        (
            "user_id",
            ColumnDef::new(NotificationPreferences::UserId)
                .big_integer()
                .not_null()
                .to_owned(),
        ),
        // `NULL` = the global default; a value = the override for that repository.
        (
            "repo_id",
            ColumnDef::new(NotificationPreferences::RepoId)
                .big_integer()
                .null()
                .to_owned(),
        ),
        // Same value space as `notifications.event_type`.
        (
            "event_type",
            ColumnDef::new(NotificationPreferences::EventType)
                .string_len(64)
                .not_null()
                .to_owned(),
        ),
        // `in_app` | `email`.
        (
            "channel",
            ColumnDef::new(NotificationPreferences::Channel)
                .string_len(16)
                .not_null()
                .to_owned(),
        ),
        (
            "enabled",
            ColumnDef::new(NotificationPreferences::Enabled)
                .boolean()
                .not_null()
                .to_owned(),
        ),
        (
            "updated_at",
            ColumnDef::new(NotificationPreferences::UpdatedAt)
                .timestamp_with_time_zone()
                .not_null()
                .to_owned(),
        ),
    ]
}

fn table_create() -> TableCreateStatement {
    let mut table = Table::create();
    table.table(NotificationPreferences::Table).if_not_exists();
    for (_, definition) in column_defs() {
        table.col(definition);
    }
    table
        .foreign_key(
            ForeignKey::create()
                .from(
                    NotificationPreferences::Table,
                    NotificationPreferences::UserId,
                )
                .to(Users::Table, Users::Id)
                .on_delete(ForeignKeyAction::Cascade),
        )
        .foreign_key(
            ForeignKey::create()
                .from(
                    NotificationPreferences::Table,
                    NotificationPreferences::RepoId,
                )
                .to(Repositories::Table, Repositories::Id)
                .on_delete(ForeignKeyAction::Cascade),
        )
        .to_owned()
}

fn scope_index() -> IndexCreateStatement {
    Index::create()
        .if_not_exists()
        .name(UQ_SCOPE)
        .table(NotificationPreferences::Table)
        .col(NotificationPreferences::UserId)
        .col(NotificationPreferences::RepoId)
        .col(NotificationPreferences::EventType)
        .col(NotificationPreferences::Channel)
        .unique()
        .to_owned()
}

/// Covers "all preferences of one user", which is what both the reader and the
/// settings page ask for.
fn user_index() -> IndexCreateStatement {
    Index::create()
        .if_not_exists()
        .name(IX_USER)
        .table(NotificationPreferences::Table)
        .col(NotificationPreferences::UserId)
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::{ConnectionTrait, Database, Statement};
    use sea_orm_migration::sea_query::{
        MysqlQueryBuilder, PostgresQueryBuilder, SqliteQueryBuilder,
    };
    use sea_orm_migration::MigratorTrait;

    async fn count(db: &sea_orm::DatabaseConnection, sql: &str) -> i64 {
        let row = db
            .query_one(Statement::from_string(db.get_database_backend(), sql))
            .await
            .unwrap()
            .unwrap();
        row.try_get_by_index::<i64>(0).unwrap()
    }

    async fn table_exists(db: &sea_orm::DatabaseConnection, table: &str) -> i64 {
        count(
            db,
            &format!(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = '{table}'"
            ),
        )
        .await
    }

    /// CLAUDE.md pitfall #13: the plural table name, the nullable `repo_id` and
    /// the constraints survive every dialect the platform supports.
    #[test]
    fn ddl_uses_the_plural_table_name_and_constraints_on_all_backends() {
        let table = table_create();
        for sql in [
            table.to_string(SqliteQueryBuilder),
            table.to_string(PostgresQueryBuilder),
            table.to_string(MysqlQueryBuilder),
        ] {
            assert!(sql.contains(TABLE), "{TABLE} missing in: {sql}");
            assert!(
                !sql.contains("notification_preference "),
                "singular table in: {sql}"
            );
            assert!(
                sql.contains("FOREIGN KEY (\"user_id\")")
                    || sql.contains("FOREIGN KEY (`user_id`)"),
                "user foreign key missing: {sql}"
            );
            assert!(
                sql.contains("FOREIGN KEY (\"repo_id\")")
                    || sql.contains("FOREIGN KEY (`repo_id`)"),
                "repo foreign key missing: {sql}"
            );
        }
        // The unique key must list all four columns: a shorter key would allow a
        // second row for the same scope and make the resolution ambiguous.
        let sql = scope_index().to_string(SqliteQueryBuilder);
        for column in ["user_id", "repo_id", "event_type", "channel"] {
            assert!(
                sql.contains(column),
                "{column} missing from the unique key: {sql}"
            );
        }
        assert!(
            sql.contains("UNIQUE"),
            "the scope key must be unique: {sql}"
        );
        for (name, index) in [(UQ_SCOPE, scope_index()), (IX_USER, user_index())] {
            for sql in [
                index.to_string(SqliteQueryBuilder),
                index.to_string(PostgresQueryBuilder),
                index.to_string(MysqlQueryBuilder),
            ] {
                assert!(sql.contains(name), "{name} missing in: {sql}");
            }
        }
    }

    /// How many migrations to roll back so this one is undone. Counting from the
    /// end keeps the test correct when a later migration is appended.
    fn rollback_steps() -> u32 {
        let migrations = crate::migrations::Migrator::migrations();
        let position = migrations
            .iter()
            .rev()
            .position(|migration| {
                migration.name() == "m20261115_000002_create_notification_preferences"
            })
            .expect("this migration must be registered");
        (position + 1) as u32
    }

    /// `down` really drops the table and both indexes; `up` recreates them.
    #[tokio::test]
    async fn down_drops_the_table_and_up_recreates_it() {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        crate::migrations::Migrator::up(&db, None).await.unwrap();
        assert_eq!(table_exists(&db, TABLE).await, 1);

        crate::migrations::Migrator::down(&db, Some(rollback_steps()))
            .await
            .unwrap();
        assert_eq!(
            table_exists(&db, TABLE).await,
            0,
            "down must drop the table"
        );

        crate::migrations::Migrator::up(&db, None).await.unwrap();
        assert_eq!(table_exists(&db, TABLE).await, 1);
        assert_eq!(
            count(
                &db,
                &format!(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name = '{UQ_SCOPE}'"
                )
            )
            .await,
            1,
            "the unique scope key must be back after re-apply"
        );
    }

    /// Re-running `up` on a schema that already has the table converges instead
    /// of erroring, and restores a column dropped by hand.
    #[tokio::test]
    async fn up_is_idempotent_and_repairs_a_missing_column() {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        crate::migrations::Migrator::up(&db, None).await.unwrap();
        db.execute_unprepared("ALTER TABLE notification_preferences DROP COLUMN enabled")
            .await
            .expect("sqlite 3.35+ can drop a column");
        let manager = SchemaManager::new(&db);
        Migration.up(&manager).await.unwrap();
        assert_eq!(
            count(
                &db,
                "SELECT COUNT(*) FROM pragma_table_info('notification_preferences') \
                 WHERE name = 'enabled'"
            )
            .await,
            1,
            "the guarded re-run must restore the missing column"
        );
    }

    /// The rollback round trip on PostgreSQL / MySQL when the environment names
    /// a database. Without it there is nothing to do (SQLite is covered above).
    #[tokio::test]
    async fn down_and_up_round_trip_on_the_backend_from_the_environment() {
        let Ok(url) = std::env::var("IRONFORGE_TEST_DATABASE_URL") else {
            return;
        };
        let db = crate::connect_with_pool(&url, 5, 60, 4)
            .await
            .expect("connect to IRONFORGE_TEST_DATABASE_URL");
        crate::migrations::Migrator::up(&db, None).await.unwrap();
        let manager = SchemaManager::new(&db);
        assert!(manager.has_table(TABLE).await.unwrap());
        crate::migrations::Migrator::down(&db, Some(1))
            .await
            .unwrap();
        assert!(
            !manager.has_table(TABLE).await.unwrap(),
            "down must drop the table on this backend too"
        );
        crate::migrations::Migrator::up(&db, None).await.unwrap();
        assert!(manager.has_table(TABLE).await.unwrap());
    }
}
