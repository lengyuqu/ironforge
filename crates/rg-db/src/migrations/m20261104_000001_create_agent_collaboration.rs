//! Create the same-account multi-agent collaboration tables.
//!
//! `agent_sessions`, `collab_tasks`, `collab_messages`, `collab_handoffs` and
//! `collab_reads` (see `ironforge-docs/architecture/multi-agent-collaboration-design-2026-11.md`).
//!
//! Design notes:
//!
//! - Ids of the four collaboration objects are server-generated UUID strings
//!   (`VARCHAR(36)`); account and repository ids keep their existing `BIGINT` type.
//! - "Every referenced object belongs to the same repository" is enforced by the
//!   database, not only by the service layer: referenced tables expose a
//!   `UNIQUE (id, repo_id)` (or `(id, task_id)`) key and referencing tables use
//!   composite foreign keys onto it. Nullable references (`owner_session_id`,
//!   `reply_to_id`, `target_session_id`) are not checked while `NULL`.
//! - Every table name is set explicitly and matches the entity `table_name`.
//! - `collab_tasks.next_seq` is the number of the last committed message of the
//!   task. Appending a message increments it inside the inserting transaction,
//!   which serialises writers per task so `seq` order equals commit order.

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261104_000001_create_agent_collaboration"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        if !manager.has_table("agent_sessions").await? {
            manager.create_table(agent_sessions_table()).await?;
            manager
                .create_index(
                    Index::create()
                        .if_not_exists()
                        .name("idx_agent_sessions_repo_seen")
                        .table(AgentSessions::Table)
                        .col(AgentSessions::RepoId)
                        .col(AgentSessions::LastSeenAt)
                        .col(AgentSessions::Id)
                        .to_owned(),
                )
                .await?;
            manager
                .create_index(
                    Index::create()
                        .if_not_exists()
                        .name("idx_agent_sessions_user_id")
                        .table(AgentSessions::Table)
                        .col(AgentSessions::UserId)
                        .to_owned(),
                )
                .await?;
        }

        if !manager.has_table("collab_tasks").await? {
            manager.create_table(collab_tasks_table()).await?;
            manager
                .create_index(
                    Index::create()
                        .if_not_exists()
                        .name("idx_collab_tasks_repo_updated")
                        .table(CollabTasks::Table)
                        .col(CollabTasks::RepoId)
                        .col(CollabTasks::UpdatedAt)
                        .col(CollabTasks::Id)
                        .to_owned(),
                )
                .await?;
            manager
                .create_index(
                    Index::create()
                        .if_not_exists()
                        .name("idx_collab_tasks_repo_status_updated")
                        .table(CollabTasks::Table)
                        .col(CollabTasks::RepoId)
                        .col(CollabTasks::Status)
                        .col(CollabTasks::UpdatedAt)
                        .col(CollabTasks::Id)
                        .to_owned(),
                )
                .await?;
        }

        if !manager.has_table("collab_messages").await? {
            manager.create_table(collab_messages_table()).await?;
            manager
                .create_index(
                    Index::create()
                        .if_not_exists()
                        .name("idx_collab_messages_target")
                        .table(CollabMessages::Table)
                        .col(CollabMessages::TargetSessionId)
                        .to_owned(),
                )
                .await?;
        }

        if !manager.has_table("collab_handoffs").await? {
            manager.create_table(collab_handoffs_table()).await?;
            manager
                .create_index(
                    Index::create()
                        .if_not_exists()
                        .name("idx_collab_handoffs_task_created")
                        .table(CollabHandoffs::Table)
                        .col(CollabHandoffs::TaskId)
                        .col(CollabHandoffs::CreatedAt)
                        .col(CollabHandoffs::Id)
                        .to_owned(),
                )
                .await?;
            manager
                .create_index(
                    Index::create()
                        .if_not_exists()
                        .name("idx_collab_handoffs_to_session")
                        .table(CollabHandoffs::Table)
                        .col(CollabHandoffs::ToSessionId)
                        .to_owned(),
                )
                .await?;
        }

        if !manager.has_table("collab_reads").await? {
            manager.create_table(collab_reads_table()).await?;
            manager
                .create_index(
                    Index::create()
                        .if_not_exists()
                        .name("idx_collab_reads_session")
                        .table(CollabReads::Table)
                        .col(CollabReads::SessionId)
                        .to_owned(),
                )
                .await?;
        }

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        for table in [
            Table::drop().table(CollabReads::Table).if_exists().to_owned(),
            Table::drop().table(CollabHandoffs::Table).if_exists().to_owned(),
            Table::drop().table(CollabMessages::Table).if_exists().to_owned(),
            Table::drop().table(CollabTasks::Table).if_exists().to_owned(),
            Table::drop().table(AgentSessions::Table).if_exists().to_owned(),
        ] {
            manager.drop_table(table).await?;
        }
        Ok(())
    }
}

