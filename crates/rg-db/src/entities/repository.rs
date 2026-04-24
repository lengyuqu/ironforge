//! Repository entity — maps to the `repositories` table.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "repositories")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    /// Repository owner (user id)
    pub owner_id: i64,
    /// Repository slug / name (e.g. "myrepo")
    pub name: String,
    pub description: Option<String>,
    pub is_private: bool,
    pub default_branch: String,
    pub fork_id: Option<i64>,
    pub stars_count: i64,
    pub forks_count: i64,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::user::Entity",
        from = "Column::OwnerId",
        to = "super::user::Column::Id"
    )]
    Owner,
}

impl Related<super::user::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Owner.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
