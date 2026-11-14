//! SeaORM entity for `usage_counters`.
//!
//! Platform-measured usage per `(subject, repo, metric, period)`; see
//! `m20261114_000001_create_usage_counters`. `value` is an absolute measurement
//! for the period, so writing the same measurement twice is harmless.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "usage_counters")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    /// `user` today.
    pub subject_kind: String,
    /// `users.id` of the subject.
    pub subject_id: i64,
    pub repo_id: i64,
    /// `ci_minutes` | `ci_concurrent_pipelines`.
    pub metric: String,
    /// UTC day the measurement belongs to.
    pub period_start: Date,
    /// Absolute value for the period (never a delta).
    pub value: i64,
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
