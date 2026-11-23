//! Remote credential REST API.
//!
//! Tokens for *other* Git hosts, keyed by host. One configured row covers every
//! mirror (and later every push) against that platform for its owner, so a
//! private upstream no longer needs its token typed into each mirror dialog.
//!
//! User-owned:
//!
//! ```text
//! GET    /api/v1/user/remote-credentials
//! POST   /api/v1/user/remote-credentials
//! PATCH  /api/v1/user/remote-credentials/{id}
//! DELETE /api/v1/user/remote-credentials/{id}
//! ```
//!
//! Org-owned (admin to write, member to read):
//!
//! ```text
//! GET    /api/v1/orgs/{name}/remote-credentials
//! POST   /api/v1/orgs/{name}/remote-credentials
//! PATCH  /api/v1/orgs/{name}/remote-credentials/{id}
//! DELETE /api/v1/orgs/{name}/remote-credentials/{id}
//! ```
//!
//! No response ever contains the token: [`CredentialView`] carries a
//! `****abcd` hint at most.

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
use rg_core::remote_credential::service::{
    self as credential_service, CredentialInput, CredentialPatch, Scope,
};

/// Request body for configuring a host's token.
#[derive(Deserialize, ToSchema)]
pub struct CreateRemoteCredentialRequest {
    /// Host or URL as typed: `github.com`, `gitea.local:3000`, or a full URL.
    pub host: String,
    /// Basic-auth user; defaults to `x-access-token`. Ignored for `bearer`.
    #[serde(default)]
    pub username: Option<String>,
    /// The token itself. Stored AES-256-GCM encrypted, never returned.
    pub token: String,
    /// `basic` (default) or `bearer`.
    #[serde(default)]
    pub auth_scheme: Option<String>,
    /// Display name.
    #[serde(default)]
    pub label: Option<String>,
}

/// Request body for changing an existing credential. The host cannot change.
#[derive(Deserialize, ToSchema)]
pub struct UpdateRemoteCredentialRequest {
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub token: Option<String>,
    #[serde(default)]
    pub auth_scheme: Option<String>,
    #[serde(default)]
    pub label: Option<String>,
}

impl From<CreateRemoteCredentialRequest> for CredentialInput {
    fn from(value: CreateRemoteCredentialRequest) -> Self {
        CredentialInput {
            host: value.host,
            username: value.username,
            token: value.token,
            auth_scheme: value.auth_scheme,
            label: value.label,
        }
    }
}

impl From<UpdateRemoteCredentialRequest> for CredentialPatch {
    fn from(value: UpdateRemoteCredentialRequest) -> Self {
        CredentialPatch {
            username: value.username,
            token: value.token,
            auth_scheme: value.auth_scheme,
            label: value.label,
        }
    }
}

/// Resolve the caller's user id, or the 401 to return.
fn caller_id(headers: &HeaderMap, jwt_secret: &str) -> Result<i64, AppError> {
    let claims = extract_bearer_claims(headers, jwt_secret)
        .ok_or_else(|| AppError::unauthorized("authentication required"))?;
    claims
        .sub
        .parse()
        .map_err(|_| AppError::unauthorized("invalid token subject"))
}

/// The instance encryption key, derived the same way as every other secret in
/// the deployment (`ci_secrets`, `sso_providers`, `mfa`).
fn enc_key(state: &AppState) -> [u8; 32] {
    rg_core::auth::encryption::derive_key(&state.jwt_secret)
}

// ── User-owned endpoints ────────────────────────────────────────────────

/// GET /api/v1/user/remote-credentials
#[utoipa::path(
    get,
    path = "/user/remote-credentials",
    tag = "RemoteCredentials",
    responses(
        (status = 200, description = "Configured hosts", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
    ),
)]
pub async fn list_user_credentials(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let user_id = match caller_id(&headers, &state.jwt_secret) {
        Ok(id) => id,
        Err(e) => return e.into_response(),
    };
    match credential_service::list_views(&state.db, Scope::User(user_id), &enc_key(&state)).await {
        Ok(items) => Json(serde_json::json!(items)).into_response(),
        Err(e) => AppError::from(e).into_response(),
    }
}

