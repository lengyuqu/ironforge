//! Collaboration delivery receipts and durable notices (P1 U04).
//!
//! Design: `ironforge-docs/architecture/agent-collaboration-p1-design-2026-11.md`
//! §2 and §6. Only new tables; earlier collaboration migrations are untouched.
//!
//! * `collab_delivery_receipts` — the target session of a *targeted* message
//!   confirmed it read it. No row means "unknown", not "unread".
//! * `collab_notices` — a per-session reminder that something changed. Notices
//!   are hints: the consumer re-reads the object they point at. The unique key
//!   `(recipient_session_id, kind, source_type, source_id, generation)` makes
//!   emission idempotent in the database.
//!
//! Conventions follow `m20261104_000001`: explicit table names equal to the
//! entity `table_name` (CLAUDE.md pitfall 13) and composite foreign keys so
//! every referenced object belongs to the same repository.

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261104_000005_create_collab_notices"
    }
}

fn uuid_col<T: IntoIden>(name: T) -> ColumnDef {
    let mut col = ColumnDef::new(name);
    col.string_len(36);
    col
}

fn timestamp<T: IntoIden>(name: T) -> ColumnDef {
    let mut col = ColumnDef::new(name);
    col.timestamp_with_time_zone();
    col
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        if !manager.has_table("collab_delivery_receipts").await? {
            manager.create_table(receipts_table()).await?;
        }
        if !manager.has_table("collab_notices").await? {
            manager.create_table(notices_table()).await?;
            manager
                .create_index(
                    Index::create()
                        .if_not_exists()
                        .name("idx_collab_notices_recipient_created")
                        .table(CollabNotices::Table)
                        .col(CollabNotices::RecipientSessionId)
                        .col(CollabNotices::CreatedAt)
                        .col(CollabNotices::Id)
                        .to_owned(),
                )
                .await?;
        }
        Ok(())
    }

    /// Drops both tables (they hold no parents of other tables). `up` is
    /// idempotent so a rollback followed by a re-apply works.
    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(CollabNotices::Table).if_exists().to_owned())
            .await?;
        manager
            .drop_table(
                Table::drop()
                    .table(CollabDeliveryReceipts::Table)
                    .if_exists()
                    .to_owned(),
            )
            .await?;
        Ok(())
    }
}

fn receipts_table() -> TableCreateStatement {
    Table::create()
        .table(CollabDeliveryReceipts::Table)
        .if_not_exists()
        .col(uuid_col(CollabDeliveryReceipts::MessageId).not_null())
        .col(uuid_col(CollabDeliveryReceipts::TargetSessionId).not_null())
        .col(ColumnDef::new(CollabDeliveryReceipts::RepoId).big_integer().not_null())
        .col(uuid_col(CollabDeliveryReceipts::TaskId).not_null())
        .col(timestamp(CollabDeliveryReceipts::ReadAt).not_null())
        .primary_key(
            Index::create()
                .name("pk_collab_delivery_receipts")
                .col(CollabDeliveryReceipts::MessageId)
                .col(CollabDeliveryReceipts::TargetSessionId),
        )
        .foreign_key(
            ForeignKey::create()
                .from(CollabDeliveryReceipts::Table, CollabDeliveryReceipts::MessageId)
                .from(CollabDeliveryReceipts::Table, CollabDeliveryReceipts::TaskId)
                .to(CollabMessages::Table, CollabMessages::Id)
                .to(CollabMessages::Table, CollabMessages::TaskId)
                .on_delete(ForeignKeyAction::Cascade),
        )
        .foreign_key(
            ForeignKey::create()
                .from(CollabDeliveryReceipts::Table, CollabDeliveryReceipts::TaskId)
                .from(CollabDeliveryReceipts::Table, CollabDeliveryReceipts::RepoId)
                .to(CollabTasks::Table, CollabTasks::Id)
                .to(CollabTasks::Table, CollabTasks::RepoId)
                .on_delete(ForeignKeyAction::Cascade),
        )
        .foreign_key(
            ForeignKey::create()
                .from(CollabDeliveryReceipts::Table, CollabDeliveryReceipts::TargetSessionId)
                .from(CollabDeliveryReceipts::Table, CollabDeliveryReceipts::RepoId)
                .to(AgentSessions::Table, AgentSessions::Id)
                .to(AgentSessions::Table, AgentSessions::RepoId)
                .on_delete(ForeignKeyAction::Cascade),
        )
        .to_owned()
}

fn notices_table() -> TableCreateStatement {
    Table::create()
        .table(CollabNotices::Table)
        .if_not_exists()
        .col(uuid_col(CollabNotices::Id).not_null().primary_key())
        .col(ColumnDef::new(CollabNotices::RepoId).big_integer().not_null())
        .col(uuid_col(CollabNotices::TaskId).not_null())
        .col(uuid_col(CollabNotices::RecipientSessionId).not_null())
        .col(ColumnDef::new(CollabNotices::Kind).string_len(32).not_null())
        // `handoff` | `message` | `question` | `takeover` | `task` | `dependency`.
        .col(ColumnDef::new(CollabNotices::SourceType).string_len(32).not_null())
        .col(ColumnDef::new(CollabNotices::SourceId).string_len(64).not_null())
        .col(ColumnDef::new(CollabNotices::Generation).big_integer().not_null())
        // Platform-derived facts only (ids, enumerations); never client text.
        .col(ColumnDef::new(CollabNotices::HintJson).text().not_null())
        .col(timestamp(CollabNotices::CreatedAt).not_null())
        .col(timestamp(CollabNotices::AckAt).null())
        // Set when the condition behind an escalation ended; the notice is then
        // no longer asking for attention.
        .col(timestamp(CollabNotices::ResolvedAt).null())
        .index(
            Index::create()
                .name("uq_collab_notices_dedupe")
                .col(CollabNotices::RecipientSessionId)
                .col(CollabNotices::Kind)
                .col(CollabNotices::SourceType)
                .col(CollabNotices::SourceId)
                .col(CollabNotices::Generation)
                .unique(),
        )
        .foreign_key(
            ForeignKey::create()
                .from(CollabNotices::Table, CollabNotices::TaskId)
                .from(CollabNotices::Table, CollabNotices::RepoId)
                .to(CollabTasks::Table, CollabTasks::Id)
                .to(CollabTasks::Table, CollabTasks::RepoId)
                .on_delete(ForeignKeyAction::Cascade),
        )
        .foreign_key(
            ForeignKey::create()
                .from(CollabNotices::Table, CollabNotices::RecipientSessionId)
                .from(CollabNotices::Table, CollabNotices::RepoId)
                .to(AgentSessions::Table, AgentSessions::Id)
                .to(AgentSessions::Table, AgentSessions::RepoId)
                .on_delete(ForeignKeyAction::Cascade),
        )
        .to_owned()
}

#[derive(Iden)]
enum CollabDeliveryReceipts {
    Table,
    MessageId,
    TargetSessionId,
    RepoId,
    TaskId,
    ReadAt,
}

#[derive(Iden)]
enum CollabNotices {
    Table,
    Id,
    RepoId,
    TaskId,
    RecipientSessionId,
    Kind,
    SourceType,
    SourceId,
    Generation,
    HintJson,
    CreatedAt,
    AckAt,
    ResolvedAt,
}

// Referenced tables from the first collaboration migration.
#[derive(Iden)]
enum CollabTasks {
    Table,
    Id,
    RepoId,
}

#[derive(Iden)]
enum CollabMessages {
    Table,
    Id,
    TaskId,
}

#[derive(Iden)]
enum AgentSessions {
    Table,
    Id,
    RepoId,
}
