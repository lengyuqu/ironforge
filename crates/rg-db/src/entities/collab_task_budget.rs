//! SeaORM entity for `collab_task_budgets` — a per-task budget in one unit from
//! one source. `amount` / `alert_at` are normalized decimal text; the database
//! never aggregates them.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "collab_task_budgets")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    pub repo_id: i64,
    pub task_id: String,
    pub unit: String,
    pub source: String,
    pub amount: String,
    pub alert_at: Option<String>,
    pub sampling_period: String,
    pub version: i64,
    pub set_by_kind: String,
    pub set_by_id: String,
    pub payload_hash: String,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
