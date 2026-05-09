//! REST API handlers for CI/CD pipelines.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use sea_orm::{DatabaseConnection, EntityTrait, QueryFilter, ColumnTrait};
use serde::{Deserialize, Serialize};

use crate::AppState;
use crate::pagination::{PaginationParams, PaginatedResponse};

// ── Response types ───────────────────────────────────────────────

#[derive(Serialize)]
struct PipelineResponse {
    id: i64,
    repo_id: i64,
    commit_sha: String,
    ref_name: String,
    status: String,
    trigger_type: String,
    triggered_by: Option<i64>,
    started_at: Option<String>,
    finished_at: Option<String>,
    created_at: String,
}

#[derive(Serialize)]
struct StageResponse {
    id: i64,
    pipeline_id: i64,
    name: String,
    stage_order: i32,
    status: String,
    started_at: Option<String>,
    finished_at: Option<String>,
}

#[derive(Serialize)]
struct JobResponse {
    id: i64,
    stage_id: i64,
    name: String,
    image: Option<String>,
    script: String,
    status: String,
    exit_code: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    log: Option<String>,
    started_at: Option<String>,
    finished_at: Option<String>,
}

#[derive(Serialize)]
struct PipelineDetailResponse {
    pipeline: PipelineResponse,
    stages: Vec<StageWithJobsResponse>,
}

#[derive(Serialize)]
struct StageWithJobsResponse {
    stage: StageResponse,
    jobs: Vec<JobResponse>,
}

#[derive(Deserialize)]
pub struct TriggerPipelineRequest {
    ref_name: Option<String>,
}

#[derive(Deserialize)]
pub struct ListPipelinesQuery {
    #[serde(flatten)]
    pagination: PaginationParams,
}

// ── Handlers ─────────────────────────────────────────────────────

/// GET /api/v1/repos/:owner/:name/pipelines
/// List all pipelines for a repository.
pub async fn list_pipelines(
    State(state): State<AppState>,
    Path((owner, name)): Path<(String, String)>,
    Query(params): Query<ListPipelinesQuery>,
) -> impl IntoResponse {
    let pagination = params.pagination.clamp();
    let offset = pagination.offset();
    let limit = pagination.limit();

    let repo = match find_repo(&state.db, &owner, &name).await {
        Ok(Some(r)) => r,
        Ok(None) => return (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "repository not found"}))).into_response(),
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    };

    match rg_db::ops::pipeline_ops::list_pipelines_by_repo_paginated(&state.db, repo.id, offset, limit).await {
        Ok((pipelines, total)) => {
            let resp: Vec<PipelineResponse> = pipelines
                .into_iter()
                .map(|p| PipelineResponse {
                    id: p.id,
                    repo_id: p.repo_id,
                    commit_sha: p.commit_sha,
                    ref_name: p.ref_name,
                    status: p.status,
                    trigger_type: p.trigger_type,
                    triggered_by: p.triggered_by,
                    started_at: p.started_at.map(|t| t.to_string()),
                    finished_at: p.finished_at.map(|t| t.to_string()),
                    created_at: p.created_at.to_string(),
                })
                .collect();
            Json(PaginatedResponse::new(resp, &pagination, total as u64)).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

/// GET /api/v1/repos/:owner/:name/pipelines/:id
/// Get pipeline detail with stages and jobs.
pub async fn get_pipeline(
    State(state): State<AppState>,
    Path((_owner, _name, id)): Path<(String, String, i64)>,
) -> impl IntoResponse {
    let pipeline = match rg_db::ops::pipeline_ops::get_pipeline(&state.db, id).await {
        Ok(Some(p)) => p,
        Ok(None) => return (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "pipeline not found"}))).into_response(),
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    };

    let stages = match rg_db::ops::pipeline_ops::list_stages_by_pipeline(&state.db, id).await {
        Ok(s) => s,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    };

    let mut stages_with_jobs: Vec<StageWithJobsResponse> = Vec::new();

    for stage in stages {
        let jobs = match rg_db::ops::pipeline_ops::list_jobs_by_stage(&state.db, stage.id).await {
            Ok(j) => j,
            Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
        };

        stages_with_jobs.push(StageWithJobsResponse {
            stage: StageResponse {
                id: stage.id,
                pipeline_id: stage.pipeline_id,
                name: stage.name,
                stage_order: stage.stage_order,
                status: stage.status,
                started_at: stage.started_at.map(|t| t.to_string()),
                finished_at: stage.finished_at.map(|t| t.to_string()),
            },
            jobs: jobs
                .into_iter()
                .map(|j| JobResponse {
                    id: j.id,
                    stage_id: j.stage_id,
                    name: j.name,
                    image: j.image,
                    script: j.script,
                    status: j.status,
                    exit_code: j.exit_code,
                    log: j.log,
                    started_at: j.started_at.map(|t| t.to_string()),
                    finished_at: j.finished_at.map(|t| t.to_string()),
                })
                .collect(),
        });
    }

    let resp = PipelineDetailResponse {
        pipeline: PipelineResponse {
            id: pipeline.id,
            repo_id: pipeline.repo_id,
            commit_sha: pipeline.commit_sha,
            ref_name: pipeline.ref_name,
            status: pipeline.status,
            trigger_type: pipeline.trigger_type,
            triggered_by: pipeline.triggered_by,
            started_at: pipeline.started_at.map(|t| t.to_string()),
            finished_at: pipeline.finished_at.map(|t| t.to_string()),
            created_at: pipeline.created_at.to_string(),
        },
        stages: stages_with_jobs,
    };

    Json(resp).into_response()
}

