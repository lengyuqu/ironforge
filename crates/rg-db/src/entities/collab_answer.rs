//! SeaORM entity for `collab_answers` — the single, immutable answer of a question.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "collab_answers")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    pub repo_id: i64,
    pub question_id: String,
    /// `session` or `user`; the door used, not proof of who sat behind it.
    pub actor_kind: String,
    pub actor_id: String,
    pub body: String,
    pub option_id: Option<String>,
    /// Hash of the context the answerer acknowledged.
    pub context_hash: String,
    pub client_answer_id: String,
    pub payload_hash: String,
    pub created_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
