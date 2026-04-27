//! Add compression fields to lfs_objects table (2026-04-27)
//!
//! This migration adds compression support for LFS objects:
//! - `compression`: Algorithm used (e.g., "zstd"), NULL = uncompressed
//! - `compressed_size`: Size after compression (NULL for uncompressed)

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Add compression column
        manager
            .alter_table(
                Table::alter()
                    .table(LfsObjects::Table)
                    .add_column_if_not_exists(
                        ColumnDef::new(Alias::new("compression"))
                            .string()
                            .null(),
                    )
                    .add_column_if_not_exists(
                        ColumnDef::new(Alias::new("compressed_size"))
                            .big_integer()
                            .null(),
                    )
                    .to_owned(),
            )
            .await?;

        tracing::info!("Added compression fields to lfs_objects table");
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(LfsObjects::Table)
                    .drop_column(Alias::new("compression"))
                    .drop_column(Alias::new("compressed_size"))
                    .to_owned(),
            )
            .await?;

        tracing::info!("Dropped compression fields from lfs_objects table");
        Ok(())
    }
}

#[derive(Iden)]
pub enum LfsObjects {
    Table,
}
