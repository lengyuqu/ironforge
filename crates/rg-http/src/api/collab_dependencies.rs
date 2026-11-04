//! Task dependency endpoints: "task A waits for task B", guarded by a per-repository
//! graph version. Declarations only; nothing here blocks, starts or finishes work.

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use chrono::Utc;
use rg_core::collaboration::dependency::{self as svc, GraphLimits};
use serde::Deserialize;
use utoipa::ToSchema;

use super::collab::{require_collab_read, require_collab_write, write_actor};
use crate::error::AppError;
use crate::AppState;

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AddDependencyBody {
    /// Client-chosen idempotency key, unique per acting session or user.
    pub client_request_id: String,
    /// The task this task waits for (same repository).
    pub depends_on_task_id: String,
    /// The graph version last read from `GET .../dependencies` (0 = no edge yet).
    pub expected_graph_version: i64,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RemoveDependencyBody {
    pub client_request_id: String,
    pub expected_graph_version: i64,
}

#[utoipa::path(
    get,
    path = "/repos/{owner}/{name}/collaboration/tasks/{id}/dependencies",
    tag = "Collaboration",
    params(("owner" = String, Path), ("name" = String, Path), ("id" = String, Path)),
    responses(
        (status = 200, description = "Direct dependencies with their declared status: {task_id, graph_version, dependencies, blocked_by, status_source}. Statuses are declarations, not evidence", body = serde_json::Value),
        (status = 403, description = "Read access denied"),
        (status = 404, description = "No such task in this repository")
    )
)]
pub async fn list_dependencies(
    State(state): State<AppState>,
    Path((owner, name, id)): Path<(String, String, String)>,
    headers: HeaderMap,
) -> Result<Json<svc::DependencyList>, AppError> {
    let (repo, _user) = require_collab_read(&state, &headers, &owner, &name).await?;
    Ok(Json(svc::list(&state.db, repo.id, &id).await?))
}

#[utoipa::path(
    post,
    path = "/repos/{owner}/{name}/collaboration/tasks/{id}/dependencies",
    tag = "Collaboration",
    params(("owner" = String, Path), ("name" = String, Path), ("id" = String, Path)),
    request_body = AddDependencyBody,
    responses(
        (status = 201, description = "Edge added: {dependency, graph_version, replayed}", body = serde_json::Value),
        (status = 200, description = "Identical earlier request replayed", body = serde_json::Value),
        (status = 400, description = "Self dependency or malformed input"),
        (status = 403, description = "Write access denied or invalid session credentials"),
        (status = 404, description = "No such task (or dependency task) in this repository"),
        (status = 409, description = "VERSION_CONFLICT, DEPENDENCY_CYCLE, GRAPH_LIMIT_EXCEEDED, DEPENDENCY_EXISTS, IDEMPOTENCY_KEY_REUSED or SESSION_EXPIRED")
    )
)]
pub async fn add_dependency(
    State(state): State<AppState>,
    Path((owner, name, id)): Path<(String, String, String)>,
    headers: HeaderMap,
    super::collab_json::CollabJson(body): super::collab_json::CollabJson<AddDependencyBody>,
) -> Result<(StatusCode, Json<svc::AddResult>), AppError> {
    let (repo, user_id) = require_collab_write(&state, &headers, &owner, &name).await?;
    let actor = write_actor(&state, &headers, repo.id, user_id).await?;
    let req = svc::AddRequest {
        client_request_id: body.client_request_id,
        depends_on_task_id: body.depends_on_task_id,
        expected_graph_version: body.expected_graph_version,
    };
    let result = svc::add(
        &state.db,
        repo.id,
        &id,
        &actor,
        req,
        &GraphLimits::default(),
        Utc::now(),
    )
    .await?;
    let status = if result.replayed {
        StatusCode::OK
    } else {
        StatusCode::CREATED
    };
    Ok((status, Json(result)))
}

#[utoipa::path(
    delete,
    path = "/repos/{owner}/{name}/collaboration/tasks/{id}/dependencies/{depends_on_id}",
    tag = "Collaboration",
    params(
        ("owner" = String, Path),
        ("name" = String, Path),
        ("id" = String, Path),
        ("depends_on_id" = String, Path)
    ),
    request_body = RemoveDependencyBody,
    responses(
        (status = 200, description = "{removed, graph_version}; removed=false when the edge did not exist (idempotent)", body = serde_json::Value),
        (status = 403, description = "Write access denied or invalid session credentials"),
        (status = 404, description = "No such task in this repository"),
        (status = 409, description = "VERSION_CONFLICT or SESSION_EXPIRED")
    )
)]
pub async fn remove_dependency(
    State(state): State<AppState>,
    Path((owner, name, id, depends_on_id)): Path<(String, String, String, String)>,
    headers: HeaderMap,
    super::collab_json::CollabJson(body): super::collab_json::CollabJson<RemoveDependencyBody>,
) -> Result<Json<svc::RemoveResult>, AppError> {
    let (repo, user_id) = require_collab_write(&state, &headers, &owner, &name).await?;
    // Authenticates the session when headers are present, like every write.
    write_actor(&state, &headers, repo.id, user_id).await?;
    let req = svc::RemoveRequest {
        client_request_id: body.client_request_id,
        expected_graph_version: body.expected_graph_version,
    };
    let result = svc::remove(&state.db, repo.id, &id, &depends_on_id, req, Utc::now()).await?;
    Ok(Json(result))
}
