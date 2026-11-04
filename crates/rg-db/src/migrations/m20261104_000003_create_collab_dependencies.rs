//! Collaboration P1 task dependencies: the per-repository graph version row and
//! the dependency edges.
//!
//! Design: `ironforge-docs/architecture/agent-collaboration-p1-design-2026-11.md` §2–§3.
//! Only new tables; earlier collaboration migrations are not touched.
//!
//! `collab_dependency_graphs` holds one row per repository whose `version` is the
//! arbiter of every edge change (a conditional `UPDATE` inside the same
//! transaction as the cycle check and the edge write). `collab_task_dependencies`
//! stores edges `task_id -> depends_on_task_id` ("task waits for dependency");
//! both ends reference `collab_tasks` through composite foreign keys, so an edge
//! can never cross repositories.

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261104_000003_create_collab_dependencies"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        if !manager.has_table("collab_dependency_graphs").await? {
            manager.create_table(graphs_table()).await?;
        }
        if !manager.has_table("collab_task_dependencies").await? {
            manager.create_table(dependencies_table()).await?;
            manager
                .create_index(
                    Index::create()
                        .if_not_exists()
                        .name("idx_collab_task_dependencies_target")
                        .table(CollabTaskDependencies::Table)
                        .col(CollabTaskDependencies::DependsOnTaskId)
                        .to_owned(),
                )
                .await?;
        }
        Ok(())
    }

    /// Drops the edges first, then the graph rows; destroys their data like the
    /// other collaboration migrations' `down`. `up` is idempotent.
    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        for table in [
            Table::drop()
                .table(CollabTaskDependencies::Table)
                .if_exists()
                .to_owned(),
            Table::drop()
                .table(CollabDependencyGraphs::Table)
                .if_exists()
                .to_owned(),
        ] {
            manager.drop_table(table).await?;
        }
        Ok(())
    }
}

fn graphs_table() -> TableCreateStatement {
    Table::create()
        .table(CollabDependencyGraphs::Table)
        .if_not_exists()
        .col(
            ColumnDef::new(CollabDependencyGraphs::RepoId)
                .big_integer()
                .not_null()
                .primary_key(),
        )
        // Starts at 1 when the first edge is written; "no row" reads as version 0.
        .col(
            ColumnDef::new(CollabDependencyGraphs::Version)
                .big_integer()
                .not_null(),
        )
        .col(
            ColumnDef::new(CollabDependencyGraphs::UpdatedAt)
                .timestamp_with_time_zone()
                .not_null(),
        )
        .foreign_key(
            ForeignKey::create()
                .from(
                    CollabDependencyGraphs::Table,
                    CollabDependencyGraphs::RepoId,
                )
                .to(Repositories::Table, Repositories::Id)
                .on_delete(ForeignKeyAction::Cascade),
        )
        .to_owned()
}

fn dependencies_table() -> TableCreateStatement {
    Table::create()
        .table(CollabTaskDependencies::Table)
        .if_not_exists()
        .col(
            ColumnDef::new(CollabTaskDependencies::RepoId)
                .big_integer()
                .not_null(),
        )
        .col(
            ColumnDef::new(CollabTaskDependencies::TaskId)
                .string_len(36)
                .not_null(),
        )
        .col(
            ColumnDef::new(CollabTaskDependencies::DependsOnTaskId)
                .string_len(36)
                .not_null(),
        )
        // `session` or `user`: the door used, not an identity proof.
        .col(
            ColumnDef::new(CollabTaskDependencies::CreatedByKind)
                .string_len(16)
                .not_null(),
        )
        .col(
            ColumnDef::new(CollabTaskDependencies::CreatedById)
                .string_len(64)
                .not_null(),
        )
        .col(
            ColumnDef::new(CollabTaskDependencies::ClientDependencyId)
                .string_len(128)
                .not_null(),
        )
        .col(
            ColumnDef::new(CollabTaskDependencies::PayloadHash)
                .string_len(64)
                .not_null(),
        )
        // Graph version this edge's insertion produced.
        .col(
            ColumnDef::new(CollabTaskDependencies::GraphVersion)
                .big_integer()
                .not_null(),
        )
        .col(
            ColumnDef::new(CollabTaskDependencies::CreatedAt)
                .timestamp_with_time_zone()
                .not_null(),
        )
        .primary_key(
            Index::create()
                .name("pk_collab_task_dependencies")
                .col(CollabTaskDependencies::TaskId)
                .col(CollabTaskDependencies::DependsOnTaskId),
        )
        .index(
            Index::create()
                .name("uq_collab_task_dependencies_creator_client_id")
                .col(CollabTaskDependencies::CreatedByKind)
                .col(CollabTaskDependencies::CreatedById)
                .col(CollabTaskDependencies::ClientDependencyId)
                .unique(),
        )
        .foreign_key(
            ForeignKey::create()
                .from(
                    CollabTaskDependencies::Table,
                    CollabTaskDependencies::TaskId,
                )
                .from(
                    CollabTaskDependencies::Table,
                    CollabTaskDependencies::RepoId,
                )
                .to(CollabTasks::Table, CollabTasks::Id)
                .to(CollabTasks::Table, CollabTasks::RepoId)
                .on_delete(ForeignKeyAction::Cascade),
        )
        .foreign_key(
            ForeignKey::create()
                .from(
                    CollabTaskDependencies::Table,
                    CollabTaskDependencies::DependsOnTaskId,
                )
                .from(
                    CollabTaskDependencies::Table,
                    CollabTaskDependencies::RepoId,
                )
                .to(CollabTasks::Table, CollabTasks::Id)
                .to(CollabTasks::Table, CollabTasks::RepoId)
                .on_delete(ForeignKeyAction::Cascade),
        )
        .to_owned()
}

#[derive(Iden)]
enum CollabDependencyGraphs {
    Table,
    RepoId,
    Version,
    UpdatedAt,
}

#[derive(Iden)]
enum CollabTaskDependencies {
    Table,
    RepoId,
    TaskId,
    DependsOnTaskId,
    CreatedByKind,
    CreatedById,
    ClientDependencyId,
    PayloadHash,
    GraphVersion,
    CreatedAt,
}

// Referenced tables.
#[derive(Iden)]
enum CollabTasks {
    Table,
    Id,
    RepoId,
}

#[derive(Iden)]
enum Repositories {
    Table,
    Id,
}
