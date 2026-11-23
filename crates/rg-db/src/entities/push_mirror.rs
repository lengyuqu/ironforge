//! Push mirror entity — maps to the `push_mirrors` table.
//!
//! One row forwards this repository's branches (and optionally tags) to an
//! external Git host after every push. The credential is *not* stored here: it
//! comes from the owner's [`super::remote_credential`] row for the target host.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// Push after every push to this repository (the default).
pub const TRIGGER_ON_PUSH: &str = "on_push";
/// Push only when explicitly requested through the sync endpoint.
pub const TRIGGER_MANUAL: &str = "manual";

/// Forwarding is live.
pub const STATUS_ACTIVE: &str = "active";
/// Forwarding is configured but deliberately stopped.
pub const STATUS_PAUSED: &str = "paused";
/// The last push failed; the row keeps the operator's error for inspection.
pub const STATUS_ERROR: &str = "error";

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "push_mirrors")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    /// Target repository; unique, so a repository has at most one push target.
    pub repo_id: i64,
    /// Where the refs go. Validated by `net_guard::validate_mirror_url` on
    /// every push — a stored URL is not trusted just because it was checked
    /// once.
    pub url: String,
    /// Push tags alongside branches.
    pub push_tags: bool,
    /// [`TRIGGER_ON_PUSH`] or [`TRIGGER_MANUAL`].
    pub trigger_mode: String,
    /// [`STATUS_ACTIVE`] | [`STATUS_PAUSED`] | [`STATUS_ERROR`].
    pub status: String,
    pub last_push_at: Option<DateTimeUtc>,
    pub last_push_error: Option<String>,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::repository::Entity",
        from = "Column::RepoId",
        to = "super::repository::Column::Id"
    )]
    Repository,
}

impl Related<super::repository::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Repository.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
