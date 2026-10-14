//! Corrective migration: rename `mirror` to `mirrors`.
//!
//! `m20260607_000001_create_mirrors` declared `#[derive(Iden)] enum Mirror`
//! for the table identifier, which resolves to the singular table name
//! `mirror`, while the entity maps to `#[sea_orm(table_name = "mirrors")]`.
//! Every mirror query therefore failed at runtime with
//! `no such table: mirrors` — including the periodic `mirror.sync_due`
//! background task, which logged the error on every tick.
//!
//! This is the same class of bug that `m20260617_000002_rename_board_time_tables_plural`
//! and friends fixed for other tables. The original migration is already
//! applied in existing databases and must not be edited, so the rename is
//! added here as a guarded, idempotent step.

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261015_000001_rename_mirror_plural"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Guarded so re-running on an already-fixed database is a no-op, and
        // so a database that never created the singular table is left alone.
        if manager.has_table("mirror").await? && !manager.has_table("mirrors").await? {
            manager
                .rename_table(
                    Table::rename()
                        .table(Alias::new("mirror"), Alias::new("mirrors"))
                        .to_owned(),
                )
                .await?;
        }

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        if manager.has_table("mirrors").await? && !manager.has_table("mirror").await? {
            manager
                .rename_table(
                    Table::rename()
                        .table(Alias::new("mirrors"), Alias::new("mirror"))
                        .to_owned(),
                )
                .await?;
        }

        Ok(())
    }
}
