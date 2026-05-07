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

use anyhow::{Context, Result};
use sea_orm::{Database, DatabaseConnection};
use sea_orm_migration::MigratorTrait;

pub use sea_orm;

/// Connect to the SQLite database.  URL example: `sqlite:///path/to/db?mode=rwc`
pub async fn connect(db_url: &str) -> Result<DatabaseConnection> {
    tracing::info!(url = %db_url, "Connecting to database");
    let db = Database::connect(db_url)
        .await
        .with_context(|| format!("failed to connect to database: {}", db_url))?;
    Ok(db)
}

/// Run all pending migrations.
pub async fn run_migrations(db: &DatabaseConnection) -> Result<()> {
    tracing::info!("Running database migrations");
    migrations::Migrator::up(db, None)
        .await
        .context("migration failed")?;
    Ok(())
}
