//! SeaORM entity for `collab_coordination_links` — a reference from a group to
//! a task and/or commit of a target repository, by stable repository id.
//!
//! Removal is soft (`removed_at`); the row stays as the audit trail.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "collab_coordination_links")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    pub group_id: String,
    pub anchor_repo_id: i64,
    pub target_repo_id: i64,
    pub target_task_id: Option<String>,
    /// Full lower-case commit id, verified to exist when the link was added.
    pub commit_sha: Option<String>,
    /// Uniqueness key of the *active* link; `None` once removed.
    pub active_key: Option<String>,
    pub created_by_user_id: i64,
    pub actor_kind: String,
    pub actor_id: String,
    pub client_link_id: String,
    pub payload_hash: String,
    pub created_at: DateTimeUtc,
    pub removed_at: Option<DateTimeUtc>,
    pub removed_by_user_id: Option<i64>,
    pub removed_actor_kind: Option<String>,
    pub removed_actor_id: Option<String>,
    pub remove_client_id: Option<String>,
    pub remove_payload_hash: Option<String>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
