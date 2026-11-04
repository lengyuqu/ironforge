//! Database operations for CI/CD pipelines.

use anyhow::{Context, Result};
use sea_orm::sea_query::Expr;
use sea_orm::*;

use crate::entities::{pipeline, pipeline_job, pipeline_stage};

// Earlier PostgreSQL migrations created these columns as INTEGER, while the
// public model uses i64. Explicit casts let both INTEGER and BIGINT installations
// decode the same way without changing an already-applied migration.
const PG_PIPELINE_COLUMNS: &str = "id::bigint AS id, repo_id::bigint AS repo_id, commit_sha, ref_name, status, trigger_type, triggered_by::bigint AS triggered_by, started_at, finished_at, created_at";

// Project integer-backed identifiers as bigint before SeaORM decodes i64 fields.
fn stage_query<C: ConnectionTrait>(db: &C) -> Select<pipeline_stage::Entity> {
    let mut query = pipeline_stage::Entity::find();
    if db.get_database_backend() == DatabaseBackend::Postgres {
        query = query.select_only();
        for column in pipeline_stage::Column::iter() {
            query = if matches!(
                column,
                pipeline_stage::Column::Id | pipeline_stage::Column::PipelineId
            ) {
                query.column_as(
                    Expr::col(column).cast_as(sea_orm::sea_query::Alias::new("bigint")),
                    column.to_string(),
                )
            } else {
                query.column(column)
            };
        }
    }
    query
}

fn job_query<C: ConnectionTrait>(db: &C) -> Select<pipeline_job::Entity> {
    let mut query = pipeline_job::Entity::find();
    let backend = db.get_database_backend();
    if backend == DatabaseBackend::Postgres || backend == DatabaseBackend::MySql {
        query = query.select_only();
        for column in pipeline_job::Column::iter() {
            query = if matches!(column, pipeline_job::Column::UpdatedAt) {
                let expression = if backend == DatabaseBackend::Postgres {
                    "updated_at AT TIME ZONE 'UTC'"
                } else {
                    "CAST(updated_at AS DATETIME)"
                };
                query.expr_as(Expr::cust(expression), column.to_string())
            } else if backend == DatabaseBackend::Postgres
                && matches!(
                    column,
                    pipeline_job::Column::Id
                        | pipeline_job::Column::StageId
                        | pipeline_job::Column::RunnerId
                        | pipeline_job::Column::TimeoutSeconds
                        | pipeline_job::Column::EnvironmentId
                )
            {
                query.column_as(
                    Expr::col(column).cast_as(sea_orm::sea_query::Alias::new("bigint")),
                    column.to_string(),
                )
            } else {
                query.column(column)
            };
        }
    }
    query
}

const PG_STAGE_COLUMNS: &str = "id::bigint AS id, pipeline_id::bigint AS pipeline_id, name, stage_order, status, started_at, finished_at";
const PG_JOB_COLUMNS: &str = "id::bigint AS id, stage_id::bigint AS stage_id, runner_id::bigint AS runner_id, name, image, script, variables, cache_key, cache_paths, allow_failure, timeout_seconds::bigint AS timeout_seconds, when_condition, if_condition, environment_id::bigint AS environment_id, environment_name, status, exit_code, log, started_at, finished_at, updated_at AT TIME ZONE 'UTC' AS updated_at, tags";

// The entity represents updated_at as a UTC NaiveDateTime, while PostgreSQL
// stores an instant. Bind UTC explicitly instead of using the session timezone.
fn job_updated_at_value<C: ConnectionTrait>(
    db: &C,
    value: chrono::NaiveDateTime,
) -> sea_orm::Value {
    if db.get_database_backend() == DatabaseBackend::Postgres {
        value.and_utc().into()
    } else {
        value.into()
    }
}

/// SQLite deferred transactions cannot upgrade a stale read snapshot after a
/// concurrent writer commits. Acquire its writer lock before any reads; the
/// predicate matches no rows, so it neither mutates data nor fires row triggers.
async fn lock_sqlite_ci_writer(txn: &DatabaseTransaction) -> Result<()> {
    if txn.get_database_backend() == DatabaseBackend::Sqlite {
        txn.execute_unprepared("UPDATE pipeline_stages SET id = id WHERE 0")
            .await
            .context("db: acquire SQLite CI writer lock")?;
    }
    Ok(())
}

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
    if db.get_database_backend() == DatabaseBackend::Postgres {
        let sql = format!("INSERT INTO pipelines (repo_id, commit_sha, ref_name, status, trigger_type, triggered_by, created_at) VALUES ($1, $2, $3, 'pending', $4, $5, $6) RETURNING {PG_PIPELINE_COLUMNS}");
        return pipeline::Model::find_by_statement(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            sql,
            [
                repo_id.into(),
                commit_sha.into(),
                ref_name.into(),
                trigger_type.into(),
                triggered_by.into(),
                now.into(),
            ],
        ))
        .one(db)
        .await
        .context("db: create pipeline")?
        .context("db: inserted pipeline was not returned");
    }
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
async fn get_pipeline_on<C: ConnectionTrait>(db: &C, id: i64) -> Result<Option<pipeline::Model>> {
    if db.get_database_backend() == DatabaseBackend::Postgres {
        return pipeline::Model::find_by_statement(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            format!("SELECT {PG_PIPELINE_COLUMNS} FROM pipelines WHERE id = $1"),
            [id.into()],
        ))
        .one(db)
        .await
        .context("db: get pipeline");
    }
    pipeline::Entity::find_by_id(id)
        .one(db)
        .await
        .context("db: get pipeline")
}

/// Pipelines of a repository that ran against exactly `commit_sha`, newest
/// (highest id) first.
pub async fn list_by_repo_and_commit(
    db: &DatabaseConnection,
    repo_id: i64,
    commit_sha: &str,
    limit: u64,
) -> Result<Vec<pipeline::Model>> {
    use sea_orm::{ColumnTrait, QueryFilter, QueryOrder, QuerySelect};
    if db.get_database_backend() == DatabaseBackend::Postgres {
        return pipeline::Model::find_by_statement(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            format!("SELECT {PG_PIPELINE_COLUMNS} FROM pipelines WHERE repo_id = $1 AND commit_sha = $2 ORDER BY id DESC LIMIT $3"),
            [repo_id.into(), commit_sha.into(), (limit.min(i64::MAX as u64) as i64).into()],
        )).all(db).await.context("db: list pipelines by commit");
    }
    pipeline::Entity::find()
        .filter(pipeline::Column::RepoId.eq(repo_id))
        .filter(pipeline::Column::CommitSha.eq(commit_sha))
        .order_by_desc(pipeline::Column::Id)
        .limit(limit)
        .all(db)
        .await
        .context("db: list pipelines by commit")
}

