//! Symbol commit entity — maps to the `symbol_commits` table.
//!
//! One row is one indexed commit of one repository. Definitions point at
//! `id` instead of repeating the 40-byte SHA on every row: the design document
//! measured the difference at 60 MiB per commit on this repository.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// `files_total` of a commit indexed before the completeness counters existed.
///
/// The column is `NOT NULL DEFAULT 0`, so a migration cannot leave "not
/// measured" implicit: `0` is a real measurement ("read no files") and must
/// keep meaning that. Negative values are otherwise impossible, so `-1` is the
/// unambiguous "unknown" the reader turns into `partial: true`.
pub const UNKNOWN_FILES_TOTAL: i32 = -1;

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
    /// Files this indexing run read and offered to the extractor. Negative
    /// ([`UNKNOWN_FILES_TOTAL`]) for rows written before the counters existed.
    pub files_total: i32,
    /// Of [`Self::files_total`], how many the extractor reported a parse error
    /// for. Definitions from those files are partial or missing.
    pub files_failed: i32,
    /// Of [`Self::files_total`], how many hit the outline item cap, so rows
    /// were dropped.
    pub files_truncated: i32,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
