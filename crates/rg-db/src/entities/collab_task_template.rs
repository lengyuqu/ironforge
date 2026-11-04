//! SeaORM entity for `collab_task_templates` — the stable identity of a
//! repo-admin-managed task template. Content lives in immutable rows of
//! `collab_task_template_versions`; `latest_version` only points at the newest.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "collab_task_templates")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    pub repo_id: i64,
    pub name: String,
    pub latest_version: i64,
    pub created_by_user_id: i64,
    pub client_template_id: String,
    pub payload_hash: String,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
