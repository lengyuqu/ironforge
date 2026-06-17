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

use std::time::Duration;

use anyhow::{Context, Result};
use sea_orm::{ConnectOptions, ConnectionTrait, Database, Statement};
use sea_orm_migration::MigratorTrait;

pub use sea_orm;
pub use sea_orm::DatabaseConnection;

/// Default DB connect timeout (seconds).
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
pub async fn connect_with_timeouts(
    db_url: &str,
    connect_secs: u64,
    idle_secs: u64,
) -> Result<DatabaseConnection> {
    tracing::info!(url = %db_url, connect_secs, idle_secs, "Connecting to database");

    let mut opt = ConnectOptions::new(db_url.to_string());
    // SQLite only supports single-write connection.
    // Using max_connections=1 avoids SQLITE_BUSY errors from lock contention.
    opt.max_connections(1)
        .min_connections(1)
        .connect_timeout(Duration::from_secs(connect_secs))
        .idle_timeout(Duration::from_secs(idle_secs));

    let db = Database::connect(opt)
        .await
        .with_context(|| format!("failed to connect to database: {}", db_url))?;

    apply_pragmas(&db).await?;

    Ok(db)
}

/// Apply SQLite performance PRAGMAs for concurrent web server workloads.
async fn apply_pragmas(db: &DatabaseConnection) -> Result<()> {
    let backend = sea_orm::DatabaseBackend::Sqlite;

    let pragmas = [
        ("journal_mode", "WAL"),              // Write-Ahead Logging for better concurrency
        ("busy_timeout", "5000"),             // 5s busy wait instead of immediate SQLITE_BUSY
        ("synchronous", "NORMAL"),            // Good balance for WAL mode
        ("cache_size", "-64000"),             // 64MB page cache
        ("temp_store", "MEMORY"),             // Temp tables in RAM
        ("mmap_size", "268435456"),           // 256MB memory-mapped I/O
        ("foreign_keys", "ON"),               // Enforce FK constraints
    ];

    for (key, value) in &pragmas {
        let sql = format!("PRAGMA {} = {}", key, value);
        db.execute(Statement::from_string(backend, sql))
            .await
            .with_context(|| format!("failed to set PRAGMA {} = {}", key, value))?;
    }

    tracing::info!(
        pragmas = pragmas.len(),
        "Applied SQLite performance PRAGMAs"
    );

    Ok(())
}

/// Run all pending migrations.
pub async fn run_migrations(db: &DatabaseConnection) -> Result<()> {
    tracing::info!("Running database migrations");
    migrations::Migrator::up(db, None)
        .await
        .context("migration failed")?;
    Ok(())
}