/// List pipelines for a repo.
pub async fn list_pipelines_by_repo(
    db: &DatabaseConnection,
    repo_id: i64,
) -> Result<Vec<pipeline::Model>> {
    if db.get_database_backend() == DatabaseBackend::Postgres {
        return pipeline::Model::find_by_statement(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            format!("SELECT {PG_PIPELINE_COLUMNS} FROM pipelines WHERE repo_id = $1 ORDER BY created_at DESC"),
            [repo_id.into()],
        )).all(db).await.context("db: list pipelines by repo");
    }
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
    if db.get_database_backend() == DatabaseBackend::Postgres {
        let total = pipeline::Entity::find()
            .filter(pipeline::Column::RepoId.eq(repo_id))
            .count(db)
            .await
            .context("db: count pipelines by repo")? as i64;
        let pipelines = pipeline::Model::find_by_statement(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            format!("SELECT {PG_PIPELINE_COLUMNS} FROM pipelines WHERE repo_id = $1 ORDER BY created_at DESC LIMIT $2 OFFSET $3"),
            [repo_id.into(), (limit.min(i64::MAX as u64) as i64).into(), (offset.min(i64::MAX as u64) as i64).into()],
        )).all(db).await.context("db: list pipelines by repo (paginated)")?;
        return Ok((pipelines, total));
    }
    let base = pipeline::Entity::find()
        .filter(pipeline::Column::RepoId.eq(repo_id))
        .order_by_desc(pipeline::Column::CreatedAt);

    let total = base
        .clone()
        .count(db)
        .await
        .context("db: count pipelines by repo")? as i64;
    let pipelines = base
        .offset(offset)
        .limit(limit)
        .all(db)
        .await
        .context("db: list pipelines by repo (paginated)")?;

    Ok((pipelines, total))
}

/// Update pipeline status.
async fn update_pipeline_status_on<C: ConnectionTrait>(
    db: &C,
    id: i64,
    status: &str,
    started_at: Option<chrono::NaiveDateTime>,
    finished_at: Option<chrono::NaiveDateTime>,
) -> Result<()> {
    if db.get_database_backend() == DatabaseBackend::Postgres {
        let mut update = pipeline::Entity::update_many()
            .col_expr(pipeline::Column::Status, Expr::value(status))
            .filter(pipeline::Column::Id.eq(id));
        if let Some(at) = started_at {
            update = update.col_expr(pipeline::Column::StartedAt, Expr::value(at));
        }
        if let Some(at) = finished_at {
            update = update.col_expr(pipeline::Column::FinishedAt, Expr::value(at));
        }
        let result = update
            .exec(db)
            .await
            .context("db: update pipeline status")?;
        if result.rows_affected == 0 {
            anyhow::bail!("pipeline {} not found", id);
        }
        return Ok(());
    }
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
    active
        .update(db)
        .await
        .context("db: update pipeline status")?;
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
    if db.get_database_backend() == DatabaseBackend::Postgres {
        let txn = db.begin().await.context("db: begin create stage")?;
        let stage = pipeline_stage::Model::find_by_statement(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            format!("INSERT INTO pipeline_stages (pipeline_id, name, stage_order, status, started_at, finished_at) VALUES ($1, $2, $3, 'pending', NULL, NULL) RETURNING {PG_STAGE_COLUMNS}"),
            [pipeline_id.into(), name.into(), stage_order.into()],
        )).one(&txn).await.context("db: create stage")?.context("db: inserted stage was not returned")?;
        txn.commit().await.context("db: commit create stage")?;
        return Ok(stage);
    }
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
async fn list_stages_by_pipeline_on<C: ConnectionTrait>(
    db: &C,
    pipeline_id: i64,
) -> Result<Vec<pipeline_stage::Model>> {
    stage_query(db)
        .filter(pipeline_stage::Column::PipelineId.eq(pipeline_id))
        .order_by_asc(pipeline_stage::Column::StageOrder)
        .all(db)
        .await
        .context("db: list stages by pipeline")
}

/// Update stage status.
async fn update_stage_status_on<C: ConnectionTrait>(
    db: &C,
    id: i64,
    status: &str,
    started_at: Option<chrono::NaiveDateTime>,
    finished_at: Option<chrono::NaiveDateTime>,
) -> Result<()> {
    let model = stage_query(db)
        .filter(pipeline_stage::Column::Id.eq(id))
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
    let updated = pipeline_stage::Entity::update_many()
        .set(active)
        .filter(pipeline_stage::Column::Id.eq(id))
        .exec(db)
        .await
        .context("db: update stage status")?;
    if updated.rows_affected == 0 && get_stage_by_id_on(db, id).await?.is_none() {
        anyhow::bail!("stage {} not found", id);
    }
    Ok(())
}

// ── Job ops ──────────────────────────────────────────────────────

