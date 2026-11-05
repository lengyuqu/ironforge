//! Task completion conditions (O04): versioned definitions, manual
//! confirmations and evaluation snapshots. Display only: nothing here blocks or
//! changes a task.

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use chrono::Utc;
use rg_core::collaboration::{conditions as svc, task};
use serde::Deserialize;
use utoipa::ToSchema;

use super::collab::{observed_branch_head, require_collab_read, require_collab_write, write_actor};
use super::collab_json::CollabJson;
use crate::error::AppError;
use crate::AppState;

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ConditionBody {
    /// Keep an existing condition's identity by sending its id; omit to add one.
    pub id: Option<String>,
    /// `question_answered` | `work_package_present` | `ci_passed_for_commit` | `manual_confirmation`.
    pub r#type: String,
    /// Up to 200 characters.
    pub label: Option<String>,
    /// Per type: `{question_id}`, `{work_package_id}`, `{commit_sha}` or `{}`.
    #[serde(default)]
    #[schema(value_type = Object)]
    pub params: serde_json::Value,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PutConditionsRequest {
    /// Idempotency key: 1–128 characters of `[A-Za-z0-9._:-]`.
    pub client_request_id: String,
    /// The `version` last read (0 = none defined yet).
    pub expected_version: i64,
    /// The complete new definition, at most 20 conditions; `[]` clears it.
    pub conditions: Vec<ConditionBody>,
}

#[utoipa::path(
    get,
    path = "/repos/{owner}/{name}/collaboration/tasks/{id}/completion-conditions",
    tag = "Collaboration",
    params(("owner" = String, Path), ("name" = String, Path), ("id" = String, Path)),
    responses(
        (status = 200, description = "{task_id, version, conditions:[{id, type, label, params, introduced_in_version, created_by_kind, created_by_id}], created_by_kind, created_by_id, created_at, origin}. version 0 = none defined", body = serde_json::Value),
        (status = 403, description = "Read access denied"),
        (status = 404, description = "No such task in this repository")
    )
)]
pub async fn get_conditions(
    State(state): State<AppState>,
    Path((owner, name, id)): Path<(String, String, String)>,
    headers: HeaderMap,
) -> Result<Json<svc::ConditionSetView>, AppError> {
    let (repo, _user) = require_collab_read(&state, &headers, &owner, &name).await?;
    Ok(Json(svc::get_current(&state.db, repo.id, &id).await?))
}

#[utoipa::path(
    put,
    path = "/repos/{owner}/{name}/collaboration/tasks/{id}/completion-conditions",
    tag = "Collaboration",
    params(("owner" = String, Path), ("name" = String, Path), ("id" = String, Path)),
    request_body = PutConditionsRequest,
    responses(
        (status = 201, description = "A new version was stored", body = serde_json::Value),
        (status = 200, description = "Replay of an earlier identical request", body = serde_json::Value),
        (status = 400, description = "Invalid condition, unknown param or reference outside this task"),
        (status = 403, description = "Write access denied or invalid session credentials"),
        (status = 404, description = "No such task in this repository"),
        (status = 409, description = "VERSION_CONFLICT (details.current_version) or IDEMPOTENCY_KEY_REUSED")
    )
)]
pub async fn put_conditions(
    State(state): State<AppState>,
    Path((owner, name, id)): Path<(String, String, String)>,
    headers: HeaderMap,
    CollabJson(req): CollabJson<PutConditionsRequest>,
) -> Result<(StatusCode, Json<svc::SetCreated>), AppError> {
    let (repo, user_id) = require_collab_write(&state, &headers, &owner, &name).await?;
    let actor = write_actor(&state, &headers, repo.id, user_id).await?;
    let created = svc::set_conditions(
        &state.db,
        repo.id,
        &id,
        &actor,
        svc::NewConditionSet {
            client_request_id: req.client_request_id,
            expected_version: req.expected_version,
            conditions: req
                .conditions
                .into_iter()
                .map(|c| svc::ConditionInput {
                    id: c.id,
                    r#type: c.r#type,
                    label: c.label,
                    params: c.params,
                })
                .collect(),
        },
        Utc::now(),
    )
    .await?;
    let status = if created.replayed {
        StatusCode::OK
    } else {
        StatusCode::CREATED
    };
    Ok((status, Json(created)))
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct VersionsQuery {
    /// 1–100, default 50.
    pub limit: Option<u64>,
    /// Only versions older than this one.
    pub before_version: Option<i64>,
}

#[utoipa::path(
    get,
    path = "/repos/{owner}/{name}/collaboration/tasks/{id}/completion-conditions/versions",
    tag = "Collaboration",
    params(("owner" = String, Path), ("name" = String, Path), ("id" = String, Path), VersionsQuery),
    responses(
        (status = 200, description = "{items: [every stored version of the definition, newest first]}. Versions are immutable; page back with before_version", body = serde_json::Value),
        (status = 403, description = "Read access denied"),
        (status = 404, description = "No such task in this repository")
    )
)]
pub async fn list_versions(
    State(state): State<AppState>,
    Path((owner, name, id)): Path<(String, String, String)>,
    Query(query): Query<VersionsQuery>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, AppError> {
    let (repo, _user) = require_collab_read(&state, &headers, &owner, &name).await?;
    let items = svc::list_versions(
        &state.db,
        repo.id,
        &id,
        query.before_version,
        rg_core::collaboration::clamp_limit(query.limit),
    )
    .await?;
    Ok(Json(serde_json::json!({ "items": items })))
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ConfirmRequest {
    /// Idempotency key: 1–128 characters of `[A-Za-z0-9._:-]`.
    pub client_request_id: String,
    /// Up to 1000 bytes.
    pub note: Option<String>,
}

#[utoipa::path(
    post,
    path = "/repos/{owner}/{name}/collaboration/tasks/{id}/completion-conditions/{condition_id}/confirmations",
    tag = "Collaboration",
    params(("owner" = String, Path), ("name" = String, Path), ("id" = String, Path), ("condition_id" = String, Path)),
    request_body = ConfirmRequest,
    responses(
        (status = 201, description = "Confirmation recorded against the branch head observed now", body = serde_json::Value),
        (status = 200, description = "Replay of an earlier identical request", body = serde_json::Value),
        (status = 403, description = "Write access denied"),
        (status = 404, description = "No such task, or no manual_confirmation condition with this id in the current definition"),
        (status = 409, description = "IDEMPOTENCY_KEY_REUSED")
    )
)]
pub async fn confirm_condition(
    State(state): State<AppState>,
    Path((owner, name, id, condition_id)): Path<(String, String, String, String)>,
    headers: HeaderMap,
    CollabJson(req): CollabJson<ConfirmRequest>,
) -> Result<(StatusCode, Json<svc::ConfirmationCreated>), AppError> {
    // An explicit confirmation by an authorised account (the user door). It is
    // a collaboration record, not a merge approval.
    let (repo, user_id) = require_collab_write(&state, &headers, &owner, &name).await?;
    let row = task::find(&state.db, repo.id, &id).await?;
    let head = observed_branch_head(&state, &owner, &name, row.branch.as_deref()).await;
    let created = svc::confirm(
        &state.db,
        repo.id,
        &row.id,
        &condition_id,
        user_id,
        svc::NewConfirmation {
            client_request_id: req.client_request_id,
            note: req.note,
        },
        head,
        Utc::now(),
    )
    .await?;
    let status = if created.replayed {
        StatusCode::OK
    } else {
        StatusCode::CREATED
    };
    Ok((status, Json(created)))
}

