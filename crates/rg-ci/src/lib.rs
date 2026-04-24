//! IronForge CI/CD engine (stub).
//!
//! Will be implemented in a future phase.

use anyhow::Result;

/// Run a CI pipeline (stub).
pub async fn run_pipeline(_repo_path: &str, _ref_name: &str) -> Result<()> {
    tracing::info!("CI pipeline triggered (stub)");
    // TODO: implement CI/CD pipeline
    Ok(())
}