/// Create a pipeline job.
#[allow(clippy::too_many_arguments)]
pub async fn create_job(
    db: &DatabaseConnection,
    stage_id: i64,
    name: &str,
    script: &str,
    image: Option<&str>,
    tags: Option<&str>,
    variables: Option<&str>,
    cache_key: Option<&str>,
    cache_paths: Option<&str>,
    allow_failure: bool,
    timeout_seconds: Option<i64>,
    when_condition: Option<&str>,
    if_condition: Option<&str>,
) -> Result<pipeline_job::Model> {
    let when_condition = when_condition.unwrap_or("on_success");
    if db.get_database_backend() == DatabaseBackend::Postgres {
        let txn = db.begin().await.context("db: begin create job")?;
        let status = if when_condition == "manual" {
            "manual"
        } else {
            "pending"
        };
        let job = pipeline_job::Model::find_by_statement(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            format!("INSERT INTO pipeline_jobs (stage_id, name, script, image, tags, variables, cache_key, cache_paths, allow_failure, timeout_seconds, when_condition, if_condition, environment_id, environment_name, status, exit_code, log, started_at, finished_at) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,NULL,NULL,$13,NULL,NULL,NULL,NULL) RETURNING {PG_JOB_COLUMNS}"),
            [stage_id.into(), name.into(), script.into(), image.map(str::to_string).into(), tags.map(str::to_string).into(), variables.map(str::to_string).into(), cache_key.map(str::to_string).into(), cache_paths.map(str::to_string).into(), allow_failure.into(), timeout_seconds.into(), when_condition.into(), if_condition.map(str::to_string).into(), status.into()],
        )).one(&txn).await.context("db: create job")?.context("db: inserted job was not returned")?;
        txn.commit().await.context("db: commit create job")?;
        return Ok(job);
    }
    let model = pipeline_job::ActiveModel {
        stage_id: Set(stage_id),
        name: Set(name.to_string()),
        script: Set(script.to_string()),
        variables: Set(variables.map(str::to_string)),
        cache_key: Set(cache_key.map(str::to_string)),
        cache_paths: Set(cache_paths.map(str::to_string)),
        allow_failure: Set(allow_failure),
        timeout_seconds: Set(timeout_seconds),
        when_condition: Set(when_condition.to_string()),
        if_condition: Set(if_condition.map(str::to_string)),
        environment_id: Set(None),
        environment_name: Set(None),
        image: Set(image.map(|s| s.to_string())),
        status: Set(if when_condition == "manual" {
            "manual".to_string()
        } else {
            "pending".to_string()
        }),
        tags: Set(tags.map(|s| s.to_string())),
        exit_code: Set(None),
        log: Set(None),
        started_at: Set(None),
        finished_at: Set(None),
        ..Default::default()
    };
    let result = model.insert(db).await.context("db: create job")?;
    Ok(result)
}

/// Atomically release a manual job for execution. Returns false if another
/// request already released it or the job is not manual.
pub async fn play_manual_job(db: &DatabaseConnection, id: i64) -> Result<bool> {
    let now = chrono::Utc::now().naive_utc();
    let result = pipeline_job::Entity::update_many()
        .filter(pipeline_job::Column::Id.eq(id))
        .filter(pipeline_job::Column::Status.eq("manual"))
        .filter(pipeline_job::Column::WhenCondition.eq("manual"))
        .col_expr(pipeline_job::Column::Status, Expr::value("pending"))
        .col_expr(
            pipeline_job::Column::RunnerId,
            Expr::value(sea_orm::Value::BigInt(None)),
        )
        .col_expr(
            pipeline_job::Column::UpdatedAt,
            Expr::value(job_updated_at_value(db, now)),
        )
        .exec(db)
        .await
        .context("db: play manual job")?;
    Ok(result.rows_affected == 1)
}

/// Put a stage and pipeline back into schedulable state after a manual job is
/// released. Existing start timestamps are retained for duration accounting.
pub async fn resume_pipeline_chain(
    db: &DatabaseConnection,
    pipeline_id: i64,
    stage_id: i64,
) -> Result<()> {
    pipeline_stage::Entity::update_many()
        .filter(pipeline_stage::Column::Id.eq(stage_id))
        .filter(pipeline_stage::Column::Status.eq("manual"))
        .col_expr(pipeline_stage::Column::Status, Expr::value("pending"))
        .col_expr(
            pipeline_stage::Column::FinishedAt,
            Expr::value(sea_orm::Value::ChronoDateTime(None)),
        )
        .exec(db)
        .await
        .context("db: resume manual stage")?;
    pipeline::Entity::update_many()
        .filter(pipeline::Column::Id.eq(pipeline_id))
        .filter(pipeline::Column::Status.eq("manual"))
        .col_expr(pipeline::Column::Status, Expr::value("pending"))
        .col_expr(
            pipeline::Column::FinishedAt,
            Expr::value(sea_orm::Value::ChronoDateTime(None)),
        )
        .exec(db)
        .await
        .context("db: resume manual pipeline")?;
    Ok(())
}

pub async fn resume_approval_chain(
    db: &DatabaseConnection,
    pipeline_id: i64,
    stage_id: i64,
) -> Result<()> {
    pipeline_stage::Entity::update_many()
        .filter(pipeline_stage::Column::Id.eq(stage_id))
        .filter(pipeline_stage::Column::Status.eq("waiting_approval"))
        .col_expr(pipeline_stage::Column::Status, Expr::value("pending"))
        .exec(db)
        .await
        .context("db: resume approved stage")?;
    pipeline::Entity::update_many()
        .filter(pipeline::Column::Id.eq(pipeline_id))
        .filter(pipeline::Column::Status.eq("waiting_approval"))
        .col_expr(pipeline::Column::Status, Expr::value("pending"))
        .exec(db)
        .await
        .context("db: resume approved pipeline")?;
    Ok(())
}

/// Get a job by ID.
pub async fn get_job(db: &DatabaseConnection, id: i64) -> Result<Option<pipeline_job::Model>> {
    job_query(db)
        .filter(pipeline_job::Column::Id.eq(id))
        .one(db)
        .await
        .context("db: get job")
}

/// Update just the log field of a job (lightweight, used by log write queue).
pub async fn update_job_log(db: &DatabaseConnection, id: i64, log: &str) -> Result<()> {
    let model = job_query(db)
        .filter(pipeline_job::Column::Id.eq(id))
        .one(db)
        .await
        .context("db: find job for log update")?
        .ok_or_else(|| anyhow::anyhow!("job {} not found", id))?;

    let mut active: pipeline_job::ActiveModel = model.into();
    active.log = Set(Some(log.to_string()));
    let updated = pipeline_job::Entity::update_many()
        .set(active)
        .filter(pipeline_job::Column::Id.eq(id))
        .exec(db)
        .await
        .context("db: update job log")?;
    if updated.rows_affected == 0 && job_query(db).filter(pipeline_job::Column::Id.eq(id)).one(db).await?.is_none() {
        anyhow::bail!("job {} not found", id);
    }
    Ok(())
}

/// List jobs for a stage.
async fn list_jobs_by_stage_on<C: ConnectionTrait>(
    db: &C,
    stage_id: i64,
) -> Result<Vec<pipeline_job::Model>> {
    job_query(db)
        .filter(pipeline_job::Column::StageId.eq(stage_id))
        .all(db)
        .await
        .context("db: list jobs by stage")
}

