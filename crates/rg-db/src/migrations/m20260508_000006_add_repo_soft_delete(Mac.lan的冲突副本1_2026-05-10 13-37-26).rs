use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260508_000006_add_repo_soft_delete"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Add deleted_at column (separate ALTER TABLE for SQLite)
        manager
            .alter_table(
                Table::alter()
                    .table(Repositories::Table)
                    .add_column(
                        ColumnDef::new(Repositories::DeletedAt)
                            .timestamp_with_time_zone()
                            .null(),
                    )
                    .to_owned(),
            )
            .await?;

        // Add origin_repo_id column (separate ALTER TABLE for SQLite)
        manager
            .alter_table(
                Table::alter()
                    .table(Repositories::Table)
                    .add_column(
                        ColumnDef::new(Repositories::OriginRepoId)
                            .big_integer()
                            .null(),
                    )
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Drop origin_repo_id column first (separate ALTER TABLE for SQLite)
        manager
            .alter_table(
                Table::alter()
                    .table(Repositories::Table)
                    .drop_column(Repositories::OriginRepoId)
                    .to_owned(),
            )
            .await?;

        // Drop deleted_at column
        manager
            .alter_table(
                Table::alter()
                    .table(Repositories::Table)
                    .drop_column(Repositories::DeletedAt)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }
}

#[derive(Iden)]
enum Repositories {
    Table,
    DeletedAt,
    OriginRepoId,
}
