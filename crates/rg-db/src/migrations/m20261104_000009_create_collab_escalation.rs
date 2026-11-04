//! Per-repository escalation settings for collaboration notices (O05).
//!
//! Design: `ironforge-docs/architecture/agent-collaboration-operability-requirements-2026-11.md`
//! §6. One row per repository, absent = escalation disabled. Only a new table;
//! escalation reminders themselves are ordinary rows of `collab_notices`.

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261104_000009_create_collab_escalation"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        if !manager.has_table("collab_escalation_settings").await? {
            manager.create_table(settings_table()).await?;
        }
        Ok(())
    }

    /// Drops the table; `up` is idempotent so a rollback and re-apply works.
    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(
                Table::drop()
                    .table(CollabEscalationSettings::Table)
                    .if_exists()
                    .to_owned(),
            )
            .await
    }
}

fn settings_table() -> TableCreateStatement {
    Table::create()
        .table(CollabEscalationSettings::Table)
        .if_not_exists()
        .col(
            ColumnDef::new(CollabEscalationSettings::RepoId)
                .big_integer()
                .not_null()
                .primary_key(),
        )
        .col(ColumnDef::new(CollabEscalationSettings::Enabled).boolean().not_null())
        // Seconds a task may stay blocked/waiting, a question may stay open, or a
        // handoff may stay pending after its target expired, before a reminder.
        .col(ColumnDef::new(CollabEscalationSettings::BlockedAfterSecs).big_integer().not_null())
        .col(ColumnDef::new(CollabEscalationSettings::QuestionAfterSecs).big_integer().not_null())
        .col(ColumnDef::new(CollabEscalationSettings::HandoffAfterSecs).big_integer().not_null())
        // 0 = remind once per episode; otherwise the earliest repeat, and only
        // after the previous reminder was acknowledged.
        .col(ColumnDef::new(CollabEscalationSettings::RepeatAfterSecs).big_integer().not_null())
        // Quiet window in minutes after 00:00 UTC; both null = none.
        .col(ColumnDef::new(CollabEscalationSettings::QuietStartMinute).integer().null())
        .col(ColumnDef::new(CollabEscalationSettings::QuietEndMinute).integer().null())
        .col(ColumnDef::new(CollabEscalationSettings::Version).big_integer().not_null())
        .col(ColumnDef::new(CollabEscalationSettings::UpdatedByUserId).big_integer().not_null())
        .col(
            ColumnDef::new(CollabEscalationSettings::UpdatedAt)
                .timestamp_with_time_zone()
                .not_null(),
        )
        .foreign_key(
            ForeignKey::create()
                .from(CollabEscalationSettings::Table, CollabEscalationSettings::RepoId)
                .to(Repositories::Table, Repositories::Id)
                .on_delete(ForeignKeyAction::Cascade),
        )
        .to_owned()
}

#[derive(Iden)]
enum CollabEscalationSettings {
    Table,
    RepoId,
    Enabled,
    BlockedAfterSecs,
    QuestionAfterSecs,
    HandoffAfterSecs,
    RepeatAfterSecs,
    QuietStartMinute,
    QuietEndMinute,
    Version,
    UpdatedByUserId,
    UpdatedAt,
}

#[derive(Iden)]
enum Repositories {
    Table,
    Id,
}
