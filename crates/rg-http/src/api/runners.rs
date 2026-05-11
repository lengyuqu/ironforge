//! REST API handlers for CI/CD Runners.

use axum::extract::{Path, Query, Request, State};
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::error::AppError;
use crate::AppState;
use utoipa::ToSchema;

// ── Request/Response types ─────────────────────────────────

#[derive(Deserialize)]
pub struct RegisterRunnerRequest {
    pub name: String,
    pub labels: Option<Vec<String>>,
    pub version: Option<String>,
    pub os: Option<String>,
    pub arch: Option<String>,
}

#[derive(Serialize)]
struct RegisterRunnerResponse {
    id: i64,
    token: String,
    message: String,
}

#[derive(Serialize)]
struct HeartbeatResponse {
    status: String,
    server_time: String,
}

#[derive(Serialize)]
pub struct PollJobResponse {
    job_id: i64,
    pipeline_id: i64,
    stage_id: i64,
    name: String,
    script: Vec<String>,
    image: Option<String>,
    variables: Option<serde_json::Value>,
    timeout: i64,
}

#[derive(Deserialize)]
pub struct PollJobQuery {
    pub timeout: Option<u64>, // seconds, default 30
}

#[derive(Serialize)]
pub struct RunnerInfoResponse {
    id: i64,
    name: String,
    status: String,
    labels: String,
    last_seen_at: String,
    version: Option<String>,
    os: Option<String>,
    arch: Option<String>,
}

// ── Handlers ─────────────────────────────────────────────

/// POST /api/v1/runners/register
/// Register a new runner and receive a token.
#[utoipa::path(
    post,
    path = "/runners/register",
    tag = "Runners",
    request_body(content = serde_json::Value),
    responses(
        (status = 201, description = "Created", body = serde_json::Value),
        (status = 400, description = "Bad request", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
    ),
)]
pub async fn register(
    State(state): State<AppState>,
    Json(req): Json<RegisterRunnerRequest>,
) -> impl IntoResponse {
    let labels_json = serde_json::to_string(&req.labels.unwrap_or_default())
        .unwrap_or_else(|_| "[]".to_string());

    match rg_db::ops::runner_ops::register_runner(
        &state.db,
        &req.name,
        &labels_json,
        req.version.as_deref(),
        req.os.as_deref(),
        req.arch.as_deref(),
    )
    .await
    {
        Ok(runner) => (
            StatusCode::CREATED,
            Json(RegisterRunnerResponse {
                id: runner.id,
                token: runner.token,
                message: "Runner registered successfully".to_string(),
            }),
        )
            .into_response(),
        Err(e) => AppError::internal(e).into_response(),
    }
}

/// POST /api/v1/runners/:id/heartbeat
/// Update runner heartbeat (called every 30 seconds).
/// Auth handled by `authenticate_runner` middleware.
pub async fn heartbeat(
    State(_state): State<AppState>,
    Path(_runner_id): Path<i64>,
) -> impl IntoResponse {
    // Heartbeat is already updated by the authenticate_runner middleware
    (
        StatusCode::OK,
        Json(HeartbeatResponse {
            status: "ok".to_string(),
            server_time: chrono::Utc::now().to_rfc3339(),
        }),
    )
        .into_response()
}

/// GET /api/v1/runners/:id/jobs/poll?timeout=30
/// Long-polling endpoint for runners to fetch jobs.
/// Auth handled by `authenticate_runner` middleware.
pub async fn poll_job(
    State(state): State<AppState>,
    Path(runner_id): Path<i64>,
    Query(query): Query<PollJobQuery>,
) -> impl IntoResponse {
    let timeout_secs = query.timeout.unwrap_or(30).min(300) as u64;

    // Use tokio::time::timeout to wrap a polling loop
    let poll_future = async {
        loop {
            match rg_db::ops::pipeline_ops::find_pending_job(&state.db).await {
                Ok(Some(job)) => {
                    // Found a job — assign it to this runner
                    if let Err(e) = rg_db::ops::pipeline_ops::assign_job(&state.db, job.id, runner_id).await {
                        eprintln!("[poll_job] failed to assign job {}: {}", job.id, e);
                        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                        continue;
                    }
                    // Mark job as assigned
                    let now = Some(chrono::Utc::now().naive_utc());
                    let _ = rg_db::ops::pipeline_ops::update_job_result(
                        &state.db, job.id, "assigned", None, None, now, None,
                    ).await;

                    // Fetch stage to get pipeline_id
                    let mut pipeline_id = 0i64;
                    if let Ok(Some(stage)) = rg_db::ops::pipeline_ops::get_stage_by_id(&state.db, job.stage_id).await {
                        pipeline_id = stage.pipeline_id;
                    }

                    let resp = PollJobResponse {
                        job_id: job.id,
                        pipeline_id,
                        stage_id: job.stage_id,
                        name: job.name,
                        script: job.script.lines().map(|s| s.to_string()).collect(),
                        image: job.image,
                        variables: None,
                        timeout: 3600,
                    };
                    return Ok((StatusCode::OK, Json(resp)));
                }
                Ok(None) => {
                    // No job yet — wait and retry
                    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                    continue;
                }
                Err(e) => {
                    eprintln!("[poll_job] db error: {}", e);
                    return Err((
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(serde_json::json!({"error": e.to_string()})),
                    ).into_response());
                }
            }
        }
    };

    match tokio::time::timeout(std::time::Duration::from_secs(timeout_secs), poll_future).await {
        Ok(Ok(resp)) => resp.into_response(),
        Ok(Err(resp)) => resp.into_response(),
        Err(_elapsed) => (StatusCode::NO_CONTENT, Json(serde_json::json!({}))).into_response(),
    }
}

