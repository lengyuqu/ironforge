//! Collaboration task endpoints.

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use chrono::{DateTime, Utc};
use rg_core::collaboration::{self, task as svc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::collab::{
    acting_session, cursor_key, present_or_null, require_collab_read, require_collab_write,
};
use super::collab_json::{require_known, CollabJson};
use crate::error::AppError;
use crate::AppState;
use rg_core::collaboration::compat::TaskStatus;

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateTaskRequest {
    /// 1–200 characters.
    pub title: String,
    /// Up to 16 KiB.
    pub description: Option<String>,
    /// A live session of this repository; omit for an unowned task.
    pub owner_session_id: Option<String>,
    /// Declared working branch (not checked against Git).
    pub branch: Option<String>,
    /// Declared repository-relative paths, at most 100, stored literally.
    #[serde(default)]
    pub paths: Vec<String>,
    pub issue_number: Option<i64>,
    pub pr_number: Option<i64>,
}

/// Partial update guarded by `expected_version`. Omitted fields are unchanged;
/// an explicit `null` clears `description`, `owner_session_id`, `branch`,
/// `issue_number` or `pr_number`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateTaskRequest {
    pub expected_version: i64,
    pub title: Option<String>,
    #[serde(default, deserialize_with = "present_or_null")]
    pub description: Option<Option<String>>,
    /// `open` | `in_progress` | `waiting` | `blocked` | `done`.
    pub status: Option<String>,
    #[serde(default, deserialize_with = "present_or_null")]
    pub owner_session_id: Option<Option<String>>,
    #[serde(default, deserialize_with = "present_or_null")]
    pub branch: Option<Option<String>>,
    pub paths: Option<Vec<String>>,
    #[serde(default, deserialize_with = "present_or_null")]
    pub issue_number: Option<Option<i64>>,
    #[serde(default, deserialize_with = "present_or_null")]
    pub pr_number: Option<Option<i64>>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct TaskResponse {
    pub id: String,
    pub title: String,
    pub description: Option<String>,
    pub status: String,
    pub owner_session_id: Option<String>,
    pub branch: Option<String>,
    pub paths: Vec<String>,
    pub issue_number: Option<i64>,
    pub pr_number: Option<i64>,
    /// Send back as `expected_version` when updating.
    pub version: i64,
    /// Sequence number of the newest message (0 = none).
    pub last_message_seq: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// Always `user_content`: every text field is a client declaration.
    pub origin: String,
}

impl From<svc::TaskView> for TaskResponse {
    fn from(v: svc::TaskView) -> Self {
        Self {
            id: v.id,
            title: v.title,
            description: v.description,
            status: v.status,
            owner_session_id: v.owner_session_id,
            branch: v.branch,
            paths: v.paths,
            issue_number: v.issue_number,
            pr_number: v.pr_number,
            version: v.version,
            last_message_seq: v.last_message_seq,
            created_at: v.created_at,
            updated_at: v.updated_at,
            origin: v.origin.to_string(),
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub struct TaskListResponse {
    pub items: Vec<TaskResponse>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct TaskListQuery {
    /// Filter by status.
    pub status: Option<String>,
    pub limit: Option<u64>,
    pub cursor: Option<String>,
}

#[utoipa::path(
    post,
    path = "/repos/{owner}/{name}/collaboration/tasks",
    tag = "Collaboration",
    params(("owner" = String, Path), ("name" = String, Path)),
    request_body = CreateTaskRequest,
    responses(
        (status = 201, description = "Task created", body = TaskResponse),
        (status = 400, description = "Invalid field or cross-repository reference"),
        (status = 403, description = "Write access denied or invalid session credentials"),
        (status = 409, description = "SESSION_EXPIRED")
    )
)]
pub async fn create_task(
    State(state): State<AppState>,
    Path((owner, name)): Path<(String, String)>,
    headers: HeaderMap,
    CollabJson(req): CollabJson<CreateTaskRequest>,
) -> Result<(StatusCode, Json<TaskResponse>), AppError> {
    let (repo, user_id) = require_collab_write(&state, &headers, &owner, &name).await?;
    let actor = acting_session(&state, &headers, repo.id, user_id).await?;
    let task = svc::create(
        &state.db,
        repo.id,
        &actor,
        svc::NewTask {
            title: req.title,
            description: req.description,
            owner_session_id: req.owner_session_id,
            branch: req.branch,
            paths: req.paths,
            issue_number: req.issue_number,
            pr_number: req.pr_number,
        },
        Utc::now(),
    )
    .await?;
    Ok((StatusCode::CREATED, Json(task.into())))
}

#[utoipa::path(
    get,
    path = "/repos/{owner}/{name}/collaboration/tasks",
    tag = "Collaboration",
    params(("owner" = String, Path), ("name" = String, Path), TaskListQuery),
    responses(
        (status = 200, description = "Tasks, most recently updated first (eventually consistent while tasks change)", body = TaskListResponse),
        (status = 400, description = "Invalid status or cursor"),
        (status = 401, description = "Authentication required"),
        (status = 403, description = "Read access denied")
    )
)]
pub async fn list_tasks(
    State(state): State<AppState>,
    Path((owner, name)): Path<(String, String)>,
    Query(query): Query<TaskListQuery>,
    headers: HeaderMap,
) -> Result<Json<TaskListResponse>, AppError> {
    let (repo, _user) = require_collab_read(&state, &headers, &owner, &name).await?;
    require_known::<TaskStatus>("status", query.status.as_deref())?;
    let page = svc::list(
        &state.db,
        cursor_key(&state),
        repo.id,
        query.status.as_deref(),
        collaboration::clamp_limit(query.limit),
        query.cursor.as_deref(),
    )
    .await?;
    Ok(Json(TaskListResponse {
        items: page.items.into_iter().map(Into::into).collect(),
        next_cursor: page.next_cursor,
    }))
}

#[utoipa::path(
    get,
    path = "/repos/{owner}/{name}/collaboration/tasks/{id}",
    tag = "Collaboration",
    params(("owner" = String, Path), ("name" = String, Path), ("id" = String, Path)),
    responses(
        (status = 200, description = "Latest task state", body = TaskResponse),
        (status = 404, description = "No such task in this repository")
    )
)]
pub async fn get_task(
    State(state): State<AppState>,
    Path((owner, name, id)): Path<(String, String, String)>,
    headers: HeaderMap,
) -> Result<Json<TaskResponse>, AppError> {
    let (repo, _user) = require_collab_read(&state, &headers, &owner, &name).await?;
    Ok(Json(svc::get(&state.db, repo.id, &id).await?.into()))
}