fn uuid_col<T: IntoIden>(name: T) -> ColumnDef {
    let mut col = ColumnDef::new(name);
    col.string_len(36);
    col
}

fn agent_sessions_table() -> TableCreateStatement {
    Table::create()
        .table(AgentSessions::Table)
        .if_not_exists()
        .col(uuid_col(AgentSessions::Id).not_null().primary_key())
        .col(ColumnDef::new(AgentSessions::RepoId).big_integer().not_null())
        .col(ColumnDef::new(AgentSessions::UserId).big_integer().not_null())
        .col(ColumnDef::new(AgentSessions::SecretHash).string_len(64).not_null())
        .col(ColumnDef::new(AgentSessions::DisplayName).string_len(80).not_null())
        .col(ColumnDef::new(AgentSessions::ClientKind).string_len(64).not_null())
        .col(
            ColumnDef::new(AgentSessions::CreatedAt)
                .timestamp_with_time_zone()
                .not_null(),
        )
        .col(
            ColumnDef::new(AgentSessions::LastSeenAt)
                .timestamp_with_time_zone()
                .not_null(),
        )
        .col(
            ColumnDef::new(AgentSessions::ExpiresAt)
                .timestamp_with_time_zone()
                .not_null(),
        )
        .col(ColumnDef::new(AgentSessions::EndedAt).timestamp_with_time_zone().null())
        // Target of composite foreign keys that must stay inside one repository.
        .index(
            Index::create()
                .name("uq_agent_sessions_id_repo")
                .col(AgentSessions::Id)
                .col(AgentSessions::RepoId)
                .unique(),
        )
        .foreign_key(
            ForeignKey::create()
                .from(AgentSessions::Table, AgentSessions::RepoId)
                .to(Repositories::Table, Repositories::Id)
                .on_delete(ForeignKeyAction::Cascade),
        )
        .foreign_key(
            ForeignKey::create()
                .from(AgentSessions::Table, AgentSessions::UserId)
                .to(Users::Table, Users::Id)
                .on_delete(ForeignKeyAction::Cascade),
        )
        .to_owned()
}

