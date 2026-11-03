//! SeaORM entity for `collab_messages` — append-only task messages.
//!
//! Messages are never edited or deleted. `(task_id, seq)` is the polling order;
//! `(author_session_id, client_message_id)` makes retries idempotent and
//! `payload_hash` distinguishes a retry from key reuse with a different body.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "collab_messages")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    pub repo_id: i64,
    pub task_id: String,
    pub seq: i64,
    pub author_session_id: String,
    pub kind: String,
    pub body: String,
    pub reply_to_id: Option<String>,
    pub target_session_id: Option<String>,
    pub commit_sha: Option<String>,
    pub client_message_id: String,
    pub payload_hash: String,
    pub created_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
