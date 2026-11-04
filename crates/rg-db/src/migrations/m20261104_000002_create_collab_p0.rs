//! Collaboration P0 tables: takeovers, questions, answers and work packages.
//!
//! Design: `ironforge-docs/architecture/agent-collaboration-p0-design-2026-11.md`.
//! Only new tables; the first collaboration migration is not touched.
//!
//! Conventions follow `m20261104_000001`: server-generated UUID ids
//! (`VARCHAR(36)`), explicit table names equal to the entity `table_name`,
//! composite foreign keys so every referenced object belongs to the same
//! repository, and per-object client idempotency keys instead of a global
//! request table.
//!
//! `actor_kind` / `*_kind` columns record which door a write came through
//! (`session` or `user`). They are not an identity proof: sessions are labels
//! under one account.

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261104_000002_create_collab_p0"
    }
}

fn uuid_col<T: IntoIden>(name: T) -> ColumnDef {
    let mut col = ColumnDef::new(name);
    col.string_len(36);
    col
}

fn created_at<T: IntoIden>(name: T) -> ColumnDef {
    let mut col = ColumnDef::new(name);
    col.timestamp_with_time_zone().not_null();
    col
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        if !manager.has_table("collab_takeovers").await? {
            manager.create_table(takeovers_table()).await?;
            manager
                .create_index(
                    Index::create()
                        .if_not_exists()
                        .name("idx_collab_takeovers_task_created")
                        .table(CollabTakeovers::Table)
                        .col(CollabTakeovers::TaskId)
                        .col(CollabTakeovers::CreatedAt)
                        .col(CollabTakeovers::Id)
                        .to_owned(),
                )
                .await?;
        }
        if !manager.has_table("collab_questions").await? {
            manager.create_table(questions_table()).await?;
            manager
                .create_index(
                    Index::create()
                        .if_not_exists()
                        .name("idx_collab_questions_task_created")
                        .table(CollabQuestions::Table)
                        .col(CollabQuestions::TaskId)
                        .col(CollabQuestions::CreatedAt)
                        .col(CollabQuestions::Id)
                        .to_owned(),
                )
                .await?;
            manager
                .create_index(
                    Index::create()
                        .if_not_exists()
                        .name("idx_collab_questions_repo_status")
                        .table(CollabQuestions::Table)
                        .col(CollabQuestions::RepoId)
                        .col(CollabQuestions::Status)
                        .to_owned(),
                )
                .await?;
        }
        // Answers reference questions, so they come after them.
        if !manager.has_table("collab_answers").await? {
            manager.create_table(answers_table()).await?;
        }
        if !manager.has_table("collab_work_packages").await? {
            manager.create_table(work_packages_table()).await?;
        }
        Ok(())
    }

    /// Drops the four tables, children first, so that rolling back the first
    /// collaboration migration afterwards does not leave tables whose foreign
    /// keys point at tables that no longer exist. Like that migration's `down`,
    /// it destroys the data in them; `up` is idempotent.
    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        for table in [
            Table::drop()
                .table(CollabAnswers::Table)
                .if_exists()
                .to_owned(),
            Table::drop()
                .table(CollabQuestions::Table)
                .if_exists()
                .to_owned(),
            Table::drop()
                .table(CollabWorkPackages::Table)
                .if_exists()
                .to_owned(),
            Table::drop()
                .table(CollabTakeovers::Table)
                .if_exists()
                .to_owned(),
        ] {
            manager.drop_table(table).await?;
        }
        Ok(())
    }
}

fn takeovers_table() -> TableCreateStatement {
    Table::create()
        .table(CollabTakeovers::Table)
        .if_not_exists()
        .col(uuid_col(CollabTakeovers::Id).not_null().primary_key())
        .col(
            ColumnDef::new(CollabTakeovers::RepoId)
                .big_integer()
                .not_null(),
        )
        .col(uuid_col(CollabTakeovers::TaskId).not_null())
        .col(uuid_col(CollabTakeovers::FromSessionId).null())
        .col(uuid_col(CollabTakeovers::ToSessionId).not_null())
        // `session` (self-claim) or `user` (reassignment through a user door).
        .col(
            ColumnDef::new(CollabTakeovers::ActorKind)
                .string_len(16)
                .not_null(),
        )
        .col(
            ColumnDef::new(CollabTakeovers::ActorId)
                .string_len(64)
                .not_null(),
        )
        .col(
            ColumnDef::new(CollabTakeovers::PreviousTaskVersion)
                .big_integer()
                .not_null(),
        )
        .col(
            ColumnDef::new(CollabTakeovers::NewTaskVersion)
                .big_integer()
                .not_null(),
        )
        // `claimed` or `reassigned`.
        .col(
            ColumnDef::new(CollabTakeovers::Outcome)
                .string_len(16)
                .not_null(),
        )
        .col(ColumnDef::new(CollabTakeovers::Reason).text().not_null())
        .col(
            ColumnDef::new(CollabTakeovers::ClientTakeoverId)
                .string_len(128)
                .not_null(),
        )
        .col(
            ColumnDef::new(CollabTakeovers::PayloadHash)
                .string_len(64)
                .not_null(),
        )
        .col(created_at(CollabTakeovers::CreatedAt))
        .index(
            Index::create()
                .name("uq_collab_takeovers_actor_client_id")
                .col(CollabTakeovers::ActorKind)
                .col(CollabTakeovers::ActorId)
                .col(CollabTakeovers::ClientTakeoverId)
                .unique(),
        )
        .foreign_key(
            ForeignKey::create()
                .from(CollabTakeovers::Table, CollabTakeovers::TaskId)
                .from(CollabTakeovers::Table, CollabTakeovers::RepoId)
                .to(CollabTasks::Table, CollabTasks::Id)
                .to(CollabTasks::Table, CollabTasks::RepoId)
                .on_delete(ForeignKeyAction::Cascade),
        )
        .foreign_key(
            ForeignKey::create()
                .from(CollabTakeovers::Table, CollabTakeovers::ToSessionId)
                .from(CollabTakeovers::Table, CollabTakeovers::RepoId)
                .to(AgentSessions::Table, AgentSessions::Id)
                .to(AgentSessions::Table, AgentSessions::RepoId)
                .on_delete(ForeignKeyAction::Cascade),
        )
        .to_owned()
}

