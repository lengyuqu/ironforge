//! `usage_counters`: platform-measured usage, one row per
//! `(subject, repo, metric, period)`.
//!
//! R10 (#40) minimal loop: CI minutes per UTC day and the current number of
//! concurrent pipelines, per agent per repository. The existing
//! `collab_task_budget` / `collab_task_usage` tables are informational — the
//! client reports them and the platform never measures anything — so they are
//! not reused here.
//!
//! `value` is always an *absolute* measurement for the period, never a delta:
//! callers recompute it (`rg_ci::quota`), which is what makes recording the same
//! pipeline twice harmless. `period_start` is a date because both metrics are
//! bucketed by UTC day (`ci_concurrent_pipelines` is a gauge and overwrites the
//! day's row).
//!
//! Only a new table; `up` is idempotent (table, columns and index are each
//! guarded) and `down` drops it.

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261114_000001_create_usage_counters"
    }
}

const TABLE: &str = "usage_counters";
const UNIQUE_INDEX: &str = "uq_usage_counters_subject_metric_period";

// Explicit table name, matching the entity's `table_name` (CLAUDE.md pitfall 13):
// a derived `Iden` would produce the singular `usage_counter`.
#[derive(DeriveIden)]
enum UsageCounters {
    #[sea_orm(iden = "usage_counters")]
    Table,
    Id,
    SubjectKind,
    SubjectId,
    RepoId,
    Metric,
    PeriodStart,
    Value,
    UpdatedAt,
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
            self.add_missing_columns(manager).await?;
        } else {
            manager.create_table(table_create()).await?;
        }
        if !manager.has_index(TABLE, UNIQUE_INDEX).await? {
            manager
                .create_index(
                    Index::create()
                        .if_not_exists()
                        .name(UNIQUE_INDEX)
                        .table(UsageCounters::Table)
                        .col(UsageCounters::SubjectKind)
                        .col(UsageCounters::SubjectId)
                        .col(UsageCounters::RepoId)
                        .col(UsageCounters::Metric)
                        .col(UsageCounters::PeriodStart)
                        .unique()
                        .to_owned(),
                )
                .await?;
        }
        Ok(())
    }

    /// Drops the table. Its rows are derived measurements and are rebuilt from
    /// `pipelines` / `pipeline_jobs` on the next trigger, so nothing is lost.
    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(
                Table::drop()
                    .table(UsageCounters::Table)
                    .if_exists()
                    .to_owned(),
            )
            .await
    }
}

impl Migration {
    async fn add_missing_columns(&self, manager: &SchemaManager<'_>) -> Result<(), DbErr> {
        let columns: [(UsageCounters, ColumnDef); 8] = [
            (
                UsageCounters::Id,
                ColumnDef::new(UsageCounters::Id)
                    .big_integer()
                    .not_null()
                    .auto_increment()
                    .primary_key()
                    .to_owned(),
            ),
            (
                UsageCounters::SubjectKind,
                ColumnDef::new(UsageCounters::SubjectKind)
                    .string_len(16)
                    .not_null()
                    .to_owned(),
            ),
            (
                UsageCounters::SubjectId,
                ColumnDef::new(UsageCounters::SubjectId)
                    .big_integer()
                    .not_null()
                    .to_owned(),
            ),
            (
                UsageCounters::RepoId,
                ColumnDef::new(UsageCounters::RepoId)
                    .big_integer()
                    .not_null()
                    .to_owned(),
            ),
            (
                UsageCounters::Metric,
                ColumnDef::new(UsageCounters::Metric)
                    .string_len(48)
                    .not_null()
                    .to_owned(),
            ),
            (
                UsageCounters::PeriodStart,
                ColumnDef::new(UsageCounters::PeriodStart)
                    .date()
                    .not_null()
                    .to_owned(),
            ),
            (
                UsageCounters::Value,
                ColumnDef::new(UsageCounters::Value)
                    .big_integer()
                    .not_null()
                    .default(0)
                    .to_owned(),
            ),
            (
                UsageCounters::UpdatedAt,
                ColumnDef::new(UsageCounters::UpdatedAt)
                    .timestamp_with_time_zone()
                    .not_null()
                    .to_owned(),
            ),
        ];
        for (column, definition) in columns {
            let name = column.to_string();
            if !manager.has_column(TABLE, name.as_str()).await? {
                manager
                    .alter_table(
                        Table::alter()
                            .table(UsageCounters::Table)
                            .add_column(definition)
                            .to_owned(),
                    )
                    .await?;
            }
        }
        Ok(())
    }
}

