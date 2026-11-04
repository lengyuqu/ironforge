//! SeaORM entity for `collab_task_dependencies` — edge `task_id -> depends_on_task_id`
//! ("task waits for dependency"). A declaration; it blocks nothing by itself.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "collab_task_dependencies")]
pub struct Model {
    pub repo_id: i64,
    #[sea_orm(primary_key, auto_increment = false)]
    pub task_id: String,
    #[sea_orm(primary_key, auto_increment = false)]
    pub depends_on_task_id: String,
    /// `session` or `user`: the door used, not proof of who used it.
    pub created_by_kind: String,
    pub created_by_id: String,
    pub client_dependency_id: String,
    pub payload_hash: String,
    /// Graph version produced by inserting this edge.
    pub graph_version: i64,
    pub created_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
