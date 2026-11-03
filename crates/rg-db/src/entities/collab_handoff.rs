//! SeaORM entity for `collab_handoffs` — a declared transfer of a task between sessions.
//!
//! All text sections and evidence are unverified claims by the creating session.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "collab_handoffs")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    pub repo_id: i64,
    pub task_id: String,
    pub from_session_id: String,
    pub to_session_id: String,
    pub summary: String,
    pub completed: String,
    pub next_steps: String,
    pub blockers: String,
    pub commit_sha: Option<String>,
    /// JSON array of evidence URIs / ids (claims, not verification).
    pub evidence_json: String,
    pub client_handoff_id: String,
    pub payload_hash: String,
    pub status: String,
    pub version: i64,
    pub created_at: DateTimeUtc,
    pub accepted_at: Option<DateTimeUtc>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
