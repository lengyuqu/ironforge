//! SeaORM entity for `collab_task_template_links` — pins a task to the template
//! version it was created from, with a copy of what it was created from.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "collab_task_template_links")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub task_id: String,
    pub repo_id: i64,
    pub template_id: String,
    pub template_version: i64,
    pub snapshot_json: String,
    /// Commit the caller's `rules_ref` resolved to when the task was created.
    pub rules_commit: Option<String>,
    pub creator_kind: String,
    pub creator_id: String,
    pub client_task_id: String,
    pub payload_hash: String,
    pub created_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
