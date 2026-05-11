//! REST API handlers for CI Artifacts.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::AppState;
use crate::error::AppError;
use utoipa::ToSchema;

// ── Response types ─────────────────────────────────────

#[derive(Serialize)]
struct ArtifactResponse {
    id: i64,
    job_id: i64,
    name: String,
    size: i64,
    created_at: String,
    expires_at: Option<String>,
}

#[derive(Serialize)]
struct UploadArtifactResponse {
    id: i64,
    message: String,
}

// ── Handlers ───────────────────────────────────────────

/// POST /api/v1/runners/:id/jobs/:job_id/artifacts
/// Upload an artifact for a job.
pub async fn upload_artifact(
    State(state): State<AppState>,
    Path((runner_id, job_id)): Path<(i64, i64)>,
    Json(req): Json<UploadArtifactRequest>,
) -> impl IntoResponse {
    // Verify job belongs to this runner
    let job = match rg_db::ops::pipeline_ops::get_job(&state.db, job_id).await {
        Ok(Some(j)) => j,
        Ok(None) => {
            return AppError::not_found("job not found").into_response();
        }
        Err(e) => {
            return AppError::internal(e.to_string()).into_response();
        }
    };

    if job.runner_id != Some(runner_id) {
        return AppError::forbidden("job not assigned to this runner").into_response();
    }

    match rg_db::ops::artifact_ops::create_artifact(
        &state.db,
        job_id,
        &req.name,
        &req.file_path,
        req.size.unwrap_or(0),
        None,
    )
    .await
    {
        Ok(artifact) => (
            StatusCode::CREATED,
            Json(UploadArtifactResponse {
                id: artifact.id,
                message: "Artifact created successfully".to_string(),
            }),
        )
            .into_response(),
        Err(e) => AppError::internal(e.to_string()).into_response(),
    }
}

/// GET /api/v1/repos/:owner/:name/pipelines/:id/artifacts
/// List all artifacts for a pipeline.
#[utoipa::path(
    get,
    path = "/repos/{owner}/{name}/pipelines/{id}/artifacts",
    tag = "Artifacts",
    params(
        ("owner" = String, Path, description = "owner"),
        ("name" = String, Path, description = "name"),
        ("id" = i64, Path, description = "id"),
    ),
    responses(
        (status = 200, description = "Success", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
    ),
)]
pub async fn list_pipeline_artifacts(
    State(state): State<AppState>,
    Path((_owner, _name, pipeline_id)): Path<(String, String, i64)>,
) -> impl IntoResponse {
    match rg_db::ops::artifact_ops::list_by_pipeline(&state.db, pipeline_id).await {
        Ok(artifacts) => {
            let resp: Vec<ArtifactResponse> = artifacts
                .into_iter()
                .map(|a| ArtifactResponse {
                    id: a.id,
                    job_id: a.job_id,
                    name: a.name,
                    size: a.size,
                    created_at: a.created_at.to_string(),
                    expires_at: a.expires_at.map(|t| t.to_string()),
                })
                .collect();
            (StatusCode::OK, Json(resp)).into_response()
        }
        Err(e) => AppError::internal(e.to_string()).into_response(),
    }
}

/// GET /api/v1/artifacts/:id
/// Get artifact metadata.
#[utoipa::path(
    get,
    path = "/artifacts/{id}",
    tag = "Artifacts",
    params(
        ("id" = i64, Path, description = "id"),
    ),
    responses(
        (status = 200, description = "Success", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
    ),
)]
pub async fn get_artifact(
    State(state): State<AppState>,
    Path(artifact_id): Path<i64>,
) -> impl IntoResponse {
    match rg_db::ops::artifact_ops::get_by_id(&state.db, artifact_id).await {
        Ok(Some(a)) => (
            StatusCode::OK,
            Json(ArtifactResponse {
                id: a.id,
                job_id: a.job_id,
                name: a.name,
                size: a.size,
                created_at: a.created_at.to_string(),
                expires_at: a.expires_at.map(|t| t.to_string()),
            }),
        )
            .into_response(),
        Ok(None) => AppError::not_found("artifact not found").into_response(),
        Err(e) => AppError::internal(e.to_string()).into_response(),
    }
}

/// DELETE /api/v1/artifacts/:id
/// Delete an artifact.
#[utoipa::path(
    delete,
    path = "/artifacts/{id}",
    tag = "Artifacts",
    params(
        ("id" = i64, Path, description = "id"),
    ),
    responses(
        (status = 200, description = "Deleted", body = serde_json::Value),
        (status = 204, description = "No content"),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
    ),
)]
pub async fn delete_artifact(
    State(state): State<AppState>,
    Path(artifact_id): Path<i64>,
) -> impl IntoResponse {
    match rg_db::ops::artifact_ops::delete_by_id(&state.db, artifact_id).await {
        Ok(true) => (StatusCode::NO_CONTENT, Json(serde_json::json!({"status": "deleted"}))).into_response(),
        Ok(false) => AppError::not_found("artifact not found").into_response(),
        Err(e) => AppError::internal(e.to_string()).into_response(),
    }
}

// ── Request types ───────────────────────────────────────

#[derive(Deserialize)]
pub struct UploadArtifactRequest {
    pub name: String,
    pub file_path: String,
    pub size: Option<i64>,
}
