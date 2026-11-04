//! Platform-recorded CI facts that collaboration records may point at.
//!
//! Only `pipelines` rows count: they are written by the CI engine, not by the
//! account that talks to the collaboration API. Commit statuses are *not* used
//! as evidence because any writer of the repository — including the agent whose
//! work is being judged — can post one.

use chrono::{DateTime, Utc};
use rg_db::ops::pipeline_ops;
use sea_orm::DatabaseConnection;
use serde::Serialize;

use super::error::CollabResult;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct PipelineEvidence {
    /// `verified_reference` (found in this repository) or `unavailable`.
    pub verification: &'static str,
    pub pipeline_id: i64,
    /// `pending` | `running` | `success` | `failed` | `canceled`.
    pub status: Option<String>,
    /// The commit the pipeline actually ran against.
    pub commit_sha: Option<String>,
    pub ref_name: Option<String>,
    pub finished_at: Option<DateTime<Utc>>,
    pub resolved_at: DateTime<Utc>,
}

/// Look a pipeline up by id. A pipeline of another repository reads as
/// `unavailable`, exactly like a missing one.
pub async fn resolve_pipeline(
    db: &DatabaseConnection,
    repo_id: i64,
    pipeline_id: i64,
    now: DateTime<Utc>,
) -> CollabResult<PipelineEvidence> {
    let row = pipeline_ops::get_pipeline(db, pipeline_id)
        .await?
        .filter(|p| p.repo_id == repo_id);
    Ok(match row {
        Some(p) => PipelineEvidence {
            verification: "verified_reference",
            pipeline_id,
            status: Some(p.status),
            commit_sha: Some(p.commit_sha),
            ref_name: Some(p.ref_name),
            finished_at: p.finished_at.map(|t| t.and_utc()),
            resolved_at: now,
        },
        None => PipelineEvidence {
            verification: "unavailable",
            pipeline_id,
            status: None,
            commit_sha: None,
            ref_name: None,
            finished_at: None,
            resolved_at: now,
        },
    })
}