pub async fn stage_has_job_status(
    db: &DatabaseConnection,
    stage_id: i64,
    status: &str,
) -> Result<bool> {
    Ok(pipeline_job::Entity::find()
        .filter(pipeline_job::Column::StageId.eq(stage_id))
        .filter(pipeline_job::Column::Status.eq(status))
        .count(db)
        .await
        .context("db: count jobs by stage status")?
        > 0)
}

/// Update job result.
async fn update_job_result_on<C: ConnectionTrait>(
    db: &C,
    id: i64,
    status: &str,
    exit_code: Option<i32>,
    log: Option<&str>,
    started_at: Option<chrono::NaiveDateTime>,
    finished_at: Option<chrono::NaiveDateTime>,
) -> Result<()> {
    let model = job_query(db)
        .filter(pipeline_job::Column::Id.eq(id))
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
    let updated = pipeline_job::Entity::update_many()
        .set(active)
        .filter(pipeline_job::Column::Id.eq(id))
        .exec(db)
        .await
        .context("db: update job result")?;
    if updated.rows_affected == 0 && job_query(db).filter(pipeline_job::Column::Id.eq(id)).one(db).await?.is_none() {
        anyhow::bail!("job {} not found", id);
    }
    Ok(())
}

/// Get a stage by ID.
async fn get_stage_by_id_on<C: ConnectionTrait>(
    db: &C,
    id: i64,
) -> Result<Option<pipeline_stage::Model>> {
    stage_query(db)
        .filter(pipeline_stage::Column::Id.eq(id))
        .one(db)
        .await
        .context("db: get stage by id")
}

/// List all jobs for a pipeline (across all stages).
async fn list_jobs_by_pipeline_on<C: ConnectionTrait>(
    db: &C,
    pipeline_id: i64,
) -> Result<Vec<pipeline_job::Model>> {
    // First get all stages for this pipeline
    let stages = stage_query(db)
        .filter(pipeline_stage::Column::PipelineId.eq(pipeline_id))
        .all(db)
        .await
        .context("db: list stages for jobs")?;

    let stage_ids: Vec<i64> = stages.iter().map(|s| s.id).collect();
    if stage_ids.is_empty() {
        return Ok(Vec::new());
    }

    job_query(db)
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
    if db.get_database_backend() == DatabaseBackend::Postgres {
        return pipeline::Model::find_by_statement(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            format!("SELECT {PG_PIPELINE_COLUMNS} FROM pipelines WHERE repo_id = $1 AND commit_sha = $2 ORDER BY created_at DESC LIMIT 1"),
            [repo_id.into(), commit_sha.into()],
        ))
        .one(db)
        .await
        .context("db: find latest pipeline by repo and commit");
    }
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
async fn check_stage_jobs_on<C: ConnectionTrait>(db: &C, stage_id: i64) -> Result<(bool, bool)> {
    let jobs = list_jobs_by_stage_on(db, stage_id).await?;
    if jobs.is_empty() {
        return Ok((true, false));
    }
    let all_done = jobs.iter().all(|j| {
        matches!(
            j.status.as_str(),
            "success" | "failure" | "failed" | "error" | "skipped"
        )
    });
    let any_failure = jobs.iter().any(|j| {
        (j.status == "failure" || j.status == "error" || j.status == "failed") && !j.allow_failure
    });
    Ok((all_done, any_failure))
}

/// If every non-manual job in a stage has finished and a manual job remains,
/// expose the persisted gate on both the stage and pipeline.
pub async fn try_pause_stage_at_manual(db: &DatabaseConnection, stage_id: i64) -> Result<bool> {
    let txn = db.begin().await.context("db: begin manual stage pause")?;
    lock_sqlite_ci_writer(&txn).await?;
    let paused = try_pause_stage_in_transaction(&txn, stage_id).await?;
    txn.commit()
        .await
        .context("db: commit manual stage pause")?;
    Ok(paused)
}

async fn try_pause_stage_in_transaction<C: ConnectionTrait>(db: &C, stage_id: i64) -> Result<bool> {
    let jobs = list_jobs_by_stage_on(db, stage_id).await?;
    let gate_status = if jobs.iter().any(|job| job.status == "waiting_approval") {
        Some("waiting_approval")
    } else if jobs.iter().any(|job| job.status == "manual") {
        Some("manual")
    } else {
        None
    };
    let automatic_done = jobs
        .iter()
        .filter(|job| !matches!(job.status.as_str(), "manual" | "waiting_approval"))
        .all(|job| {
            matches!(
                job.status.as_str(),
                "success" | "failure" | "failed" | "error" | "skipped" | "canceled"
            )
        });
    let Some(gate_status) = gate_status else {
        return Ok(false);
    };
    if !automatic_done {
        return Ok(false);
    }
    let stage = get_stage_by_id_on(db, stage_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("stage {} not found", stage_id))?;
    update_stage_status_on(db, stage_id, gate_status, None, None).await?;
    update_pipeline_status_on(db, stage.pipeline_id, gate_status, None, None).await?;
    Ok(true)
}

/// After a job finishes, update stage status if all jobs in the stage are done.
/// Returns the new stage status if updated, or None if not all done.
pub async fn try_update_stage(db: &DatabaseConnection, stage_id: i64) -> Result<Option<String>> {
    let txn = db.begin().await.context("db: begin stage update")?;
    lock_sqlite_ci_writer(&txn).await?;
    let result = try_update_stage_in_transaction(&txn, stage_id).await?;
    txn.commit().await.context("db: commit stage update")?;
    Ok(result)
}

async fn try_update_stage_in_transaction<C: ConnectionTrait>(
    db: &C,
    stage_id: i64,
) -> Result<Option<String>> {
    if try_pause_stage_in_transaction(db, stage_id).await? {
        return Ok(Some("manual".to_string()));
    }
    let (all_done, any_failure) = check_stage_jobs_on(db, stage_id).await?;
    if !all_done {
        return Ok(None);
    }
    let new_status = if any_failure { "failed" } else { "success" };
    if any_failure {
        if let Some(stage) = get_stage_by_id_on(db, stage_id).await? {
            // Decode the full cascade before making its first write.
            list_jobs_by_pipeline_on(db, stage.pipeline_id).await?;
        }
    }
    let now = Some(chrono::Utc::now().naive_utc());
    update_stage_status_on(db, stage_id, new_status, None, now).await?;
    if any_failure {
        skip_downstream_stages(db, stage_id).await?;
    }
    Ok(Some(new_status.to_string()))
}

