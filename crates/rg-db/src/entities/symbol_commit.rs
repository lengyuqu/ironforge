//! Symbol commit entity — maps to the `symbol_commits` table.
//!
//! One row is one indexed commit of one repository. Definitions point at
//! `id` instead of repeating the 40-byte SHA on every row: the design document
//! measured the difference at 60 MiB per commit on this repository.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "symbol_commits")]
pub struct Model {
    /// `INTEGER` in the schema (not `BIGINT`): it is the parent of
    /// `symbol_definitions.commit_id` and both sides must keep the same width
    /// for MySQL's foreign-key type parity.
    #[sea_orm(primary_key)]
    pub id: i32,
    pub repo_id: i64,
    /// Full 40-character hexadecimal commit SHA.
    pub commit_sha: String,
    /// When the commit became the indexed snapshot (the prune key).
    pub created_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