/// POST /api/v1/user/remote-credentials
///
/// Upsert by host: submitting a host that is already configured replaces its
/// token, which is how an expired token is rotated.
#[utoipa::path(
    post,
    path = "/user/remote-credentials",
    tag = "RemoteCredentials",
    request_body = CreateRemoteCredentialRequest,
    responses(
        (status = 201, description = "Configured", body = serde_json::Value),
        (status = 400, description = "Invalid host or token", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
        (status = 409, description = "Concurrent create for the same host", body = serde_json::Value),
    ),
)]
pub async fn create_user_credential(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<CreateRemoteCredentialRequest>,
) -> impl IntoResponse {
    let user_id = match caller_id(&headers, &state.jwt_secret) {
        Ok(id) => id,
        Err(e) => return e.into_response(),
    };
    let key = enc_key(&state);
    match credential_service::upsert(&state.db, Scope::User(user_id), body.into(), &key).await {
        Ok(model) => (
            StatusCode::CREATED,
            Json(serde_json::json!(credential_service::view(&model, &key))),
        )
            .into_response(),
        Err(e) => AppError::from(e).into_response(),
    }
}

/// PATCH /api/v1/user/remote-credentials/{id}
#[utoipa::path(
    patch,
    path = "/user/remote-credentials/{id}",
    tag = "RemoteCredentials",
    params(("id" = i64, Path, description = "credential id")),
    request_body = UpdateRemoteCredentialRequest,
    responses(
        (status = 200, description = "Updated", body = serde_json::Value),
        (status = 400, description = "Invalid token or scheme", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
        (status = 404, description = "Not found", body = serde_json::Value),
    ),
)]
pub async fn update_user_credential(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(body): Json<UpdateRemoteCredentialRequest>,
) -> impl IntoResponse {
    let user_id = match caller_id(&headers, &state.jwt_secret) {
        Ok(id) => id,
        Err(e) => return e.into_response(),
    };
    let key = enc_key(&state);
    match credential_service::update(&state.db, Scope::User(user_id), id, body.into(), &key).await {
        Ok(model) => {
            Json(serde_json::json!(credential_service::view(&model, &key))).into_response()
        }
        Err(e) => AppError::from(e).into_response(),
    }
}

/// DELETE /api/v1/user/remote-credentials/{id}
#[utoipa::path(
    delete,
    path = "/user/remote-credentials/{id}",
    tag = "RemoteCredentials",
    params(("id" = i64, Path, description = "credential id")),
    responses(
        (status = 204, description = "Deleted"),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
        (status = 404, description = "Not found", body = serde_json::Value),
    ),
)]
pub async fn delete_user_credential(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    let user_id = match caller_id(&headers, &state.jwt_secret) {
        Ok(id) => id,
        Err(e) => return e.into_response(),
    };
    match credential_service::delete(&state.db, Scope::User(user_id), id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => AppError::from(e).into_response(),
    }
}

// ── Org-owned endpoints ─────────────────────────────────────────────────

/// Resolve `{org}` and authorize the caller to read it.
async fn org_scope_for_read(state: &AppState, org: &str, user_id: i64) -> Result<Scope, AppError> {
    let org = rg_core::org::require_org_read(&state.db, org, Some(user_id))
        .await
        .map_err(AppError::from)?;
    Ok(Scope::Org(org.id))
}

/// Resolve `{org}` and authorize the caller to administer it.
async fn org_scope_for_write(state: &AppState, org: &str, user_id: i64) -> Result<Scope, AppError> {
    let org = rg_core::org::require_org_admin(&state.db, org, user_id)
        .await
        .map_err(AppError::from)?;
    Ok(Scope::Org(org.id))
}

/// GET /api/v1/orgs/{name}/remote-credentials
#[utoipa::path(
    get,
    path = "/orgs/{name}/remote-credentials",
    tag = "RemoteCredentials",
    params(("name" = String, Path, description = "organization name")),
    responses(
        (status = 200, description = "Configured hosts", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
        (status = 403, description = "Not a member", body = serde_json::Value),
        (status = 404, description = "Organization not found", body = serde_json::Value),
    ),
)]
pub async fn list_org_credentials(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(org): Path<String>,
) -> impl IntoResponse {
    let user_id = match caller_id(&headers, &state.jwt_secret) {
        Ok(id) => id,
        Err(e) => return e.into_response(),
    };
    let scope = match org_scope_for_read(&state, &org, user_id).await {
        Ok(scope) => scope,
        Err(e) => return e.into_response(),
    };
    match credential_service::list_views(&state.db, scope, &enc_key(&state)).await {
        Ok(items) => Json(serde_json::json!(items)).into_response(),
        Err(e) => AppError::from(e).into_response(),
    }
}

