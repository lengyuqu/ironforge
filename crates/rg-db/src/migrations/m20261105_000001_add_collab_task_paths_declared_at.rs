//! When a task's declared `paths` were last stated (P1 U03, conflict warnings).
//!
//! `collab_tasks.updated_at` moves on every edit, so it cannot say how fresh the
//! *path declaration* is. This nullable column is written only when the paths
//! are declared (at creation with a non-empty list, or when a later update
//! carries `paths`). Existing rows stay `NULL`: their declaration time is
//! unknown and is reported as such, never back-filled from `updated_at`.

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261105_000001_add_collab_task_paths_declared_at"
    }
}

#[derive(DeriveIden)]
enum CollabTasks {
    Table,
    PathsDeclaredAt,
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    /// Idempotent: `down` is a no-op, so a re-apply must not fail with a
    /// duplicate column.
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        if manager.has_table("collab_tasks").await?
            && !manager
                .has_column("collab_tasks", "paths_declared_at")
                .await?
        {
            manager
                .alter_table(
                    Table::alter()
                        .table(CollabTasks::Table)
                        .add_column(
                            ColumnDef::new(CollabTasks::PathsDeclaredAt)
                                .timestamp_with_time_zone()
                                .null(),
                        )
                        .to_owned(),
                )
                .await?;
        }
        Ok(())
    }

    /// Intentionally empty: SQLite cannot reliably drop a column through the
    /// schema builder, and a nullable column is harmless to older code. `up` is
    /// idempotent, so rollback followed by re-apply works.
    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Ok(())
    }
}
