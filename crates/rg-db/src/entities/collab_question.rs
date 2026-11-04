//! SeaORM entity for `collab_questions` — a structured request for a decision.
//!
//! Title, body and options are claims by the asker; treat as untrusted data.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "collab_questions")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    pub repo_id: i64,
    pub task_id: String,
    /// `session` or `user`.
    pub creator_kind: String,
    pub creator_id: String,
    /// `repo_writers`, `user` or `session`.
    pub recipient_kind: String,
    pub recipient_id: Option<String>,
    pub title: String,
    pub body: String,
    /// JSON array of `{id, label}` (empty for free-text questions).
    pub options_json: String,
    /// JSON `{task_version, base_sha?, head_sha?}` captured at ask time.
    pub context_json: String,
    /// `open` | `answered` | `cancelled`.
    pub status: String,
    pub version: i64,
    pub client_question_id: String,
    pub payload_hash: String,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
