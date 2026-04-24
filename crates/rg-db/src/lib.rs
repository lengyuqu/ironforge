//! IronForge database layer using SeaORM.

use anyhow::Result;

/// Initialize the database connection.
/// In Phase 1, this is a stub that will be implemented with actual SeaORM.
pub async fn init_database(db_url: &str) -> Result<()> {
    tracing::info!(url = %db_url, "Initializing database (stub)");
    // TODO: implement with SeaORM
    Ok(())
}
