//! Capability and environment declaration endpoints (O07).
//!
//! A session's self-reported tools, test ability and network limits. Source is
//! always `user_content` and the authorization it carries is always `none`.

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use chrono::Utc;
use rg_core::collaboration::{self, declaration as svc, CollabError};
use serde::Deserialize;
use utoipa::ToSchema;

use super::collab::{acting_session, cursor_key, require_collab_read, require_collab_write};
use crate::error::AppError;
use crate::AppState;

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PutDeclarationRequest {
    /// The `version` of the declaration you last saw; 0 when you have not
    /// declared yet. A stale value is a `VERSION_CONFLICT`. Re-sending the
    /// current version with unchanged content refreshes `declared_at`.
    pub expected_version: i64,
    /// Tools the session says it has, at most 50 labels of up to 128 characters.
    #[serde(default)]
    pub tools: Vec<String>,
    /// `yes` | `no` | `unknown` (default).
    pub can_run_tests: Option<String>,
    /// `unrestricted` | `restricted` | `offline` | `unknown` (default).
    pub network: Option<String>,
    /// Up to 1 KiB, e.g. which hosts are reachable.
    #[serde(default)]
    pub network_notes: String,
    /// Free-form, up to 4 KiB.
    #[serde(default)]
    pub notes: String,
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct StaleQuery {
    /// A declaration older than this is flagged `stale` (default 86400, clamped
    /// to 60..2592000).
    pub stale_after_seconds: Option<i64>,
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct DeclarationListQuery {
    pub stale_after_seconds: Option<i64>,
    pub limit: Option<u64>,
    pub cursor: Option<String>,
}

#[utoipa::path(
    put,
    path = "/repos/{owner}/{name}/collaboration/sessions/{id}/declaration",
    tag = "Collaboration",
    params(("owner" = String, Path), ("name" = String, Path), ("id" = String, Path, description = "must be the authenticated session"), StaleQuery),
    request_body = PutDeclarationRequest,
    responses(
        (status = 200, description = "Declaration replaced (or an identical earlier replace replayed). Self-reported, source user_content, authorization none", body = serde_json::Value),
        (status = 400, description = "Invalid field"),
        (status = 403, description = "Write access denied, or the session credentials do not match the session in the path"),
        (status = 409, description = "VERSION_CONFLICT (error.details.current_version) or SESSION_EXPIRED")
    )
)]
pub async fn put_declaration(
    State(state): State<AppState>,
    Path((owner, name, id)): Path<(String, String, String)>,
    Query(query): Query<StaleQuery>,
    headers: HeaderMap,
    super::collab_json::CollabJson(req): super::collab_json::CollabJson<PutDeclarationRequest>,
) -> Result<(StatusCode, Json<svc::DeclarationView>), AppError> {
    let (repo, user_id) = require_collab_write(&state, &headers, &owner, &name).await?;
    let session = acting_session(&state, &headers, repo.id, user_id).await?;
    if session.id != id {
        // A session declares only for itself.
        return Err(CollabError::SessionCredentials.into());
    }
    let saved = svc::put(
        &state.db,
        repo.id,
        &session,
        svc::NewDeclaration {
            expected_version: req.expected_version,
            tools: req.tools,
            can_run_tests: req.can_run_tests,
            network: req.network,
            network_notes: req.network_notes,
            notes: req.notes,
        },
        svc::stale_threshold(query.stale_after_seconds),
        Utc::now(),
    )
    .await?;
    Ok((StatusCode::OK, Json(saved.declaration)))
}

#[utoipa::path(
    get,
    path = "/repos/{owner}/{name}/collaboration/sessions/{id}/declaration",
    tag = "Collaboration",
    params(("owner" = String, Path), ("name" = String, Path), ("id" = String, Path), StaleQuery),
    responses(
        (status = 200, description = "The session's declaration with staleness, or status `undeclared` (unknown, not 'cannot'). Never an authorization", body = serde_json::Value),
        (status = 403, description = "Read access denied"),
        (status = 404, description = "No such session in this repository")
    )
)]
pub async fn get_declaration(
    State(state): State<AppState>,
    Path((owner, name, id)): Path<(String, String, String)>,
    Query(query): Query<StaleQuery>,
    headers: HeaderMap,
) -> Result<Json<svc::SessionDeclaration>, AppError> {
    let (repo, _user) = require_collab_read(&state, &headers, &owner, &name).await?;
    Ok(Json(
        svc::get(
            &state.db,
            repo.id,
            &id,
            svc::stale_threshold(query.stale_after_seconds),
            Utc::now(),
        )
        .await?,
    ))
}

#[utoipa::path(
    get,
    path = "/repos/{owner}/{name}/collaboration/declarations",
    tag = "Collaboration",
    params(("owner" = String, Path), ("name" = String, Path), DeclarationListQuery),
    responses(
        (status = 200, description = "Declarations, most recently declared first: {items, next_cursor}. Join with the session list by session_id; sessions that never declared are absent (unknown). Each item carries staleness, source user_content and authorization none", body = serde_json::Value),
        (status = 403, description = "Read access denied")
    )
)]
pub async fn list_declarations(
    State(state): State<AppState>,
    Path((owner, name)): Path<(String, String)>,
    Query(query): Query<DeclarationListQuery>,
    headers: HeaderMap,
) -> Result<Json<collaboration::Page<svc::DeclarationView>>, AppError> {
    let (repo, _user) = require_collab_read(&state, &headers, &owner, &name).await?;
    Ok(Json(
        svc::list(
            &state.db,
            cursor_key(&state),
            repo.id,
            svc::stale_threshold(query.stale_after_seconds),
            collaboration::clamp_limit(query.limit),
            query.cursor.as_deref(),
            Utc::now(),
        )
        .await?,
    ))
}
