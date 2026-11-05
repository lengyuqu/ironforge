//! SeaORM entity for `collab_inbox_reads` — the highest object version an actor
//! marked as read for one inbox item. Reading only; no object state.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "collab_inbox_reads")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub repo_id: i64,
    #[sea_orm(primary_key, auto_increment = false)]
    pub actor_kind: String,
    #[sea_orm(primary_key, auto_increment = false)]
    pub actor_id: String,
    #[sea_orm(primary_key, auto_increment = false)]
    pub item_key: String,
    pub observed_version: i64,
    pub read_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
