//! SeaORM entity for `collab_work_packages` — immutable, numbered hand-over
//! summaries of a task. Every claim in `body_json` is the author's own.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "collab_work_packages")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    pub repo_id: i64,
    pub task_id: String,
    /// 1-based, unique per task.
    pub revision: i64,
    pub author_kind: String,
    pub author_id: String,
    /// Version of the task when the package was written.
    pub task_version: i64,
    pub commit_sha: Option<String>,
    pub body_json: String,
    pub client_package_id: String,
    pub payload_hash: String,
    pub created_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
