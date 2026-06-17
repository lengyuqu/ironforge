//! IronForge database layer — SeaORM + SQLite.
//!
//! # Usage
//!
//! ```rust,no_run
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     let db = rg_db::connect("sqlite:///tmp/ironforge/ironforge.db?mode=rwc").await?;
//!     rg_db::run_migrations(&db).await?;
//!     Ok(())
//! }
//! ```

pub mod entities;
pub mod migrations;
pub mod ops;

use std::str::FromStr;
use std::time::Duration;

use anyhow::{Context, Result};
use sea_orm::sqlx::sqlite::{
    SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous,
};
use sea_orm::SqlxSqliteConnector;
use sea_orm_migration::MigratorTrait;

pub use sea_orm;
pub use sea_orm::DatabaseConnection;

/// Default DB connect (acquire) timeout (seconds).
pub const DEFAULT_CONNECT_TIMEOUT_SECS: u64 = 10;
/// Default DB idle timeout (seconds).
pub const DEFAULT_IDLE_TIMEOUT_SECS: u64 = 600;

/// Connect to the SQLite database with default pool timeouts.
/// URL example: `sqlite:///path/to/db?mode=rwc`
pub async fn connect(db_url: &str) -> Result<DatabaseConnection> {
    connect_with_timeouts(db_url, DEFAULT_CONNECT_TIMEOUT_SECS, DEFAULT_IDLE_TIMEOUT_SECS).await
}

/// Connect to the SQLite database with configurable connect/idle timeouts plus
/// connection pool tuning and PRAGMA optimization.
///
/// PRAGMAs are attached to the sqlx [`SqliteConnectOptions`] so they are applied
/// to **every** physical connection the pool opens — including reconnections
/// after an idle timeout. (The previous approach ran the PRAGMAs once after
/// connect, which silently lost per-connection settings such as `foreign_keys`
/// and `busy_timeout` whenever the pool re-established a connection.)
pub async fn connect_with_timeouts(
    db_url: &str,
    connect_secs: u64,
    idle_secs: u64,
) -> Result<DatabaseConnection> {
    tracing::info!(url = %db_url, connect_secs, idle_secs, "Connecting to database");

    // Per-connection options — applied on every connect by sqlx.
    let conn_opts = SqliteConnectOptions::from_str(db_url)
        .with_context(|| format!("invalid sqlite url: {db_url}"))?
        .create_if_missing(true)                 // honour `?mode=rwc`
        .journal_mode(SqliteJournalMode::Wal)    // WAL for reader/writer concurrency
        .synchronous(SqliteSynchronous::Normal)  // good balance for WAL
        .busy_timeout(Duration::from_secs(5))    // wait instead of immediate SQLITE_BUSY
        .foreign_keys(true)                      // enforce FK constraints
        .pragma("cache_size", "-64000")          // 64MB page cache
        .pragma("temp_store", "MEMORY")          // temp tables in RAM
        .pragma("mmap_size", "268435456");       // 256MB memory-mapped I/O

    // SQLite serialises writers; keep a single connection to avoid lock
    // contention while WAL + busy_timeout absorb brief overlaps.
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .min_connections(1)
        .acquire_timeout(Duration::from_secs(connect_secs))
        .idle_timeout(Duration::from_secs(idle_secs))
        .connect_with(conn_opts)
        .await
        .with_context(|| format!("failed to connect to database: {db_url}"))?;

    tracing::info!("Applied SQLite PRAGMAs via per-connection options");
    Ok(SqlxSqliteConnector::from_sqlx_sqlite_pool(pool))
}

/// Run all pending migrations.
pub async fn run_migrations(db: &DatabaseConnection) -> Result<()> {
    tracing::info!("Running database migrations");
    migrations::Migrator::up(db, None)
        .await
        .context("migration failed")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};

    /// Regression guard: every connection handed out by the pool must have the
    /// per-connection PRAGMAs applied. A `connect()` that loses `foreign_keys`
    /// (e.g. by running PRAGMAs once instead of per-connection) silently
    /// disables FK enforcement — this asserts it stays on.
    #[tokio::test]
    async fn connect_applies_per_connection_pragmas() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!("ironforge_pragma_test_{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let url = format!("sqlite://{}?mode=rwc", path.display());

        let db = connect(&url).await.expect("connect");

        let row = db
            .query_one(Statement::from_string(
                DatabaseBackend::Sqlite,
                "PRAGMA foreign_keys".to_string(),
            ))
            .await
            .expect("query foreign_keys")
            .expect("one row");
        let fk: i32 = row.try_get_by_index(0).expect("fk value");
        assert_eq!(fk, 1, "foreign_keys must be ON for every connection");

        let row = db
            .query_one(Statement::from_string(
                DatabaseBackend::Sqlite,
                "PRAGMA journal_mode".to_string(),
            ))
            .await
            .expect("query journal_mode")
            .expect("one row");
        let mode: String = row.try_get_by_index(0).expect("journal mode value");
        assert_eq!(mode.to_lowercase(), "wal", "journal_mode must be WAL");

        let _ = std::fs::remove_file(&path);
    }
}
