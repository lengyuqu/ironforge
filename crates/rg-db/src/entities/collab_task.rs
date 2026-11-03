//! SeaORM entity for `collab_tasks` — a unit of work agents coordinate around.
//!
//! `version` is the optimistic-lock counter; `next_seq` is the sequence number
//! of the task's last committed message (0 = none yet).

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "collab_tasks")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    pub repo_id: i64,
    pub title: String,
    pub description: Option<String>,
    pub status: String,
    pub owner_session_id: Option<String>,
    pub branch: Option<String>,
    /// JSON array of declared repository-relative paths.
    pub paths_json: String,
    pub issue_number: Option<i64>,
    pub pr_number: Option<i64>,
    pub version: i64,
    pub next_seq: i64,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::repository::Entity",
        from = "Column::RepoId",
        to = "super::repository::Column::Id"
    )]
    Repository,
}

impl ActiveModelBehavior for ActiveModel {}
