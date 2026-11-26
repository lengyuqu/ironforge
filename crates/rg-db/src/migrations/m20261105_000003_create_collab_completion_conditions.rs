//! Task completion conditions (O04): versioned definitions, explicit manual
//! confirmations and immutable evaluation snapshots.
//!
//! Design: `docs/design/agent-collaboration.md` §8.
//! Only new tables. A condition set is never edited: every change inserts the
//! next `version` of the task's set, so history stays traceable and a lost race
//! is a unique-key violation on `(task_id, version)`. CollabCompletionSnapshots store the
//! evaluation as it was; a live evaluation is never history.

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261105_000003_create_collab_completion_conditions"
    }
}

fn uuid_col<T: IntoIden>(name: T) -> ColumnDef {
    let mut col = ColumnDef::new(name);
    col.string_len(36);
    col
}

fn ts<T: IntoIden>(name: T) -> ColumnDef {
    let mut col = ColumnDef::new(name);
    col.timestamp_with_time_zone().not_null();
    col
}

fn task_fk<T: IntoIden + Copy + 'static>(table: T, task: T, repo: T) -> ForeignKeyCreateStatement {
    ForeignKey::create()
        .from(table, task)
        .from(table, repo)
        .to(CollabTasks::Table, CollabTasks::Id)
        .to(CollabTasks::Table, CollabTasks::RepoId)
        .on_delete(ForeignKeyAction::Cascade)
        .to_owned()
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        if !manager
            .has_table("collab_completion_condition_sets")
            .await?
        {
            manager.create_table(sets_table()).await?;
        }
        if !manager.has_table("collab_completion_confirmations").await? {
            manager.create_table(confirmations_table()).await?;
            manager
                .create_index(
                    Index::create()
                        .if_not_exists()
                        .name("idx_collab_confirmations_condition")
                        .table(CollabCompletionConfirmations::Table)
                        .col(CollabCompletionConfirmations::TaskId)
                        .col(CollabCompletionConfirmations::ConditionId)
                        .to_owned(),
                )
                .await?;
        }
        if !manager.has_table("collab_completion_snapshots").await? {
            manager.create_table(snapshots_table()).await?;
            manager
                .create_index(
                    Index::create()
                        .if_not_exists()
                        .name("idx_collab_snapshots_task")
                        .table(CollabCompletionSnapshots::Table)
                        .col(CollabCompletionSnapshots::TaskId)
                        .col(CollabCompletionSnapshots::CreatedAt)
                        .to_owned(),
                )
                .await?;
        }
        Ok(())
    }

    /// Drops the tables; `up` is idempotent so a rollback and re-apply works.
    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        for table in [
            CollabCompletionConditionSets::Table.into_iden(),
            CollabCompletionConfirmations::Table.into_iden(),
            CollabCompletionSnapshots::Table.into_iden(),
        ] {
            manager
                .drop_table(Table::drop().table(table).if_exists().to_owned())
                .await?;
        }
        Ok(())
    }
}

fn sets_table() -> TableCreateStatement {
    Table::create()
        .table(CollabCompletionConditionSets::Table)
        .if_not_exists()
        .col(
            uuid_col(CollabCompletionConditionSets::Id)
                .not_null()
                .primary_key(),
        )
        .col(
            ColumnDef::new(CollabCompletionConditionSets::RepoId)
                .big_integer()
                .not_null(),
        )
        .col(uuid_col(CollabCompletionConditionSets::TaskId).not_null())
        .col(
            ColumnDef::new(CollabCompletionConditionSets::Version)
                .big_integer()
                .not_null(),
        )
        .col(
            ColumnDef::new(CollabCompletionConditionSets::ConditionsJson)
                .text()
                .not_null(),
        )
        .col(
            ColumnDef::new(CollabCompletionConditionSets::ActorKind)
                .string_len(16)
                .not_null(),
        )
        .col(
            ColumnDef::new(CollabCompletionConditionSets::ActorId)
                .string_len(64)
                .not_null(),
        )
        .col(
            ColumnDef::new(CollabCompletionConditionSets::ClientRequestId)
                .string_len(64)
                .not_null(),
        )
        .col(
            ColumnDef::new(CollabCompletionConditionSets::PayloadHash)
                .string_len(64)
                .not_null(),
        )
        .col(ts(CollabCompletionConditionSets::CreatedAt))
        .index(
            Index::create()
                .name("uq_collab_condition_sets_task_version")
                .col(CollabCompletionConditionSets::TaskId)
                .col(CollabCompletionConditionSets::Version)
                .unique(),
        )
        .index(
            Index::create()
                .name("uq_collab_condition_sets_task_request")
                .col(CollabCompletionConditionSets::TaskId)
                .col(CollabCompletionConditionSets::ClientRequestId)
                .unique(),
        )
        .foreign_key(&mut task_fk(
            CollabCompletionConditionSets::Table,
            CollabCompletionConditionSets::TaskId,
            CollabCompletionConditionSets::RepoId,
        ))
        .to_owned()
}

