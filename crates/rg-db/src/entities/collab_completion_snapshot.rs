//! SeaORM entity for `collab_completion_snapshots` — an evaluation recorded as
//! it was at `created_at` (O04). A live evaluation is never history.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "collab_completion_snapshots")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    pub repo_id: i64,
    pub task_id: String,
    pub condition_set_version: i64,
    pub task_version: i64,
    pub overall: String,
    pub result_json: String,
    pub actor_kind: String,
    pub actor_id: String,
    pub created_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