async fn skip_downstream_stages<C: ConnectionTrait>(db: &C, failed_stage_id: i64) -> Result<()> {
    let Some(failed_stage) = get_stage_by_id_on(db, failed_stage_id).await? else {
        return Ok(());
    };
    let now = Some(chrono::Utc::now().naive_utc());
    for stage in list_stages_by_pipeline_on(db, failed_stage.pipeline_id)
        .await?
        .into_iter()
        .filter(|stage| stage.stage_order > failed_stage.stage_order)
    {
        let jobs = list_jobs_by_stage_on(db, stage.id).await?;
        if jobs
            .iter()
            .any(|job| matches!(job.status.as_str(), "assigned" | "running"))
        {
            continue;
        }
        for job in jobs {
            if !matches!(
                job.status.as_str(),
                "success" | "failure" | "failed" | "error" | "skipped" | "canceled"
            ) {
                update_job_result_on(db, job.id, "skipped", None, None, None, now).await?;
            }
        }
        if !matches!(
            stage.status.as_str(),
            "success" | "failure" | "failed" | "error" | "skipped" | "canceled"
        ) {
            update_stage_status_on(db, stage.id, "skipped", None, now).await?;
        }
    }
    Ok(())
}

/// Check if all stages in a pipeline are done.
/// Returns (all_done, any_failure).
async fn check_pipeline_stages_on<C: ConnectionTrait>(
    db: &C,
    pipeline_id: i64,
) -> Result<(bool, bool)> {
    let stages = list_stages_by_pipeline_on(db, pipeline_id).await?;
    if stages.is_empty() {
        return Ok((true, false));
    }
    let all_done = stages.iter().all(|stage| {
        matches!(
            stage.status.as_str(),
            "success" | "failure" | "failed" | "error" | "skipped" | "canceled"
        )
    });
    let any_failure = stages
        .iter()
        .any(|stage| matches!(stage.status.as_str(), "failure" | "failed" | "error"));
    Ok((all_done, any_failure))
}

/// After a stage finishes, update pipeline status if all stages are done.
async fn try_update_pipeline_on<C: ConnectionTrait>(
    db: &C,
    pipeline_id: i64,
) -> Result<Option<String>> {
    let (all_done, any_failure) = check_pipeline_stages_on(db, pipeline_id).await?;
    if !all_done {
        return Ok(None);
    }
    let new_status = if any_failure { "failed" } else { "success" };
    let now = Some(chrono::Utc::now().naive_utc());
    update_pipeline_status_on(db, pipeline_id, new_status, None, now).await?;
    Ok(Some(new_status.to_string()))
}

/// Find a pending job (status = "pending" and runner_id is NULL).
/// Returns the oldest pending job (by id).
pub async fn find_pending_job(db: &DatabaseConnection) -> Result<Option<pipeline_job::Model>> {
    let jobs = job_query(db)
        .filter(pipeline_job::Column::Status.eq("pending"))
        .filter(pipeline_job::Column::RunnerId.is_null())
        .order_by_asc(pipeline_job::Column::Id)
        .all(db)
        .await
        .context("db: find pending job")?;
    for job in jobs {
        if job_is_schedulable(db, &job).await? {
            return Ok(Some(job));
        }
    }
    Ok(None)
}

/// Find a pending job that matches the given runner labels.
///
/// A job matches if:
/// - It has no tags (any runner can pick it up)
/// - OR at least one of its tags matches one of the runner's labels
pub async fn find_pending_job_matching_labels(
    db: &DatabaseConnection,
    runner_labels: &[String],
) -> Result<Option<pipeline_job::Model>> {
    let all_pending: Vec<pipeline_job::Model> = job_query(db)
        .filter(pipeline_job::Column::Status.eq("pending"))
        .filter(pipeline_job::Column::RunnerId.is_null())
        .order_by_asc(pipeline_job::Column::Id)
        .all(db)
        .await
        .context("db: find pending jobs")?;

    let labels_lower: Vec<String> = runner_labels.iter().map(|l| l.to_lowercase()).collect();

    for job in all_pending {
        if !job_is_schedulable(db, &job).await? {
            continue;
        }
        if runner_labels.is_empty() {
            return Ok(Some(job));
        }
        let job_tags: Vec<String> = job
            .tags
            .as_ref()
            .and_then(|t| serde_json::from_str(t).ok())
            .unwrap_or_default();

        if job_tags.is_empty() {
            return Ok(Some(job));
        }

        if job_tags
            .iter()
            .any(|t| labels_lower.contains(&t.to_lowercase()))
        {
            return Ok(Some(job));
        }
    }
    Ok(None)
}

async fn job_is_schedulable(db: &DatabaseConnection, job: &pipeline_job::Model) -> Result<bool> {
    let Some(stage) = get_stage_by_id_on(db, job.stage_id).await? else {
        return Ok(false);
    };
    if matches!(
        stage.status.as_str(),
        "manual" | "waiting_approval" | "canceled"
    ) {
        return Ok(false);
    }
    let Some(pipeline) = get_pipeline_on(db, stage.pipeline_id).await? else {
        return Ok(false);
    };
    if !matches!(pipeline.status.as_str(), "pending" | "running") {
        return Ok(false);
    }
    let stages = list_stages_by_pipeline_on(db, stage.pipeline_id).await?;
    Ok(stages
        .iter()
        .filter(|candidate| candidate.stage_order < stage.stage_order)
        .all(|candidate| candidate.status == "success"))
}

/// Find stuck jobs: "assigned"/"running" but not updated within timeout.
pub async fn find_stuck_jobs(
    db: &DatabaseConnection,
    timeout_secs: i64,
) -> Result<Vec<pipeline_job::Model>> {
    let cutoff = chrono::Utc::now().naive_utc() - chrono::Duration::seconds(timeout_secs);

    job_query(db)
        .filter(pipeline_job::Column::Status.is_in(["assigned", "running"]))
        .filter(
            pipeline_job::Column::UpdatedAt
                .is_not_null()
                .and(pipeline_job::Column::UpdatedAt.lte(job_updated_at_value(db, cutoff))),
        )
        .all(db)
        .await
        .context("db: find stuck jobs")
}

