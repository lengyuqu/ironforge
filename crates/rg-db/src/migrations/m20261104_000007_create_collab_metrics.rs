//! Collaboration health metrics: the platform event log behind R13.
//!
//! Design: `ironforge-docs/architecture/agent-collaboration-p2-design-2026-11.md` §3/§5.
//! Only a new table; earlier collaboration migrations are not touched. No
//! rollup table: every metric is computed from the events.
//!
//! Events are written by the same transaction as the business change they
//! describe and carry server time only. They hold ids and states, never
//! message bodies, titles or secrets.

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261104_000007_create_collab_metrics"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        if manager.has_table("collab_metric_events").await? {
            return Ok(());
        }
        manager.create_table(events_table()).await?;
        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("idx_collab_metric_events_repo_occurred")
                    .table(CollabMetricEvents::Table)
                    .col(CollabMetricEvents::RepoId)
                    .col(CollabMetricEvents::OccurredAt)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("idx_collab_metric_events_task_occurred")
                    .table(CollabMetricEvents::Table)
                    .col(CollabMetricEvents::TaskId)
                    .col(CollabMetricEvents::OccurredAt)
                    .to_owned(),
            )
            .await?;
        Ok(())
    }

    /// Drops the table so that rolling back the first collaboration migration
    /// afterwards leaves no table whose foreign key points at `collab_tasks`.
    /// Like theirs, it destroys the data; `up` is idempotent.
    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(CollabMetricEvents::Table).if_exists().to_owned())
            .await
    }
}

fn events_table() -> TableCreateStatement {
    Table::create()
        .table(CollabMetricEvents::Table)
        .if_not_exists()
        .col(ColumnDef::new(CollabMetricEvents::Id).string_len(36).not_null().primary_key())
        .col(ColumnDef::new(CollabMetricEvents::RepoId).big_integer().not_null())
        .col(ColumnDef::new(CollabMetricEvents::TaskId).string_len(36).not_null())
        // e.g. `task_status_changed`, `handoff_accepted`; see rg_db::ops::collab_metric_ops.
        .col(ColumnDef::new(CollabMetricEvents::EventType).string_len(32).not_null())
        .col(ColumnDef::new(CollabMetricEvents::FromState).string_len(16).null())
        .col(ColumnDef::new(CollabMetricEvents::ToState).string_len(16).null())
        // Task version after a status change: orders events that share a timestamp.
        .col(ColumnDef::new(CollabMetricEvents::TaskVersion).big_integer().null())
        // `task` | `handoff` | `question` | `takeover` | `baseline_snapshot`.
        .col(ColumnDef::new(CollabMetricEvents::SourceType).string_len(24).not_null())
        .col(ColumnDef::new(CollabMetricEvents::SourceId).string_len(128).not_null())
        .col(
            ColumnDef::new(CollabMetricEvents::OccurredAt)
                .timestamp_with_time_zone()
                .not_null(),
        )
        .col(
            ColumnDef::new(CollabMetricEvents::RecordedAt)
                .timestamp_with_time_zone()
                .not_null(),
        )
        // Recording the same transition twice is a no-op, never a duplicate.
        .index(
            Index::create()
                .name("uq_collab_metric_events_source")
                .col(CollabMetricEvents::RepoId)
                .col(CollabMetricEvents::SourceType)
                .col(CollabMetricEvents::SourceId)
                .col(CollabMetricEvents::EventType)
                .unique(),
        )
        .foreign_key(
            ForeignKey::create()
                .from(CollabMetricEvents::Table, CollabMetricEvents::TaskId)
                .from(CollabMetricEvents::Table, CollabMetricEvents::RepoId)
                .to(CollabTasks::Table, CollabTasks::Id)
                .to(CollabTasks::Table, CollabTasks::RepoId)
                .on_delete(ForeignKeyAction::Cascade),
        )
        .to_owned()
}

#[derive(Iden)]
enum CollabMetricEvents {
    Table,
    Id,
    RepoId,
    TaskId,
    EventType,
    FromState,
    ToState,
    TaskVersion,
    SourceType,
    SourceId,
    OccurredAt,
    RecordedAt,
}

#[derive(Iden)]
enum CollabTasks {
    Table,
    Id,
    RepoId,
}
