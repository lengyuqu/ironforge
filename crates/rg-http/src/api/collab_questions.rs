//! Question and answer endpoints.
//!
//! Sessions ask and answer with their credentials; a user can ask and answer
//! through the account alone (no session headers). Both act under one account,
//! so an answer is a collaboration decision, never a merge approval.

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use chrono::Utc;
use rg_core::collaboration::{self, question as svc};
use serde::Deserialize;
use utoipa::ToSchema;

use super::collab::{
    cursor_key, ensure_commit, observed_branch_head, require_collab_read, require_collab_write,
    write_actor,
};
use crate::error::AppError;
use crate::AppState;

#[derive(Debug, Deserialize, ToSchema)]
pub struct RecipientBody {
    /// `repo_writers`, `user` (id = user id) or `session` (id = session id).
    pub kind: String,
    pub id: Option<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct ContextBody {
    /// Must equal the task's current version when given; defaults to it.
    pub task_version: Option<i64>,
    /// Full commit ids that exist in this repository (existence only).
    pub base_sha: Option<String>,
    pub head_sha: Option<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateQuestionRequest {
    /// Idempotency key, unique per asking session or user.
    pub client_question_id: String,
    pub recipient: RecipientBody,
    /// 1–200 characters.
    pub title: String,
    /// 1 byte to 16 KiB.
    pub body: String,
    /// At most 10 labels; the server assigns ids `o1`, `o2`, ...
    #[serde(default)]
    pub options: Vec<String>,
    #[serde(default)]
    pub context: Option<ContextBody>,
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct QuestionListQuery {
    /// `open`, `answered` or `cancelled`.
    pub status: Option<String>,
    pub limit: Option<u64>,
    pub cursor: Option<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct AnswerRequest {
    pub client_answer_id: String,
    /// The question `version` the caller last saw.
    pub expected_version: i64,
    /// Up to 16 KiB; required unless the question has options.
    #[serde(default)]
    pub body: String,
    /// Required when the question has options.
    pub option_id: Option<String>,
    /// `context_hash` of the question as read; proves the context was seen.
    pub acknowledged_context_hash: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CancelQuestionRequest {
    pub expected_version: i64,
}

#[utoipa::path(
    post,
    path = "/repos/{owner}/{name}/collaboration/tasks/{id}/questions",
    tag = "Collaboration",
    params(("owner" = String, Path), ("name" = String, Path), ("id" = String, Path)),
    request_body = CreateQuestionRequest,
    responses(
        (status = 201, description = "Question opened", body = serde_json::Value),
        (status = 200, description = "Identical earlier request replayed", body = serde_json::Value),
        (status = 400, description = "Invalid field, recipient or commit"),
        (status = 403, description = "Write access denied or invalid session credentials"),
        (status = 404, description = "No such task in this repository"),
        (status = 409, description = "VERSION_CONFLICT, IDEMPOTENCY_KEY_REUSED or SESSION_EXPIRED")
    )
)]
pub async fn create_question(
    State(state): State<AppState>,
    Path((owner, name, id)): Path<(String, String, String)>,
    headers: HeaderMap,
    Json(req): Json<CreateQuestionRequest>,
) -> Result<(StatusCode, Json<svc::QuestionView>), AppError> {
    let (repo, user_id) = require_collab_write(&state, &headers, &owner, &name).await?;
    let actor = write_actor(&state, &headers, repo.id, user_id).await?;
    let ctx = req.context.unwrap_or(ContextBody {
        task_version: None,
        base_sha: None,
        head_sha: None,
    });
    let base_sha = ensure_commit(&state, &owner, &name, "context.base_sha", ctx.base_sha.as_deref()).await?;
    let head_sha = ensure_commit(&state, &owner, &name, "context.head_sha", ctx.head_sha.as_deref()).await?;
    let created = svc::create(
        &state.db,
        repo.id,
        &id,
        &actor,
        svc::NewQuestion {
            client_question_id: req.client_question_id,
            recipient: svc::Recipient {
                kind: req.recipient.kind,
                id: req.recipient.id,
            },
            title: req.title,
            body: req.body,
            options: req.options,
            task_version: ctx.task_version,
            base_sha,
            head_sha,
        },
        Utc::now(),
    )
    .await?;
    let status = if created.replayed { StatusCode::OK } else { StatusCode::CREATED };
    Ok((status, Json(created.question)))
}

#[utoipa::path(
    get,
    path = "/repos/{owner}/{name}/collaboration/tasks/{id}/questions",
    tag = "Collaboration",
    params(("owner" = String, Path), ("name" = String, Path), ("id" = String, Path), QuestionListQuery),
    responses(
        (status = 200, description = "Questions by creation time: {items, next_cursor}; items carry no answer or freshness (read one question for those)", body = serde_json::Value),
        (status = 400, description = "Invalid status or cursor"),
        (status = 403, description = "Read access denied"),
        (status = 404, description = "No such task in this repository")
    )
)]
pub async fn list_questions(
    State(state): State<AppState>,
    Path((owner, name, id)): Path<(String, String, String)>,
    Query(query): Query<QuestionListQuery>,
    headers: HeaderMap,
) -> Result<Json<collaboration::Page<svc::QuestionView>>, AppError> {
    let (repo, _user) = require_collab_read(&state, &headers, &owner, &name).await?;
    let page = svc::list(
        &state.db,
        cursor_key(&state),
        repo.id,
        &id,
        query.status.as_deref(),
        collaboration::clamp_limit(query.limit),
        query.cursor.as_deref(),
    )
    .await?;
    Ok(Json(page))
}

/// Observed head of the declared branch of the task a question belongs to.
async fn head_for(
    state: &AppState,
    owner: &str,
    name: &str,
    repo_id: i64,
    question_id: &str,
) -> Option<String> {
    let question = svc::find(&state.db, repo_id, question_id).await.ok()?;
    let task = collaboration::task::find(&state.db, repo_id, &question.task_id)
        .await
        .ok()?;
    observed_branch_head(state, owner, name, task.branch.as_deref()).await
}

#[utoipa::path(
    get,
    path = "/repos/{owner}/{name}/collaboration/questions/{id}",
    tag = "Collaboration",
    params(("owner" = String, Path), ("name" = String, Path), ("id" = String, Path)),
    responses(
        (status = 200, description = "Question with its answer and `context_freshness` (fresh | stale | unknown)", body = serde_json::Value),
        (status = 403, description = "Read access denied"),
        (status = 404, description = "No such question in this repository")
    )
)]
pub async fn get_question(
    State(state): State<AppState>,
    Path((owner, name, id)): Path<(String, String, String)>,
    headers: HeaderMap,
) -> Result<Json<svc::QuestionView>, AppError> {
    let (repo, _user) = require_collab_read(&state, &headers, &owner, &name).await?;
    let head = head_for(&state, &owner, &name, repo.id, &id).await;
    let question = svc::get(&state.db, repo.id, &id, |_| head).await?;
    Ok(Json(question))
}

