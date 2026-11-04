//! SeaORM entity for `collab_takeovers` — immutable history of task ownership changes.
//!
//! A takeover is a declaration of who works on a task. It grants no Git, CI or
//! approval rights, and `reason` is an unverified claim.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "collab_takeovers")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    pub repo_id: i64,
    pub task_id: String,
    pub from_session_id: Option<String>,
    pub to_session_id: String,
    /// `session` (self-claim) or `user` (reassignment).
    pub actor_kind: String,
    pub actor_id: String,
    pub previous_task_version: i64,
    pub new_task_version: i64,
    /// `claimed` or `reassigned`.
    pub outcome: String,
    pub reason: String,
    pub client_takeover_id: String,
    pub payload_hash: String,
    pub created_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
