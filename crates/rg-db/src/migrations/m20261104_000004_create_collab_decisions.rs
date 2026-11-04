//! Collaboration P1 decision traceability: `collab_decisions` and
//! `collab_decision_changes`.
//!
//! Design: `ironforge-docs/architecture/agent-collaboration-p1-design-2026-11.md` §2, §7.
//! Only new tables; earlier collaboration migrations are not touched.
//!
//! `seq` numbers a task's decisions 1, 2, 3, ... (unique per task). It is the
//! order that keeps supersession acyclic, because timestamps can tie (MySQL
//! `timestamp` has one-second resolution) and ids are random.
//!
//! A decision row is immutable. Superseding or revoking it never edits it: a
//! change row is appended, and `uq_collab_decision_changes_decision` lets the
//! database pick the single winner when two writers race. Composite foreign
//! keys keep every referenced object in the same task and repository.
//!
//! `actor_kind` records the door a write came through (`session` or `user`);
//! it is not an identity proof.

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261104_000004_create_collab_decisions"
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
        if !manager.has_table("collab_decisions").await? {
            manager.create_table(decisions_table()).await?;
        }
        // Change rows reference decisions, so they come after them.
        if !manager.has_table("collab_decision_changes").await? {
            manager.create_table(changes_table()).await?;
            manager
                .create_index(
                    Index::create()
                        .if_not_exists()
                        .name("idx_collab_decision_changes_replacement")
                        .table(CollabDecisionChanges::Table)
                        .col(CollabDecisionChanges::ReplacementDecisionId)
                        .to_owned(),
                )
                .await?;
        }
        Ok(())
    }

    /// Drops children first so rolling back earlier collaboration migrations
    /// afterwards does not leave foreign keys pointing at missing tables. It
    /// destroys the data in both tables; `up` is idempotent.
    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        for table in [
            Table::drop()
                .table(CollabDecisionChanges::Table)
                .if_exists()
                .to_owned(),
            Table::drop()
                .table(CollabDecisions::Table)
                .if_exists()
                .to_owned(),
        ] {
            manager.drop_table(table).await?;
        }
        Ok(())
    }
}

fn decisions_table() -> TableCreateStatement {
    Table::create()
        .table(CollabDecisions::Table)
        .if_not_exists()
        .col(uuid_col(CollabDecisions::Id).not_null().primary_key())
        .col(
            ColumnDef::new(CollabDecisions::RepoId)
                .big_integer()
                .not_null(),
        )
        .col(uuid_col(CollabDecisions::TaskId).not_null())
        .col(
            ColumnDef::new(CollabDecisions::Seq)
                .big_integer()
                .not_null(),
        )
        .col(uuid_col(CollabDecisions::QuestionId).null())
        .col(uuid_col(CollabDecisions::AnswerId).null())
        .col(
            ColumnDef::new(CollabDecisions::Title)
                .string_len(800)
                .not_null(),
        )
        .col(ColumnDef::new(CollabDecisions::Rationale).text().not_null())
        // JSON `{task_version, base_sha?, head_sha?}` captured at decision time.
        .col(
            ColumnDef::new(CollabDecisions::ContextJson)
                .text()
                .not_null(),
        )
        .col(
            ColumnDef::new(CollabDecisions::ActorKind)
                .string_len(16)
                .not_null(),
        )
        .col(
            ColumnDef::new(CollabDecisions::ActorId)
                .string_len(64)
                .not_null(),
        )
        .col(
            ColumnDef::new(CollabDecisions::ClientDecisionId)
                .string_len(128)
                .not_null(),
        )
        .col(
            ColumnDef::new(CollabDecisions::PayloadHash)
                .string_len(64)
                .not_null(),
        )
        .col(created_at(CollabDecisions::CreatedAt))
        .index(
            Index::create()
                .name("uq_collab_decisions_actor_client_id")
                .col(CollabDecisions::ActorKind)
                .col(CollabDecisions::ActorId)
                .col(CollabDecisions::ClientDecisionId)
                .unique(),
        )
        .index(
            Index::create()
                .name("uq_collab_decisions_task_seq")
                .col(CollabDecisions::TaskId)
                .col(CollabDecisions::Seq)
                .unique(),
        )
        // Target of the changes' same-task, same-repository foreign keys.
        .index(
            Index::create()
                .name("uq_collab_decisions_id_task_repo")
                .col(CollabDecisions::Id)
                .col(CollabDecisions::TaskId)
                .col(CollabDecisions::RepoId)
                .unique(),
        )
        .foreign_key(
            ForeignKey::create()
                .from(CollabDecisions::Table, CollabDecisions::TaskId)
                .from(CollabDecisions::Table, CollabDecisions::RepoId)
                .to(CollabTasks::Table, CollabTasks::Id)
                .to(CollabTasks::Table, CollabTasks::RepoId)
                .on_delete(ForeignKeyAction::Cascade),
        )
        .foreign_key(
            ForeignKey::create()
                .from(CollabDecisions::Table, CollabDecisions::QuestionId)
                .from(CollabDecisions::Table, CollabDecisions::RepoId)
                .to(CollabQuestions::Table, CollabQuestions::Id)
                .to(CollabQuestions::Table, CollabQuestions::RepoId)
                .on_delete(ForeignKeyAction::Cascade),
        )
        // `collab_answers` only has `id` as a referencable key; that the answer
        // belongs to `question_id` is checked by the service (answers are
        // immutable, so the check cannot be invalidated afterwards).
        .foreign_key(
            ForeignKey::create()
                .from(CollabDecisions::Table, CollabDecisions::AnswerId)
                .to(CollabAnswers::Table, CollabAnswers::Id)
                .on_delete(ForeignKeyAction::Cascade),
        )
        .to_owned()
}