fn collab_tasks_table() -> TableCreateStatement {
    Table::create()
        .table(CollabTasks::Table)
        .if_not_exists()
        .col(uuid_col(CollabTasks::Id).not_null().primary_key())
        .col(ColumnDef::new(CollabTasks::RepoId).big_integer().not_null())
        .col(ColumnDef::new(CollabTasks::Title).string_len(200).not_null())
        .col(ColumnDef::new(CollabTasks::Description).text().null())
        .col(
            ColumnDef::new(CollabTasks::Status)
                .string_len(16)
                .not_null()
                .default("open"),
        )
        .col(uuid_col(CollabTasks::OwnerSessionId).null())
        .col(ColumnDef::new(CollabTasks::Branch).string_len(255).null())
        .col(ColumnDef::new(CollabTasks::PathsJson).text().not_null())
        .col(ColumnDef::new(CollabTasks::IssueNumber).big_integer().null())
        .col(ColumnDef::new(CollabTasks::PrNumber).big_integer().null())
        .col(
            ColumnDef::new(CollabTasks::Version)
                .big_integer()
                .not_null()
                .default(1),
        )
        .col(
            ColumnDef::new(CollabTasks::NextSeq)
                .big_integer()
                .not_null()
                .default(0),
        )
        .col(
            ColumnDef::new(CollabTasks::CreatedAt)
                .timestamp_with_time_zone()
                .not_null(),
        )
        .col(
            ColumnDef::new(CollabTasks::UpdatedAt)
                .timestamp_with_time_zone()
                .not_null(),
        )
        .index(
            Index::create()
                .name("uq_collab_tasks_id_repo")
                .col(CollabTasks::Id)
                .col(CollabTasks::RepoId)
                .unique(),
        )
        .foreign_key(
            ForeignKey::create()
                .from(CollabTasks::Table, CollabTasks::RepoId)
                .to(Repositories::Table, Repositories::Id)
                .on_delete(ForeignKeyAction::Cascade),
        )
        .foreign_key(
            ForeignKey::create()
                .from(CollabTasks::Table, CollabTasks::OwnerSessionId)
                .from(CollabTasks::Table, CollabTasks::RepoId)
                .to(AgentSessions::Table, AgentSessions::Id)
                .to(AgentSessions::Table, AgentSessions::RepoId)
                .on_delete(ForeignKeyAction::Cascade),
        )
        .to_owned()
}

fn collab_messages_table() -> TableCreateStatement {
    Table::create()
        .table(CollabMessages::Table)
        .if_not_exists()
        .col(uuid_col(CollabMessages::Id).not_null().primary_key())
        .col(ColumnDef::new(CollabMessages::RepoId).big_integer().not_null())
        .col(uuid_col(CollabMessages::TaskId).not_null())
        .col(ColumnDef::new(CollabMessages::Seq).big_integer().not_null())
        .col(uuid_col(CollabMessages::AuthorSessionId).not_null())
        .col(ColumnDef::new(CollabMessages::Kind).string_len(16).not_null())
        .col(ColumnDef::new(CollabMessages::Body).text().not_null())
        .col(uuid_col(CollabMessages::ReplyToId).null())
        .col(uuid_col(CollabMessages::TargetSessionId).null())
        .col(ColumnDef::new(CollabMessages::CommitSha).string_len(64).null())
        .col(ColumnDef::new(CollabMessages::ClientMessageId).string_len(128).not_null())
        .col(ColumnDef::new(CollabMessages::PayloadHash).string_len(64).not_null())
        .col(
            ColumnDef::new(CollabMessages::CreatedAt)
                .timestamp_with_time_zone()
                .not_null(),
        )
        .index(
            Index::create()
                .name("uq_collab_messages_task_seq")
                .col(CollabMessages::TaskId)
                .col(CollabMessages::Seq)
                .unique(),
        )
        .index(
            Index::create()
                .name("uq_collab_messages_author_client_id")
                .col(CollabMessages::AuthorSessionId)
                .col(CollabMessages::ClientMessageId)
                .unique(),
        )
        // Target of the same-task `reply_to` foreign key below.
        .index(
            Index::create()
                .name("uq_collab_messages_id_task")
                .col(CollabMessages::Id)
                .col(CollabMessages::TaskId)
                .unique(),
        )
        .foreign_key(
            ForeignKey::create()
                .from(CollabMessages::Table, CollabMessages::TaskId)
                .from(CollabMessages::Table, CollabMessages::RepoId)
                .to(CollabTasks::Table, CollabTasks::Id)
                .to(CollabTasks::Table, CollabTasks::RepoId)
                .on_delete(ForeignKeyAction::Cascade),
        )
        .foreign_key(
            ForeignKey::create()
                .from(CollabMessages::Table, CollabMessages::AuthorSessionId)
                .from(CollabMessages::Table, CollabMessages::RepoId)
                .to(AgentSessions::Table, AgentSessions::Id)
                .to(AgentSessions::Table, AgentSessions::RepoId)
                .on_delete(ForeignKeyAction::Cascade),
        )
        .foreign_key(
            ForeignKey::create()
                .from(CollabMessages::Table, CollabMessages::TargetSessionId)
                .from(CollabMessages::Table, CollabMessages::RepoId)
                .to(AgentSessions::Table, AgentSessions::Id)
                .to(AgentSessions::Table, AgentSessions::RepoId)
                .on_delete(ForeignKeyAction::Cascade),
        )
        .foreign_key(
            ForeignKey::create()
                .from(CollabMessages::Table, CollabMessages::ReplyToId)
                .from(CollabMessages::Table, CollabMessages::TaskId)
                .to(CollabMessages::Table, CollabMessages::Id)
                .to(CollabMessages::Table, CollabMessages::TaskId)
                .on_delete(ForeignKeyAction::Cascade),
        )
        .to_owned()
}

