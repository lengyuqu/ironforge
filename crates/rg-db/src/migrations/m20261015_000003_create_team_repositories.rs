//! Create `team_repositories`: which repositories an organization team may use.
//!
//! Before this table, an org team with `permission = 'write'` granted write
//! access to **every** repository in the organization — there was no way to
//! scope a team to a subset. `repo::service::can_write_repo` looked only at
//! `teams.permission`.
//!
//! Semantics (backwards compatible):
//!
//! - A team with **no** rows here keeps the historical behaviour and can access
//!   every repository in its organization. Existing deployments are unaffected.
//! - Once a team has at least one row, it is restricted to exactly those
//!   repositories.

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261015_000003_create_team_repositories"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        if manager.has_table("team_repositories").await? {
            return Ok(());
        }
        manager
            .create_table(
                Table::create()
                    .table(TeamRepositories::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(TeamRepositories::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(TeamRepositories::TeamId).big_integer().not_null())
                    .col(ColumnDef::new(TeamRepositories::RepoId).big_integer().not_null())
                    .col(
                        ColumnDef::new(TeamRepositories::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(TeamRepositories::Table, TeamRepositories::TeamId)
                            .to(Teams::Table, Teams::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(TeamRepositories::Table, TeamRepositories::RepoId)
                            .to(Repositories::Table, Repositories::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        // One row per (team, repo) pair.
        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("uq_team_repositories_team_repo")
                    .table(TeamRepositories::Table)
                    .col(TeamRepositories::TeamId)
                    .col(TeamRepositories::RepoId)
                    .unique()
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("idx_team_repositories_repo_id")
                    .table(TeamRepositories::Table)
                    .col(TeamRepositories::RepoId)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(
                Table::drop()
                    .table(TeamRepositories::Table)
                    .if_exists()
                    .to_owned(),
            )
            .await
    }
}

#[derive(Iden)]
enum TeamRepositories {
    Table,
    Id,
    TeamId,
    RepoId,
    CreatedAt,
}

#[derive(Iden)]
enum Teams {
    Table,
    Id,
}

#[derive(Iden)]
enum Repositories {
    Table,
    Id,
}