fn changes_table() -> TableCreateStatement {
    Table::create()
        .table(CollabDecisionChanges::Table)
        .if_not_exists()
        .col(uuid_col(CollabDecisionChanges::Id).not_null().primary_key())
        .col(
            ColumnDef::new(CollabDecisionChanges::RepoId)
                .big_integer()
                .not_null(),
        )
        .col(uuid_col(CollabDecisionChanges::TaskId).not_null())
        .col(uuid_col(CollabDecisionChanges::DecisionId).not_null())
        // `superseded` or `revoked`.
        .col(
            ColumnDef::new(CollabDecisionChanges::Kind)
                .string_len(16)
                .not_null(),
        )
        .col(uuid_col(CollabDecisionChanges::ReplacementDecisionId).null())
        .col(
            ColumnDef::new(CollabDecisionChanges::Reason)
                .text()
                .not_null(),
        )
        .col(
            ColumnDef::new(CollabDecisionChanges::ActorKind)
                .string_len(16)
                .not_null(),
        )
        .col(
            ColumnDef::new(CollabDecisionChanges::ActorId)
                .string_len(64)
                .not_null(),
        )
        .col(
            ColumnDef::new(CollabDecisionChanges::ClientChangeId)
                .string_len(128)
                .not_null(),
        )
        .col(
            ColumnDef::new(CollabDecisionChanges::PayloadHash)
                .string_len(64)
                .not_null(),
        )
        .col(created_at(CollabDecisionChanges::CreatedAt))
        // At most one terminal event per decision: the database decides a race.
        .index(
            Index::create()
                .name("uq_collab_decision_changes_decision")
                .col(CollabDecisionChanges::DecisionId)
                .unique(),
        )
        .index(
            Index::create()
                .name("uq_collab_decision_changes_actor_client_id")
                .col(CollabDecisionChanges::ActorKind)
                .col(CollabDecisionChanges::ActorId)
                .col(CollabDecisionChanges::ClientChangeId)
                .unique(),
        )
        .foreign_key(
            ForeignKey::create()
                .from(
                    CollabDecisionChanges::Table,
                    CollabDecisionChanges::DecisionId,
                )
                .from(CollabDecisionChanges::Table, CollabDecisionChanges::TaskId)
                .from(CollabDecisionChanges::Table, CollabDecisionChanges::RepoId)
                .to(CollabDecisions::Table, CollabDecisions::Id)
                .to(CollabDecisions::Table, CollabDecisions::TaskId)
                .to(CollabDecisions::Table, CollabDecisions::RepoId)
                .on_delete(ForeignKeyAction::Cascade),
        )
        // A replacement must be a decision of the same task (and repository).
        .foreign_key(
            ForeignKey::create()
                .from(
                    CollabDecisionChanges::Table,
                    CollabDecisionChanges::ReplacementDecisionId,
                )
                .from(CollabDecisionChanges::Table, CollabDecisionChanges::TaskId)
                .from(CollabDecisionChanges::Table, CollabDecisionChanges::RepoId)
                .to(CollabDecisions::Table, CollabDecisions::Id)
                .to(CollabDecisions::Table, CollabDecisions::TaskId)
                .to(CollabDecisions::Table, CollabDecisions::RepoId)
                .on_delete(ForeignKeyAction::Cascade),
        )
        .to_owned()
}

#[derive(Iden)]
enum CollabDecisions {
    Table,
    Id,
    RepoId,
    TaskId,
    Seq,
    QuestionId,
    AnswerId,
    Title,
    Rationale,
    ContextJson,
    ActorKind,
    ActorId,
    ClientDecisionId,
    PayloadHash,
    CreatedAt,
}

#[derive(Iden)]
enum CollabDecisionChanges {
    Table,
    Id,
    RepoId,
    TaskId,
    DecisionId,
    Kind,
    ReplacementDecisionId,
    Reason,
    ActorKind,
    ActorId,
    ClientChangeId,
    PayloadHash,
    CreatedAt,
}

// Referenced tables from earlier collaboration migrations.
#[derive(Iden)]
enum CollabTasks {
    Table,
    Id,
    RepoId,
}

#[derive(Iden)]
enum CollabQuestions {
    Table,
    Id,
    RepoId,
}

#[derive(Iden)]
enum CollabAnswers {
    Table,
    Id,
}