/// GET /api/v1/repos/:owner/:name/pipelines/:id/jobs/:job_id
/// Get job detail with log.
pub async fn get_job(
    State(state): State<AppState>,
    Path((_owner, _name, _pipeline_id, job_id)): Path<(String, String, i64, i64)>,
) -> impl IntoResponse {
    match rg_db::ops::pipeline_ops::get_job(&state.db, job_id).await {
        Ok(Some(j)) => Json(JobResponse {
            id: j.id,
            stage_id: j.stage_id,
            name: j.name,
            image: j.image,
            script: j.script,
            status: j.status,
            exit_code: j.exit_code,
            log: j.log,
            started_at: j.started_at.map(|t| t.to_string()),
            finished_at: j.finished_at.map(|t| t.to_string()),
        })
        .into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "job not found"})),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

/// POST /api/v1/repos/:owner/:name/pipelines
/// Manually trigger a pipeline.
pub async fn trigger_pipeline(
    State(state): State<AppState>,
    Path((owner, name)): Path<(String, String)>,
    Json(body): Json<TriggerPipelineRequest>,
) -> impl IntoResponse {
    let repo = match find_repo(&state.db, &owner, &name).await {
        Ok(Some(r)) => r,
        Ok(None) => return (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "repository not found"}))).into_response(),
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    };

    let repo_path = state.repo_root.join(format!("{}/{}.git", owner, name));
    if !repo_path.exists() {
        return (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "repo path not found"}))).into_response();
    }

    // Resolve HEAD commit SHA
    let ref_name = body.ref_name.unwrap_or_else(|| "refs/heads/main".to_string());
    let commit_sha = match resolve_commit_sha(&repo_path, &ref_name) {
        Some(sha) => sha,
        None => return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "cannot resolve commit SHA for ref"}))).into_response(),
    };

    // Check if CI config exists
    if !rg_ci::has_ci_config(&repo_path, &commit_sha) {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "no .ironforge-ci.yml found"}))).into_response();
    }

    match rg_ci::trigger_pipeline(
        state.db.clone(),
        &repo_path,
        repo.id,
        &commit_sha,
        &ref_name,
        "manual",
        None,
        state.docker_enabled,
    )
    .await
    {
        Ok(pipeline_id) => (
            StatusCode::CREATED,
            Json(serde_json::json!({
                "id": pipeline_id,
                "status": "pending",
                "commit_sha": commit_sha,
                "ref_name": ref_name,
            })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

/// POST /api/v1/repos/:owner/:name/pipelines/:id/retry
/// Retry a failed pipeline.
pub async fn retry_pipeline(
    State(state): State<AppState>,
    Path((owner, name, id)): Path<(String, String, i64)>,
) -> impl IntoResponse {
    let pipeline = match rg_db::ops::pipeline_ops::get_pipeline(&state.db, id).await {
        Ok(Some(p)) => p,
        Ok(None) => return (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "pipeline not found"}))).into_response(),
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    };

    let repo_path = state.repo_root.join(format!("{}/{}.git", owner, name));
    if !repo_path.exists() {
        return (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "repo path not found"}))).into_response();
    }

    match rg_ci::trigger_pipeline(
        state.db.clone(),
        &repo_path,
        pipeline.repo_id,
        &pipeline.commit_sha,
        &pipeline.ref_name,
        "retry",
        pipeline.triggered_by,
        state.docker_enabled,
    )
    .await
    {
        Ok(new_id) => (
            StatusCode::CREATED,
            Json(serde_json::json!({
                "id": new_id,
                "status": "pending",
                "original_pipeline_id": id,
            })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

/// POST /api/v1/repos/:owner/:name/pipelines/:id/cancel
/// Cancel a running pipeline.
pub async fn cancel_pipeline(
    State(state): State<AppState>,
    Path((_owner, _name, id)): Path<(String, String, i64)>,
) -> impl IntoResponse {
    let pipeline = match rg_db::ops::pipeline_ops::get_pipeline(&state.db, id).await {
        Ok(Some(p)) => p,
        Ok(None) => return (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "pipeline not found"}))).into_response(),
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    };

    if pipeline.status != "running" && pipeline.status != "pending" {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "pipeline is not running or pending"})),
        )
            .into_response();
    }

    let now = chrono::Utc::now().naive_utc();

    // Mark pipeline as canceled
    if let Err(e) =
        rg_db::ops::pipeline_ops::update_pipeline_status(&state.db, id, "canceled", None, Some(now)).await
    {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response();
    }

    // Mark all running stages/jobs as canceled
    let stages = match rg_db::ops::pipeline_ops::list_stages_by_pipeline(&state.db, id).await {
        Ok(s) => s,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    };

    for stage in stages {
        if stage.status == "running" || stage.status == "pending" {
            let _ = rg_db::ops::pipeline_ops::update_stage_status(
                &state.db,
                stage.id,
                "canceled",
                None,
                Some(now),
            )
            .await;

            let jobs = match rg_db::ops::pipeline_ops::list_jobs_by_stage(&state.db, stage.id).await {
                Ok(j) => j,
                Err(_) => continue,
            };

            for job in jobs {
                if job.status == "running" || job.status == "pending" {
                    let _ = rg_db::ops::pipeline_ops::update_job_result(
                        &state.db,
                        job.id,
                        "canceled",
                        None,
                        None,
                        None,
                        Some(now),
                    )
                    .await;
                }
            }
        }
    }

    Json(serde_json::json!({"id": id, "status": "canceled"})).into_response()
}

