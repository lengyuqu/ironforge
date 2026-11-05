//! Idempotent snapshot creation (O04): an optional client key per snapshot, unique
//! per task, so a retried `POST` returns the snapshot it already made instead of
//! storing another one.
//!
//! The column is nullable (older rows and key-less requests have none). `up` is
//! idempotent and `down` only drops the index: dropping a column is not portable
//! across the three backends, and leaving it is harmless.

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261105_000004_add_collab_snapshot_request_id"
    }
}

const TABLE: &str = "collab_completion_snapshots";
const INDEX: &str = "uq_collab_snapshots_request";

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        if !manager.has_column(TABLE, "client_request_id").await? {
            manager
                .alter_table(
                    Table::alter()
                        .table(Alias::new(TABLE))
                        .add_column(ColumnDef::new(Alias::new("client_request_id")).string_len(128))
                        .to_owned(),
                )
                .await?;
        }
        if !manager.has_index(TABLE, INDEX).await? {
            manager
                .create_index(
                    Index::create()
                        .name(INDEX)
                        .table(Alias::new(TABLE))
                        .col(Alias::new("task_id"))
                        .col(Alias::new("client_request_id"))
                        .unique()
                        .to_owned(),
                )
                .await?;
        }
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        if manager.has_index(TABLE, INDEX).await? {
            manager
                .drop_index(
                    Index::drop()
                        .name(INDEX)
                        .table(Alias::new(TABLE))
                        .to_owned(),
                )
                .await?;
        }
        Ok(())
    }
}
