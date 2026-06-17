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
/// Default max pool connections.
///
/// In WAL mode SQLite allows many concurrent readers alongside a single
/// writer, so a small pool lets read-heavy API/UI traffic run in parallel
/// instead of serialising on one connection. Writers still serialise at the
/// SQLite level; `busy_timeout` (set per connection) absorbs brief overlaps.
pub const DEFAULT_MAX_CONNECTIONS: u32 = 5;

/// Connect to the SQLite database with default pool timeouts and pool size.
/// URL example: `sqlite:///path/to/db?mode=rwc`
pub async fn connect(db_url: &str) -> Result<DatabaseConnection> {
    connect_with_timeouts(db_url, DEFAULT_CONNECT_TIMEOUT_SECS, DEFAULT_IDLE_TIMEOUT_SECS).await
}

/// Connect with configurable connect/idle timeouts and the default pool size.
pub async fn connect_with_timeouts(
    db_url: &str,
    connect_secs: u64,
    idle_secs: u64,
) -> Result<DatabaseConnection> {
    connect_with_pool(db_url, connect_secs, idle_secs, DEFAULT_MAX_CONNECTIONS).await
}

/// Connect to the SQLite database with full pool control plus PRAGMA optimization.
///
/// PRAGMAs are attached to the sqlx [`SqliteConnectOptions`] so they are applied
/// to **every** physical connection the pool opens — including reconnections
/// after an idle timeout. (The previous approach ran the PRAGMAs once after
/// connect, which silently lost per-connection settings such as `foreign_keys`
/// and `busy_timeout` whenever the pool re-established a connection.)
pub async fn connect_with_pool(
    db_url: &str,
    connect_secs: u64,
    idle_secs: u64,
    max_connections: u32,
) -> Result<DatabaseConnection> {
    let max_connections = max_connections.max(1);
    tracing::info!(url = %db_url, connect_secs, idle_secs, max_connections, "Connecting to database");

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

    // WAL allows concurrent readers; writers serialise (busy_timeout absorbs
    // brief overlaps). Keep min_connections low to stay light on idle.
    let pool = SqlitePoolOptions::new()
        .max_connections(max_connections)
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

    /// Stands in for a load test: hammer the multi-connection pool with
    /// concurrent writers and readers and assert nothing errors and no write
    /// is lost. Proves WAL + per-connection busy_timeout safely absorb the
    /// write contention enabled by `max_connections > 1`.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_reads_and_writes_do_not_error() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!("ironforge_concurrency_test_{}.db", std::process::id()));
        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{}", path.display(), suffix));
        }
        let url = format!("sqlite://{}?mode=rwc", path.display());

        let db = connect(&url).await.expect("connect");
        db.execute(Statement::from_string(
            DatabaseBackend::Sqlite,
            "CREATE TABLE t (id INTEGER PRIMARY KEY, n INTEGER NOT NULL)".to_string(),
        ))
        .await
        .expect("create table");

        const TASKS: i64 = 32;
        let mut handles = Vec::new();
        for n in 0..TASKS {
            let db = db.clone();
            handles.push(tokio::spawn(async move {
                // Write
                db.execute(Statement::from_string(
                    DatabaseBackend::Sqlite,
                    format!("INSERT INTO t (n) VALUES ({n})"),
                ))
                .await
                .expect("concurrent insert should not error");
                // Read
                db.query_one(Statement::from_string(
                    DatabaseBackend::Sqlite,
                    "SELECT COUNT(*) FROM t".to_string(),
                ))
                .await
                .expect("concurrent read should not error");
            }));
        }
        for h in handles {
            h.await.expect("task panicked");
        }

        let row = db
            .query_one(Statement::from_string(
                DatabaseBackend::Sqlite,
                "SELECT COUNT(*) FROM t".to_string(),
            ))
            .await
            .expect("final count")
            .expect("one row");
        let count: i64 = row.try_get_by_index(0).expect("count value");
        assert_eq!(count, TASKS, "every concurrent write must be persisted");

        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{}", path.display(), suffix));
        }
    }
}
