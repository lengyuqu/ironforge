//! Create the reactions table for issue / issue-comment emoji reactions.
//!
//! Mirrors Gitea's semantics: `comment_id = 0` targets the issue itself,
//! non-zero targets an issue comment. This avoids NULL-in-unique-index
//! divergence across SQLite/PostgreSQL/MySQL while keeping a single
//! uniqueness constraint for both targets.

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260820_000001_create_reactions"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        if manager.has_table("reactions").await? {
            return Ok(());
        }
        manager
            .create_table(
                Table::create()
                    .table(Reactions::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Reactions::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Reactions::IssueId).big_integer().not_null())
                    // 0 = reaction on the issue itself; non-zero targets an
                    // issue comment (validated at the application layer, no FK
                    // because 0 is not a valid comment id).
                    .col(
                        ColumnDef::new(Reactions::CommentId)
                            .big_integer()
                            .not_null()
                            .default(0),
                    )
                    .col(ColumnDef::new(Reactions::UserId).big_integer().not_null())
                    .col(ColumnDef::new(Reactions::Content).string().not_null())
                    .col(
                        ColumnDef::new(Reactions::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(Reactions::Table, Reactions::IssueId)
                            .to(Issues::Table, Issues::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(Reactions::Table, Reactions::UserId)
                            .to(Users::Table, Users::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_reactions_unique_target_user_content")
                    .table(Reactions::Table)
                    .col(Reactions::IssueId)
                    .col(Reactions::CommentId)
                    .col(Reactions::UserId)
                    .col(Reactions::Content)
                    .unique()
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_reactions_comment")
                    .table(Reactions::Table)
                    .col(Reactions::CommentId)
                    .to_owned(),
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Reactions::Table).if_exists().to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum Reactions {
    Table,
    Id,
    IssueId,
    CommentId,
    UserId,
    Content,
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
