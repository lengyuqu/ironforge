//! SeaORM entity for `agent_sessions` — a traceable collaboration identity under one account.
//!
//! `secret_hash` is a domain-separated digest of the session secret; the secret
//! itself is never stored. Name and client kind are self-reported labels.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "agent_sessions")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    pub repo_id: i64,
    pub user_id: i64,
    #[serde(skip_serializing)]
    pub secret_hash: String,
    pub display_name: String,
    pub client_kind: String,
    pub created_at: DateTimeUtc,
    pub last_seen_at: DateTimeUtc,
    pub expires_at: DateTimeUtc,
    pub ended_at: Option<DateTimeUtc>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::repository::Entity",
        from = "Column::RepoId",
        to = "super::repository::Column::Id"
    )]
    Repository,
    #[sea_orm(
        belongs_to = "super::user::Entity",
        from = "Column::UserId",
        to = "super::user::Column::Id"
    )]
    User,
}

impl ActiveModelBehavior for ActiveModel {}