fn questions_table() -> TableCreateStatement {
    Table::create()
        .table(CollabQuestions::Table)
        .if_not_exists()
        .col(uuid_col(CollabQuestions::Id).not_null().primary_key())
        .col(
            ColumnDef::new(CollabQuestions::RepoId)
                .big_integer()
                .not_null(),
        )
        .col(uuid_col(CollabQuestions::TaskId).not_null())
        .col(
            ColumnDef::new(CollabQuestions::CreatorKind)
                .string_len(16)
                .not_null(),
        )
        .col(
            ColumnDef::new(CollabQuestions::CreatorId)
                .string_len(64)
                .not_null(),
        )
        // `repo_writers`, `user` or `session`; the id is required for the last two.
        .col(
            ColumnDef::new(CollabQuestions::RecipientKind)
                .string_len(16)
                .not_null(),
        )
        .col(
            ColumnDef::new(CollabQuestions::RecipientId)
                .string_len(64)
                .null(),
        )
        .col(
            ColumnDef::new(CollabQuestions::Title)
                .string_len(800)
                .not_null(),
        )
        .col(ColumnDef::new(CollabQuestions::Body).text().not_null())
        .col(
            ColumnDef::new(CollabQuestions::OptionsJson)
                .text()
                .not_null(),
        )
        .col(
            ColumnDef::new(CollabQuestions::ContextJson)
                .text()
                .not_null(),
        )
        .col(
            ColumnDef::new(CollabQuestions::Status)
                .string_len(16)
                .not_null(),
        )
        .col(
            ColumnDef::new(CollabQuestions::Version)
                .big_integer()
                .not_null(),
        )
        .col(
            ColumnDef::new(CollabQuestions::ClientQuestionId)
                .string_len(128)
                .not_null(),
        )
        .col(
            ColumnDef::new(CollabQuestions::PayloadHash)
                .string_len(64)
                .not_null(),
        )
        .col(created_at(CollabQuestions::CreatedAt))
        .col(created_at(CollabQuestions::UpdatedAt))
        .index(
            Index::create()
                .name("uq_collab_questions_creator_client_id")
                .col(CollabQuestions::CreatorKind)
                .col(CollabQuestions::CreatorId)
                .col(CollabQuestions::ClientQuestionId)
                .unique(),
        )
        // Target of the answers' same-repository foreign key.
        .index(
            Index::create()
                .name("uq_collab_questions_id_repo")
                .col(CollabQuestions::Id)
                .col(CollabQuestions::RepoId)
                .unique(),
        )
        .foreign_key(
            ForeignKey::create()
                .from(CollabQuestions::Table, CollabQuestions::TaskId)
                .from(CollabQuestions::Table, CollabQuestions::RepoId)
                .to(CollabTasks::Table, CollabTasks::Id)
                .to(CollabTasks::Table, CollabTasks::RepoId)
                .on_delete(ForeignKeyAction::Cascade),
        )
        .to_owned()
}

