//! Task template endpoints (O06).
//!
//! Repository admins manage templates; every version is immutable (an edit is a
//! new version). A task created from a template is pinned to the version it was
//! created from and keeps a copy of it, so later edits never change it.

use std::collections::BTreeMap;

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use chrono::Utc;
use rg_core::collaboration::rules;
use rg_core::collaboration::template::{self as svc, ChecklistItem, Content};
use rg_core::collaboration::{self, CollabError};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::collab::{
    acting_session, cursor_key, require_collab_read, require_collab_write, ListQuery,
};
use super::collab_tasks::TaskResponse;
use crate::api::repo_access::resolve_repo;
use crate::error::AppError;
use crate::AppState;

#[derive(Debug, Deserialize, ToSchema)]
pub struct ChecklistItemBody {
    /// One line, at most 256 bytes.
    pub text: String,
    /// Informational; default `true`.
    #[serde(default = "default_true")]
    pub required: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct TemplateContentBody {
    /// Task title with `{name}` placeholders (A-Za-z0-9_, at most 32); at most
    /// 200 characters once rendered. Other braces are rejected.
    pub title_pattern: String,
    /// 1–2048 bytes.
    pub goal: String,
    /// At most 20 single-line entries.
    #[serde(default)]
    pub deliverables: Vec<String>,
    /// Acceptance checklist, at most 30 entries. Text only: not tracked, not
    /// evidence.
    #[serde(default)]
    pub checklist: Vec<ChecklistItemBody>,
}

impl From<TemplateContentBody> for Content {
    fn from(b: TemplateContentBody) -> Self {
        Content {
            title_pattern: b.title_pattern,
            goal: b.goal,
            deliverables: b.deliverables,
            checklist: b
                .checklist
                .into_iter()
                .map(|i| ChecklistItem { text: i.text, required: i.required })
                .collect(),
        }
    }
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateTemplateRequest {
    /// Idempotency key, unique per creating account.
    pub client_template_id: String,
    /// 1–100 characters, unique in the repository.
    pub name: String,
    #[serde(flatten)]
    pub content: TemplateContentBody,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct NewTemplateVersionRequest {
    /// Idempotency key, unique per creating account.
    pub client_version_id: String,
    /// The template's `latest_version` the editor saw; a stale value is a
    /// `VERSION_CONFLICT`.
    pub expected_version: i64,
    #[serde(flatten)]
    pub content: TemplateContentBody,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct FromTemplateRequest {
    /// Idempotency key, unique per creating session.
    pub client_task_id: String,
    pub template_id: String,
    /// Pin this version; default the latest at the time of the request.
    pub version: Option<i64>,
    /// Values for the title pattern's placeholders, exactly those names.
    #[serde(default)]
    pub params: BTreeMap<String, String>,
    /// Free text appended to the task description (up to 4 KiB).
    pub notes: Option<String>,
    /// A live session of this repository; omit for an unowned task.
    pub owner_session_id: Option<String>,
    /// Declared working branch (not checked against Git).
    pub branch: Option<String>,
    #[serde(default)]
    pub paths: Vec<String>,
    /// Optional branch, tag or commit. When given, the commit it resolves to now
    /// is recorded on the task as `rules_commit` (O01); an unresolvable ref is a
    /// 400. This records which rules were current, not that anyone read them.
    pub rules_ref: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct FromTemplateResponse {
    pub task: TaskResponse,
    pub template_link: svc::LinkView,
    pub replayed: bool,
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct TemplateVersionQuery {
    /// Version to read; default the latest.
    pub version: Option<i64>,
}

/// Repository administrator, decided from the database (not the 30 s permission
/// cache) so a revocation blocks the very next request.
async fn require_template_admin(
    state: &AppState,
    headers: &HeaderMap,
    owner: &str,
    name: &str,
) -> Result<(rg_db::entities::repository::Model, i64), AppError> {
    let actor_id = crate::api::auth::extract_user_id(headers, &state.jwt_secret)
        .ok_or_else(|| AppError::unauthorized("authentication required"))?;
    let repo = resolve_repo(state, owner, name).await?;
    rg_core::repo::service::invalidate_perm_cache_user(repo.id, actor_id);
    match rg_core::repo::service::can_admin_repo(&state.db, &repo, Some(actor_id)).await {
        Ok(true) => Ok((repo, actor_id)),
        Ok(false) => Err(AppError::forbidden("repository admin access required")),
        Err(e) => Err(AppError::from(e)),
    }
}

#[utoipa::path(
    post,
    path = "/repos/{owner}/{name}/collaboration/templates",
    tag = "Collaboration",
    params(("owner" = String, Path), ("name" = String, Path)),
    request_body = CreateTemplateRequest,
    responses(
        (status = 201, description = "Template created with version 1", body = serde_json::Value),
        (status = 200, description = "Identical earlier request replayed", body = serde_json::Value),
        (status = 400, description = "Invalid field"),
        (status = 403, description = "Repository admin access required"),
        (status = 409, description = "TEMPLATE_NAME_TAKEN or IDEMPOTENCY_KEY_REUSED")
    )
)]
pub async fn create_template(
    State(state): State<AppState>,
    Path((owner, name)): Path<(String, String)>,
    headers: HeaderMap,
    Json(req): Json<CreateTemplateRequest>,
) -> Result<(StatusCode, Json<svc::TemplateView>), AppError> {
    let (repo, user_id) = require_template_admin(&state, &headers, &owner, &name).await?;
    let created = svc::create(
        &state.db,
        repo.id,
        user_id,
        svc::NewTemplate {
            client_template_id: req.client_template_id,
            name: req.name,
            content: req.content.into(),
        },
        Utc::now(),
    )
    .await?;
    let status = if created.replayed { StatusCode::OK } else { StatusCode::CREATED };
    Ok((status, Json(created.template)))
}

#[utoipa::path(
    post,
    path = "/repos/{owner}/{name}/collaboration/templates/{id}/versions",
    tag = "Collaboration",
    params(("owner" = String, Path), ("name" = String, Path), ("id" = String, Path)),
    request_body = NewTemplateVersionRequest,
    responses(
        (status = 201, description = "New immutable version; older versions and tasks made from them are unchanged", body = serde_json::Value),
        (status = 200, description = "Identical earlier request replayed", body = serde_json::Value),
        (status = 400, description = "Invalid field"),
        (status = 403, description = "Repository admin access required"),
        (status = 404, description = "No such template in this repository"),
        (status = 409, description = "VERSION_CONFLICT (error.details.current_version) or IDEMPOTENCY_KEY_REUSED")
    )
)]
pub async fn new_template_version(
    State(state): State<AppState>,
    Path((owner, name, id)): Path<(String, String, String)>,
    headers: HeaderMap,
    Json(req): Json<NewTemplateVersionRequest>,
) -> Result<(StatusCode, Json<svc::TemplateView>), AppError> {
    let (repo, user_id) = require_template_admin(&state, &headers, &owner, &name).await?;
    let created = svc::new_version(
        &state.db,
        repo.id,
        user_id,
        &id,
        svc::NewVersion {
            client_version_id: req.client_version_id,
            expected_version: req.expected_version,
            content: req.content.into(),
        },
        Utc::now(),
    )
    .await?;
    let status = if created.replayed { StatusCode::OK } else { StatusCode::CREATED };
    Ok((status, Json(created.template)))
}

#[utoipa::path(
    get,
    path = "/repos/{owner}/{name}/collaboration/templates",
    tag = "Collaboration",
    params(("owner" = String, Path), ("name" = String, Path), ListQuery),
    responses(
        (status = 200, description = "Templates newest first, each with its latest version: {items, next_cursor}", body = serde_json::Value),
        (status = 403, description = "Read access denied")
    )
)]
pub async fn list_templates(
    State(state): State<AppState>,
    Path((owner, name)): Path<(String, String)>,
    Query(query): Query<ListQuery>,
    headers: HeaderMap,
) -> Result<Json<collaboration::Page<svc::TemplateView>>, AppError> {
    let (repo, _user) = require_collab_read(&state, &headers, &owner, &name).await?;
    let page = svc::list(
        &state.db,
        cursor_key(&state),
        repo.id,
        collaboration::clamp_limit(query.limit),
        query.cursor.as_deref(),
    )
    .await?;
    Ok(Json(page))
}

