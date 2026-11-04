//! Create the issue_assignees table for multi-assignee support (ISSUE-105).
//!
//! Mirrors Gitea's semantics: the `issues.assignee_id` column is kept as a
//! redundant "primary assignee" (first in the list) for compatibility with
//! existing read paths, while the junction table holds the full set.
//! Existing single-assignee rows are backfilled from `issues.assignee_id`.

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260821_000001_create_issue_assignees"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        if manager.has_table("issue_assignees").await? {
            return Ok(());
        }
        manager
            .create_table(
                Table::create()
                    .table(IssueAssignees::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(IssueAssignees::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(IssueAssignees::IssueId)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(IssueAssignees::UserId)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(IssueAssignees::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(IssueAssignees::Table, IssueAssignees::IssueId)
                            .to(Issues::Table, Issues::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(IssueAssignees::Table, IssueAssignees::UserId)
                            .to(Users::Table, Users::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_issue_assignees_unique_issue_user")
                    .table(IssueAssignees::Table)
                    .col(IssueAssignees::IssueId)
                    .col(IssueAssignees::UserId)
                    .unique()
                    .to_owned(),
            )
            .await?;

        // Backfill: each issue has at most one legacy assignee, so the insert
        // cannot violate the unique index. Standard SQL, works on
        // SQLite/PostgreSQL/MySQL.
        manager
            .get_connection()
            .execute_unprepared(
                "INSERT INTO issue_assignees (issue_id, user_id, created_at) \
                 SELECT id, assignee_id, updated_at FROM issues WHERE assignee_id IS NOT NULL",
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(IssueAssignees::Table).if_exists().to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum IssueAssignees {
    Table,
    Id,
    IssueId,
    UserId,
    CreatedAt,
}

#[derive(DeriveIden)]
enum Issues {
    Table,
    Id,
}

#[derive(DeriveIden)]
enum Users {
    Table,
    Id,
}
