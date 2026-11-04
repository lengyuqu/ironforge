//! Decision endpoints: immutable decision records on a task, with at most one
//! superseding or revoking change each.
//!
//! A decision is a collaboration record written under the account. It is not an
//! approval, a review or a merge authorization and grants no Git, CI or
//! branch-protection right; a stale decision is reported as such and never
//! revoked automatically.

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use chrono::Utc;
use rg_core::collaboration::{self, decision as svc};
use serde::Deserialize;
use utoipa::ToSchema;

use super::collab::{
    cursor_key, ensure_commit, observed_branch_head, require_collab_read, require_collab_write,
    write_actor, ListQuery,
};
use crate::error::AppError;
use crate::AppState;

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct DecisionContextBody {
    /// Must equal the task's current version when given; defaults to it.
    pub task_version: Option<i64>,
    /// Full commit ids that exist in this repository (existence only).
    pub base_sha: Option<String>,
    pub head_sha: Option<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateDecisionRequest {
    /// Idempotency key, unique per writing session or user.
    pub client_decision_id: String,
    /// 1–200 characters.
    pub title: String,
    /// 1 byte to 16 KiB. The writer's own account of why; not verified.
    pub rationale: String,
    /// A question of this task. Optional.
    pub question_id: Option<String>,
    /// The answer of `question_id`; requires `question_id`.
    pub answer_id: Option<String>,
    #[serde(default)]
    pub context: Option<DecisionContextBody>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ChangeDecisionRequest {
    /// Idempotency key, unique per writing session or user.
    pub client_change_id: String,
    /// `superseded` or `revoked`.
    pub kind: String,
    /// Required for `superseded`, forbidden for `revoked`. A decision of the
    /// same task recorded after the one it replaces.
    pub replacement_decision_id: Option<String>,
    /// 1 byte to 4 KiB.
    pub reason: String,
}

/// Observed head of the declared branch of a task.
async fn head_for_task(
    state: &AppState,
    owner: &str,
    name: &str,
    repo_id: i64,
    task_id: &str,
) -> Option<String> {
    let task = collaboration::task::find(&state.db, repo_id, task_id).await.ok()?;
    observed_branch_head(state, owner, name, task.branch.as_deref()).await
}

async fn head_for_decision(
    state: &AppState,
    owner: &str,
    name: &str,
    repo_id: i64,
    decision_id: &str,
) -> Option<String> {
    let decision = svc::find(&state.db, repo_id, decision_id).await.ok()?;
    head_for_task(state, owner, name, repo_id, &decision.task_id).await
}

#[utoipa::path(
    post,
    path = "/repos/{owner}/{name}/collaboration/tasks/{id}/decisions",
    tag = "Collaboration",
    params(("owner" = String, Path), ("name" = String, Path), ("id" = String, Path)),
    request_body = CreateDecisionRequest,
    responses(
        (status = 201, description = "Immutable decision recorded. A decision is not an approval and grants no Git, CI or merge right.", body = serde_json::Value),
        (status = 200, description = "Identical earlier request replayed", body = serde_json::Value),
        (status = 400, description = "Invalid field, commit, or a question/answer that does not belong to this task"),
        (status = 403, description = "Write access denied or invalid session credentials"),
        (status = 404, description = "No such task in this repository"),
        (status = 409, description = "VERSION_CONFLICT (context.task_version is not the current version), IDEMPOTENCY_KEY_REUSED or SESSION_EXPIRED")
    )
)]
pub async fn create_decision(
    State(state): State<AppState>,
    Path((owner, name, id)): Path<(String, String, String)>,
    headers: HeaderMap,
    super::collab_json::CollabJson(req): super::collab_json::CollabJson<CreateDecisionRequest>,
) -> Result<(StatusCode, Json<svc::DecisionView>), AppError> {
    let (repo, user_id) = require_collab_write(&state, &headers, &owner, &name).await?;
    let actor = write_actor(&state, &headers, repo.id, user_id).await?;
    let ctx = req.context.unwrap_or(DecisionContextBody {
        task_version: None,
        base_sha: None,
        head_sha: None,
    });
    let base_sha =
        ensure_commit(&state, &owner, &name, "context.base_sha", ctx.base_sha.as_deref()).await?;
    let head_sha =
        ensure_commit(&state, &owner, &name, "context.head_sha", ctx.head_sha.as_deref()).await?;
    let head = head_for_task(&state, &owner, &name, repo.id, &id).await;
    let created = svc::create(
        &state.db,
        repo.id,
        &id,
        &actor,
        svc::NewDecision {
            client_decision_id: req.client_decision_id,
            title: req.title,
            rationale: req.rationale,
            question_id: req.question_id,
            answer_id: req.answer_id,
            task_version: ctx.task_version,
            base_sha,
            head_sha,
        },
        head.as_deref(),
        Utc::now(),
    )
    .await?;
    let status = if created.replayed { StatusCode::OK } else { StatusCode::CREATED };
    Ok((status, Json(created.decision)))
}

