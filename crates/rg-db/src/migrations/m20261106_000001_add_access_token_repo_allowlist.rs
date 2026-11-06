//! Optional repository allow-list on personal access tokens.
//!
//! `access_tokens.repo_allowlist` is a nullable TEXT holding a JSON array of
//! `owner/name` strings. `NULL` means "no limit" (every existing token, and any
//! token created without `repos`). Existing rows stay `NULL`: their behaviour
//! does not change.

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261106_000001_add_access_token_repo_allowlist"
    }
}

// The table is declared with an explicit name: a derived `Iden` would produce the
// singular `access_token`, which is not the table the entity maps to.
#[derive(DeriveIden)]
enum AccessTokens {
    #[sea_orm(iden = "access_tokens")]
    Table,
    RepoAllowlist,
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    /// Idempotent: `down` is a no-op, so a re-apply must not fail with a
    /// duplicate column.
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        if manager.has_table("access_tokens").await?
            && !manager
                .has_column("access_tokens", "repo_allowlist")
                .await?
        {
            manager
                .alter_table(
                    Table::alter()
                        .table(AccessTokens::Table)
                        .add_column(ColumnDef::new(AccessTokens::RepoAllowlist).text().null())
                        .to_owned(),
                )
                .await?;
        }
        Ok(())
    }

    /// Intentionally empty: SQLite cannot reliably drop a column through the
    /// schema builder, and a nullable column is harmless to older code. `up` is
    /// idempotent, so rollback followed by re-apply works.
    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Ok(())
    }
}