#[utoipa::path(
    post,
    path = "/repos/{owner}/{name}/collaboration/questions/{id}/answer",
    tag = "Collaboration",
    params(("owner" = String, Path), ("name" = String, Path), ("id" = String, Path)),
    request_body = AnswerRequest,
    responses(
        (status = 200, description = "Answered (or an identical earlier answer replayed)", body = serde_json::Value),
        (status = 400, description = "Invalid option, empty answer or wrong acknowledged_context_hash"),
        (status = 403, description = "Write access denied, invalid session credentials or not the addressed recipient"),
        (status = 404, description = "No such question in this repository"),
        (status = 409, description = "QUESTION_NOT_OPEN, CONTEXT_STALE, VERSION_CONFLICT, IDEMPOTENCY_KEY_REUSED or SESSION_EXPIRED")
    )
)]
pub async fn answer_question(
    State(state): State<AppState>,
    Path((owner, name, id)): Path<(String, String, String)>,
    headers: HeaderMap,
    Json(req): Json<AnswerRequest>,
) -> Result<Json<svc::QuestionView>, AppError> {
    let (repo, user_id) = require_collab_write(&state, &headers, &owner, &name).await?;
    let actor = write_actor(&state, &headers, repo.id, user_id).await?;
    let head = head_for(&state, &owner, &name, repo.id, &id).await;
    let answered = svc::answer(
        &state.db,
        repo.id,
        &id,
        &actor,
        svc::NewAnswer {
            client_answer_id: req.client_answer_id,
            expected_version: req.expected_version,
            body: req.body,
            option_id: req.option_id,
            acknowledged_context_hash: req.acknowledged_context_hash,
        },
        |_| head,
        Utc::now(),
    )
    .await?;
    Ok(Json(answered.question))
}

#[utoipa::path(
    post,
    path = "/repos/{owner}/{name}/collaboration/questions/{id}/cancel",
    tag = "Collaboration",
    params(("owner" = String, Path), ("name" = String, Path), ("id" = String, Path)),
    request_body = CancelQuestionRequest,
    responses(
        (status = 200, description = "Question cancelled", body = serde_json::Value),
        (status = 403, description = "Write access denied or not the creator"),
        (status = 404, description = "No such question in this repository"),
        (status = 409, description = "QUESTION_NOT_OPEN, VERSION_CONFLICT or SESSION_EXPIRED")
    )
)]
pub async fn cancel_question(
    State(state): State<AppState>,
    Path((owner, name, id)): Path<(String, String, String)>,
    headers: HeaderMap,
    Json(req): Json<CancelQuestionRequest>,
) -> Result<Json<svc::QuestionView>, AppError> {
    let (repo, user_id) = require_collab_write(&state, &headers, &owner, &name).await?;
    let actor = write_actor(&state, &headers, repo.id, user_id).await?;
    let question =
        svc::cancel(&state.db, repo.id, &id, &actor, req.expected_version, Utc::now()).await?;
    Ok(Json(question))
}
