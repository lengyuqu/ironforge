//! Agent principals: an account is either a person or an agent owned by one.
//!
//! - `users.kind`: `human` (the default, and every existing row) or `agent`.
//! - `users.owner_user_id`: the person an agent belongs to; `NULL` for people.
//! - `users.agent_description`: what the agent is for. Display only.
//!
//! `owner_user_id` has no foreign key: SQLite cannot add one to an existing
//! table, and the application refuses to delete a person who still owns agents
//! (`rg_core::user::service::delete_user`).

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261110_000001_add_user_kind_and_owner"
    }
}

// Explicit table name: a derived `Iden` would produce the singular `user`.
#[derive(DeriveIden)]
enum Users {
    #[sea_orm(iden = "users")]
    Table,
    Kind,
    OwnerUserId,
    AgentDescription,
}

const OWNER_INDEX: &str = "idx_users_owner_user_id";

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    /// Idempotent: `down` is a no-op, so a re-apply must not fail with a
    /// duplicate column or index.
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        if !manager.has_table("users").await? {
            return Ok(());
        }
        if !manager.has_column("users", "kind").await? {
            manager
                .alter_table(
                    Table::alter()
                        .table(Users::Table)
                        .add_column(
                            ColumnDef::new(Users::Kind)
                                .string_len(16)
                                .not_null()
                                .default("human"),
                        )
                        .to_owned(),
                )
                .await?;
        }
        if !manager.has_column("users", "owner_user_id").await? {
            manager
                .alter_table(
                    Table::alter()
                        .table(Users::Table)
                        .add_column(ColumnDef::new(Users::OwnerUserId).big_integer().null())
                        .to_owned(),
                )
                .await?;
        }
        if !manager.has_column("users", "agent_description").await? {
            manager
                .alter_table(
                    Table::alter()
                        .table(Users::Table)
                        .add_column(ColumnDef::new(Users::AgentDescription).text().null())
                        .to_owned(),
                )
                .await?;
        }
        if !manager.has_index("users", OWNER_INDEX).await? {
            manager
                .create_index(
                    Index::create()
                        .name(OWNER_INDEX)
                        .table(Users::Table)
                        .col(Users::OwnerUserId)
                        .to_owned(),
                )
                .await?;
        }
        Ok(())
    }

    /// Intentionally empty: SQLite cannot reliably drop a column through the
    /// schema builder, and the columns are harmless to older code (`kind`
    /// defaults to `human`). `up` is idempotent, so rollback then re-apply works.
    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Ok(())
    }
}