fn collab_handoffs_table() -> TableCreateStatement {
    Table::create()
        .table(CollabHandoffs::Table)
        .if_not_exists()
        .col(uuid_col(CollabHandoffs::Id).not_null().primary_key())
        .col(ColumnDef::new(CollabHandoffs::RepoId).big_integer().not_null())
        .col(uuid_col(CollabHandoffs::TaskId).not_null())
        .col(uuid_col(CollabHandoffs::FromSessionId).not_null())
        .col(uuid_col(CollabHandoffs::ToSessionId).not_null())
        .col(ColumnDef::new(CollabHandoffs::Summary).text().not_null())
        .col(ColumnDef::new(CollabHandoffs::Completed).text().not_null())
        .col(ColumnDef::new(CollabHandoffs::NextSteps).text().not_null())
        .col(ColumnDef::new(CollabHandoffs::Blockers).text().not_null())
        .col(ColumnDef::new(CollabHandoffs::CommitSha).string_len(64).null())
        .col(ColumnDef::new(CollabHandoffs::EvidenceJson).text().not_null())
        .col(ColumnDef::new(CollabHandoffs::ClientHandoffId).string_len(128).not_null())
        .col(ColumnDef::new(CollabHandoffs::PayloadHash).string_len(64).not_null())
        .col(
            ColumnDef::new(CollabHandoffs::Status)
                .string_len(16)
                .not_null()
                .default("pending"),
        )
        .col(
            ColumnDef::new(CollabHandoffs::Version)
                .big_integer()
                .not_null()
                .default(1),
        )
        .col(
            ColumnDef::new(CollabHandoffs::CreatedAt)
                .timestamp_with_time_zone()
                .not_null(),
        )
        .col(
            ColumnDef::new(CollabHandoffs::AcceptedAt)
                .timestamp_with_time_zone()
                .null(),
        )
        .index(
            Index::create()
                .name("uq_collab_handoffs_from_client_id")
                .col(CollabHandoffs::FromSessionId)
                .col(CollabHandoffs::ClientHandoffId)
                .unique(),
        )
        .foreign_key(
            ForeignKey::create()
                .from(CollabHandoffs::Table, CollabHandoffs::TaskId)
                .from(CollabHandoffs::Table, CollabHandoffs::RepoId)
                .to(CollabTasks::Table, CollabTasks::Id)
                .to(CollabTasks::Table, CollabTasks::RepoId)
                .on_delete(ForeignKeyAction::Cascade),
        )
        .foreign_key(
            ForeignKey::create()
                .from(CollabHandoffs::Table, CollabHandoffs::FromSessionId)
                .from(CollabHandoffs::Table, CollabHandoffs::RepoId)
                .to(AgentSessions::Table, AgentSessions::Id)
                .to(AgentSessions::Table, AgentSessions::RepoId)
                .on_delete(ForeignKeyAction::Cascade),
        )
        .foreign_key(
            ForeignKey::create()
                .from(CollabHandoffs::Table, CollabHandoffs::ToSessionId)
                .from(CollabHandoffs::Table, CollabHandoffs::RepoId)
                .to(AgentSessions::Table, AgentSessions::Id)
                .to(AgentSessions::Table, AgentSessions::RepoId)
                .on_delete(ForeignKeyAction::Cascade),
        )
        .to_owned()
}

