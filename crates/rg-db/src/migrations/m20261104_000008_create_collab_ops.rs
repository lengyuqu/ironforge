//! Collaboration operability tables (O06 task templates, O07 capability
//! declarations, O08 budgets and usage).
//!
//! Design: `ironforge-docs/architecture/agent-collaboration-operability-requirements-2026-11.md` §6.
//! Only new tables; no existing table is altered. Conventions follow
//! `m20261104_000002`: server-generated UUID ids (`VARCHAR(36)`), explicit
//! table names equal to the entity `table_name`, composite foreign keys so every
//! referenced object belongs to the same repository, and per-object client
//! idempotency keys instead of a global request table.
//!
//! Amounts (`amount`, `alert_at`) are stored as normalized decimal *text* and
//! compared in integer micro-units by the service; the database never
//! aggregates them, so no float rounding and no cross-unit arithmetic can
//! happen in SQL.

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261104_000008_create_collab_ops"
    }
}

fn uuid_col<T: IntoIden>(name: T) -> ColumnDef {
    let mut col = ColumnDef::new(name);
    col.string_len(36);
    col
}

fn ts<T: IntoIden>(name: T) -> ColumnDef {
    let mut col = ColumnDef::new(name);
    col.timestamp_with_time_zone().not_null();
    col
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        if !manager.has_table("collab_task_templates").await? {
            manager.create_table(templates_table()).await?;
        }
        // Versions reference templates, links reference both.
        if !manager.has_table("collab_task_template_versions").await? {
            manager.create_table(template_versions_table()).await?;
        }
        if !manager.has_table("collab_task_template_links").await? {
            manager.create_table(template_links_table()).await?;
        }
        if !manager.has_table("collab_session_declarations").await? {
            manager.create_table(declarations_table()).await?;
        }
        if !manager.has_table("collab_task_budgets").await? {
            manager.create_table(budgets_table()).await?;
        }
        if !manager.has_table("collab_task_usage").await? {
            manager.create_table(usage_table()).await?;
            manager
                .create_index(
                    Index::create()
                        .if_not_exists()
                        .name("idx_collab_task_usage_task_unit_source")
                        .table(CollabTaskUsage::Table)
                        .col(CollabTaskUsage::TaskId)
                        .col(CollabTaskUsage::Unit)
                        .col(CollabTaskUsage::Source)
                        .to_owned(),
                )
                .await?;
        }
        Ok(())
    }

    /// Drops the six tables, children first, so that rolling back the first
    /// collaboration migration afterwards does not leave tables whose foreign
    /// keys point at tables that no longer exist. It destroys the data in them;
    /// `up` is idempotent.
    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        for table in [
            Table::drop().table(CollabTaskTemplateLinks::Table).if_exists().to_owned(),
            Table::drop().table(CollabTaskTemplateVersions::Table).if_exists().to_owned(),
            Table::drop().table(CollabTaskTemplates::Table).if_exists().to_owned(),
            Table::drop().table(CollabSessionDeclarations::Table).if_exists().to_owned(),
            Table::drop().table(CollabTaskUsage::Table).if_exists().to_owned(),
            Table::drop().table(CollabTaskBudgets::Table).if_exists().to_owned(),
        ] {
            manager.drop_table(table).await?;
        }
        Ok(())
    }
}

fn templates_table() -> TableCreateStatement {
    Table::create()
        .table(CollabTaskTemplates::Table)
        .if_not_exists()
        .col(uuid_col(CollabTaskTemplates::Id).not_null().primary_key())
        .col(ColumnDef::new(CollabTaskTemplates::RepoId).big_integer().not_null())
        .col(ColumnDef::new(CollabTaskTemplates::Name).string_len(100).not_null())
        // Newest version number; the conditional UPDATE on it arbitrates
        // concurrent "new version" requests.
        .col(ColumnDef::new(CollabTaskTemplates::LatestVersion).big_integer().not_null())
        .col(ColumnDef::new(CollabTaskTemplates::CreatedByUserId).big_integer().not_null())
        .col(ColumnDef::new(CollabTaskTemplates::ClientTemplateId).string_len(128).not_null())
        .col(ColumnDef::new(CollabTaskTemplates::PayloadHash).string_len(64).not_null())
        .col(ts(CollabTaskTemplates::CreatedAt))
        .col(ts(CollabTaskTemplates::UpdatedAt))
        .index(
            Index::create()
                .name("uq_collab_task_templates_repo_name")
                .col(CollabTaskTemplates::RepoId)
                .col(CollabTaskTemplates::Name)
                .unique(),
        )
        .index(
            Index::create()
                .name("uq_collab_task_templates_creator_client_id")
                .col(CollabTaskTemplates::CreatedByUserId)
                .col(CollabTaskTemplates::ClientTemplateId)
                .unique(),
        )
        // Target of the versions' same-repository foreign key.
        .index(
            Index::create()
                .name("uq_collab_task_templates_id_repo")
                .col(CollabTaskTemplates::Id)
                .col(CollabTaskTemplates::RepoId)
                .unique(),
        )
        .foreign_key(
            ForeignKey::create()
                .from(CollabTaskTemplates::Table, CollabTaskTemplates::RepoId)
                .to(Repositories::Table, Repositories::Id)
                .on_delete(ForeignKeyAction::Cascade),
        )
        .to_owned()
}