fn table_create() -> TableCreateStatement {
    Table::create()
        .table(UsageCounters::Table)
        .if_not_exists()
        .col(
            ColumnDef::new(UsageCounters::Id)
                .big_integer()
                .not_null()
                .auto_increment()
                .primary_key(),
        )
        // `user` today; kept open for `org` / `token` later.
        .col(
            ColumnDef::new(UsageCounters::SubjectKind)
                .string_len(16)
                .not_null(),
        )
        // `users.id` of the subject. No foreign key: a counter must outlive the
        // account it names (same reasoning as `audit_log.on_behalf_of_user_id`).
        .col(
            ColumnDef::new(UsageCounters::SubjectId)
                .big_integer()
                .not_null(),
        )
        .col(
            ColumnDef::new(UsageCounters::RepoId)
                .big_integer()
                .not_null(),
        )
        // `ci_minutes` | `ci_concurrent_pipelines`.
        .col(
            ColumnDef::new(UsageCounters::Metric)
                .string_len(48)
                .not_null(),
        )
        // UTC day the measurement belongs to.
        .col(ColumnDef::new(UsageCounters::PeriodStart).date().not_null())
        .col(
            ColumnDef::new(UsageCounters::Value)
                .big_integer()
                .not_null()
                .default(0),
        )
        .col(
            ColumnDef::new(UsageCounters::UpdatedAt)
                .timestamp_with_time_zone()
                .not_null(),
        )
        // The arbiter of concurrent writers: recomputing a period twice updates
        // this one row instead of creating a second.
        .index(
            Index::create()
                .if_not_exists()
                .name(UNIQUE_INDEX)
                .col(UsageCounters::SubjectKind)
                .col(UsageCounters::SubjectId)
                .col(UsageCounters::RepoId)
                .col(UsageCounters::Metric)
                .col(UsageCounters::PeriodStart)
                .unique(),
        )
        .foreign_key(
            ForeignKey::create()
                .from(UsageCounters::Table, UsageCounters::RepoId)
                .to(Repositories::Table, Repositories::Id)
                .on_delete(ForeignKeyAction::Cascade),
        )
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

    /// CLAUDE.md pitfall #13: the explicit table name must match the entity, on
    /// every backend, and the unique index must survive each dialect.
    #[test]
    fn ddl_uses_the_plural_table_name_and_unique_index_on_all_backends() {
        for sql in [
            table_create().to_string(SqliteQueryBuilder),
            table_create().to_string(PostgresQueryBuilder),
            table_create().to_string(MysqlQueryBuilder),
        ] {
            assert!(sql.contains(TABLE), "{TABLE} missing in: {sql}");
            assert!(!sql.contains("usage_counter "), "singular table in: {sql}");
            assert!(
                sql.contains(UNIQUE_INDEX),
                "{UNIQUE_INDEX} missing in: {sql}"
            );
        }
    }

    /// `down` really drops the table, and `up` recreates it: rollback followed
    /// by re-apply must not leave a half-applied schema behind.
    #[tokio::test]
    async fn down_drops_the_table_and_up_recreates_it() {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        crate::migrations::Migrator::up(&db, None).await.unwrap();
        assert_eq!(
            count(
                &db,
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'usage_counters'"
            )
            .await,
            1
        );

        crate::migrations::Migrator::down(&db, Some(1))
            .await
            .unwrap();
        assert_eq!(
            count(
                &db,
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'usage_counters'"
            )
            .await,
            0,
            "down must drop the table"
        );

        crate::migrations::Migrator::up(&db, None).await.unwrap();
        assert_eq!(
            count(
                &db,
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'usage_counters'"
            )
            .await,
            1
        );
        assert_eq!(
            count(
                &db,
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name = 'uq_usage_counters_subject_metric_period'"
            )
            .await,
            1,
            "the unique index must be back after re-apply"
        );
    }
}
