//! `collab_health_baselines`: when a repository's observation baseline was taken.
//!
//! The baseline used to exist only as events written for objects that had none
//! (see `collab_metric_ops::take_baseline`). When there was nothing to write (an
//! empty repository, or every object already had events) the call succeeded but
//! left no trace, so the health page kept saying "not established". One row per
//! repository records that the baseline was taken and when. Only a new table;
//! `up` is idempotent. Repositories whose baseline was taken before this table
//! existed keep their events, which are still read as the baseline time.

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261109_000001_create_collab_health_baselines"
    }
}

// Declared with an explicit name: a derived `Iden` for a singular enum would
// produce the singular table name, which is not the table the entity maps to.
#[derive(DeriveIden)]
enum CollabHealthBaselines {
    #[sea_orm(iden = "collab_health_baselines")]
    Table,
    RepoId,
    TakenAt,
}

#[derive(DeriveIden)]
enum Repositories {
    Table,
    Id,
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        if manager.has_table("collab_health_baselines").await? {
            return Ok(());
        }
        manager
            .create_table(
                Table::create()
                    .table(CollabHealthBaselines::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(CollabHealthBaselines::RepoId)
                            .big_integer()
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(CollabHealthBaselines::TakenAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(CollabHealthBaselines::Table, CollabHealthBaselines::RepoId)
                            .to(Repositories::Table, Repositories::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(
                Table::drop()
                    .table(CollabHealthBaselines::Table)
                    .if_exists()
                    .to_owned(),
            )
            .await
    }
}