fn confirmations_table() -> TableCreateStatement {
    Table::create()
        .table(CollabCompletionConfirmations::Table)
        .if_not_exists()
        .col(
            uuid_col(CollabCompletionConfirmations::Id)
                .not_null()
                .primary_key(),
        )
        .col(
            ColumnDef::new(CollabCompletionConfirmations::RepoId)
                .big_integer()
                .not_null(),
        )
        .col(uuid_col(CollabCompletionConfirmations::TaskId).not_null())
        .col(uuid_col(CollabCompletionConfirmations::ConditionId).not_null())
        .col(
            ColumnDef::new(CollabCompletionConfirmations::ConditionSetVersion)
                .big_integer()
                .not_null(),
        )
        .col(
            ColumnDef::new(CollabCompletionConfirmations::TaskVersion)
                .big_integer()
                .not_null(),
        )
        // The head of the task's branch when confirmed; null = none readable.
        .col(
            ColumnDef::new(CollabCompletionConfirmations::HeadSha)
                .string_len(64)
                .null(),
        )
        .col(
            ColumnDef::new(CollabCompletionConfirmations::Note)
                .text()
                .not_null(),
        )
        .col(
            ColumnDef::new(CollabCompletionConfirmations::ActorUserId)
                .big_integer()
                .not_null(),
        )
        .col(
            ColumnDef::new(CollabCompletionConfirmations::ClientRequestId)
                .string_len(64)
                .not_null(),
        )
        .col(
            ColumnDef::new(CollabCompletionConfirmations::PayloadHash)
                .string_len(64)
                .not_null(),
        )
        .col(ts(CollabCompletionConfirmations::CreatedAt))
        .index(
            Index::create()
                .name("uq_collab_confirmations_task_request")
                .col(CollabCompletionConfirmations::TaskId)
                .col(CollabCompletionConfirmations::ClientRequestId)
                .unique(),
        )
        .foreign_key(&mut task_fk(
            CollabCompletionConfirmations::Table,
            CollabCompletionConfirmations::TaskId,
            CollabCompletionConfirmations::RepoId,
        ))
        .to_owned()
}

fn snapshots_table() -> TableCreateStatement {
    Table::create()
        .table(CollabCompletionSnapshots::Table)
        .if_not_exists()
        .col(
            uuid_col(CollabCompletionSnapshots::Id)
                .not_null()
                .primary_key(),
        )
        .col(
            ColumnDef::new(CollabCompletionSnapshots::RepoId)
                .big_integer()
                .not_null(),
        )
        .col(uuid_col(CollabCompletionSnapshots::TaskId).not_null())
        // 0 = the task had no defined condition set.
        .col(
            ColumnDef::new(CollabCompletionSnapshots::ConditionSetVersion)
                .big_integer()
                .not_null(),
        )
        .col(
            ColumnDef::new(CollabCompletionSnapshots::TaskVersion)
                .big_integer()
                .not_null(),
        )
        .col(
            ColumnDef::new(CollabCompletionSnapshots::Overall)
                .string_len(16)
                .not_null(),
        )
        .col(
            ColumnDef::new(CollabCompletionSnapshots::ResultJson)
                .text()
                .not_null(),
        )
        .col(
            ColumnDef::new(CollabCompletionSnapshots::ActorKind)
                .string_len(16)
                .not_null(),
        )
        .col(
            ColumnDef::new(CollabCompletionSnapshots::ActorId)
                .string_len(64)
                .not_null(),
        )
        .col(ts(CollabCompletionSnapshots::CreatedAt))
        .foreign_key(&mut task_fk(
            CollabCompletionSnapshots::Table,
            CollabCompletionSnapshots::TaskId,
            CollabCompletionSnapshots::RepoId,
        ))
        .to_owned()
}

#[derive(Iden, Clone, Copy)]
enum CollabCompletionConditionSets {
    Table,
    Id,
    RepoId,
    TaskId,
    Version,
    ConditionsJson,
    ActorKind,
    ActorId,
    ClientRequestId,
    PayloadHash,
    CreatedAt,
}

#[derive(Iden, Clone, Copy)]
enum CollabCompletionConfirmations {
    Table,
    Id,
    RepoId,
    TaskId,
    ConditionId,
    ConditionSetVersion,
    TaskVersion,
    HeadSha,
    Note,
    ActorUserId,
    ClientRequestId,
    PayloadHash,
    CreatedAt,
}

#[derive(Iden, Clone, Copy)]
enum CollabCompletionSnapshots {
    Table,
    Id,
    RepoId,
    TaskId,
    ConditionSetVersion,
    TaskVersion,
    Overall,
    ResultJson,
    ActorKind,
    ActorId,
    CreatedAt,
}

#[derive(Iden)]
enum CollabTasks {
    Table,
    Id,
    RepoId,
}
