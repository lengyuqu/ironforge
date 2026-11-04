//! Migration: record a SHA-256 digest for release assets.
//!
//! Release assets had no integrity digest: the upload path hashed nothing and
//! the download path returned only content type/length/disposition, so a client
//! had no way to verify the bytes it received. The digest is computed on upload
//! and exposed as `x-checksum-sha256` on download.
//!
//! The column is nullable: assets uploaded before this migration have no
//! recorded digest, and backfilling them would require re-reading every blob.
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Idempotent on purpose: the `down` path below cannot drop the column
        // on SQLite, so a rollback-then-reapply cycle must not fail with
        // "duplicate column name: sha256".
        if manager.has_column("release_assets", "sha256").await? {
            return Ok(());
        }

        manager
            .alter_table(
                Table::alter()
                    .table(ReleaseAssets::Table)
                    .add_column(ColumnDef::new(ReleaseAssetsCol::Sha256).string_len(64).null())
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // SQLite cannot reliably DROP COLUMN without recreating the table, and
        // the digest is additive — leaving the column in place on rollback is
        // harmless. `up` is idempotent so re-applying works regardless.
        let _ = manager;
        Ok(())
    }
}

#[derive(Iden)]
enum ReleaseAssets {
    Table,
}

#[derive(Iden)]
enum ReleaseAssetsCol {
    Sha256,
}
