//! Repository event entity — maps to the `repo_events` table.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "repo_events")]
pub struct Model {
    /// Monotonically increasing: the subscriber's cursor.
    #[sea_orm(primary_key)]
    pub id: i64,
    pub repo_id: i64,
    /// The webhook event name (`push`, `pull_request`, `issues`, ...).
    pub event_type: String,
    /// The webhook payload as JSON text (may contain user-written text).
    pub payload: Option<String>,
    pub created_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
