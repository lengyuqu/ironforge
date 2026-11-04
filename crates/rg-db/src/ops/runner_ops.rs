//! Database operations for runners.

use anyhow::{Context, Result};
use chrono::Utc;
use sea_orm::sea_query::Expr;
use sea_orm::*;

use crate::entities::runner::{ActiveModel, Column, Entity as RunnerEntity, Model as Runner};

/// A freshly registered runner together with its one-time plaintext token.
///
/// The token is stored **hashed**; the plaintext is returned exactly once, here,
/// and can never be recovered from the database afterwards.
pub struct RegisteredRunner {
    pub runner: Runner,
    pub token: String,
}

/// Hash a runner token for storage/lookup.
///
/// Tokens are high-entropy random values, so a plain SHA-256 is sufficient (no
/// password-style KDF is needed) and keeps lookup a single indexed query.
pub fn hash_token(token: &str) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(token.as_bytes()))
}

/// Register a new runner.
///
/// Generates a unique token, stores only its hash, and returns the plaintext
/// once so the caller can hand it to the runner agent.
pub async fn register_runner(
    db: &DatabaseConnection,
    name: &str,
    labels: &str,
    version: Option<&str>,
    os: Option<&str>,
    arch: Option<&str>,
) -> Result<RegisteredRunner> {
    let now = Utc::now();
    let token = generate_token();

    let active_model = ActiveModel {
        id: NotSet,
        name: Set(name.to_string()),
        // Only the digest is persisted; a database leak no longer exposes
        // usable runner credentials.
        token: Set(hash_token(&token)),
        status: Set("offline".to_string()),
        labels: Set(labels.to_string()),
        last_seen_at: Set(now),
        version: Set(version.map(|v| v.to_string())),
        os: Set(os.map(|v| v.to_string())),
        arch: Set(arch.map(|v| v.to_string())),
        created_at: Set(now),
        updated_at: Set(now),
    };

    let runner = active_model.insert(db).await.context("db: register runner")?;
    Ok(RegisteredRunner { runner, token })
}

/// Update runner heartbeat (last_seen_at).
pub async fn update_heartbeat(db: &DatabaseConnection, runner_id: i64) -> Result<()> {
    let now = Utc::now();

    RunnerEntity::update_many()
        .col_expr(Column::LastSeenAt, Expr::value(now))
        .col_expr(Column::UpdatedAt, Expr::value(now))
        .filter(Column::Id.eq(runner_id))
        .exec(db)
        .await
        .context("db: update runner heartbeat")?;

    Ok(())
}

/// Update runner status.
pub async fn update_status(db: &DatabaseConnection, runner_id: i64, status: &str) -> Result<()> {
    let now = Utc::now().naive_utc();

    RunnerEntity::update_many()
        .col_expr(Column::Status, Expr::value(status.to_string()))
        .col_expr(Column::UpdatedAt, Expr::value(now))
        .filter(Column::Id.eq(runner_id))
        .exec(db)
        .await
        .context("db: update runner status")?;

    Ok(())
}

/// Find a runner by ID.
pub async fn find_by_id(db: &DatabaseConnection, id: i64) -> Result<Option<Runner>> {
    RunnerEntity::find_by_id(id)
        .one(db)
        .await
        .context("db: find runner by id")
}

/// Find a runner by a presented (plaintext) token.
///
/// The stored value is a digest, so the lookup hashes the input first. Callers
/// must still compare the returned runner's id against the id in the request
/// path — a token identifies one runner.
pub async fn find_by_token(db: &DatabaseConnection, token: &str) -> Result<Option<Runner>> {
    RunnerEntity::find()
        .filter(Column::Token.eq(hash_token(token)))
        .one(db)
        .await
        .context("db: find runner by token")
}

/// List online runners (last_seen_at within 90 seconds).
pub async fn list_online_runners(db: &DatabaseConnection) -> Result<Vec<Runner>> {
    let cutoff = Utc::now().naive_utc() - chrono::Duration::seconds(90);

    RunnerEntity::find()
        .filter(Column::LastSeenAt.gt(cutoff))
        .all(db)
        .await
        .context("db: list online runners")
}

/// List all runners (for admin).
pub async fn list_all(db: &DatabaseConnection) -> Result<Vec<Runner>> {
    RunnerEntity::find()
        .order_by_desc(Column::LastSeenAt)
        .all(db)
        .await
        .context("db: list all runners")
}

/// Delete a runner by ID.
pub async fn delete_runner(db: &DatabaseConnection, runner_id: i64) -> Result<bool> {
    let result = RunnerEntity::delete_by_id(runner_id).exec(db).await?;
    Ok(result.rows_affected > 0)
}

/// Generate a unique token for runner authentication.
fn generate_token() -> String {
    // Use UUID v4 to generate a unique token (36 chars with hyphens)
    // Remove hyphens to get 32-char token
    uuid::Uuid::new_v4().to_string().replace('-', "")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_hash_is_stable_and_not_the_token() {
        let hash = hash_token("abc123");
        assert_eq!(hash, hash_token("abc123"), "lookup must be deterministic");
        assert_ne!(hash, "abc123", "the stored value must not be the token");
        assert_eq!(hash.len(), 64, "sha-256 hex digest");
        assert_ne!(hash_token("abc123"), hash_token("abc124"));
    }
}