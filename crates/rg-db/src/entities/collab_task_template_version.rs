//! SeaORM entity for `collab_task_template_versions` — one immutable version of
//! a task template. Rows are only ever inserted; an edit is a new version.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "collab_task_template_versions")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    pub repo_id: i64,
    pub template_id: String,
    /// 1-based, unique per template.
    pub version: i64,
    pub title_pattern: String,
    pub goal: String,
    pub deliverables_json: String,
    pub checklist_json: String,
    pub created_by_user_id: i64,
    pub client_version_id: String,
    pub payload_hash: String,
    pub created_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