#[utoipa::path(
    patch,
    path = "/repos/{owner}/{name}/collaboration/tasks/{id}",
    tag = "Collaboration",
    params(("owner" = String, Path), ("name" = String, Path), ("id" = String, Path)),
    request_body = UpdateTaskRequest,
    responses(
        (status = 200, description = "Task updated", body = TaskResponse),
        (status = 400, description = "Invalid field or no field to update"),
        (status = 403, description = "Write access denied or invalid session credentials"),
        (status = 404, description = "No such task in this repository"),
        (status = 409, description = "VERSION_CONFLICT (details.current_version) or SESSION_EXPIRED")
    )
)]
pub async fn update_task(
    State(state): State<AppState>,
    Path((owner, name, id)): Path<(String, String, String)>,
    headers: HeaderMap,
    CollabJson(req): CollabJson<UpdateTaskRequest>,
) -> Result<Json<TaskResponse>, AppError> {
    let (repo, user_id) = require_collab_write(&state, &headers, &owner, &name).await?;
    let _actor = acting_session(&state, &headers, repo.id, user_id).await?;
    require_known::<TaskStatus>("status", req.status.as_deref())?;
    let task = svc::update(
        &state.db,
        repo.id,
        &id,
        svc::TaskChanges {
            expected_version: req.expected_version,
            title: req.title,
            description: req.description,
            status: req.status,
            owner_session_id: req.owner_session_id,
            branch: req.branch,
            paths: req.paths,
            issue_number: req.issue_number,
            pr_number: req.pr_number,
        },
        Utc::now(),
    )
    .await?;
    Ok(Json(task.into()))
}

#[utoipa::path(
    get,
    path = "/repos/{owner}/{name}/collaboration/tasks/{id}/completion-status",
    tag = "Collaboration",
    params(("owner" = String, Path), ("name" = String, Path), ("id" = String, Path)),
    responses(
        (status = 200, description = "What the task claims versus what platform records support: per-condition satisfied | unsatisfied | stale | unknown. Read-only; never blocks or changes the task", body = serde_json::Value),
        (status = 403, description = "Read access denied"),
        (status = 404, description = "No such task in this repository")
    )
)]
pub async fn get_completion_status(
    State(state): State<AppState>,
    Path((owner, name, id)): Path<(String, String, String)>,
    headers: HeaderMap,
) -> Result<Json<collaboration::completion::CompletionStatus>, AppError> {
    let (repo, _user) = require_collab_read(&state, &headers, &owner, &name).await?;
    let task = svc::get(&state.db, repo.id, &id).await?;
    let head =
        super::collab::observed_branch_head(&state, &owner, &name, task.branch.as_deref()).await;
    let status =
        collaboration::completion::evaluate(&state.db, repo.id, &id, head, Utc::now()).await?;
    Ok(Json(status))
}