/// POST /api/v1/runners/:id/jobs/:job_id/start
/// Notify server that the runner has started executing a job.
pub async fn start_job(
    State(state): State<AppState>,
    Path((runner_id, job_id)): Path<(i64, i64)>,
) -> impl IntoResponse {
    // Verify the job is assigned to this runner
    let job = match rg_db::ops::pipeline_ops::get_job(&state.db, job_id).await {
        Ok(Some(j)) => j,
        Ok(None) => {
            return AppError::not_found("job not found").into_response();
        }
        Err(e) => {
            return AppError::internal(e).into_response();
        }
    };

    if job.runner_id != Some(runner_id) {
            return AppError::forbidden("job not assigned to this runner").into_response();
    }

    let now = Some(chrono::Utc::now().naive_utc());
    if let Err(e) = rg_db::ops::pipeline_ops::update_job_result(
        &state.db, job_id, "running", None, None, now, None,
    ).await {
        return AppError::internal(e).into_response();
    }

    // Mark runner as busy
    let _ = rg_db::ops::runner_ops::update_status(&state.db, runner_id, "busy").await;

    (StatusCode::OK, Json(serde_json::json!({"status": "ok"}))).into_response()
}

/// POST /api/v1/runners/:id/jobs/:job_id/log
/// Upload job log (streaming or batch).
pub async fn upload_log(
    State(state): State<AppState>,
    Path((runner_id, job_id)): Path<(i64, i64)>,
    body: String,
) -> impl IntoResponse {
    // Verify the job is assigned to this runner
    let job = match rg_db::ops::pipeline_ops::get_job(&state.db, job_id).await {
        Ok(Some(j)) => j,
        Ok(None) => {
            return AppError::not_found("job not found").into_response();
        }
        Err(e) => {
            return AppError::internal(e).into_response();
        }
    };

    if job.runner_id != Some(runner_id) {
            return AppError::forbidden("job not assigned to this runner").into_response();
    }

    // Append log (keep existing log + new log)
    let existing_log = job.log.unwrap_or_default();
    let combined = if existing_log.is_empty() {
        body.clone()
    } else {
        format!("{}\n{}", existing_log, body)
    };

    // Broadcast log via WebSocket to frontend
    crate::ws::push_job_log(&state.notification_hub, job_id, &body);

    if let Err(e) = rg_db::ops::pipeline_ops::update_job_result(
        &state.db, job_id, &job.status, None, Some(&combined), None, None,
    ).await {
        return AppError::internal(e).into_response();
    }

    (StatusCode::OK, Json(serde_json::json!({"status": "ok"}))).into_response()
}