fn answers_table() -> TableCreateStatement {
    Table::create()
        .table(CollabAnswers::Table)
        .if_not_exists()
        .col(uuid_col(CollabAnswers::Id).not_null().primary_key())
        .col(
            ColumnDef::new(CollabAnswers::RepoId)
                .big_integer()
                .not_null(),
        )
        .col(uuid_col(CollabAnswers::QuestionId).not_null())
        .col(
            ColumnDef::new(CollabAnswers::ActorKind)
                .string_len(16)
                .not_null(),
        )
        .col(
            ColumnDef::new(CollabAnswers::ActorId)
                .string_len(64)
                .not_null(),
        )
        .col(ColumnDef::new(CollabAnswers::Body).text().not_null())
        .col(
            ColumnDef::new(CollabAnswers::OptionId)
                .string_len(64)
                .null(),
        )
        .col(
            ColumnDef::new(CollabAnswers::ContextHash)
                .string_len(64)
                .not_null(),
        )
        .col(
            ColumnDef::new(CollabAnswers::ClientAnswerId)
                .string_len(128)
                .not_null(),
        )
        .col(
            ColumnDef::new(CollabAnswers::PayloadHash)
                .string_len(64)
                .not_null(),
        )
        .col(created_at(CollabAnswers::CreatedAt))
        // At most one answer per question: the database decides a race.
        .index(
            Index::create()
                .name("uq_collab_answers_question")
                .col(CollabAnswers::QuestionId)
                .unique(),
        )
        .foreign_key(
            ForeignKey::create()
                .from(CollabAnswers::Table, CollabAnswers::QuestionId)
                .from(CollabAnswers::Table, CollabAnswers::RepoId)
                .to(CollabQuestions::Table, CollabQuestions::Id)
                .to(CollabQuestions::Table, CollabQuestions::RepoId)
                .on_delete(ForeignKeyAction::Cascade),
        )
        .to_owned()
}

fn work_packages_table() -> TableCreateStatement {
    Table::create()
        .table(CollabWorkPackages::Table)
        .if_not_exists()
        .col(uuid_col(CollabWorkPackages::Id).not_null().primary_key())
        .col(
            ColumnDef::new(CollabWorkPackages::RepoId)
                .big_integer()
                .not_null(),
        )
        .col(uuid_col(CollabWorkPackages::TaskId).not_null())
        .col(
            ColumnDef::new(CollabWorkPackages::Revision)
                .big_integer()
                .not_null(),
        )
        .col(
            ColumnDef::new(CollabWorkPackages::AuthorKind)
                .string_len(16)
                .not_null(),
        )
        .col(
            ColumnDef::new(CollabWorkPackages::AuthorId)
                .string_len(64)
                .not_null(),
        )
        .col(
            ColumnDef::new(CollabWorkPackages::TaskVersion)
                .big_integer()
                .not_null(),
        )
        .col(
            ColumnDef::new(CollabWorkPackages::CommitSha)
                .string_len(64)
                .null(),
        )
        .col(
            ColumnDef::new(CollabWorkPackages::BodyJson)
                .text()
                .not_null(),
        )
        .col(
            ColumnDef::new(CollabWorkPackages::ClientPackageId)
                .string_len(128)
                .not_null(),
        )
        .col(
            ColumnDef::new(CollabWorkPackages::PayloadHash)
                .string_len(64)
                .not_null(),
        )
        .col(created_at(CollabWorkPackages::CreatedAt))
        .index(
            Index::create()
                .name("uq_collab_work_packages_task_revision")
                .col(CollabWorkPackages::TaskId)
                .col(CollabWorkPackages::Revision)
                .unique(),
        )
        .index(
            Index::create()
                .name("uq_collab_work_packages_author_client_id")
                .col(CollabWorkPackages::AuthorKind)
                .col(CollabWorkPackages::AuthorId)
                .col(CollabWorkPackages::ClientPackageId)
                .unique(),
        )
        .foreign_key(
            ForeignKey::create()
                .from(CollabWorkPackages::Table, CollabWorkPackages::TaskId)
                .from(CollabWorkPackages::Table, CollabWorkPackages::RepoId)
                .to(CollabTasks::Table, CollabTasks::Id)
                .to(CollabTasks::Table, CollabTasks::RepoId)
                .on_delete(ForeignKeyAction::Cascade),
        )
        .to_owned()
}

#[derive(Iden)]
enum CollabTakeovers {
    Table,
    Id,
    RepoId,
    TaskId,
    FromSessionId,
    ToSessionId,
    ActorKind,
    ActorId,
    PreviousTaskVersion,
    NewTaskVersion,
    Outcome,
    Reason,
    ClientTakeoverId,
    PayloadHash,
    CreatedAt,
}

#[derive(Iden)]
enum CollabQuestions {
    Table,
    Id,
    RepoId,
    TaskId,
    CreatorKind,
    CreatorId,
    RecipientKind,
    RecipientId,
    Title,
    Body,
    OptionsJson,
    ContextJson,
    Status,
    Version,
    ClientQuestionId,
    PayloadHash,
    CreatedAt,
    UpdatedAt,
}

#[derive(Iden)]
enum CollabAnswers {
    Table,
    Id,
    RepoId,
    QuestionId,
    ActorKind,
    ActorId,
    Body,
    OptionId,
    ContextHash,
    ClientAnswerId,
    PayloadHash,
    CreatedAt,
}

#[derive(Iden)]
enum CollabWorkPackages {
    Table,
    Id,
    RepoId,
    TaskId,
    Revision,
    AuthorKind,
    AuthorId,
    TaskVersion,
    CommitSha,
    BodyJson,
    ClientPackageId,
    PayloadHash,
    CreatedAt,
}

// Referenced tables from the first collaboration migration.
#[derive(Iden)]
enum CollabTasks {
    Table,
    Id,
    RepoId,
}

#[derive(Iden)]
enum AgentSessions {
    Table,
    Id,
    RepoId,
}
