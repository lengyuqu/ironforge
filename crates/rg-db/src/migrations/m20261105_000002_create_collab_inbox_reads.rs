//! Personal read marks for inbox items (P0 design §inbox).
//!
//! One row per `(repo, actor, item_key)` holding the highest object version the
//! actor marked as read. It only expresses reading: it never changes the state
//! of the question, handoff or task it points at. Actors are not foreign keys
//! (a session or an account), so rows of a deleted actor are orphans that no
//! query can reach; they go with the repository.

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261105_000002_create_collab_inbox_reads"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        if !manager.has_table("collab_inbox_reads").await? {
            manager.create_table(table()).await?;
        }
        Ok(())
    }

    /// Drops the table; `up` is idempotent so a rollback and re-apply works.
    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(
                Table::drop()
                    .table(CollabInboxReads::Table)
                    .if_exists()
                    .to_owned(),
            )
            .await
    }
}

fn table() -> TableCreateStatement {
    Table::create()
        .table(CollabInboxReads::Table)
        .if_not_exists()
        .col(
            ColumnDef::new(CollabInboxReads::RepoId)
                .big_integer()
                .not_null(),
        )
        // `session` | `user`.
        .col(
            ColumnDef::new(CollabInboxReads::ActorKind)
                .string_len(16)
                .not_null(),
        )
        .col(
            ColumnDef::new(CollabInboxReads::ActorId)
                .string_len(64)
                .not_null(),
        )
        // `<type>:<object id>`, as in the inbox.
        .col(
            ColumnDef::new(CollabInboxReads::ItemKey)
                .string_len(100)
                .not_null(),
        )
        .col(
            ColumnDef::new(CollabInboxReads::ObservedVersion)
                .big_integer()
                .not_null(),
        )
        .col(
            ColumnDef::new(CollabInboxReads::ReadAt)
                .timestamp_with_time_zone()
                .not_null(),
        )
        .primary_key(
            Index::create()
                .name("pk_collab_inbox_reads")
                .col(CollabInboxReads::RepoId)
                .col(CollabInboxReads::ActorKind)
                .col(CollabInboxReads::ActorId)
                .col(CollabInboxReads::ItemKey),
        )
        .foreign_key(
            ForeignKey::create()
                .from(CollabInboxReads::Table, CollabInboxReads::RepoId)
                .to(Repositories::Table, Repositories::Id)
                .on_delete(ForeignKeyAction::Cascade),
        )
        .to_owned()
}

#[derive(Iden)]
enum CollabInboxReads {
    Table,
    RepoId,
    ActorKind,
    ActorId,
    ItemKey,
    ObservedVersion,
    ReadAt,
}

#[derive(Iden)]
enum Repositories {
    Table,
    Id,
}
