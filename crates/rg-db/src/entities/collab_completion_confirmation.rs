//! SeaORM entity for `collab_completion_confirmations` — an explicit manual
//! confirmation of one condition by an authorised account (O04). A
//! collaboration record, not a human approval.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "collab_completion_confirmations")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    pub repo_id: i64,
    pub task_id: String,
    pub condition_id: String,
    pub condition_set_version: i64,
    pub task_version: i64,
    pub head_sha: Option<String>,
    pub note: String,
    pub actor_user_id: i64,
    pub client_request_id: String,
    pub payload_hash: String,
    pub created_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