/// POST /api/v1/orgs/{name}/remote-credentials
#[utoipa::path(
    post,
    path = "/orgs/{name}/remote-credentials",
    tag = "RemoteCredentials",
    params(("name" = String, Path, description = "organization name")),
    request_body = CreateRemoteCredentialRequest,
    responses(
        (status = 201, description = "Configured", body = serde_json::Value),
        (status = 400, description = "Invalid host or token", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
        (status = 403, description = "Not an org admin", body = serde_json::Value),
        (status = 409, description = "Concurrent create for the same host", body = serde_json::Value),
    ),
)]
pub async fn create_org_credential(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(org): Path<String>,
    Json(body): Json<CreateRemoteCredentialRequest>,
) -> impl IntoResponse {
    let user_id = match caller_id(&headers, &state.jwt_secret) {
        Ok(id) => id,
        Err(e) => return e.into_response(),
    };
    let scope = match org_scope_for_write(&state, &org, user_id).await {
        Ok(scope) => scope,
        Err(e) => return e.into_response(),
    };
    let key = enc_key(&state);
    match credential_service::upsert(&state.db, scope, body.into(), &key).await {
        Ok(model) => (
            StatusCode::CREATED,
            Json(serde_json::json!(credential_service::view(&model, &key))),
        )
            .into_response(),
        Err(e) => AppError::from(e).into_response(),
    }
}

/// PATCH /api/v1/orgs/{name}/remote-credentials/{id}
#[utoipa::path(
    patch,
    path = "/orgs/{name}/remote-credentials/{id}",
    tag = "RemoteCredentials",
    params(
        ("name" = String, Path, description = "organization name"),
        ("id" = i64, Path, description = "credential id"),
    ),
    request_body = UpdateRemoteCredentialRequest,
    responses(
        (status = 200, description = "Updated", body = serde_json::Value),
        (status = 400, description = "Invalid token or scheme", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
        (status = 403, description = "Not an org admin", body = serde_json::Value),
        (status = 404, description = "Not found", body = serde_json::Value),
    ),
)]
pub async fn update_org_credential(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((org, id)): Path<(String, i64)>,
    Json(body): Json<UpdateRemoteCredentialRequest>,
) -> impl IntoResponse {
    let user_id = match caller_id(&headers, &state.jwt_secret) {
        Ok(id) => id,
        Err(e) => return e.into_response(),
    };
    let scope = match org_scope_for_write(&state, &org, user_id).await {
        Ok(scope) => scope,
        Err(e) => return e.into_response(),
    };
    let key = enc_key(&state);
    match credential_service::update(&state.db, scope, id, body.into(), &key).await {
        Ok(model) => {
            Json(serde_json::json!(credential_service::view(&model, &key))).into_response()
        }
        Err(e) => AppError::from(e).into_response(),
    }
}

/// DELETE /api/v1/orgs/{name}/remote-credentials/{id}
#[utoipa::path(
    delete,
    path = "/orgs/{name}/remote-credentials/{id}",
    tag = "RemoteCredentials",
    params(
        ("name" = String, Path, description = "organization name"),
        ("id" = i64, Path, description = "credential id"),
    ),
    responses(
        (status = 204, description = "Deleted"),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
        (status = 403, description = "Not an org admin", body = serde_json::Value),
        (status = 404, description = "Not found", body = serde_json::Value),
    ),
)]
pub async fn delete_org_credential(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((org, id)): Path<(String, i64)>,
) -> impl IntoResponse {
    let user_id = match caller_id(&headers, &state.jwt_secret) {
        Ok(id) => id,
        Err(e) => return e.into_response(),
    };
    let scope = match org_scope_for_write(&state, &org, user_id).await {
        Ok(scope) => scope,
        Err(e) => return e.into_response(),
    };
    match credential_service::delete(&state.db, scope, id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => AppError::from(e).into_response(),
    }
}
