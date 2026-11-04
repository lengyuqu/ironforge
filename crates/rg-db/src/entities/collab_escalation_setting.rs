//! SeaORM entity for `collab_escalation_settings` — per-repository thresholds
//! for escalation reminders. No row = escalation disabled.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "collab_escalation_settings")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub repo_id: i64,
    pub enabled: bool,
    pub blocked_after_secs: i64,
    pub question_after_secs: i64,
    pub handoff_after_secs: i64,
    pub repeat_after_secs: i64,
    pub quiet_start_minute: Option<i32>,
    pub quiet_end_minute: Option<i32>,
    pub version: i64,
    pub updated_by_user_id: i64,
    pub updated_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
