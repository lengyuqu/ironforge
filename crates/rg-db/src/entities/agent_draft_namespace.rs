//! Agent draft namespace entity — maps to the `agent_draft_namespaces` table.
//!
//! One row reserves the branch directory `refs/heads/<prefix>` for one agent
//! (#18). See the migration for the column semantics; `prefix` always ends with
//! `/` and never contains a wildcard.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// Pushes are accepted: the directory is an ordinary draft area.
pub const STATE_ACTIVE: &str = "active";
/// `expires_at` has passed: pushes are refused, the branches are still readable
/// and the row may still be extended back to `active`. Deletion happens after
/// `expires_at + grace_secs`.
pub const STATE_EXPIRING: &str = "expiring";
/// The branches were removed (or were already gone). The row is kept as the
/// record of the reservation so a resurrected branch is refused.
pub const STATE_DELETED: &str = "deleted";

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "agent_draft_namespaces")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub repo_id: i64,
    /// The reserved directory, with the trailing slash: `agent/42/`.
    pub prefix: String,
    /// The account that registered the namespace. No foreign key, see the
    /// migration.
    pub owner_user_id: i64,
    /// The TTL the caller asked for; `extend` caps the total lifetime at
    /// `4 × ttl_secs` from `created_at`.
    pub ttl_secs: i64,
    /// The moment pushes stop being accepted, and the anchor of the grace
    /// period: the branches go away at `expires_at + grace_secs`.
    pub expires_at: DateTimeUtc,
    pub grace_secs: i64,
    /// `active` | `expiring` | `deleted`.
    pub state: String,
    /// How many times `extend` succeeded (audit only).
    pub extended_count: i32,
    pub created_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
