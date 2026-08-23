//! Background job entity — maps to the `background_jobs` table (QUEUE-001).

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// Job statuses. Lifecycle: `pending` → `running` → `succeeded`, or
/// `running` → `pending` (retry with backoff) → … → `dead`.
pub mod status {
    pub const PENDING: &str = "pending";
    pub const RUNNING: &str = "running";
    pub const SUCCEEDED: &str = "succeeded";
    pub const DEAD: &str = "dead";
}

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "background_jobs")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    /// Task type discriminator, e.g. `webhook.deliver`, `email.send`.
    pub task_type: String,
    /// JSON-encoded task payload.
    pub payload: String,
    /// Lifecycle status (see [`status`]).
    pub status: String,
    /// Number of attempts executed so far.
    pub attempts: i32,
    /// Attempts allowed before the job is dead-lettered.
    pub max_attempts: i32,
    /// Earliest time the job may be claimed.
    pub run_at: DateTimeUtc,
    /// Worker id holding the claim while the job runs.
    pub locked_by: Option<String>,
    /// When the claim was taken (staleness detection).
    pub locked_at: Option<DateTimeUtc>,
    /// Error message of the last failed attempt.
    pub last_error: Option<String>,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
