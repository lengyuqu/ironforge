//! Request idempotency (`Idempotency-Key`): `idempotency_keys`.
//!
//! A client that retries a create request (a network error, a timeout) sends the
//! same `Idempotency-Key`; the server then replays the first response instead of
//! doing the work twice. One row per `(user, token, key)`:
//!
//! * `token_id` is `0` for an ordinary login and the personal access token's id
//!   otherwise, so two tokens of one user never share keys;
//! * `fingerprint` is the SHA-256 of method, path and body, so the same key with
//!   a different request is refused instead of replayed;
//! * `state` is `pending` while the first request runs and `done` once its
//!   response (status and body) is stored.
//!
//! Only a new table: nothing existing is altered. Rows are deleted after 24
//! hours. `up` is idempotent.

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261106_000003_create_idempotency_keys"
    }
}

// Declared with an explicit name: a derived `Iden` would produce the singular
// `idempotency_key`, which is not the table the entity maps to.
#[derive(DeriveIden)]
enum IdempotencyKeys {
    #[sea_orm(iden = "idempotency_keys")]
    Table,
    Id,
    UserId,
    TokenId,
    IdemKey,
    Fingerprint,
    State,
    StatusCode,
    ResponseBody,
    CreatedAt,
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        if manager.has_table("idempotency_keys").await? {
            return Ok(());
        }
        manager
            .create_table(
                Table::create()
                    .table(IdempotencyKeys::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(IdempotencyKeys::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(IdempotencyKeys::UserId)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(IdempotencyKeys::TokenId)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(IdempotencyKeys::IdemKey)
                            .string_len(255)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(IdempotencyKeys::Fingerprint)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(IdempotencyKeys::State)
                            .string_len(16)
                            .not_null(),
                    )
                    .col(ColumnDef::new(IdempotencyKeys::StatusCode).integer().null())
                    .col(ColumnDef::new(IdempotencyKeys::ResponseBody).text().null())
                    .col(
                        ColumnDef::new(IdempotencyKeys::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;
        // The lookup key, and the arbiter of two requests racing with one key.
        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("uq_idempotency_keys_actor_key")
                    .table(IdempotencyKeys::Table)
                    .col(IdempotencyKeys::UserId)
                    .col(IdempotencyKeys::TokenId)
                    .col(IdempotencyKeys::IdemKey)
                    .unique()
                    .to_owned(),
            )
            .await?;
        // The retention sweep.
        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("idx_idempotency_keys_created_at")
                    .table(IdempotencyKeys::Table)
                    .col(IdempotencyKeys::CreatedAt)
                    .to_owned(),
            )
            .await
    }

    /// Drops the table (its rows are only replay caches).
    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(
                Table::drop()
                    .table(IdempotencyKeys::Table)
                    .if_exists()
                    .to_owned(),
            )
            .await
    }
}
