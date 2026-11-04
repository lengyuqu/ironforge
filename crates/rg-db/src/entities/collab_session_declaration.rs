//! SeaORM entity for `collab_session_declarations` — what a session says about
//! its own tools, test ability and network limits. A self-report: never
//! verified and never an authorization.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "collab_session_declarations")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub session_id: String,
    pub repo_id: i64,
    pub version: i64,
    pub tools_json: String,
    pub can_run_tests: String,
    pub network: String,
    pub network_notes: String,
    pub notes: String,
    pub payload_hash: String,
    pub declared_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
