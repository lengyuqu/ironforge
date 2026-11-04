//! Cross-repository coordination groups (P2 X02): `collab_coordination_groups`
//! and `collab_coordination_links`.
//!
//! Design: `ironforge-docs/architecture/agent-collaboration-p2-design-2026-11.md`
//! §3–§4. Only new tables; earlier collaboration migrations are not touched.
//!
//! A group belongs to one *anchor* repository. A link points at another (or the
//! same) repository by **repository id** — never by name — and optionally at one
//! of its collaboration tasks and/or a full commit id. Names are resolved when a
//! link is read, so a rename can never leave a stale copy behind.
//!
//! Conventions follow `m20261104_000001/2`: server-generated UUID ids, explicit
//! table names equal to the entity `table_name`, composite foreign keys so that
//! a link's anchor and target task are really in the repository the row claims,
//! and per-object client idempotency keys (`client_*_id` + `payload_hash`)
//! instead of a global request table.
//!
//! `created_by_user_id` / `*_actor_id` are plain references (no foreign key): a
//! hard account deletion must not silently remove other people's groups.

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261104_000006_create_collab_groups"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        if !manager.has_table("collab_coordination_groups").await? {
            manager.create_table(groups_table()).await?;
        }
        // Links reference groups, so they come second.
        if !manager.has_table("collab_coordination_links").await? {
            manager.create_table(links_table()).await?;
            manager
                .create_index(
                    Index::create()
                        .if_not_exists()
                        .name("idx_collab_links_group_created")
                        .table(CollabCoordinationLinks::Table)
                        .col(CollabCoordinationLinks::GroupId)
                        .col(CollabCoordinationLinks::CreatedAt)
                        .col(CollabCoordinationLinks::Id)
                        .to_owned(),
                )
                .await?;
            manager
                .create_index(
                    Index::create()
                        .if_not_exists()
                        .name("idx_collab_links_target_repo")
                        .table(CollabCoordinationLinks::Table)
                        .col(CollabCoordinationLinks::TargetRepoId)
                        .to_owned(),
                )
                .await?;
        }
        Ok(())
    }

    /// Drops both tables, links first, so that rolling back the first
    /// collaboration migration afterwards leaves no table whose foreign keys
    /// point at a table that no longer exists. Destroys their data; `up` is
    /// idempotent.
    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(
                Table::drop()
                    .table(CollabCoordinationLinks::Table)
                    .if_exists()
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(
                Table::drop()
                    .table(CollabCoordinationGroups::Table)
                    .if_exists()
                    .to_owned(),
            )
            .await
    }
}

fn groups_table() -> TableCreateStatement {
    Table::create()
        .table(CollabCoordinationGroups::Table)
        .if_not_exists()
        .col(
            ColumnDef::new(CollabCoordinationGroups::Id)
                .string_len(36)
                .not_null()
                .primary_key(),
        )
        .col(ColumnDef::new(CollabCoordinationGroups::AnchorRepoId).big_integer().not_null())
        // Readable by everyone who can read the anchor repository's collaboration
        // data; the server never writes target-repository content into them.
        .col(ColumnDef::new(CollabCoordinationGroups::Title).string_len(800).not_null())
        .col(ColumnDef::new(CollabCoordinationGroups::Description).text().not_null())
        .col(ColumnDef::new(CollabCoordinationGroups::CreatedByUserId).big_integer().not_null())
        // `session` or `user`: the door the write came through, not an identity proof.
        .col(ColumnDef::new(CollabCoordinationGroups::ActorKind).string_len(16).not_null())
        .col(ColumnDef::new(CollabCoordinationGroups::ActorId).string_len(64).not_null())
        // Compare-and-swap counter, bumped by every link change.
        .col(ColumnDef::new(CollabCoordinationGroups::Version).big_integer().not_null())
        .col(ColumnDef::new(CollabCoordinationGroups::ClientGroupId).string_len(128).not_null())
        .col(ColumnDef::new(CollabCoordinationGroups::PayloadHash).string_len(64).not_null())
        .col(
            ColumnDef::new(CollabCoordinationGroups::CreatedAt)
                .timestamp_with_time_zone()
                .not_null(),
        )
        .col(
            ColumnDef::new(CollabCoordinationGroups::UpdatedAt)
                .timestamp_with_time_zone()
                .not_null(),
        )
        .index(
            Index::create()
                .name("uq_collab_groups_actor_client_id")
                .col(CollabCoordinationGroups::ActorKind)
                .col(CollabCoordinationGroups::ActorId)
                .col(CollabCoordinationGroups::ClientGroupId)
                .unique(),
        )
        // Target of the links' same-anchor foreign key.
        .index(
            Index::create()
                .name("uq_collab_groups_id_anchor")
                .col(CollabCoordinationGroups::Id)
                .col(CollabCoordinationGroups::AnchorRepoId)
                .unique(),
        )
        .foreign_key(
            ForeignKey::create()
                .from(CollabCoordinationGroups::Table, CollabCoordinationGroups::AnchorRepoId)
                .to(Repositories::Table, Repositories::Id)
                .on_delete(ForeignKeyAction::Cascade),
        )
        .to_owned()
}

