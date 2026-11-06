//! Idempotency key operations.
//!
//! The first statement of every claim is an `INSERT`, and the unique
//! `(user_id, token_id, idem_key)` index decides which of two racing requests
//! owns the key. `ON CONFLICT DO NOTHING` is not used: SeaORM renders it as an
//! invalid `INSERT IGNORE` on MySQL, so a unique violation is read from the
//! error instead.

use sea_orm::sea_query::Expr;
use sea_orm::*;

use crate::entities::idempotency_key::{self, Entity};

pub const STATE_PENDING: &str = "pending";
pub const STATE_DONE: &str = "done";

/// The outcome of trying to claim a key.
#[derive(Debug)]
pub enum Claim {
    /// This request owns the key and should run.
    Claimed(i64),
    /// The key already exists (another request, earlier or still running).
    Existing(idempotency_key::Model),
}

/// Try to claim `(user_id, token_id, key)` for a request with `fingerprint`.
///
/// A row older than `expires_before` is dropped first, so an old key can be
/// reused without waiting for the retention sweep.
pub async fn claim(
    db: &DatabaseConnection,
    user_id: i64,
    token_id: i64,
    key: &str,
    fingerprint: &str,
    expires_before: chrono::DateTime<chrono::Utc>,
) -> Result<Claim, DbErr> {
    let insert = |now| {
        let model = idempotency_key::ActiveModel {
            id: NotSet,
            user_id: Set(user_id),
            token_id: Set(token_id),
            idem_key: Set(key.to_string()),
            fingerprint: Set(fingerprint.to_string()),
            state: Set(STATE_PENDING.to_string()),
            status_code: Set(None),
            response_body: Set(None),
            created_at: Set(now),
        };
        async move { model.insert(db).await }
    };
    match insert(chrono::Utc::now()).await {
        Ok(row) => Ok(Claim::Claimed(row.id)),
        Err(error) if is_unique_violation(&error) => {
            // An expired row is replaced; anything else is the winner of the race.
            let expired = Entity::delete_many()
                .filter(idempotency_key::Column::UserId.eq(user_id))
                .filter(idempotency_key::Column::TokenId.eq(token_id))
                .filter(idempotency_key::Column::IdemKey.eq(key))
                .filter(idempotency_key::Column::CreatedAt.lt(expires_before))
                .exec(db)
                .await?
                .rows_affected;
            if expired > 0 {
                return match insert(chrono::Utc::now()).await {
                    Ok(row) => Ok(Claim::Claimed(row.id)),
                    Err(error) if is_unique_violation(&error) => {
                        existing(db, user_id, token_id, key).await
                    }
                    Err(error) => Err(error),
                };
            }
            existing(db, user_id, token_id, key).await
        }
        Err(error) => Err(error),
    }
}

async fn existing(
    db: &DatabaseConnection,
    user_id: i64,
    token_id: i64,
    key: &str,
) -> Result<Claim, DbErr> {
    match Entity::find()
        .filter(idempotency_key::Column::UserId.eq(user_id))
        .filter(idempotency_key::Column::TokenId.eq(token_id))
        .filter(idempotency_key::Column::IdemKey.eq(key))
        .one(db)
        .await?
    {
        Some(row) => Ok(Claim::Existing(row)),
        // Deleted between the conflict and the read: the caller may retry.
        None => Err(DbErr::RecordNotFound("idempotency key vanished".into())),
    }
}

fn is_unique_violation(error: &DbErr) -> bool {
    matches!(error.sql_err(), Some(SqlErr::UniqueConstraintViolation(_)))
}

/// Store the response of the request that owns row `id`.
pub async fn complete(
    db: &DatabaseConnection,
    id: i64,
    status: i32,
    body: &str,
) -> Result<(), DbErr> {
    Entity::update_many()
        .col_expr(idempotency_key::Column::State, Expr::value(STATE_DONE))
        .col_expr(idempotency_key::Column::StatusCode, Expr::value(status))
        .col_expr(idempotency_key::Column::ResponseBody, Expr::value(body))
        .filter(idempotency_key::Column::Id.eq(id))
        .exec(db)
        .await?;
    Ok(())
}

/// Release a key without a stored response (the request failed on the server or
/// its response was too large to keep), so a retry runs again.
pub async fn release(db: &DatabaseConnection, id: i64) -> Result<(), DbErr> {
    Entity::delete_by_id(id).exec(db).await?;
    Ok(())
}

/// Delete rows older than `before`. Returns how many.
pub async fn delete_older_than(
    db: &DatabaseConnection,
    before: chrono::DateTime<chrono::Utc>,
) -> Result<u64, DbErr> {
    Ok(Entity::delete_many()
        .filter(idempotency_key::Column::CreatedAt.lt(before))
        .exec(db)
        .await?
        .rows_affected)
}
