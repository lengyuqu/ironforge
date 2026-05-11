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

/// Paginated list of pipelines for a repo. Returns (data, total).
pub async fn list_pipelines_by_repo_paginated(
    db: &DatabaseConnection,
    repo_id: i64,
    offset: u64,
    limit: u64,
) -> Result<(Vec<pipeline::Model>, i64)> {
    let base = pipeline::Entity::find()
        .filter(pipeline::Column::RepoId.eq(repo_id))
        .order_by_desc(pipeline::Column::CreatedAt);

    let total = base.clone().count(db).await.context("db: count pipelines by repo")? as i64;
    let pipelines = base
        .offset(offset)
        .limit(limit)
        .all(db)
        .await
        .context("db: list pipelines by repo (paginated)")?;

    Ok((pipelines, total))
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
    if log.is_some() {
        active.log = Set(log.map(|s| s.to_string()));
    }
    if started_at.is_some() {
        active.started_at = Set(started_at);
    }
    if finished_at.is_some() {
        active.finished_at = Set(finished_at);
    }
    active.update(db).await.context("db: update job result")?;
    Ok(())
}

/// Get a stage by ID.
pub async fn get_stage_by_id(
    db: &DatabaseConnection,
    id: i64,
) -> Result<Option<pipeline_stage::Model>> {
    pipeline_stage::Entity::find_by_id(id)
        .one(db)
        .await
        .context("db: get stage by id")
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

/// Find the latest pipeline for a repo + commit SHA.
/// Used by branch protection status checks to verify CI passed.
pub async fn find_latest_by_repo_and_commit(
    db: &DatabaseConnection,
    repo_id: i64,
    commit_sha: &str,
) -> Result<Option<pipeline::Model>> {
    pipeline::Entity::find()
        .filter(pipeline::Column::RepoId.eq(repo_id))
        .filter(pipeline::Column::CommitSha.eq(commit_sha))
        .order_by_desc(pipeline::Column::CreatedAt)
        .limit(1)
        .one(db)
        .await
        .context("db: find latest pipeline by repo and commit")
}

// ── Status cascade helpers ──────────────────────────
// After a job finishes, check if its stage is done; if so, update stage status.
// After a stage finishes, check if all stages in the pipeline are done; if so, update pipeline status.

/// Check if all jobs in a stage are finished.
/// Returns (all_done, any_failure).
pub async fn check_stage_jobs(
    db: &DatabaseConnection,
    stage_id: i64,
) -> Result<(bool, bool)> {
    let jobs = list_jobs_by_stage(db, stage_id).await?;
    if jobs.is_empty() {
        return Ok((true, false));
    }
    let all_done = jobs.iter().all(|j| j.status == "success" || j.status == "failure" || j.status == "error");
    let any_failure = jobs.iter().any(|j| j.status == "failure" || j.status == "error");
    Ok((all_done, any_failure))
}

/// After a job finishes, update stage status if all jobs in the stage are done.
/// Returns the new stage status if updated, or None if not all done.
pub async fn try_update_stage(
    db: &DatabaseConnection,
    stage_id: i64,
) -> Result<Option<String>> {
    let (all_done, any_failure) = check_stage_jobs(db, stage_id).await?;
    if !all_done {
        return Ok(None);
    }
    let new_status = if any_failure { "failure" } else { "success" };
    let now = Some(chrono::Utc::now().naive_utc());
    update_stage_status(db, stage_id, new_status, None, now).await?;
    Ok(Some(new_status.to_string()))
}

/// Check if all stages in a pipeline are done.
/// Returns (all_done, any_failure).
pub async fn check_pipeline_stages(
    db: &DatabaseConnection,
    pipeline_id: i64,
) -> Result<(bool, bool)> {
    let stages = list_stages_by_pipeline(db, pipeline_id).await?;
    if stages.is_empty() {
        return Ok((true, false));
    }
    let all_done = stages.iter().all(|s| s.status == "success" || s.status == "failure");
    let any_failure = stages.iter().any(|s| s.status == "failure");
    Ok((all_done, any_failure))
}

/// After a stage finishes, update pipeline status if all stages are done.
pub async fn try_update_pipeline(
    db: &DatabaseConnection,
    pipeline_id: i64,
) -> Result<Option<String>> {
    let (all_done, any_failure) = check_pipeline_stages(db, pipeline_id).await?;
    if !all_done {
        return Ok(None);
    }
    let new_status = if any_failure { "failure" } else { "success" };
    let now = Some(chrono::Utc::now().naive_utc());
    update_pipeline_status(db, pipeline_id, new_status, None, now).await?;
    Ok(Some(new_status.to_string()))
}

/// Find a pending job (status = "pending" and runner_id is NULL).
/// Returns the oldest pending job (by id).
pub async fn find_pending_job(db: &DatabaseConnection) -> Result<Option<pipeline_job::Model>> {
    pipeline_job::Entity::find()
        .filter(pipeline_job::Column::Status.eq("pending"))
        .filter(pipeline_job::Column::RunnerId.is_null())
        .order_by_asc(pipeline_job::Column::Id)
        .one(db)
        .await
        .context("db: find pending job")
}

/// Assign a job to a runner.
pub async fn assign_job(db: &DatabaseConnection, job_id: i64, runner_id: i64) -> Result<()> {
    let now = chrono::Utc::now().naive_utc();
    let model = pipeline_job::Entity::find_by_id(job_id)
        .one(db)
        .await
        .context("db: find job for assign")?
        .ok_or_else(|| anyhow::anyhow!("job {} not found", job_id))?;

    let mut active: pipeline_job::ActiveModel = model.into();
    active.status = Set("assigned".to_string());
    active.runner_id = Set(Some(runner_id));
    active.updated_at = Set(Some(now));
    active.update(db).await.context("db: assign job")?;
    Ok(())
}
