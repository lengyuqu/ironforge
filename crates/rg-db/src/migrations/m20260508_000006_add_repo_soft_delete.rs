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
        manager
            .alter_table(
                Table::alter()
                    .table(Repositories::Table)
                    .add_column(
                        ColumnDef::new(Repositories::DeletedAt)
                            .timestamp_with_time_zone()
                            .null(),
                    )
                    .add_column(
                        ColumnDef::new(Repositories::OriginRepoId)
                            .big_integer()
                            .null(),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Repositories::Table)
                    .drop_column(Repositories::DeletedAt)
                    .drop_column(Repositories::OriginRepoId)
                    .to_owned(),
            )
            .await
    }
}

#[derive(Iden)]
enum Repositories {
    Table,
    DeletedAt,
    OriginRepoId,
}
