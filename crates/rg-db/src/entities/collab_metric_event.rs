//! SeaORM entity for `collab_metric_events` — the platform event log behind the
//! collaboration health metrics.
//!
//! Rows are written by the same transaction as the business change and carry
//! server time only. They never hold message bodies, titles or secrets.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "collab_metric_events")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    pub repo_id: i64,
    pub task_id: String,
    pub event_type: String,
    pub from_state: Option<String>,
    pub to_state: Option<String>,
    /// Task version after a status change; orders events with equal timestamps.
    pub task_version: Option<i64>,
    /// `task` | `handoff` | `question` | `takeover` | `baseline_snapshot`.
    pub source_type: String,
    pub source_id: String,
    /// Server time of the change (never client time).
    pub occurred_at: DateTimeUtc,
    pub recorded_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