fn collab_reads_table() -> TableCreateStatement {
    Table::create()
        .table(CollabReads::Table)
        .if_not_exists()
        .col(uuid_col(CollabReads::TaskId).not_null())
        .col(uuid_col(CollabReads::SessionId).not_null())
        .col(
            ColumnDef::new(CollabReads::LastReadSeq)
                .big_integer()
                .not_null()
                .default(0),
        )
        .col(
            ColumnDef::new(CollabReads::UpdatedAt)
                .timestamp_with_time_zone()
                .not_null(),
        )
        .primary_key(
            Index::create()
                .name("pk_collab_reads")
                .col(CollabReads::TaskId)
                .col(CollabReads::SessionId),
        )
        .foreign_key(
            ForeignKey::create()
                .from(CollabReads::Table, CollabReads::TaskId)
                .to(CollabTasks::Table, CollabTasks::Id)
                .on_delete(ForeignKeyAction::Cascade),
        )
        .foreign_key(
            ForeignKey::create()
                .from(CollabReads::Table, CollabReads::SessionId)
                .to(AgentSessions::Table, AgentSessions::Id)
                .on_delete(ForeignKeyAction::Cascade),
        )
        .to_owned()
}

#[derive(Iden)]
enum AgentSessions {
    Table,
    Id,
    RepoId,
    UserId,
    SecretHash,
    DisplayName,
    ClientKind,
    CreatedAt,
    LastSeenAt,
    ExpiresAt,
    EndedAt,
}

#[derive(Iden)]
enum CollabTasks {
    Table,
    Id,
    RepoId,
    Title,
    Description,
    Status,
    OwnerSessionId,
    Branch,
    PathsJson,
    IssueNumber,
    PrNumber,
    Version,
    NextSeq,
    CreatedAt,
    UpdatedAt,
}

#[derive(Iden)]
enum CollabMessages {
    Table,
    Id,
    RepoId,
    TaskId,
    Seq,
    AuthorSessionId,
    Kind,
    Body,
    ReplyToId,
    TargetSessionId,
    CommitSha,
    ClientMessageId,
    PayloadHash,
    CreatedAt,
}

#[derive(Iden)]
enum CollabHandoffs {
    Table,
    Id,
    RepoId,
    TaskId,
    FromSessionId,
    ToSessionId,
    Summary,
    Completed,
    NextSteps,
    Blockers,
    CommitSha,
    EvidenceJson,
    ClientHandoffId,
    PayloadHash,
    Status,
    Version,
    CreatedAt,
    AcceptedAt,
}

#[derive(Iden)]
enum CollabReads {
    Table,
    TaskId,
    SessionId,
    LastReadSeq,
    UpdatedAt,
}

#[derive(Iden)]
enum Repositories {
    Table,
    Id,
}

#[derive(Iden)]
enum Users {
    Table,
    Id,
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm_migration::sea_query::{MysqlQueryBuilder, PostgresQueryBuilder, SqliteQueryBuilder};

    /// The `#[derive(Iden)]` pitfall (CLAUDE.md #13): enum names become
    /// snake_case, so `CollabTasks` yields `collab_tasks`. Check every table name
    /// the DDL emits equals the plural name the entities declare.
    #[test]
    fn ddl_uses_plural_table_names_on_all_backends() {
        let tables = [
            (agent_sessions_table(), "agent_sessions"),
            (collab_tasks_table(), "collab_tasks"),
            (collab_messages_table(), "collab_messages"),
            (collab_handoffs_table(), "collab_handoffs"),
            (collab_reads_table(), "collab_reads"),
        ];
        for (stmt, name) in tables {
            for ddl in [
                stmt.to_string(SqliteQueryBuilder),
                stmt.to_string(PostgresQueryBuilder),
                stmt.to_string(MysqlQueryBuilder),
            ] {
                assert!(ddl.contains(&format!("\"{name}\"")) || ddl.contains(&format!("`{name}`")), "{ddl}");
                assert!(ddl.contains("FOREIGN KEY"), "{ddl}");
            }
        }
    }
}