#[utoipa::path(
    get,
    path = "/repos/{owner}/{name}/collaboration/tasks/{id}/decisions",
    tag = "Collaboration",
    params(("owner" = String, Path), ("name" = String, Path), ("id" = String, Path), ListQuery),
    responses(
        (status = 200, description = "Decisions by creation time: {items, next_cursor}; each item carries its terminal `change` (if any), `status` and `context_freshness` (fresh | stale | unknown)", body = serde_json::Value),
        (status = 400, description = "Invalid cursor"),
        (status = 403, description = "Read access denied"),
        (status = 404, description = "No such task in this repository")
    )
)]
pub async fn list_decisions(
    State(state): State<AppState>,
    Path((owner, name, id)): Path<(String, String, String)>,
    Query(query): Query<ListQuery>,
    headers: HeaderMap,
) -> Result<Json<collaboration::Page<svc::DecisionView>>, AppError> {
    let (repo, _user) = require_collab_read(&state, &headers, &owner, &name).await?;
    let head = head_for_task(&state, &owner, &name, repo.id, &id).await;
    let page = svc::list(
        &state.db,
        cursor_key(&state),
        repo.id,
        &id,
        |_| head,
        collaboration::clamp_limit(query.limit),
        query.cursor.as_deref(),
    )
    .await?;
    Ok(Json(page))
}

#[utoipa::path(
    get,
    path = "/repos/{owner}/{name}/collaboration/decisions/{id}",
    tag = "Collaboration",
    params(("owner" = String, Path), ("name" = String, Path), ("id" = String, Path)),
    responses(
        (status = 200, description = "Decision with its terminal `change`, `status`, `context_freshness` (fresh | stale | unknown) and `supersedes` (decisions it replaces)", body = serde_json::Value),
        (status = 403, description = "Read access denied"),
        (status = 404, description = "No such decision in this repository")
    )
)]
pub async fn get_decision(
    State(state): State<AppState>,
    Path((owner, name, id)): Path<(String, String, String)>,
    headers: HeaderMap,
) -> Result<Json<svc::DecisionView>, AppError> {
    let (repo, _user) = require_collab_read(&state, &headers, &owner, &name).await?;
    let head = head_for_decision(&state, &owner, &name, repo.id, &id).await;
    let decision = svc::get(&state.db, repo.id, &id, |_| head).await?;
    Ok(Json(decision))
}

#[utoipa::path(
    post,
    path = "/repos/{owner}/{name}/collaboration/decisions/{id}/changes",
    tag = "Collaboration",
    params(("owner" = String, Path), ("name" = String, Path), ("id" = String, Path)),
    request_body = ChangeDecisionRequest,
    responses(
        (status = 201, description = "The decision's one terminal change was recorded; the decision itself is not edited", body = serde_json::Value),
        (status = 200, description = "Identical earlier request replayed", body = serde_json::Value),
        (status = 400, description = "Invalid kind/reason, missing or forbidden replacement, self-reference, or a replacement of another task"),
        (status = 403, description = "Write access denied or invalid session credentials"),
        (status = 404, description = "No such decision in this repository"),
        (status = 409, description = "DECISION_ALREADY_CHANGED, DECISION_CYCLE, REPLACEMENT_NOT_NEWER, IDEMPOTENCY_KEY_REUSED or SESSION_EXPIRED")
    )
)]
pub async fn change_decision(
    State(state): State<AppState>,
    Path((owner, name, id)): Path<(String, String, String)>,
    headers: HeaderMap,
    super::collab_json::CollabJson(req): super::collab_json::CollabJson<ChangeDecisionRequest>,
) -> Result<(StatusCode, Json<svc::DecisionView>), AppError> {
    let (repo, user_id) = require_collab_write(&state, &headers, &owner, &name).await?;
    let actor = write_actor(&state, &headers, repo.id, user_id).await?;
    let head = head_for_decision(&state, &owner, &name, repo.id, &id).await;
    let changed = svc::change(
        &state.db,
        repo.id,
        &id,
        &actor,
        svc::NewChange {
            client_change_id: req.client_change_id,
            kind: req.kind,
            replacement_decision_id: req.replacement_decision_id,
            reason: req.reason,
        },
        |_| head,
        Utc::now(),
    )
    .await?;
    let status = if changed.replayed { StatusCode::OK } else { StatusCode::CREATED };
    Ok((status, Json(changed.decision)))
}
