//! SeaORM entity for `collab_notices` — a durable per-session reminder.
//!
//! A notice is a hint that something changed, never the authoritative state:
//! the consumer re-reads the object `source_type`/`source_id` point at.
//! `hint_json` holds platform-derived facts only (ids, enumerations).

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "collab_notices")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    pub repo_id: i64,
    pub task_id: String,
    pub recipient_session_id: String,
    pub kind: String,
    pub source_type: String,
    pub source_id: String,
    pub generation: i64,
    pub hint_json: String,
    pub created_at: DateTimeUtc,
    pub ack_at: Option<DateTimeUtc>,
    /// The condition behind an escalation ended; no longer asks for attention.
    pub resolved_at: Option<DateTimeUtc>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