fn template_versions_table() -> TableCreateStatement {
    Table::create()
        .table(CollabTaskTemplateVersions::Table)
        .if_not_exists()
        .col(uuid_col(CollabTaskTemplateVersions::Id).not_null().primary_key())
        .col(ColumnDef::new(CollabTaskTemplateVersions::RepoId).big_integer().not_null())
        .col(uuid_col(CollabTaskTemplateVersions::TemplateId).not_null())
        .col(ColumnDef::new(CollabTaskTemplateVersions::Version).big_integer().not_null())
        .col(ColumnDef::new(CollabTaskTemplateVersions::TitlePattern).string_len(800).not_null())
        .col(ColumnDef::new(CollabTaskTemplateVersions::Goal).text().not_null())
        .col(ColumnDef::new(CollabTaskTemplateVersions::DeliverablesJson).text().not_null())
        .col(ColumnDef::new(CollabTaskTemplateVersions::ChecklistJson).text().not_null())
        .col(ColumnDef::new(CollabTaskTemplateVersions::CreatedByUserId).big_integer().not_null())
        .col(ColumnDef::new(CollabTaskTemplateVersions::ClientVersionId).string_len(128).not_null())
        .col(ColumnDef::new(CollabTaskTemplateVersions::PayloadHash).string_len(64).not_null())
        .col(ts(CollabTaskTemplateVersions::CreatedAt))
        .index(
            Index::create()
                .name("uq_collab_task_template_versions_template_version")
                .col(CollabTaskTemplateVersions::TemplateId)
                .col(CollabTaskTemplateVersions::Version)
                .unique(),
        )
        .index(
            Index::create()
                .name("uq_collab_task_template_versions_creator_client_id")
                .col(CollabTaskTemplateVersions::CreatedByUserId)
                .col(CollabTaskTemplateVersions::ClientVersionId)
                .unique(),
        )
        .foreign_key(
            ForeignKey::create()
                .from(CollabTaskTemplateVersions::Table, CollabTaskTemplateVersions::TemplateId)
                .from(CollabTaskTemplateVersions::Table, CollabTaskTemplateVersions::RepoId)
                .to(CollabTaskTemplates::Table, CollabTaskTemplates::Id)
                .to(CollabTaskTemplates::Table, CollabTaskTemplates::RepoId)
                .on_delete(ForeignKeyAction::Cascade),
        )
        .to_owned()
}

fn template_links_table() -> TableCreateStatement {
    Table::create()
        .table(CollabTaskTemplateLinks::Table)
        .if_not_exists()
        // One link per task: the task is pinned to exactly one template version.
        .col(uuid_col(CollabTaskTemplateLinks::TaskId).not_null().primary_key())
        .col(ColumnDef::new(CollabTaskTemplateLinks::RepoId).big_integer().not_null())
        .col(uuid_col(CollabTaskTemplateLinks::TemplateId).not_null())
        .col(ColumnDef::new(CollabTaskTemplateLinks::TemplateVersion).big_integer().not_null())
        // Copy of what the task was created from; template edits never touch it.
        .col(ColumnDef::new(CollabTaskTemplateLinks::SnapshotJson).text().not_null())
        .col(ColumnDef::new(CollabTaskTemplateLinks::RulesCommit).string_len(64).null())
        .col(ColumnDef::new(CollabTaskTemplateLinks::CreatorKind).string_len(16).not_null())
        .col(ColumnDef::new(CollabTaskTemplateLinks::CreatorId).string_len(64).not_null())
        .col(ColumnDef::new(CollabTaskTemplateLinks::ClientTaskId).string_len(128).not_null())
        .col(ColumnDef::new(CollabTaskTemplateLinks::PayloadHash).string_len(64).not_null())
        .col(ts(CollabTaskTemplateLinks::CreatedAt))
        .index(
            Index::create()
                .name("uq_collab_task_template_links_creator_client_id")
                .col(CollabTaskTemplateLinks::CreatorKind)
                .col(CollabTaskTemplateLinks::CreatorId)
                .col(CollabTaskTemplateLinks::ClientTaskId)
                .unique(),
        )
        .foreign_key(
            ForeignKey::create()
                .from(CollabTaskTemplateLinks::Table, CollabTaskTemplateLinks::TaskId)
                .from(CollabTaskTemplateLinks::Table, CollabTaskTemplateLinks::RepoId)
                .to(CollabTasks::Table, CollabTasks::Id)
                .to(CollabTasks::Table, CollabTasks::RepoId)
                .on_delete(ForeignKeyAction::Cascade),
        )
        .foreign_key(
            ForeignKey::create()
                .from(CollabTaskTemplateLinks::Table, CollabTaskTemplateLinks::TemplateId)
                .from(CollabTaskTemplateLinks::Table, CollabTaskTemplateLinks::TemplateVersion)
                .to(CollabTaskTemplateVersions::Table, CollabTaskTemplateVersions::TemplateId)
                .to(CollabTaskTemplateVersions::Table, CollabTaskTemplateVersions::Version)
                .on_delete(ForeignKeyAction::Cascade),
        )
        .to_owned()
}

