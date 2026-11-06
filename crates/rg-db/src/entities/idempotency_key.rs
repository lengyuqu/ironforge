//! Idempotency key entity — maps to the `idempotency_keys` table.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "idempotency_keys")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub user_id: i64,
    /// `0` for an ordinary login, otherwise the personal access token's id.
    pub token_id: i64,
    /// The client's `Idempotency-Key` value.
    pub idem_key: String,
    /// SHA-256 (hex) of method, path and body.
    pub fingerprint: String,
    /// `pending` while the first request runs, `done` once its response is stored.
    pub state: String,
    pub status_code: Option<i32>,
    pub response_body: Option<String>,
    pub created_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