/// Find offline runners: online/busy but no heartbeat within threshold.
pub async fn find_offline_runners(
    db: &DatabaseConnection,
    heartbeat_timeout_secs: i64,
) -> Result<Vec<crate::entities::runner::Model>> {
    let cutoff = chrono::Utc::now() - chrono::Duration::seconds(heartbeat_timeout_secs);
    use crate::entities::runner;
    runner::Entity::find()
        .filter(runner::Column::Status.is_in(["online", "busy"]))
        .filter(runner::Column::LastSeenAt.lt(cutoff))
        .all(db)
        .await
        .context("db: find offline runners")
}

/// Mark a job as timed out (error status).
async fn mark_job_timeout_on<C: ConnectionTrait>(db: &C, job_id: i64) -> Result<()> {
    let now = chrono::Utc::now().naive_utc();
    pipeline_job::Entity::update_many()
        .filter(pipeline_job::Column::Id.eq(job_id))
        .col_expr(pipeline_job::Column::Status, Expr::value("error"))
        .col_expr(pipeline_job::Column::ExitCode, Expr::value(-1))
        .col_expr(pipeline_job::Column::FinishedAt, Expr::value(now))
        .col_expr(
            pipeline_job::Column::UpdatedAt,
            Expr::value(job_updated_at_value(db, now)),
        )
        .exec(db)
        .await
        .context("db: mark job timeout")?;
    Ok(())
}

/// Mark all active (assigned/running) jobs of a runner as failed and cascade
/// stage/pipeline status updates.
///
/// Used when a runner goes offline (heartbeat timeout) or deregisters.
/// Jobs are marked failed instead of re-queued to avoid duplicate execution
/// (Q3.2 decision). Returns the failed job models.
pub async fn fail_runner_jobs(
    db: &DatabaseConnection,
    runner_id: i64,
) -> Result<Vec<pipeline_job::Model>> {
    let txn = db.begin().await.context("db: begin failing runner jobs")?;
    lock_sqlite_ci_writer(&txn).await?;
    let jobs = job_query(&txn)
        .filter(pipeline_job::Column::RunnerId.eq(Some(runner_id)))
        .filter(pipeline_job::Column::Status.is_in(["assigned", "running"]))
        .all(&txn)
        .await
        .context("db: find active jobs for runner")?;

    // Decode all affected pipelines before mutating any runner job.
    for job in &jobs {
        if let Some(stage) = get_stage_by_id_on(&txn, job.stage_id).await? {
            list_jobs_by_pipeline_on(&txn, stage.pipeline_id).await?;
            get_pipeline_on(&txn, stage.pipeline_id).await?;
        }
    }
    for job in &jobs {
        mark_job_timeout_on(&txn, job.id).await?;
        // Cascade: stage → pipeline (mirrors finish_job)
        if try_update_stage_in_transaction(&txn, job.stage_id)
            .await?
            .is_some()
        {
            if let Some(stage) = get_stage_by_id_on(&txn, job.stage_id).await? {
                try_update_pipeline_on(&txn, stage.pipeline_id).await?;
            }
        }
    }
    txn.commit()
        .await
        .context("db: commit failing runner jobs")?;
    Ok(jobs)
}

/// Assign a CI job to a specific runner.
pub async fn assign_job(db: &DatabaseConnection, job_id: i64, runner_id: i64) -> Result<()> {
    let now = chrono::Utc::now().naive_utc();
    let model = job_query(db)
        .filter(pipeline_job::Column::Id.eq(job_id))
        .one(db)
        .await
        .context("db: find job for assign")?
        .ok_or_else(|| anyhow::anyhow!("job {} not found", job_id))?;

    let mut active: pipeline_job::ActiveModel = model.into();
    active.status = Set("assigned".to_string());
    active.runner_id = Set(Some(runner_id));
    let updated = pipeline_job::Entity::update_many()
        .set(active)
        .filter(pipeline_job::Column::Id.eq(job_id))
        .col_expr(
            pipeline_job::Column::UpdatedAt,
            Expr::value(job_updated_at_value(db, now)),
        )
        .exec(db)
        .await
        .context("db: assign job")?;
    if updated.rows_affected == 0 && get_job(db, job_id).await?.is_none() {
        anyhow::bail!("job {} not found", job_id);
    }
    Ok(())
}

/// Status a job returns to when it is made runnable again, mirroring how it was
/// created: manual jobs wait for a click, protected-environment jobs wait for
/// approval, everything else is schedulable.
async fn reopened_job_status<C: ConnectionTrait>(
    db: &C,
    job: &pipeline_job::Model,
) -> Result<&'static str> {
    if job.when_condition == "manual" {
        return Ok("manual");
    }
    if let Some(environment_id) = job.environment_id {
        let protected = crate::entities::ci_environment::Entity::find_by_id(environment_id)
            .one(db)
            .await
            .context("db: find environment for reopened job")?
            .is_some_and(|environment| environment.protected);
        if protected {
            return Ok("waiting_approval");
        }
    }
    Ok("pending")
}

/// A rerun that was refused because of the job's or pipeline's state - the
/// caller's mistake, as opposed to a storage failure. It travels inside the
/// `anyhow::Error` returned by [`rerun_failed_job`]; callers recover it with
/// `downcast_ref::<RerunRefused>()` to answer 4xx instead of 500.
#[derive(Debug)]
pub struct RerunRefused(pub String);

impl std::fmt::Display for RerunRefused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for RerunRefused {}

