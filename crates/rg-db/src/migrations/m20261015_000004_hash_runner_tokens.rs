//! Hash any plaintext `runners.token` values left over from before the
//! token-storage change.
//!
//! Runner tokens used to be stored verbatim and looked up by equality.
//! `runner_ops::register_runner` now stores a SHA-256 digest and
//! `find_by_token` hashes the presented value before matching. Rows written
//! before that change still hold the plaintext token, so an already-registered
//! runner can no longer authenticate and would silently start failing.
//!
//! This migration hashes those rows in place so existing runners keep working.
//!
//! Distinguishing plaintext from a digest is unambiguous here: the old tokens
//! were 32-character UUID hex strings, while a SHA-256 hex digest is exactly 64
//! characters. Anything that is not 64 characters long is treated as plaintext.
//! Re-running is therefore a no-op.

use sea_orm::Statement;
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261015_000004_hash_runner_tokens"
    }
}

/// Length of a lowercase SHA-256 hex digest.
const DIGEST_LEN: usize = 64;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        if !manager.has_table("runners").await? {
            return Ok(());
        }
        hash_plaintext_tokens(manager).await
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // Hashing is intentionally one-way: the plaintext is not recoverable,
        // so there is nothing to restore. A runner that needs its token back
        // must re-register.
        Ok(())
    }
}

/// Hash every row whose stored token is still plaintext.
///
/// The digest is computed in Rust rather than in SQL: SQLite has no built-in
/// SHA-256 (`sha256()` / `SHA2()` are PostgreSQL/MySQL extensions), so doing it
/// here keeps one implementation across all three backends. The runner table is
/// small (one row per agent), so reading it in full is cheap.
async fn hash_plaintext_tokens(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    let connection = manager.get_connection();
    let backend = manager.get_database_backend();

    let rows = connection
        .query_all(Statement::from_string(
            backend,
            "SELECT id, token FROM runners".to_string(),
        ))
        .await?;

    let mut hashed = 0usize;
    for row in &rows {
        let Ok(id) = row.try_get_by_index::<i64>(0) else {
            continue;
        };
        let Ok(token) = row.try_get_by_index::<String>(1) else {
            continue;
        };
        // Already a digest — leave it alone so a re-run is a no-op.
        if token.len() == DIGEST_LEN {
            continue;
        }
        let digest = crate::ops::runner_ops::hash_token(&token);
        connection
            .execute_unprepared(&format!(
                "UPDATE runners SET token = '{}' WHERE id = {}",
                digest, id
            ))
            .await?;
        hashed += 1;
    }

    if hashed > 0 {
        tracing::info!(
            hashed,
            "hashed previously-plaintext runner tokens; existing runners keep working"
        );
    }
    Ok(())
}