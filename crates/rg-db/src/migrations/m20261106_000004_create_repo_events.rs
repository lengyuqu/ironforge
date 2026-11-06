//! A per-repository event log for long-polling subscribers: `repo_events`.
//!
//! An agent usually has no public address and cannot receive a webhook. Every
//! event that would be delivered to a webhook is also written here, and
//! `GET /repos/{o}/{n}/events?since=<cursor>` reads them back in order. It is a
//! separate table from the audit log on purpose: the audit log is for
//! compliance (90-day archive, different write timing) and has no cursor
//! semantics.
//!
//! `id` is the cursor: it only ever increases, so "events after id N" misses and
//! repeats nothing. Rows are deleted after 7 days. Only a new table; `up` is
//! idempotent.

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261106_000004_create_repo_events"
    }
}

// Declared with an explicit name: a derived `Iden` would produce the singular
// `repo_event`, which is not the table the entity maps to.
#[derive(DeriveIden)]
enum RepoEvents {
    #[sea_orm(iden = "repo_events")]
    Table,
    Id,
    RepoId,
    EventType,
    Payload,
    CreatedAt,
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        if manager.has_table("repo_events").await? {
            return Ok(());
        }
        manager
            .create_table(
                Table::create()
                    .table(RepoEvents::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(RepoEvents::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(RepoEvents::RepoId).big_integer().not_null())
                    .col(
                        ColumnDef::new(RepoEvents::EventType)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(ColumnDef::new(RepoEvents::Payload).text().null())
                    .col(
                        ColumnDef::new(RepoEvents::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;
        // The read: one repository's events after a cursor, in order.
        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("idx_repo_events_repo_id")
                    .table(RepoEvents::Table)
                    .col(RepoEvents::RepoId)
                    .col(RepoEvents::Id)
                    .to_owned(),
            )
            .await?;
        // The retention sweep.
        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("idx_repo_events_created_at")
                    .table(RepoEvents::Table)
                    .col(RepoEvents::CreatedAt)
                    .to_owned(),
            )
            .await
    }

    /// Drops the table (its rows are a bounded replay window).
    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(
                Table::drop()
                    .table(RepoEvents::Table)
                    .if_exists()
                    .to_owned(),
            )
            .await
    }
}
