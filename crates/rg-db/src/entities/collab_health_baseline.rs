//! Collaboration health baseline entity — maps to `collab_health_baselines`.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "collab_health_baselines")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub repo_id: i64,
    /// When the baseline was first taken (server time).
    pub taken_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
