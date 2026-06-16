//! Migration: add `deleted_at` columns to user, org, and issue tables
//! for soft-delete support.
use sea_orm_migration::prelude::*;
use sea_orm::Statement;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260616_000002_add_soft_delete_columns"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        let backend = db.get_database_backend();

        // Idempotent: skip tables/columns that already exist. This migration
        // previously crashed mid-way (the `organizations` table did not yet
        // exist before m20260616_0000015 renamed it), so on re-run the
        // `users.deleted_at` column may already be present.
        for table in ["users", "organizations", "issues"] {
            if manager.has_table(table).await? && !manager.has_column(table, "deleted_at").await? {
                db.execute(Statement::from_string(
                    backend,
                    format!("ALTER TABLE \"{table}\" ADD COLUMN deleted_at TIMESTAMP NULL;"),
                ))
                .await?;
            }
        }

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        let backend = db.get_database_backend();

        db.execute(Statement::from_string(
            backend,
            "ALTER TABLE users DROP COLUMN deleted_at;",
        ))
        .await?;

        db.execute(Statement::from_string(
            backend,
            "ALTER TABLE organizations DROP COLUMN deleted_at;",
        ))
        .await?;

        db.execute(Statement::from_string(
            backend,
            "ALTER TABLE issues DROP COLUMN deleted_at;",
        ))
        .await?;

        Ok(())
    }
}
