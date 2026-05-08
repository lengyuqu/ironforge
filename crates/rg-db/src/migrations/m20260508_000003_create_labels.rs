use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260508_000003_create_labels"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // --- labels ---
        manager
            .create_table(
                Table::create()
                    .table(Labels::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(Labels::Id).big_integer().not_null().auto_increment().primary_key())
                    .col(ColumnDef::new(Labels::RepoId).big_integer().not_null())
                    .col(ColumnDef::new(Labels::Name).string().not_null())
                    .col(ColumnDef::new(Labels::Color).string().not_null().default("#ffffff".to_owned()))
                    .col(ColumnDef::new(Labels::Description).string().null())
                    .col(ColumnDef::new(Labels::CreatedAt).timestamp_with_time_zone().not_null())
                    .col(ColumnDef::new(Labels::UpdatedAt).timestamp_with_time_zone().not_null())
                    .index(Index::create().unique().col(Labels::RepoId).col(Labels::Name).name("idx_labels_repo_name_unique"))
                    .index(Index::create().col(Labels::RepoId).name("idx_labels_repo_id"))
                    .foreign_key(
                        ForeignKey::create()
                            .from(Labels::Table, Labels::RepoId)
                            .to(Repositories::Table, Repositories::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        // --- issue_labels ---
        manager
            .create_table(
                Table::create()
                    .table(IssueLabels::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(IssueLabels::Id).big_integer().not_null().auto_increment().primary_key())
                    .col(ColumnDef::new(IssueLabels::IssueId).big_integer().not_null())
                    .col(ColumnDef::new(IssueLabels::LabelId).big_integer().not_null())
                    .col(ColumnDef::new(IssueLabels::CreatedAt).timestamp_with_time_zone().not_null())
                    .index(Index::create().unique().col(IssueLabels::IssueId).col(IssueLabels::LabelId).name("idx_issue_labels_issue_label_unique"))
                    .index(Index::create().col(IssueLabels::LabelId).name("idx_issue_labels_label_id"))
                    .foreign_key(
                        ForeignKey::create()
                            .from(IssueLabels::Table, IssueLabels::IssueId)
                            .to(Issues::Table, Issues::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(IssueLabels::Table, IssueLabels::LabelId)
                            .to(Labels::Table, Labels::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.drop_table(Table::drop().table(IssueLabels::Table).to_owned()).await?;
        manager.drop_table(Table::drop().table(Labels::Table).to_owned()).await
    }
}

#[derive(Iden)]
enum Labels {
    Table,
    Id,
    RepoId,
    Name,
    Color,
    Description,
    CreatedAt,
    UpdatedAt,
}

#[derive(Iden)]
enum IssueLabels {
    Table,
    Id,
    IssueId,
    LabelId,
    CreatedAt,
}

#[derive(Iden)]
enum Repositories {
    Table,
    Id,
}

#[derive(Iden)]
enum Issues {
    Table,
    Id,
}
