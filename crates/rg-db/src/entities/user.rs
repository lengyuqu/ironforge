//! User entity — maps to the `users` table.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "users")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    #[sea_orm(unique)]
    pub username: String,
    #[sea_orm(unique)]
    pub email: String,
    /// Argon2 hashed password
    pub password_hash: String,
    pub display_name: Option<String>,
    pub avatar_url: Option<String>,
    pub bio: Option<String>,
    pub is_admin: bool,
    pub is_active: bool,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::repository::Entity")]
    Repository,
    #[sea_orm(has_many = "super::ssh_key::Entity")]
    SshKey,
    #[sea_orm(has_many = "super::access_token::Entity")]
    AccessToken,
}

impl Related<super::repository::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Repository.def()
    }
}

impl Related<super::ssh_key::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::SshKey.def()
    }
}

impl Related<super::access_token::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::AccessToken.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
