//! SeaORM entity for `collab_coordination_groups` — a cross-repository
//! coordination group owned by one anchor repository.
//!
//! `title` and `description` are client-written, readable by everyone who can
//! read the anchor repository's collaboration data, and never filled in by the
//! server from other repositories.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "collab_coordination_groups")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    pub anchor_repo_id: i64,
    pub title: String,
    pub description: String,
    pub created_by_user_id: i64,
    /// `session` or `user`.
    pub actor_kind: String,
    pub actor_id: String,
    /// Compare-and-swap counter, bumped by every link change.
    pub version: i64,
    pub client_group_id: String,
    pub payload_hash: String,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