fn links_table() -> TableCreateStatement {
    use CollabCoordinationLinks as L;
    Table::create()
        .table(L::Table)
        .if_not_exists()
        .col(ColumnDef::new(L::Id).string_len(36).not_null().primary_key())
        .col(ColumnDef::new(L::GroupId).string_len(36).not_null())
        .col(ColumnDef::new(L::AnchorRepoId).big_integer().not_null())
        .col(ColumnDef::new(L::TargetRepoId).big_integer().not_null())
        .col(ColumnDef::new(L::TargetTaskId).string_len(36).null())
        .col(ColumnDef::new(L::CommitSha).string_len(64).null())
        // SHA-256 over (group, repo, task, commit) while the link is active,
        // NULL once removed. A unique index over it makes "one active link per
        // target" a database rule; NULLs never collide, so removed rows pile up
        // harmlessly (nullable columns themselves cannot be part of such a key).
        .col(ColumnDef::new(L::ActiveKey).string_len(64).null())
        .col(ColumnDef::new(L::CreatedByUserId).big_integer().not_null())
        .col(ColumnDef::new(L::ActorKind).string_len(16).not_null())
        .col(ColumnDef::new(L::ActorId).string_len(64).not_null())
        .col(ColumnDef::new(L::ClientLinkId).string_len(128).not_null())
        .col(ColumnDef::new(L::PayloadHash).string_len(64).not_null())
        .col(ColumnDef::new(L::CreatedAt).timestamp_with_time_zone().not_null())
        .col(ColumnDef::new(L::RemovedAt).timestamp_with_time_zone().null())
        .col(ColumnDef::new(L::RemovedByUserId).big_integer().null())
        .col(ColumnDef::new(L::RemovedActorKind).string_len(16).null())
        .col(ColumnDef::new(L::RemovedActorId).string_len(64).null())
        .col(ColumnDef::new(L::RemoveClientId).string_len(128).null())
        .col(ColumnDef::new(L::RemovePayloadHash).string_len(64).null())
        .index(
            Index::create()
                .name("uq_collab_links_actor_client_id")
                .col(L::ActorKind)
                .col(L::ActorId)
                .col(L::ClientLinkId)
                .unique(),
        )
        .index(
            Index::create()
                .name("uq_collab_links_active_key")
                .col(L::ActiveKey)
                .unique(),
        )
        .index(
            Index::create()
                .name("uq_collab_links_remove_client_id")
                .col(L::RemovedActorKind)
                .col(L::RemovedActorId)
                .col(L::RemoveClientId)
                .unique(),
        )
        .foreign_key(
            ForeignKey::create()
                .from(L::Table, L::GroupId)
                .from(L::Table, L::AnchorRepoId)
                .to(CollabCoordinationGroups::Table, CollabCoordinationGroups::Id)
                .to(CollabCoordinationGroups::Table, CollabCoordinationGroups::AnchorRepoId)
                .on_delete(ForeignKeyAction::Cascade),
        )
        .foreign_key(
            ForeignKey::create()
                .from(L::Table, L::TargetRepoId)
                .to(Repositories::Table, Repositories::Id)
                .on_delete(ForeignKeyAction::Cascade),
        )
        // The task must belong to the target repository; skipped when NULL.
        .foreign_key(
            ForeignKey::create()
                .from(L::Table, L::TargetTaskId)
                .from(L::Table, L::TargetRepoId)
                .to(CollabTasks::Table, CollabTasks::Id)
                .to(CollabTasks::Table, CollabTasks::RepoId)
                .on_delete(ForeignKeyAction::Cascade),
        )
        .to_owned()
}

#[derive(Iden)]
enum CollabCoordinationGroups {
    Table,
    Id,
    AnchorRepoId,
    Title,
    Description,
    CreatedByUserId,
    ActorKind,
    ActorId,
    Version,
    ClientGroupId,
    PayloadHash,
    CreatedAt,
    UpdatedAt,
}

#[derive(Iden)]
enum CollabCoordinationLinks {
    Table,
    Id,
    GroupId,
    AnchorRepoId,
    TargetRepoId,
    TargetTaskId,
    CommitSha,
    ActiveKey,
    CreatedByUserId,
    ActorKind,
    ActorId,
    ClientLinkId,
    PayloadHash,
    CreatedAt,
    RemovedAt,
    RemovedByUserId,
    RemovedActorKind,
    RemovedActorId,
    RemoveClientId,
    RemovePayloadHash,
}

#[derive(Iden)]
enum Repositories {
    Table,
    Id,
}

// Referenced table from the first collaboration migration.
#[derive(Iden)]
enum CollabTasks {
    Table,
    Id,
    RepoId,
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm_migration::sea_query::{MysqlQueryBuilder, PostgresQueryBuilder, SqliteQueryBuilder};

    /// CLAUDE.md pitfall #13: `#[derive(Iden)]` yields snake_case of the enum
    /// name, so the explicit expectation here keeps entity and DDL in step.
    #[test]
    fn ddl_uses_the_entity_table_names_on_all_backends() {
        for (stmt, expected) in [
            (groups_table(), "collab_coordination_groups"),
            (links_table(), "collab_coordination_links"),
        ] {
            for sql in [
                stmt.to_string(SqliteQueryBuilder),
                stmt.to_string(PostgresQueryBuilder),
                stmt.to_string(MysqlQueryBuilder),
            ] {
                assert!(sql.contains(expected), "{expected} missing in: {sql}");
            }
        }
    }
}
