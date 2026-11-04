//! SeaORM entity for `collab_decisions` — an immutable record of a decision
//! taken on a task. Title and rationale are claims by the writer; a decision is
//! not an approval and grants no Git, CI or merge right.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "collab_decisions")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    pub repo_id: i64,
    pub task_id: String,
    /// 1-based position among the task's decisions; unique per task.
    pub seq: i64,
    pub question_id: Option<String>,
    pub answer_id: Option<String>,
    pub title: String,
    pub rationale: String,
    /// JSON `{task_version, base_sha?, head_sha?}` captured at decision time.
    pub context_json: String,
    /// `session` or `user`; the door used, not proof of who sat behind it.
    pub actor_kind: String,
    pub actor_id: String,
    pub client_decision_id: String,
    pub payload_hash: String,
    pub created_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
