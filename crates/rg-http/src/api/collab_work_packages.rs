//! Work package endpoints: immutable, numbered hand-over summaries of a task.

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use chrono::Utc;
use rg_core::collaboration::{self, work_package as svc};
use serde::Deserialize;
use utoipa::ToSchema;

use super::collab::{
    cursor_key, ensure_commit, require_collab_read, require_collab_write, write_actor, ListQuery,
};
use super::collab_json::CollabJson;
use crate::error::AppError;
use crate::AppState;

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct EvidenceRefBody {
    /// `pipeline` (a pipeline id of this repository), `commit`, `url` or `note`.
    pub kind: String,
    pub reference: String,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateWorkPackageRequest {
    /// Idempotency key, unique per author session or user.
    pub client_package_id: String,
    /// When given, must equal the task's current version.
    pub expected_task_version: Option<i64>,
    /// Full commit id that exists in this repository (existence only).
    pub commit_sha: Option<String>,
    #[serde(default)]
    pub goal: String,
    #[serde(default)]
    pub scope: String,
    #[serde(default)]
    pub artifacts: Vec<String>,
    #[serde(default)]
    pub completed: Vec<String>,
    #[serde(default)]
    pub open_items: Vec<String>,
    #[serde(default)]
    pub next_steps: Vec<String>,
    /// At most 50. Only `pipeline` references are resolved by the platform.
    #[serde(default)]
    pub evidence_refs: Vec<EvidenceRefBody>,
}

#[utoipa::path(
    post,
    path = "/repos/{owner}/{name}/collaboration/tasks/{id}/work-packages",
    tag = "Collaboration",
    params(("owner" = String, Path), ("name" = String, Path), ("id" = String, Path)),
    request_body = CreateWorkPackageRequest,
    responses(
        (status = 201, description = "New immutable revision", body = serde_json::Value),
        (status = 200, description = "Identical earlier request replayed", body = serde_json::Value),
        (status = 400, description = "Invalid field or commit"),
        (status = 403, description = "Write access denied or invalid session credentials"),
        (status = 404, description = "No such task in this repository"),
        (status = 409, description = "VERSION_CONFLICT, IDEMPOTENCY_KEY_REUSED or SESSION_EXPIRED")
    )
)]
pub async fn create_work_package(
    State(state): State<AppState>,
    Path((owner, name, id)): Path<(String, String, String)>,
    headers: HeaderMap,
    CollabJson(req): CollabJson<CreateWorkPackageRequest>,
) -> Result<(StatusCode, Json<svc::PackageView>), AppError> {
    let (repo, user_id) = require_collab_write(&state, &headers, &owner, &name).await?;
    let actor = write_actor(&state, &headers, repo.id, user_id).await?;
    let commit_sha = ensure_commit(&state, &owner, &name, "commit_sha", req.commit_sha.as_deref()).await?;
    for e in req.evidence_refs.iter().filter(|e| e.kind == "commit") {
        ensure_commit(&state, &owner, &name, "evidence commit", Some(&e.reference)).await?;
    }
    let created = svc::create(
        &state.db,
        repo.id,
        &id,
        &actor,
        svc::NewPackage {
            client_package_id: req.client_package_id,
            expected_task_version: req.expected_task_version,
            commit_sha,
            body: svc::Body {
                goal: req.goal,
                scope: req.scope,
                artifacts: req.artifacts,
                completed: req.completed,
                open_items: req.open_items,
                next_steps: req.next_steps,
                evidence_refs: req
                    .evidence_refs
                    .into_iter()
                    .map(|e| svc::EvidenceRef { kind: e.kind, reference: e.reference })
                    .collect(),
            },
        },
        Utc::now(),
    )
    .await?;
    let status = if created.replayed { StatusCode::OK } else { StatusCode::CREATED };
    Ok((status, Json(created.package)))
}

#[utoipa::path(
    get,
    path = "/repos/{owner}/{name}/collaboration/tasks/{id}/work-packages",
    tag = "Collaboration",
    params(("owner" = String, Path), ("name" = String, Path), ("id" = String, Path), ListQuery),
    responses(
        (status = 200, description = "Revisions, newest first: {items, next_cursor}", body = serde_json::Value),
        (status = 403, description = "Read access denied"),
        (status = 404, description = "No such task in this repository")
    )
)]
pub async fn list_work_packages(
    State(state): State<AppState>,
    Path((owner, name, id)): Path<(String, String, String)>,
    Query(query): Query<ListQuery>,
    headers: HeaderMap,
) -> Result<Json<collaboration::Page<svc::PackageView>>, AppError> {
    let (repo, _user) = require_collab_read(&state, &headers, &owner, &name).await?;
    let page = svc::list(
        &state.db,
        cursor_key(&state),
        repo.id,
        &id,
        collaboration::clamp_limit(query.limit),
        query.cursor.as_deref(),
    )
    .await?;
    Ok(Json(page))
}

#[utoipa::path(
    get,
    path = "/repos/{owner}/{name}/collaboration/work-packages/{id}",
    tag = "Collaboration",
    params(("owner" = String, Path), ("name" = String, Path), ("id" = String, Path)),
    responses(
        (status = 200, description = "Package with `evidence_resolution`: the platform's lookup of its pipeline references", body = serde_json::Value),
        (status = 403, description = "Read access denied"),
        (status = 404, description = "No such work package in this repository")
    )
)]
pub async fn get_work_package(
    State(state): State<AppState>,
    Path((owner, name, id)): Path<(String, String, String)>,
    headers: HeaderMap,
) -> Result<Json<svc::PackageView>, AppError> {
    let (repo, _user) = require_collab_read(&state, &headers, &owner, &name).await?;
    Ok(Json(svc::get(&state.db, repo.id, &id, Utc::now()).await?))
}
