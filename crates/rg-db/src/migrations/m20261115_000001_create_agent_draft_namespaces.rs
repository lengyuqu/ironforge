//! `agent_draft_namespaces`: the TTL registry behind the agent draft area
//! (#18, T11).
//!
//! One row is one reserved branch directory `refs/heads/<prefix>` (for example
//! `agent/42/`). A branch under a registered prefix is a *draft*: it may be
//! pushed and it runs CI, but its push does not deliver `push` / branch
//! webhooks, does not fan out over WebSocket and does not send mail, and the
//! directory is deleted on its own once `expires_at` (plus `grace_secs`) has
//! passed.
//!
//! Deliberately narrow: no `updated_at`, no `hard_delete_at`, no cached state
//! beyond `state`. The lifecycle is derivable from three columns:
//! `expires_at` is the moment pushes stop being accepted, `grace_secs` is how
//! long the branches survive afterwards, and `state` records where the row is
//! (`active` → `expiring` → `deleted`). A separate timestamp would let the two
//! disagree.
//!
//! `owner_user_id` carries **no** foreign key on purpose: the row must outlive
//! the account it names, because the branches it owns still have to be cleaned
//! up after the account is gone (the same reasoning as
//! `audit_log.on_behalf_of_user_id`). `repo_id` does cascade — a deleted
//! repository takes its bare git directory with it.
//!
//! `up` is idempotent (table, columns and indexes are each guarded) and `down`
//! drops the table and its indexes.

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261115_000001_create_agent_draft_namespaces"
    }
}

const TABLE: &str = "agent_draft_namespaces";
const UQ_PREFIX: &str = "uq_agent_draft_namespaces_prefix";
const IX_STATE_EXPIRES: &str = "ix_agent_draft_namespaces_state_expires";

// Explicit table name matching the entity's `table_name` (CLAUDE.md pitfall
// 13): a derived `Iden` would produce the singular `agent_draft_namespace`.
#[derive(DeriveIden)]
enum AgentDraftNamespaces {
    #[sea_orm(iden = "agent_draft_namespaces")]
    Table,
    Id,
    RepoId,
    Prefix,
    OwnerUserId,
    TtlSecs,
    ExpiresAt,
    GraceSecs,
    State,
    ExtendedCount,
    CreatedAt,
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
            // re-run after a failed create (or a hand-edited database) converges.
            for (name, definition) in column_defs() {
                if !manager.has_column(TABLE, name).await? {
                    manager
                        .alter_table(
                            Table::alter()
                                .table(AgentDraftNamespaces::Table)
                                .add_column(definition)
                                .to_owned(),
                        )
                        .await?;
                }
            }
        } else {
            manager.create_table(table_create()).await?;
        }
        // Indexes are declared separately, each behind a `has_index` guard:
        // SQLite rejects the bare `CONSTRAINT name (cols)` sea-query renders for
        // a non-unique `.index()` inside `TableCreateStatement`, and one guarded
        // definition per index also covers the repair path.
        if !manager.has_index(TABLE, UQ_PREFIX).await? {
            manager.create_index(prefix_index()).await?;
        }
        if !manager.has_index(TABLE, IX_STATE_EXPIRES).await? {
            manager.create_index(state_expires_index()).await?;
        }
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(
                Table::drop()
                    .table(AgentDraftNamespaces::Table)
                    .if_exists()
                    .to_owned(),
            )
            .await
    }
}

/// `(column name, definition)`, in table order. The name comes from the `Iden`
/// so the guard and the DDL cannot drift apart.
fn column_defs() -> Vec<(&'static str, ColumnDef)> {
    vec![
        (
            "id",
            ColumnDef::new(AgentDraftNamespaces::Id)
                .big_integer()
                .not_null()
                .auto_increment()
                .primary_key()
                .to_owned(),
        ),
        (
            "repo_id",
            ColumnDef::new(AgentDraftNamespaces::RepoId)
                .big_integer()
                .not_null()
                .to_owned(),
        ),
        // The reserved directory, with the trailing slash: `agent/42/`.
        (
            "prefix",
            ColumnDef::new(AgentDraftNamespaces::Prefix)
                .string_len(255)
                .not_null()
                .to_owned(),
        ),
        // No foreign key: the row must outlive the account (module comment).
        (
            "owner_user_id",
            ColumnDef::new(AgentDraftNamespaces::OwnerUserId)
                .big_integer()
                .not_null()
                .to_owned(),
        ),
        // The TTL the caller asked for. Kept so `extend` can cap the total
        // lifetime at `4 × ttl_secs` without trusting the caller's arithmetic.
        (
            "ttl_secs",
            ColumnDef::new(AgentDraftNamespaces::TtlSecs)
                .big_integer()
                .not_null()
                .to_owned(),
        ),
        // The moment pushes stop being accepted. Also the anchor the grace
        // period is counted from, so `expires_at + grace_secs` is the delete
        // deadline.
        (
            "expires_at",
            ColumnDef::new(AgentDraftNamespaces::ExpiresAt)
                .timestamp_with_time_zone()
                .not_null()
                .to_owned(),
        ),
        (
            "grace_secs",
            ColumnDef::new(AgentDraftNamespaces::GraceSecs)
                .big_integer()
                .not_null()
                .to_owned(),
        ),
        // `active` | `expiring` | `deleted`.
        (
            "state",
            ColumnDef::new(AgentDraftNamespaces::State)
                .string_len(16)
                .not_null()
                .default("active")
                .to_owned(),
        ),
        // How many times the caller extended. Audit only: the cap is computed
        // from `created_at + 4 × ttl_secs`, not from this counter.
        (
            "extended_count",
            ColumnDef::new(AgentDraftNamespaces::ExtendedCount)
                .integer()
                .not_null()
                .default(0)
                .to_owned(),
        ),
        (
            "created_at",
            ColumnDef::new(AgentDraftNamespaces::CreatedAt)
                .timestamp_with_time_zone()
                .not_null()
                .to_owned(),
        ),
    ]
}

