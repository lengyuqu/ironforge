//! Push mirror REST API.
//!
//! Forward this repository's branches (and optionally tags) to an external Git
//! host after every push. The target's credentials come from the owner's remote
//! credential dictionary — configure `github.com` once and both the pull and
//! the push side authenticate.
//!
//! ```text
//! GET    /api/v1/repos/{owner}/{name}/push-mirror
//! POST   /api/v1/repos/{owner}/{name}/push-mirror
//! PATCH  /api/v1/repos/{owner}/{name}/push-mirror
//! DELETE /api/v1/repos/{owner}/{name}/push-mirror
//! POST   /api/v1/repos/{owner}/{name}/push-mirror/sync
//! ```

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use serde::Deserialize;
use utoipa::ToSchema;

use crate::api::auth::extract_bearer_claims;
use crate::error::AppError;
use crate::AppState;
use rg_core::mirror::push_service::{self as push_service, PushMirrorInput};

/// Request body for configuring a push target.
#[derive(Deserialize, ToSchema)]
pub struct UpsertPushMirrorRequest {
    /// Remote repository URL. Required when configuring for the first time.
    #[serde(default)]
    pub url: Option<String>,
    /// Push tags alongside branches (default true).
    #[serde(default)]
    pub push_tags: Option<bool>,
    /// `on_push` (default) or `manual`.
    #[serde(default)]
    pub trigger_mode: Option<String>,
}

/// Request body for changing a push target. Adds the pause/resume switch.
#[derive(Deserialize, ToSchema)]
pub struct UpdatePushMirrorRequest {
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub push_tags: Option<bool>,
    #[serde(default)]
    pub trigger_mode: Option<String>,
    /// `active` or `paused`.
    #[serde(default)]
    pub status: Option<String>,
}

impl From<UpsertPushMirrorRequest> for PushMirrorInput {
    fn from(value: UpsertPushMirrorRequest) -> Self {
        PushMirrorInput {
            url: value.url,
            push_tags: value.push_tags,
            trigger_mode: value.trigger_mode,
        }
    }
}

/// Resolve the repository and check write permission.
async fn writable_repo(
    state: &AppState,
    headers: &HeaderMap,
    owner: &str,
    name: &str,
) -> Result<rg_db::entities::repository::Model, AppError> {
    let claims = extract_bearer_claims(headers, &state.jwt_secret)
        .ok_or_else(|| AppError::unauthorized("authentication required"))?;
    let user_id: i64 = claims
        .sub
        .parse()
        .map_err(|_| AppError::unauthorized("invalid token subject"))?;

    let repo = rg_core::repo::service::find_repo_by_owner_name(&state.db, owner, name)
        .await
        .map_err(AppError::from)?
        .ok_or_else(|| AppError::not_found("repository not found"))?;
    match rg_core::repo::service::can_write_repo(&state.db, &repo, Some(user_id)).await {
        Ok(true) => Ok(repo),
        Ok(false) => Err(AppError::forbidden("no write permission")),
        Err(e) => Err(AppError::from(e)),
    }
}

fn enc_key(state: &AppState) -> [u8; 32] {
    rg_core::auth::encryption::derive_key(&state.jwt_secret)
}

/// GET /api/v1/repos/{owner}/{name}/push-mirror
#[utoipa::path(
    get,
    path = "/repos/{owner}/{name}/push-mirror",
    tag = "Mirrors",
    params(
        ("owner" = String, Path, description = "owner"),
        ("name" = String, Path, description = "name"),
    ),
    responses(
        (status = 200, description = "Success", body = serde_json::Value),
        (status = 404, description = "No push mirror configured", body = serde_json::Value),
    ),
)]
pub async fn get_push_mirror(
    State(state): State<AppState>,
    Path((owner, name)): Path<(String, String)>,
) -> impl IntoResponse {
    let repo = match rg_core::repo::service::find_repo_by_owner_name(&state.db, &owner, &name).await
    {
        Ok(Some(r)) => r,
        Ok(None) => return AppError::not_found("repository not found").into_response(),
        Err(e) => return AppError::from(e).into_response(),
    };
    match push_service::get_push_mirror(&state.db, repo.id).await {
        Ok(Some(model)) => (StatusCode::OK, Json(serde_json::json!(model))).into_response(),
        Ok(None) => {
            AppError::not_found("no push mirror configured for this repository").into_response()
        }
        Err(e) => AppError::from(e).into_response(),
    }
}

/// POST /api/v1/repos/{owner}/{name}/push-mirror
///
/// Upsert: reconfiguring an existing push target replaces it and re-activates
/// it (an explicit "try again" after a failure).
#[utoipa::path(
    post,
    path = "/repos/{owner}/{name}/push-mirror",
    tag = "Mirrors",
    params(
        ("owner" = String, Path, description = "owner"),
        ("name" = String, Path, description = "name"),
    ),
    request_body = UpsertPushMirrorRequest,
    responses(
        (status = 201, description = "Configured", body = serde_json::Value),
        (status = 400, description = "Invalid URL or trigger mode", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
        (status = 403, description = "No write permission", body = serde_json::Value),
    ),
)]
pub async fn create_push_mirror(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((owner, name)): Path<(String, String)>,
    Json(body): Json<UpsertPushMirrorRequest>,
) -> impl IntoResponse {
    let repo = match writable_repo(&state, &headers, &owner, &name).await {
        Ok(repo) => repo,
        Err(e) => return e.into_response(),
    };
    match push_service::upsert_push_mirror(&state.db, repo.id, body.into()).await {
        Ok(model) => (StatusCode::CREATED, Json(serde_json::json!(model))).into_response(),
        Err(e) => AppError::from(e).into_response(),
    }
}