/// Rerun a failed/errored/canceled job and rebuild what depended on it.
///
/// - The job goes back to `pending` with its runner, exit code and finish time
///   cleared; its stage and the pipeline are re-opened.
/// - Later stages that were skipped (failure propagation) or canceled (pipeline
///   cancel) are re-opened too, so the pipeline cannot turn green with work
///   that never ran. Their `skipped`/`canceled` jobs return to the status they
///   were created with. A job that was skipped *and* carries an `if:`
///   condition stays skipped: the two causes are indistinguishable, and
///   running a job whose condition may be false is the worse error.
/// - Refused when an earlier stage did not succeed: the job could never be
///   scheduled and the pipeline would wait forever.
///
/// Everything happens in one transaction.
pub async fn rerun_failed_job(db: &DatabaseConnection, job_id: i64) -> Result<()> {
    let now = chrono::Utc::now().naive_utc();
    let txn = db.begin().await.context("db: begin rerun transaction")?;
    lock_sqlite_ci_writer(&txn).await?;

    let job = job_query(db)
        .filter(pipeline_job::Column::Id.eq(job_id))
        .one(&txn)
        .await
        .context("db: find job for rerun")?
        .ok_or_else(|| anyhow::anyhow!("job {} not found", job_id))?;

    if job.status != "error" && job.status != "failed" && job.status != "canceled" {
        return Err(RerunRefused(format!(
            "job {} is not in a failed/error/canceled state (current: {})",
            job_id, job.status
        ))
        .into());
    }

    let stage_id = job.stage_id;
    let stage = stage_query(db)
        .filter(pipeline_stage::Column::Id.eq(stage_id))
        .one(&txn)
        .await
        .context("db: find stage for rerun")?
        .ok_or_else(|| anyhow::anyhow!("stage {} not found", stage_id))?;
    let pipeline_id = stage.pipeline_id;

    let stages = stage_query(db)
        .filter(pipeline_stage::Column::PipelineId.eq(pipeline_id))
        .order_by_asc(pipeline_stage::Column::StageOrder)
        .all(&txn)
        .await
        .context("db: list stages for rerun")?;

    if let Some(blocking) = stages
        .iter()
        .find(|s| s.stage_order < stage.stage_order && s.status != "success")
    {
        return Err(RerunRefused(format!(
            "cannot rerun job {}: earlier stage '{}' is {}, so the job could not be scheduled",
            job_id, blocking.name, blocking.status
        ))
        .into());
    }

    // 1. Reset the job itself.
    pipeline_job::Entity::update_many()
        .filter(pipeline_job::Column::Id.eq(job_id))
        .col_expr(pipeline_job::Column::Status, Expr::value("pending"))
        .col_expr(
            pipeline_job::Column::RunnerId,
            Expr::value(sea_orm::Value::BigInt(None)),
        )
        .col_expr(
            pipeline_job::Column::ExitCode,
            Expr::value(sea_orm::Value::Int(None)),
        )
        .col_expr(
            pipeline_job::Column::FinishedAt,
            Expr::value(sea_orm::Value::ChronoDateTime(None)),
        )
        .col_expr(
            pipeline_job::Column::UpdatedAt,
            Expr::value(job_updated_at_value(db, now)),
        )
        .exec(&txn)
        .await
        .context("db: rerun failed job")?;

    // 2. Re-open the job's own stage if it had been closed as failed.
    pipeline_stage::Entity::update_many()
        .filter(pipeline_stage::Column::Id.eq(stage_id))
        .filter(pipeline_stage::Column::Status.is_in(["error", "failed", "canceled"]))
        .col_expr(pipeline_stage::Column::Status, Expr::value("pending"))
        .col_expr(
            pipeline_stage::Column::FinishedAt,
            Expr::value(sea_orm::Value::ChronoDateTime(None)),
        )
        .exec(&txn)
        .await
        .context("db: reset stage for rerun")?;

    // 3. Re-open downstream stages that were closed because of the failure.
    for downstream in stages.iter().filter(|s| {
        s.stage_order > stage.stage_order && matches!(s.status.as_str(), "skipped" | "canceled")
    }) {
        let jobs = job_query(db)
            .filter(pipeline_job::Column::StageId.eq(downstream.id))
            .all(&txn)
            .await
            .context("db: list downstream jobs for rerun")?;
        for downstream_job in jobs {
            let reopen = match downstream_job.status.as_str() {
                "canceled" => true,
                "skipped" => downstream_job.if_condition.is_none(),
                _ => false,
            };
            if !reopen {
                continue;
            }
            let status = reopened_job_status(&txn, &downstream_job).await?;
            pipeline_job::Entity::update_many()
                .filter(pipeline_job::Column::Id.eq(downstream_job.id))
                .col_expr(pipeline_job::Column::Status, Expr::value(status))
                .col_expr(
                    pipeline_job::Column::RunnerId,
                    Expr::value(sea_orm::Value::BigInt(None)),
                )
                .col_expr(
                    pipeline_job::Column::ExitCode,
                    Expr::value(sea_orm::Value::Int(None)),
                )
                .col_expr(
                    pipeline_job::Column::FinishedAt,
                    Expr::value(sea_orm::Value::ChronoDateTime(None)),
                )
                .col_expr(
                    pipeline_job::Column::UpdatedAt,
                    Expr::value(job_updated_at_value(db, now)),
                )
                .exec(&txn)
                .await
                .context("db: reopen downstream job")?;
        }
        pipeline_stage::Entity::update_many()
            .filter(pipeline_stage::Column::Id.eq(downstream.id))
            .col_expr(pipeline_stage::Column::Status, Expr::value("pending"))
            .col_expr(
                pipeline_stage::Column::FinishedAt,
                Expr::value(sea_orm::Value::ChronoDateTime(None)),
            )
            .exec(&txn)
            .await
            .context("db: reopen downstream stage")?;
    }

    // 4. Re-open the pipeline.
    pipeline::Entity::update_many()
        .filter(pipeline::Column::Id.eq(pipeline_id))
        .filter(pipeline::Column::Status.is_in(["error", "failed", "canceled"]))
        .col_expr(pipeline::Column::Status, Expr::value("pending"))
        .col_expr(
            pipeline::Column::FinishedAt,
            Expr::value(sea_orm::Value::ChronoDateTime(None)),
        )
        .exec(&txn)
        .await
        .context("db: reset pipeline for rerun")?;

    txn.commit().await.context("db: commit rerun transaction")?;

    tracing::info!(
        job_id,
        stage_id,
        pipeline_id,
        "rerun_failed_job: job reset to pending"
    );

    Ok(())
}

// ── Concurrency Control ──────────────────────────────────────────

/// Count active (pending + running) pipelines for a repository.
pub async fn count_active_pipelines(db: &DatabaseConnection, repo_id: i64) -> Result<usize> {
    let count = pipeline::Entity::find()
        .filter(pipeline::Column::RepoId.eq(repo_id))
        .filter(pipeline::Column::Status.is_in([
            "pending",
            "running",
            "manual",
            "waiting_approval",
        ]))
        .count(db)
        .await
        .context("db: count active pipelines")? as usize;
    Ok(count)
}