fn table_create() -> TableCreateStatement {
    let mut table = Table::create();
    table.table(AgentDraftNamespaces::Table).if_not_exists();
    for (_, definition) in column_defs() {
        table.col(definition);
    }
    table
        .foreign_key(
            ForeignKey::create()
                .from(AgentDraftNamespaces::Table, AgentDraftNamespaces::RepoId)
                .to(Repositories::Table, Repositories::Id)
                .on_delete(ForeignKeyAction::Cascade),
        )
        .to_owned()
}

fn prefix_index() -> IndexCreateStatement {
    Index::create()
        .if_not_exists()
        .name(UQ_PREFIX)
        .table(AgentDraftNamespaces::Table)
        .col(AgentDraftNamespaces::RepoId)
        .col(AgentDraftNamespaces::Prefix)
        .unique()
        .to_owned()
}

fn state_expires_index() -> IndexCreateStatement {
    Index::create()
        .if_not_exists()
        .name(IX_STATE_EXPIRES)
        .table(AgentDraftNamespaces::Table)
        .col(AgentDraftNamespaces::State)
        .col(AgentDraftNamespaces::ExpiresAt)
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

    /// CLAUDE.md pitfall #13: the explicit plural table name and the
    /// constraints survive every dialect the platform supports.
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
                !sql.contains("agent_draft_namespace "),
                "singular table in: {sql}"
            );
            assert!(
                sql.contains("FOREIGN KEY (\"repo_id\")")
                    || sql.contains("FOREIGN KEY (`repo_id`)"),
                "repo foreign key missing: {sql}"
            );
            assert!(
                sql.contains("DEFAULT 'active'") || sql.contains("DEFAULT \"active\""),
                "state default missing: {sql}"
            );
        }
        for (name, index) in [
            (UQ_PREFIX, prefix_index()),
            (IX_STATE_EXPIRES, state_expires_index()),
        ] {
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
                migration.name() == "m20261115_000001_create_agent_draft_namespaces"
            })
            .expect("this migration must be registered");
        (position + 1) as u32
    }

    /// `down` really drops the table and both indexes; `up` recreates them, and
    /// the unique `(repo_id, prefix)` key is back. A rollback that leaves an
    /// index behind would make the re-apply fail on a real database.
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
                    "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name = '{UQ_PREFIX}'"
                )
            )
            .await,
            1,
            "the unique prefix key must be back after re-apply"
        );
    }

    /// Re-running `up` on a schema that already has the table (a failed earlier
    /// attempt, or an operator's hand-edited database) converges instead of
    /// erroring.
    #[tokio::test]
    async fn up_is_idempotent_and_repairs_a_missing_column() {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        crate::migrations::Migrator::up(&db, None).await.unwrap();
        // Drop one column's worth of drift the guarded path has to repair.
        db.execute_unprepared("ALTER TABLE agent_draft_namespaces DROP COLUMN extended_count")
            .await
            .expect("sqlite 3.35+ can drop a column");
        assert_eq!(
            count(
                &db,
                "SELECT COUNT(*) FROM pragma_table_info('agent_draft_namespaces') \
                 WHERE name = 'extended_count'"
            )
            .await,
            0
        );

        // `Migrator::up` would skip an already-applied migration, so the repair
        // path is exercised through the manager directly.
        let manager = SchemaManager::new(&db);
        Migration.up(&manager).await.unwrap();
        assert_eq!(
            count(
                &db,
                "SELECT COUNT(*) FROM pragma_table_info('agent_draft_namespaces') \
                 WHERE name = 'extended_count'"
            )
            .await,
            1,
            "the guarded re-run must restore the missing column"
        );
    }
    /// The rollback is the risky half of a migration, and the backends disagree
    /// about dropping tables with indexes and foreign keys. With
    /// `IRONFORGE_TEST_DATABASE_URL` set this runs the round trip on
    /// PostgreSQL / MySQL; without it there is nothing to do (SQLite is covered
    /// above).
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
        assert!(
            manager.has_table(TABLE).await.unwrap(),
            "the table must exist before the rollback"
        );

        // This migration is registered last, so one step down is this one.
        crate::migrations::Migrator::down(&db, Some(1))
            .await
            .unwrap();
        assert!(
            !manager.has_table(TABLE).await.unwrap(),
            "down must drop the table on this backend too"
        );

        crate::migrations::Migrator::up(&db, None).await.unwrap();
        assert!(
            manager.has_table(TABLE).await.unwrap(),
            "up must recreate the table after the rollback"
        );
    }
}