fn declarations_table() -> TableCreateStatement {
    Table::create()
        .table(CollabSessionDeclarations::Table)
        .if_not_exists()
        // One current declaration per session, replaced under `version`.
        .col(uuid_col(CollabSessionDeclarations::SessionId).not_null().primary_key())
        .col(ColumnDef::new(CollabSessionDeclarations::RepoId).big_integer().not_null())
        .col(ColumnDef::new(CollabSessionDeclarations::Version).big_integer().not_null())
        .col(ColumnDef::new(CollabSessionDeclarations::ToolsJson).text().not_null())
        // `yes` | `no` | `unknown`
        .col(ColumnDef::new(CollabSessionDeclarations::CanRunTests).string_len(16).not_null())
        // `unrestricted` | `restricted` | `offline` | `unknown`
        .col(ColumnDef::new(CollabSessionDeclarations::Network).string_len(16).not_null())
        .col(ColumnDef::new(CollabSessionDeclarations::NetworkNotes).text().not_null())
        .col(ColumnDef::new(CollabSessionDeclarations::Notes).text().not_null())
        .col(ColumnDef::new(CollabSessionDeclarations::PayloadHash).string_len(64).not_null())
        .col(ts(CollabSessionDeclarations::DeclaredAt))
        .foreign_key(
            ForeignKey::create()
                .from(CollabSessionDeclarations::Table, CollabSessionDeclarations::SessionId)
                .from(CollabSessionDeclarations::Table, CollabSessionDeclarations::RepoId)
                .to(AgentSessions::Table, AgentSessions::Id)
                .to(AgentSessions::Table, AgentSessions::RepoId)
                .on_delete(ForeignKeyAction::Cascade),
        )
        .to_owned()
}

fn budgets_table() -> TableCreateStatement {
    Table::create()
        .table(CollabTaskBudgets::Table)
        .if_not_exists()
        .col(uuid_col(CollabTaskBudgets::Id).not_null().primary_key())
        .col(ColumnDef::new(CollabTaskBudgets::RepoId).big_integer().not_null())
        .col(uuid_col(CollabTaskBudgets::TaskId).not_null())
        .col(ColumnDef::new(CollabTaskBudgets::Unit).string_len(32).not_null())
        .col(ColumnDef::new(CollabTaskBudgets::Source).string_len(64).not_null())
        .col(ColumnDef::new(CollabTaskBudgets::Amount).string_len(32).not_null())
        .col(ColumnDef::new(CollabTaskBudgets::AlertAt).string_len(32).null())
        .col(ColumnDef::new(CollabTaskBudgets::SamplingPeriod).string_len(64).not_null())
        .col(ColumnDef::new(CollabTaskBudgets::Version).big_integer().not_null())
        .col(ColumnDef::new(CollabTaskBudgets::SetByKind).string_len(16).not_null())
        .col(ColumnDef::new(CollabTaskBudgets::SetById).string_len(64).not_null())
        .col(ColumnDef::new(CollabTaskBudgets::PayloadHash).string_len(64).not_null())
        .col(ts(CollabTaskBudgets::CreatedAt))
        .col(ts(CollabTaskBudgets::UpdatedAt))
        // One budget per (task, unit, source): units and sources never merge.
        .index(
            Index::create()
                .name("uq_collab_task_budgets_task_unit_source")
                .col(CollabTaskBudgets::TaskId)
                .col(CollabTaskBudgets::Unit)
                .col(CollabTaskBudgets::Source)
                .unique(),
        )
        .foreign_key(
            ForeignKey::create()
                .from(CollabTaskBudgets::Table, CollabTaskBudgets::TaskId)
                .from(CollabTaskBudgets::Table, CollabTaskBudgets::RepoId)
                .to(CollabTasks::Table, CollabTasks::Id)
                .to(CollabTasks::Table, CollabTasks::RepoId)
                .on_delete(ForeignKeyAction::Cascade),
        )
        .to_owned()
}

