//! Database operations for CI/CD pipelines.

use anyhow::{Context, Result};
use sea_orm::*;

use crate::entities::{pipeline, pipeline_stage, pipeline_job};

// ── Pipeline ops ─────────────────────────────────────────────────

/// Create a new pipeline record.
pub async fn create_pipeline(
    db: &DatabaseConnection,
    repo_id: i64,
    commit_sha: &str,
    ref_name: &str,
    trigger_type: &str,
    triggered_by: Option<i64>,
) -> Result<pipeline::Model> {
    let now = chrono::Utc::now().naive_utc();
    let model = pipeline::ActiveModel {
        repo_id: Set(repo_id),
        commit_sha: Set(commit_sha.to_string()),
        ref_name: Set(ref_name.to_string()),
        status: Set("pending".to_string()),
        trigger_type: Set(trigger_type.to_string()),
        triggered_by: Set(triggered_by),
        started_at: Set(None),
        finished_at: Set(None),
        created_at: Set(now),
        ..Default::default()
    };
    let result = model.insert(db).await.context("db: create pipeline")?;
    Ok(result)
}

/// Get a pipeline by ID.
pub async fn get_pipeline(db: &DatabaseConnection, id: i64) -> Result<Option<pipeline::Model>> {
    pipeline::Entity::find_by_id(id)
        .one(db)
        .await
        .context("db: get pipeline")
}

/// List pipelines for a repo.
pub async fn list_pipelines_by_repo(
    db: &DatabaseConnection,
    repo_id: i64,
) -> Result<Vec<pipeline::Model>> {
    pipeline::Entity::find()
        .filter(pipeline::Column::RepoId.eq(repo_id))
        .order_by_desc(pipeline::Column::CreatedAt)
        .all(db)
        .await
        .context("db: list pipelines by repo")
}

/// Update pipeline status.
pub async fn update_pipeline_status(
    db: &DatabaseConnection,
    id: i64,
    status: &str,
    started_at: Option<chrono::NaiveDateTime>,
    finished_at: Option<chrono::NaiveDateTime>,
) -> Result<()> {
    let model = pipeline::Entity::find_by_id(id)
        .one(db)
        .await
        .context("db: find pipeline for status update")?
        .ok_or_else(|| anyhow::anyhow!("pipeline {} not found", id))?;

    let mut active: pipeline::ActiveModel = model.into();
    active.status = Set(status.to_string());
    if started_at.is_some() {
        active.started_at = Set(started_at);
    }
    if finished_at.is_some() {
        active.finished_at = Set(finished_at);
    }
    active.update(db).await.context("db: update pipeline status")?;
    Ok(())
}

// ── Stage ops ────────────────────────────────────────────────────

/// Create a pipeline stage.
pub async fn create_stage(
    db: &DatabaseConnection,
    pipeline_id: i64,
    name: &str,
    stage_order: i32,
) -> Result<pipeline_stage::Model> {
    let model = pipeline_stage::ActiveModel {
        pipeline_id: Set(pipeline_id),
        name: Set(name.to_string()),
        stage_order: Set(stage_order),
        status: Set("pending".to_string()),
        started_at: Set(None),
        finished_at: Set(None),
        ..Default::default()
    };
    let result = model.insert(db).await.context("db: create stage")?;
    Ok(result)
}

/// Get stages for a pipeline.
pub async fn list_stages_by_pipeline(
    db: &DatabaseConnection,
    pipeline_id: i64,
) -> Result<Vec<pipeline_stage::Model>> {
    pipeline_stage::Entity::find()
        .filter(pipeline_stage::Column::PipelineId.eq(pipeline_id))
        .order_by_asc(pipeline_stage::Column::StageOrder)
        .all(db)
        .await
        .context("db: list stages by pipeline")
}

