//! SeaORM entity for `collab_decision_changes` — the single terminal event
//! (`superseded` or `revoked`) a decision can have. The decision row itself is
//! never edited.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "collab_decision_changes")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    pub repo_id: i64,
    pub task_id: String,
    pub decision_id: String,
    /// `superseded` | `revoked`.
    pub kind: String,
    pub replacement_decision_id: Option<String>,
    pub reason: String,
    pub actor_kind: String,
    pub actor_id: String,
    pub client_change_id: String,
    pub payload_hash: String,
    pub created_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