#[utoipa::path(
    get,
    path = "/repos/{owner}/{name}/collaboration/templates/{id}",
    tag = "Collaboration",
    params(("owner" = String, Path), ("name" = String, Path), ("id" = String, Path), TemplateVersionQuery),
    responses(
        (status = 200, description = "The template with the requested version (default latest)", body = serde_json::Value),
        (status = 403, description = "Read access denied"),
        (status = 404, description = "No such template or version in this repository")
    )
)]
pub async fn get_template(
    State(state): State<AppState>,
    Path((owner, name, id)): Path<(String, String, String)>,
    Query(query): Query<TemplateVersionQuery>,
    headers: HeaderMap,
) -> Result<Json<svc::TemplateView>, AppError> {
    let (repo, _user) = require_collab_read(&state, &headers, &owner, &name).await?;
    Ok(Json(svc::get(&state.db, repo.id, &id, query.version).await?))
}

#[utoipa::path(
    post,
    path = "/repos/{owner}/{name}/collaboration/tasks/from-template",
    tag = "Collaboration",
    params(("owner" = String, Path), ("name" = String, Path)),
    request_body = FromTemplateRequest,
    responses(
        (status = 201, description = "Task created and pinned to the template version; the checklist is copied into the description", body = serde_json::Value),
        (status = 200, description = "Identical earlier request replayed", body = serde_json::Value),
        (status = 400, description = "Invalid field, params not matching the title pattern, or unresolvable rules_ref"),
        (status = 403, description = "Write access denied or invalid session credentials"),
        (status = 404, description = "No such template or version in this repository"),
        (status = 409, description = "IDEMPOTENCY_KEY_REUSED or SESSION_EXPIRED")
    )
)]
pub async fn create_task_from_template(
    State(state): State<AppState>,
    Path((owner, name)): Path<(String, String)>,
    headers: HeaderMap,
    Json(req): Json<FromTemplateRequest>,
) -> Result<(StatusCode, Json<FromTemplateResponse>), AppError> {
    let (repo, user_id) = require_collab_write(&state, &headers, &owner, &name).await?;
    let actor = acting_session(&state, &headers, repo.id, user_id).await?;
    let input = svc::FromTemplate {
        client_task_id: req.client_task_id,
        template_id: req.template_id,
        version: req.version,
        params: req.params,
        notes: req.notes,
        owner_session_id: req.owner_session_id,
        branch: req.branch,
        paths: req.paths,
        rules_ref: req.rules_ref,
    };
    let respond = |done: svc::CreatedFromTemplate| {
        let status = if done.replayed { StatusCode::OK } else { StatusCode::CREATED };
        (
            status,
            Json(FromTemplateResponse {
                task: done.task.into(),
                template_link: done.link,
                replayed: done.replayed,
            }),
        )
    };
    // A retry is answered before anything it depends on is looked at again.
    if let Some(done) = svc::replay_from_template(&state.db, repo.id, &actor, &input).await? {
        return Ok(respond(done));
    }
    let rules_commit = match input.rules_ref.as_deref() {
        None => None,
        Some(git_ref) => {
            let repo_path = state.repo_root.join(format!("{owner}/{name}.git"));
            let git_ref = git_ref.to_string();
            let found = tokio::task::spawn_blocking(move || {
                rules::discover(&repo_path, Some(&git_ref), "")
            })
            .await
            .map_err(|e| AppError::internal(format!("rules lookup task failed: {e}")))?;
            match found {
                Ok(found) => found.resolved_commit,
                Err(CollabError::NotFound(_)) | Err(CollabError::BadRequest(_)) => {
                    return Err(AppError::bad_request(
                        "rules_ref does not resolve to a commit of this repository",
                    ));
                }
                Err(other) => return Err(other.into()),
            }
        }
    };
    let done = svc::create_from_template(&state.db, repo.id, &actor, input, rules_commit, Utc::now())
        .await?;
    Ok(respond(done))
}

#[utoipa::path(
    get,
    path = "/repos/{owner}/{name}/collaboration/tasks/{id}/template",
    tag = "Collaboration",
    params(("owner" = String, Path), ("name" = String, Path), ("id" = String, Path)),
    responses(
        (status = 200, description = "The pinned template version, the copy the task was created from, `rules_commit`, and whether a newer version exists now", body = serde_json::Value),
        (status = 403, description = "Read access denied"),
        (status = 404, description = "No such task, or the task was not created from a template")
    )
)]
pub async fn get_task_template(
    State(state): State<AppState>,
    Path((owner, name, id)): Path<(String, String, String)>,
    headers: HeaderMap,
) -> Result<Json<svc::LinkView>, AppError> {
    let (repo, _user) = require_collab_read(&state, &headers, &owner, &name).await?;
    Ok(Json(svc::get_link(&state.db, repo.id, &id).await?))
}