fn usage_table() -> TableCreateStatement {
    Table::create()
        .table(CollabTaskUsage::Table)
        .if_not_exists()
        .col(uuid_col(CollabTaskUsage::Id).not_null().primary_key())
        .col(ColumnDef::new(CollabTaskUsage::RepoId).big_integer().not_null())
        .col(uuid_col(CollabTaskUsage::TaskId).not_null())
        .col(ColumnDef::new(CollabTaskUsage::Unit).string_len(32).not_null())
        .col(ColumnDef::new(CollabTaskUsage::Source).string_len(64).not_null())
        .col(ColumnDef::new(CollabTaskUsage::Amount).string_len(32).not_null())
        .col(ts(CollabTaskUsage::PeriodStart))
        .col(ts(CollabTaskUsage::PeriodEnd))
        .col(ColumnDef::new(CollabTaskUsage::ReportedByKind).string_len(16).not_null())
        .col(ColumnDef::new(CollabTaskUsage::ReportedById).string_len(64).not_null())
        .col(ColumnDef::new(CollabTaskUsage::ClientUsageId).string_len(128).not_null())
        .col(ColumnDef::new(CollabTaskUsage::PayloadHash).string_len(64).not_null())
        .col(ts(CollabTaskUsage::CreatedAt))
        .index(
            Index::create()
                .name("uq_collab_task_usage_reporter_client_id")
                .col(CollabTaskUsage::ReportedByKind)
                .col(CollabTaskUsage::ReportedById)
                .col(CollabTaskUsage::ClientUsageId)
                .unique(),
        )
        .foreign_key(
            ForeignKey::create()
                .from(CollabTaskUsage::Table, CollabTaskUsage::TaskId)
                .from(CollabTaskUsage::Table, CollabTaskUsage::RepoId)
                .to(CollabTasks::Table, CollabTasks::Id)
                .to(CollabTasks::Table, CollabTasks::RepoId)
                .on_delete(ForeignKeyAction::Cascade),
        )
        .to_owned()
}

#[derive(Iden)]
enum CollabTaskTemplates {
    Table,
    Id,
    RepoId,
    Name,
    LatestVersion,
    CreatedByUserId,
    ClientTemplateId,
    PayloadHash,
    CreatedAt,
    UpdatedAt,
}

#[derive(Iden)]
enum CollabTaskTemplateVersions {
    Table,
    Id,
    RepoId,
    TemplateId,
    Version,
    TitlePattern,
    Goal,
    DeliverablesJson,
    ChecklistJson,
    CreatedByUserId,
    ClientVersionId,
    PayloadHash,
    CreatedAt,
}

#[derive(Iden)]
enum CollabTaskTemplateLinks {
    Table,
    TaskId,
    RepoId,
    TemplateId,
    TemplateVersion,
    SnapshotJson,
    RulesCommit,
    CreatorKind,
    CreatorId,
    ClientTaskId,
    PayloadHash,
    CreatedAt,
}

#[derive(Iden)]
enum CollabSessionDeclarations {
    Table,
    SessionId,
    RepoId,
    Version,
    ToolsJson,
    CanRunTests,
    Network,
    NetworkNotes,
    Notes,
    PayloadHash,
    DeclaredAt,
}

#[derive(Iden)]
enum CollabTaskBudgets {
    Table,
    Id,
    RepoId,
    TaskId,
    Unit,
    Source,
    Amount,
    AlertAt,
    SamplingPeriod,
    Version,
    SetByKind,
    SetById,
    PayloadHash,
    CreatedAt,
    UpdatedAt,
}

#[derive(Iden)]
enum CollabTaskUsage {
    Table,
    Id,
    RepoId,
    TaskId,
    Unit,
    Source,
    Amount,
    PeriodStart,
    PeriodEnd,
    ReportedByKind,
    ReportedById,
    ClientUsageId,
    PayloadHash,
    CreatedAt,
}

// Referenced tables from earlier migrations.
#[derive(Iden)]
enum Repositories {
    Table,
    Id,
}

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