/// Update stage status.
pub async fn update_stage_status(
    db: &DatabaseConnection,
    id: i64,
    status: &str,
    started_at: Option<chrono::NaiveDateTime>,
    finished_at: Option<chrono::NaiveDateTime>,
) -> Result<()> {
    let model = pipeline_stage::Entity::find_by_id(id)
        .one(db)
        .await
        .context("db: find stage for status update")?
        .ok_or_else(|| anyhow::anyhow!("stage {} not found", id))?;

    let mut active: pipeline_stage::ActiveModel = model.into();
    active.status = Set(status.to_string());
    if started_at.is_some() {
        active.started_at = Set(started_at);
    }
    if finished_at.is_some() {
        active.finished_at = Set(finished_at);
    }
    active.update(db).await.context("db: update stage status")?;
    Ok(())
}

// ── Job ops ──────────────────────────────────────────────────────

/// Create a pipeline job.
pub async fn create_job(
    db: &DatabaseConnection,
    stage_id: i64,
    name: &str,
    script: &str,
    image: Option<&str>,
) -> Result<pipeline_job::Model> {
    let model = pipeline_job::ActiveModel {
        stage_id: Set(stage_id),
        name: Set(name.to_string()),
        script: Set(script.to_string()),
        image: Set(image.map(|s| s.to_string())),
        status: Set("pending".to_string()),
        exit_code: Set(None),
        log: Set(None),
        started_at: Set(None),
        finished_at: Set(None),
        ..Default::default()
    };
    let result = model.insert(db).await.context("db: create job")?;
    Ok(result)
}

/// Get a job by ID.
pub async fn get_job(db: &DatabaseConnection, id: i64) -> Result<Option<pipeline_job::Model>> {
    pipeline_job::Entity::find_by_id(id)
        .one(db)
        .await
        .context("db: get job")
}

/// List jobs for a stage.
pub async fn list_jobs_by_stage(
    db: &DatabaseConnection,
    stage_id: i64,
) -> Result<Vec<pipeline_job::Model>> {
    pipeline_job::Entity::find()
        .filter(pipeline_job::Column::StageId.eq(stage_id))
        .all(db)
        .await
        .context("db: list jobs by stage")
}

/// Update job result.
pub async fn update_job_result(
    db: &DatabaseConnection,
    id: i64,
    status: &str,
    exit_code: Option<i32>,
    log: Option<&str>,
    started_at: Option<chrono::NaiveDateTime>,
    finished_at: Option<chrono::NaiveDateTime>,
) -> Result<()> {
    let model = pipeline_job::Entity::find_by_id(id)
        .one(db)
        .await
        .context("db: find job for result update")?
        .ok_or_else(|| anyhow::anyhow!("job {} not found", id))?;

    let mut active: pipeline_job::ActiveModel = model.into();
    active.status = Set(status.to_string());
    active.exit_code = Set(exit_code);
    active.log = Set(log.map(|s| s.to_string()));
    if started_at.is_some() {
        active.started_at = Set(started_at);
    }
    if finished_at.is_some() {
        active.finished_at = Set(finished_at);
    }
    active.update(db).await.context("db: update job result")?;
    Ok(())
}

/// List all jobs for a pipeline (across all stages).
pub async fn list_jobs_by_pipeline(
    db: &DatabaseConnection,
    pipeline_id: i64,
) -> Result<Vec<pipeline_job::Model>> {
    // First get all stages for this pipeline
    let stages = pipeline_stage::Entity::find()
        .filter(pipeline_stage::Column::PipelineId.eq(pipeline_id))
        .all(db)
        .await
        .context("db: list stages for jobs")?;

    let stage_ids: Vec<i64> = stages.iter().map(|s| s.id).collect();
    if stage_ids.is_empty() {
        return Ok(Vec::new());
    }

    pipeline_job::Entity::find()
        .filter(pipeline_job::Column::StageId.is_in(stage_ids))
        .all(db)
        .await
        .context("db: list jobs by pipeline")
}
