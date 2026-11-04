//! SeaORM entity for `collab_delivery_receipts` — the target session of a
//! targeted message confirmed it read the message.
//!
//! A receipt is a claim by that session, not agreement or a reply. It never
//! moves the `collab_reads` watermark and the watermark never creates one.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "collab_delivery_receipts")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub message_id: String,
    #[sea_orm(primary_key, auto_increment = false)]
    pub target_session_id: String,
    pub repo_id: i64,
    pub task_id: String,
    pub read_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