/// POST /api/v1/runners/:id/jobs/:job_id/finish
/// Notify server that the runner has finished executing a job.
pub async fn finish_job(
    State(state): State<AppState>,
    Path((runner_id, job_id)): Path<(i64, i64)>,
    Json(req): Json<FinishJobRequest>,
) -> impl IntoResponse {
    // Verify the job is assigned to this runner
    let job = match rg_db::ops::pipeline_ops::get_job(&state.db, job_id).await {
        Ok(Some(j)) => j,
        Ok(None) => {
            return AppError::not_found("job not found").into_response();
        }
        Err(e) => {
            return AppError::internal(e).into_response();
        }
    };

    if job.runner_id != Some(runner_id) {
            return AppError::forbidden("job not assigned to this runner").into_response();
    }

    let now = Some(chrono::Utc::now().naive_utc());
    // log is managed via upload_log; not updated on finish
    if let Err(e) = rg_db::ops::pipeline_ops::update_job_result(
        &state.db, job_id, &req.status, Some(req.exit_code), None, None, now,
    ).await {
        return AppError::internal(e).into_response();
    }

    // Mark runner as online (ready for next job)
    let _ = rg_db::ops::runner_ops::update_status(&state.db, runner_id, "online").await;

    // Cascade: check if stage is done, then if pipeline is done
    if let Ok(Some(_stage_status)) = rg_db::ops::pipeline_ops::try_update_stage(&state.db, job.stage_id).await {
        // Stage is done — get pipeline_id and check pipeline
        if let Ok(Some(stage)) = rg_db::ops::pipeline_ops::get_stage_by_id(&state.db, job.stage_id).await {
            let _ = rg_db::ops::pipeline_ops::try_update_pipeline(&state.db, stage.pipeline_id).await;
        }
    }

    (StatusCode::OK, Json(serde_json::json!({"status": "ok"}))).into_response()
}

#[derive(Deserialize)]
pub struct FinishJobRequest {
    status: String, // success | failure | error
    exit_code: i32,
}

/// GET /api/v1/admin/runners
/// List all runners (admin only).
#[utoipa::path(
    get,
    path = "/admin/runners",
    tag = "Runners",
    responses(
        (status = 200, description = "Success", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
    ),
)]
pub async fn list_runners_admin(
    State(state): State<AppState>,
    // TODO: authenticate as admin
) -> impl IntoResponse {
    match rg_db::ops::runner_ops::list_all(&state.db).await {
        Ok(runners) => {
            let resp: Vec<RunnerInfoResponse> = runners
                .into_iter()
                .map(|r| RunnerInfoResponse {
                    id: r.id,
                    name: r.name,
                    status: r.status,
                    labels: r.labels,
                    last_seen_at: r.last_seen_at.to_string(),
                    version: r.version,
                    os: r.os,
                    arch: r.arch,
                })
                .collect();
            (StatusCode::OK, Json(resp)).into_response()
        }
        Err(e) => AppError::internal(e).into_response(),
    }
}

// ── Runner Token Authentication ──────────────────────────

/// Extract and validate runner Bearer token from Authorization header.
///
/// Used as a route-layer middleware via `from_fn_with_state`.
/// The runner_id is extracted from the path to verify token ownership.
/// Also updates heartbeat on every authenticated request.
pub async fn authenticate_runner(
    State(state): State<AppState>,
    Path(runner_id): Path<i64>,
    request: Request,
    next: Next,
) -> Response {
    let auth_header = request
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok());

    let token = match auth_header {
        Some(h) if h.starts_with("Bearer ") => &h[7..],
        _ => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({"error": "missing or invalid Authorization header"})),
            )
                .into_response();
        }
    };

    match rg_db::ops::runner_ops::find_by_token(&state.db, token).await {
        Ok(Some(runner)) if runner.id == runner_id => {
            // Valid token — also update heartbeat
            let _ = rg_db::ops::runner_ops::update_heartbeat(&state.db, runner_id).await;
            next.run(request).await
        }
        Ok(Some(_)) => (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "token does not match runner ID"})),
        )
            .into_response(),
        Ok(None) => (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "invalid runner token"})),
        )
            .into_response(),
        Err(e) => AppError::internal(e).into_response(),
    }
}

/// DELETE /api/v1/admin/runners/:id
/// Delete a runner (admin only).
#[utoipa::path(
    delete,
    path = "/admin/runners/{id}",
    tag = "Runners",
    params(
        ("id" = i64, Path, description = "id"),
    ),
    responses(
        (status = 200, description = "Deleted", body = serde_json::Value),
        (status = 204, description = "No content"),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
    ),
)]
pub async fn delete_runner_admin(
    State(state): State<AppState>,
    Path(runner_id): Path<i64>,
) -> impl IntoResponse {
    match rg_db::ops::runner_ops::delete_runner(&state.db, runner_id).await {
        Ok(true) => (StatusCode::NO_CONTENT, Json(serde_json::json!({"deleted": true}))).into_response(),
        Ok(false) => AppError::not_found("runner not found").into_response(),
        Err(e) => AppError::internal(e).into_response(),
    }
}