/// PATCH /api/v1/repos/{owner}/{name}/push-mirror
#[utoipa::path(
    patch,
    path = "/repos/{owner}/{name}/push-mirror",
    tag = "Mirrors",
    params(
        ("owner" = String, Path, description = "owner"),
        ("name" = String, Path, description = "name"),
    ),
    request_body = UpdatePushMirrorRequest,
    responses(
        (status = 200, description = "Updated", body = serde_json::Value),
        (status = 400, description = "Invalid URL, trigger mode or status", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
        (status = 404, description = "No push mirror configured", body = serde_json::Value),
    ),
)]
pub async fn update_push_mirror(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((owner, name)): Path<(String, String)>,
    Json(body): Json<UpdatePushMirrorRequest>,
) -> impl IntoResponse {
    let repo = match writable_repo(&state, &headers, &owner, &name).await {
        Ok(repo) => repo,
        Err(e) => return e.into_response(),
    };

    let touches_config =
        body.url.is_some() || body.push_tags.is_some() || body.trigger_mode.is_some();
    let mut model = if touches_config {
        let input = PushMirrorInput {
            url: body.url,
            push_tags: body.push_tags,
            trigger_mode: body.trigger_mode,
        };
        match push_service::upsert_push_mirror(&state.db, repo.id, input).await {
            Ok(model) => model,
            Err(e) => return AppError::from(e).into_response(),
        }
    } else {
        match push_service::get_push_mirror(&state.db, repo.id).await {
            Ok(Some(model)) => model,
            Ok(None) => {
                return AppError::not_found("no push mirror configured for this repository")
                    .into_response()
            }
            Err(e) => return AppError::from(e).into_response(),
        }
    };

    // Status is applied after the upsert on purpose: reconfiguring always
    // re-activates, so "change the URL and keep it paused" needs the pause to
    // be the last word.
    if let Some(status) = body.status.as_deref() {
        match push_service::set_status(&state.db, repo.id, status).await {
            Ok(updated) => model = updated,
            Err(e) => return AppError::from(e).into_response(),
        }
    }

    (StatusCode::OK, Json(serde_json::json!(model))).into_response()
}

/// DELETE /api/v1/repos/{owner}/{name}/push-mirror
#[utoipa::path(
    delete,
    path = "/repos/{owner}/{name}/push-mirror",
    tag = "Mirrors",
    params(
        ("owner" = String, Path, description = "owner"),
        ("name" = String, Path, description = "name"),
    ),
    responses(
        (status = 204, description = "Deleted"),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
        (status = 404, description = "No push mirror configured", body = serde_json::Value),
    ),
)]
pub async fn delete_push_mirror(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((owner, name)): Path<(String, String)>,
) -> impl IntoResponse {
    let repo = match writable_repo(&state, &headers, &owner, &name).await {
        Ok(repo) => repo,
        Err(e) => return e.into_response(),
    };
    match push_service::delete_push_mirror(&state.db, repo.id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => AppError::from(e).into_response(),
    }
}

/// POST /api/v1/repos/{owner}/{name}/push-mirror/sync
///
/// Run one push now. Used both for `manual` trigger mode and to retry a failed
/// target without waiting for the next push.
#[utoipa::path(
    post,
    path = "/repos/{owner}/{name}/push-mirror/sync",
    tag = "Mirrors",
    params(
        ("owner" = String, Path, description = "owner"),
        ("name" = String, Path, description = "name"),
    ),
    responses(
        (status = 200, description = "Refs pushed", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
        (status = 404, description = "No push mirror configured", body = serde_json::Value),
        (status = 502, description = "The target rejected the push", body = serde_json::Value),
    ),
)]
pub async fn trigger_push_sync(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((owner, name)): Path<(String, String)>,
) -> impl IntoResponse {
    let repo = match writable_repo(&state, &headers, &owner, &name).await {
        Ok(repo) => repo,
        Err(e) => return e.into_response(),
    };
    let push_mirror = match push_service::get_push_mirror(&state.db, repo.id).await {
        Ok(Some(model)) => model,
        Ok(None) => {
            return AppError::not_found("no push mirror configured for this repository")
                .into_response()
        }
        Err(e) => return AppError::from(e).into_response(),
    };

    let repo_path = push_service::repo_path_for(&state.repo_root, &owner, &name);
    match push_service::run_push(&state.db, &push_mirror, &repo, &repo_path, &enc_key(&state)).await
    {
        Ok(true) => (
            StatusCode::OK,
            Json(serde_json::json!({"status": "pushed"})),
        )
            .into_response(),
        // The push ran and git rejected it. The reason is on the row (and shown
        // in the UI); 502 says "the upstream is the problem", not us.
        Ok(false) => (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({
                "status": "failed",
                "error": push_service::get_push_mirror(&state.db, repo.id)
                    .await
                    .ok()
                    .flatten()
                    .and_then(|m| m.last_push_error),
            })),
        )
            .into_response(),
        Err(e) => AppError::from(e).into_response(),
    }
}
