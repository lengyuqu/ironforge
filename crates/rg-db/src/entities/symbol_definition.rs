//! Symbol definition entity — maps to the `symbol_definitions` table.
//!
//! One row is one declaration of one indexed commit (#63 step 2, T28). The
//! columns mirror `rg_core::search::outline::OutlineItem`; `name` is stored
//! verbatim and is **not** guaranteed to be an identifier: Rust `impl` rows
//! carry their first line as a name and are excluded by the query side
//! (`kind = 'impl'`), not by the writer.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "symbol_definitions")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub repo_id: i64,
    /// Foreign key into `symbol_commits.id` (`INTEGER` on both sides; a wider
    /// Rust type would fail to decode the column on PostgreSQL).
    pub commit_id: i32,
    /// Repository-relative path, '/' separated.
    pub path: String,
    /// Declaration name as extracted, verbatim.
    pub name: String,
    /// `function` | `method` | `struct` | `enum` | `trait` | `impl` | `class` |
    /// `interface` | `module` | `const` | `macro` | `type` | `other`.
    pub kind: String,
    /// 1-based first line.
    pub start_line: i32,
    /// 1-based inclusive last line.
    pub end_line: i32,
    /// First source line, whitespace-collapsed, at most 200 characters.
    pub signature: Option<String>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
