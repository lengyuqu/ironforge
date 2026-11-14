//! SeaORM entity for `authorization_requests`.
//!
//! One row is one ask to perform a privileged action, bound to
//! `(action, subject, head_sha)`; see
//! `m20261114_000002_create_authorization_requests`. The row is never deleted
//! when it stops being usable: it moves to `expired` / `revoked` / `denied`, so
//! the audit trail keeps the ask and the reason it died.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

pub const STATE_PENDING: &str = "pending";
pub const STATE_CONSUMED: &str = "consumed";
pub const STATE_EXPIRED: &str = "expired";
pub const STATE_DENIED: &str = "denied";
pub const STATE_REVOKED: &str = "revoked";

pub const KIND_HUMAN: &str = "human";
pub const KIND_AGENT_SESSION: &str = "agent_session";

/// Why a request left `pending` (or was invalidated after `consumed`).
pub const REASON_HEAD_SHA_CHANGED: &str = "head_sha_changed";
pub const REASON_EXPIRED: &str = "expired";
pub const REASON_DENIED: &str = "denied";
pub const REASON_EXECUTION_FAILED_NOT_RETRYABLE: &str = "execution_failed_not_retryable";

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "authorization_requests")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub repo_id: i64,
    /// Platform enum value (`rg_core::authorization::Action`), never a client
    /// string.
    pub action: String,
    /// The domain this decision covers: a PR id, a job id, a ref, ...
    pub subject: String,
    /// Binding point, normalised by `rg_core::collaboration::message::normalize_commit_sha`.
    pub head_sha: Option<String>,
    /// [`KIND_HUMAN`] or [`KIND_AGENT_SESSION`].
    pub requested_by_kind: String,
    /// `users.id` or `agent_sessions.id`, depending on the kind.
    pub requested_by_id: i64,
    /// Digest of the authorization rule in force when the request was filed.
    pub authorization_set_id: i64,
    /// The rule's threshold at filing time. Kept on the row so `consumed` can
    /// check the vote count inside the executing transaction without
    /// re-evaluating the rule.
    pub required_approvals: i32,
    /// [`STATE_PENDING`] / [`STATE_CONSUMED`] / [`STATE_EXPIRED`] /
    /// [`STATE_DENIED`] / [`STATE_REVOKED`].
    pub state: String,
    /// One of the `REASON_*` constants, or null.
    pub state_reason: Option<String>,
    pub expires_at: DateTimeUtc,
    pub consumed_at: Option<DateTimeUtc>,
    /// Who executed the action.
    pub consumed_by: Option<i64>,
    /// Merge sha / release version id, for auditing the effect.
    pub consumed_effect_hash: Option<String>,
    pub client_request_id: String,
    pub payload_hash: String,
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
    #[sea_orm(has_many = "super::authorization_decision::Entity")]
    Decisions,
}

impl Related<super::repository::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Repository.def()
    }
}

impl Related<super::authorization_decision::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Decisions.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
