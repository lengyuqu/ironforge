//! Audit: record which person an agent acted for.
//!
//! `audit_log.on_behalf_of_user_id` is the owner of the acting agent
//! (`users.owner_user_id`) and is `NULL` for every other row: a person's own
//! action, a system action, and every row written before this migration (not
//! backfilled, so `NULL` means "not an agent, or unknown").
//!
//! The column has no foreign key: an audit row must outlive the account it
//! names, and SQLite cannot add a foreign key to an existing table anyway.

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261110_000002_add_audit_on_behalf_of"
    }
}

// Explicit table name, matching the entity's `table_name` (CLAUDE.md pitfall 13).
#[derive(DeriveIden)]
enum AuditLog {
    #[sea_orm(iden = "audit_log")]
    Table,
    OnBehalfOfUserId,
    Id,
}

const INDEX: &str = "idx_audit_log_on_behalf_of";

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    /// Idempotent: `down` is a no-op, so a re-apply must not fail with a
    /// duplicate column or index.
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        if !manager.has_table("audit_log").await? {
            return Ok(());
        }
        if !manager
            .has_column("audit_log", "on_behalf_of_user_id")
            .await?
        {
            manager
                .alter_table(
                    Table::alter()
                        .table(AuditLog::Table)
                        .add_column(
                            ColumnDef::new(AuditLog::OnBehalfOfUserId)
                                .big_integer()
                                .null(),
                        )
                        .to_owned(),
                )
                .await?;
        }
        if !manager.has_index("audit_log", INDEX).await? {
            manager
                .create_index(
                    Index::create()
                        .name(INDEX)
                        .table(AuditLog::Table)
                        .col(AuditLog::OnBehalfOfUserId)
                        .col((AuditLog::Id, IndexOrder::Desc))
                        .to_owned(),
                )
                .await?;
        }
        Ok(())
    }

    /// Intentionally empty: SQLite cannot reliably drop a column through the
    /// schema builder, and a nullable column is harmless to older code. `up` is
    /// idempotent, so rollback then re-apply works.
    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, Database, Statement};
    use sea_orm_migration::MigratorTrait;

    async fn count(db: &sea_orm::DatabaseConnection, sql: &str) -> i64 {
        let row = db
            .query_one(Statement::from_string(db.get_database_backend(), sql))
            .await
            .unwrap()
            .unwrap();
        row.try_get_by_index::<i64>(0).unwrap()
    }

    /// `down` does nothing, so rolling back and re-applying runs `up` on a
    /// schema that already has the column and the index.
    #[tokio::test]
    async fn applying_twice_keeps_one_column_and_one_index() {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        crate::migrations::Migrator::up(&db, None).await.unwrap();
        crate::migrations::Migrator::down(&db, Some(1))
            .await
            .unwrap();
        crate::migrations::Migrator::up(&db, None)
            .await
            .expect("re-applying must be idempotent");
        assert_eq!(
            count(
                &db,
                "SELECT COUNT(*) FROM pragma_table_info('audit_log') WHERE name = 'on_behalf_of_user_id'"
            )
            .await,
            1
        );
        assert_eq!(
            count(
                &db,
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name = 'idx_audit_log_on_behalf_of'"
            )
            .await,
            1
        );
    }
}