#[utoipa::path(
    post,
    path = "/repos/{owner}/{name}/collaboration/tasks/{id}/completion-status/snapshots",
    tag = "Collaboration",
    params(("owner" = String, Path), ("name" = String, Path), ("id" = String, Path)),
    responses(
        (status = 201, description = "The evaluation now, stored unchanged as an immutable snapshot", body = serde_json::Value),
        (status = 403, description = "Write access denied or invalid session credentials"),
        (status = 404, description = "No such task in this repository")
    )
)]
pub async fn create_snapshot(
    State(state): State<AppState>,
    Path((owner, name, id)): Path<(String, String, String)>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<svc::SnapshotView>), AppError> {
    let (repo, user_id) = require_collab_write(&state, &headers, &owner, &name).await?;
    let actor = write_actor(&state, &headers, repo.id, user_id).await?;
    let row = task::find(&state.db, repo.id, &id).await?;
    let head = observed_branch_head(&state, &owner, &name, row.branch.as_deref()).await;
    let snapshot =
        svc::record_snapshot(&state.db, repo.id, &row.id, &actor, head, Utc::now()).await?;
    Ok((StatusCode::CREATED, Json(snapshot)))
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct SnapshotsQuery {
    /// 1–100, default 50 (newest first; no cursor).
    pub limit: Option<u64>,
}

#[utoipa::path(
    get,
    path = "/repos/{owner}/{name}/collaboration/tasks/{id}/completion-status/snapshots",
    tag = "Collaboration",
    params(("owner" = String, Path), ("name" = String, Path), ("id" = String, Path), SnapshotsQuery),
    responses(
        (status = 200, description = "{items: [snapshot summaries, newest first]}; read one for its stored result", body = serde_json::Value),
        (status = 403, description = "Read access denied"),
        (status = 404, description = "No such task in this repository")
    )
)]
pub async fn list_snapshots(
    State(state): State<AppState>,
    Path((owner, name, id)): Path<(String, String, String)>,
    Query(query): Query<SnapshotsQuery>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, AppError> {
    let (repo, _user) = require_collab_read(&state, &headers, &owner, &name).await?;
    let items = svc::list_snapshots(
        &state.db,
        repo.id,
        &id,
        rg_core::collaboration::clamp_limit(query.limit),
    )
    .await?;
    Ok(Json(serde_json::json!({ "items": items })))
}

#[utoipa::path(
    get,
    path = "/repos/{owner}/{name}/collaboration/tasks/{id}/completion-status/snapshots/{snapshot_id}",
    tag = "Collaboration",
    params(("owner" = String, Path), ("name" = String, Path), ("id" = String, Path), ("snapshot_id" = String, Path)),
    responses(
        (status = 200, description = "A stored evaluation, exactly as recorded", body = serde_json::Value),
        (status = 403, description = "Read access denied"),
        (status = 404, description = "No such task or snapshot in this repository")
    )
)]
pub async fn get_snapshot(
    State(state): State<AppState>,
    Path((owner, name, id, snapshot_id)): Path<(String, String, String, String)>,
    headers: HeaderMap,
) -> Result<Json<svc::SnapshotView>, AppError> {
    let (repo, _user) = require_collab_read(&state, &headers, &owner, &name).await?;
    Ok(Json(
        svc::get_snapshot(&state.db, repo.id, &id, &snapshot_id).await?,
    ))
}
