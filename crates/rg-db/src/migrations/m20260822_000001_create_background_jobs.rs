//! Create the background_jobs table for the durable background task queue
//! (QUEUE-001).
//!
//! One row per queued task (webhook delivery retry, email retry, ...).
//! Workers claim rows with a conditional UPDATE (`status = 'pending' AND
//! `run_at <= now` → `running`), so the queue is safe for multiple worker
//! processes without advisory locks. Failed attempts are rescheduled with
//! exponential backoff; exhausted jobs transition to `dead` (dead-letter).

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260822_000001_create_background_jobs"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        if manager.has_table("background_jobs").await? {
            return Ok(());
        }
        manager
            .create_table(
                Table::create()
                    .table(BackgroundJobs::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(BackgroundJobs::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(BackgroundJobs::TaskType)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(BackgroundJobs::Payload)
                            .text()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(BackgroundJobs::Status)
                            .string_len(16)
                            .not_null()
                            .default("pending"),
                    )
                    .col(
                        ColumnDef::new(BackgroundJobs::Attempts)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(BackgroundJobs::MaxAttempts)
                            .integer()
                            .not_null()
                            .default(5),
                    )
                    .col(
                        ColumnDef::new(BackgroundJobs::RunAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(BackgroundJobs::LockedBy)
                            .string_len(128)
                            .null(),
                    )
                    .col(
                        ColumnDef::new(BackgroundJobs::LockedAt)
                            .timestamp_with_time_zone()
                            .null(),
                    )
                    .col(ColumnDef::new(BackgroundJobs::LastError).text().null())
                    .col(
                        ColumnDef::new(BackgroundJobs::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(BackgroundJobs::UpdatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_background_jobs_status_run_at")
                    .table(BackgroundJobs::Table)
                    .col(BackgroundJobs::Status)
                    .col(BackgroundJobs::RunAt)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_background_jobs_type_status")
                    .table(BackgroundJobs::Table)
                    .col(BackgroundJobs::TaskType)
                    .col(BackgroundJobs::Status)
                    .to_owned(),
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(BackgroundJobs::Table).if_exists().to_owned())
            .await
    }
}

#[derive(Iden)]
enum BackgroundJobs {
    #[iden = "background_jobs"]
    Table,
    Id,
    TaskType,
    Payload,
    Status,
    Attempts,
    MaxAttempts,
    RunAt,
    LockedBy,
    LockedAt,
    LastError,
    CreatedAt,
    UpdatedAt,
}