/// Find active pipelines on a specific git ref (branch/tag).
/// Used for concurrency control by ref name.
pub async fn find_active_pipelines_by_ref(
    db: &DatabaseConnection,
    repo_id: i64,
    ref_name: &str,
) -> Result<Vec<pipeline::Model>> {
    if db.get_database_backend() == DatabaseBackend::Postgres {
        return pipeline::Model::find_by_statement(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            format!("SELECT {PG_PIPELINE_COLUMNS} FROM pipelines WHERE repo_id = $1 AND ref_name = $2 AND status IN ('pending', 'running', 'manual', 'waiting_approval') ORDER BY id ASC"),
            [repo_id.into(), ref_name.into()],
        ))
        .all(db)
        .await
        .context("db: find active pipelines by ref");
    }
    pipeline::Entity::find()
        .filter(pipeline::Column::RepoId.eq(repo_id))
        .filter(pipeline::Column::RefName.eq(ref_name))
        .filter(pipeline::Column::Status.is_in([
            "pending",
            "running",
            "manual",
            "waiting_approval",
        ]))
        .order_by_asc(pipeline::Column::Id)
        .all(db)
        .await
        .context("db: find active pipelines by ref")
}

/// Cancel a pipeline and all its stages/jobs that are still pending or running.
/// Returns whether the pipeline was actually transitioned to "canceled".
pub async fn cancel_pipeline_chain(db: &DatabaseConnection, pipeline_id: i64) -> Result<bool> {
    let txn = db.begin().await.context("db: begin cancel pipeline")?;
    lock_sqlite_ci_writer(&txn).await?;
    let pipeline_model = match get_pipeline_on(&txn, pipeline_id).await? {
        Some(p) => p,
        None => return Ok(false),
    };

    // Only cancel if still pending or running
    if pipeline_model.status != "pending"
        && pipeline_model.status != "running"
        && pipeline_model.status != "manual"
        && pipeline_model.status != "waiting_approval"
    {
        return Ok(false);
    }

    // Decode all affected rows before changing the pipeline or its children.
    let stages = list_stages_by_pipeline_on(&txn, pipeline_id).await?;
    let jobs = list_jobs_by_pipeline_on(&txn, pipeline_id).await?;
    let now = Some(chrono::Utc::now().naive_utc());

    // Cancel the pipeline
    update_pipeline_status_on(&txn, pipeline_id, "canceled", None, now).await?;

    // Cancel all stages that are not yet finished
    for stage in &stages {
        if stage.status != "success" && stage.status != "failed" && stage.status != "skipped" {
            update_stage_status_on(&txn, stage.id, "canceled", None, now).await?;
        }

        // Cancel all jobs in this stage
        for job in jobs.iter().filter(|job| job.stage_id == stage.id) {
            if job.status != "success" && job.status != "failed" && job.status != "skipped" {
                update_job_result_on(&txn, job.id, "canceled", None, None, None, now).await?;
            }
        }
    }

    txn.commit().await.context("db: commit cancel pipeline")?;
    Ok(true)
}

/// Resolve concurrency group template variables.
/// Supports: ${{ ref }}, ${{ branch }}
pub fn resolve_concurrency_group(template: &str, ref_name: &str) -> String {
    let branch = ref_name.strip_prefix("refs/heads/").unwrap_or(ref_name);
    template
        .replace("${{ ref }}", ref_name)
        .replace("${{ branch }}", branch)
}

// Public wrappers preserve the original DatabaseConnection API. Internal
// helpers also accept transactions to keep multi-row updates atomic.

pub async fn get_pipeline(db: &DatabaseConnection, id: i64) -> Result<Option<pipeline::Model>> {
    get_pipeline_on(db, id).await
}

pub async fn update_pipeline_status(db: &DatabaseConnection, id: i64, status: &str, started_at: Option<chrono::NaiveDateTime>, finished_at: Option<chrono::NaiveDateTime>) -> Result<()> {
    update_pipeline_status_on(db, id, status, started_at, finished_at).await
}

pub async fn list_stages_by_pipeline(db: &DatabaseConnection, pipeline_id: i64) -> Result<Vec<pipeline_stage::Model>> {
    list_stages_by_pipeline_on(db, pipeline_id).await
}

pub async fn update_stage_status(db: &DatabaseConnection, id: i64, status: &str, started_at: Option<chrono::NaiveDateTime>, finished_at: Option<chrono::NaiveDateTime>) -> Result<()> {
    update_stage_status_on(db, id, status, started_at, finished_at).await
}

pub async fn list_jobs_by_stage(db: &DatabaseConnection, stage_id: i64) -> Result<Vec<pipeline_job::Model>> {
    list_jobs_by_stage_on(db, stage_id).await
}

pub async fn update_job_result(db: &DatabaseConnection, id: i64, status: &str, exit_code: Option<i32>, log: Option<&str>, started_at: Option<chrono::NaiveDateTime>, finished_at: Option<chrono::NaiveDateTime>) -> Result<()> {
    update_job_result_on(db, id, status, exit_code, log, started_at, finished_at).await
}

pub async fn get_stage_by_id(db: &DatabaseConnection, id: i64) -> Result<Option<pipeline_stage::Model>> {
    get_stage_by_id_on(db, id).await
}

pub async fn list_jobs_by_pipeline(db: &DatabaseConnection, pipeline_id: i64) -> Result<Vec<pipeline_job::Model>> {
    list_jobs_by_pipeline_on(db, pipeline_id).await
}

pub async fn check_stage_jobs(db: &DatabaseConnection, stage_id: i64) -> Result<(bool, bool)> {
    check_stage_jobs_on(db, stage_id).await
}

pub async fn check_pipeline_stages(db: &DatabaseConnection, pipeline_id: i64) -> Result<(bool, bool)> {
    check_pipeline_stages_on(db, pipeline_id).await
}

pub async fn try_update_pipeline(db: &DatabaseConnection, pipeline_id: i64) -> Result<Option<String>> {
    try_update_pipeline_on(db, pipeline_id).await
}

pub async fn mark_job_timeout(db: &DatabaseConnection, job_id: i64) -> Result<()> {
    mark_job_timeout_on(db, job_id).await
}
