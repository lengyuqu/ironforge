//! Audit log archival — periodically exports old audit logs to NDJSON
//! files and purges them from the database.

use chrono::{Duration, Utc};
use sea_orm::DatabaseConnection;
use std::path::PathBuf;
use tokio::io::AsyncWriteExt;
use tokio::time;

/// How often the archive task runs.
const ARCHIVE_INTERVAL_MINUTES: i64 = 60;

/// Age after which audit logs are archived (default: 90 days).
const ARCHIVE_AFTER_DAYS: i64 = 90;

/// Start the background audit log archive task.
pub fn spawn_archiver(db: DatabaseConnection, archive_dir: PathBuf) {
    tokio::spawn(async move {
        let mut interval = time::interval(time::Duration::from_secs(
            (ARCHIVE_INTERVAL_MINUTES * 60) as u64,
        ));
        loop {
            interval.tick().await;
            if let Err(e) = run_archive(&db, &archive_dir).await {
                tracing::warn!(error = %e, "audit log archive run failed");
            }
        }
    });
}

async fn run_archive(db: &DatabaseConnection, archive_dir: &PathBuf) -> anyhow::Result<()> {
    let cutoff = Utc::now() - Duration::days(ARCHIVE_AFTER_DAYS);

    // Find old entries
    let old_entries = rg_db::ops::audit_log_ops::list_before(db, cutoff).await?;
    if old_entries.is_empty() {
        return Ok(());
    }

    // Ensure archive directory exists
    tokio::fs::create_dir_all(archive_dir).await?;

    // Write to NDJSON file
    let filename = format!("audit-{}.ndjson", cutoff.format("%Y%m%d"));
    let path = archive_dir.join(&filename);
    let mut file = tokio::fs::File::create(&path).await?;

    for entry in &old_entries {
        let line = serde_json::to_string(entry)?;
        file.write_all(line.as_bytes()).await?;
        file.write_all(b"\n").await?;
    }
    file.flush().await?;

    tracing::info!(
        count = old_entries.len(),
        path = %path.display(),
        "archived audit logs"
    );

    // Delete archived entries from DB
    let ids: Vec<i64> = old_entries.iter().map(|e| e.id).collect();
    rg_db::ops::audit_log_ops::delete_by_ids(db, &ids).await?;

    Ok(())
}