// ── Helpers ──────────────────────────────────────────────────────

async fn find_repo(
    db: &DatabaseConnection,
    owner: &str,
    name: &str,
) -> Result<Option<rg_db::entities::repository::Model>, anyhow::Error> {
    // Find user by owner name
    let user = rg_db::entities::user::Entity::find()
        .filter(rg_db::entities::user::Column::Username.eq(owner))
        .one(db)
        .await?;

    let Some(user) = user else {
        return Ok(None);
    };

    let repo = rg_db::entities::repository::Entity::find()
        .filter(rg_db::entities::repository::Column::OwnerId.eq(user.id))
        .filter(rg_db::entities::repository::Column::Name.eq(name))
        .one(db)
        .await?;

    Ok(repo)
}

fn resolve_commit_sha(repo_path: &std::path::Path, ref_name: &str) -> Option<String> {
    let repo = gix::open(repo_path).ok()?;
    
    // Try to parse the ref directly
    let ref_name_normalized = if ref_name.starts_with("refs/") {
        ref_name.to_string()
    } else {
        format!("refs/heads/{}", ref_name)
    };
    
    match repo.rev_parse_single(ref_name_normalized.as_str()) {
        Ok(id) => Some(id.to_string()),
        Err(_) => {
            // Try without refs/heads/ prefix
            let short = ref_name.strip_prefix("refs/heads/").unwrap_or(ref_name);
            repo.rev_parse_single(short).ok().map(|id| id.to_string())
        }
    }
}
