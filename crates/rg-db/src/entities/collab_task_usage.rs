//! SeaORM entity for `collab_task_usage` — append-only usage records. Every
//! value is the reporter's claim about one unit, from one source, over one
//! period.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "collab_task_usage")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    pub repo_id: i64,
    pub task_id: String,
    pub unit: String,
    pub source: String,
    pub amount: String,
    pub period_start: DateTimeUtc,
    pub period_end: DateTimeUtc,
    pub reported_by_kind: String,
    pub reported_by_id: String,
    pub client_usage_id: String,
    pub payload_hash: String,
    pub created_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
