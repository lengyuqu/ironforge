//! SeaORM entity for `authorization_decisions`: one row per approver.
//!
//! The unique key `(request_id, decider_user_id)` is the whole point: a second
//! vote by the same person is a no-op (`Ok(false)`), exactly like
//! `ci_environment_approvals`' `uq_ci_environment_approval_job_user`.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

pub const DECISION_APPROVE: &str = "approve";
pub const DECISION_DENY: &str = "deny";

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "authorization_decisions")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub request_id: i64,
    pub decider_user_id: i64,
    /// [`DECISION_APPROVE`] or [`DECISION_DENY`].
    pub decision: String,
    /// The payload digest re-computed at decision time; kept so an approval can
    /// be tied to the exact binding it was given for.
    pub payload_hash: String,
    pub created_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::authorization_request::Entity",
        from = "Column::RequestId",
        to = "super::authorization_request::Column::Id"
    )]
    Request,
}

impl Related<super::authorization_request::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Request.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
