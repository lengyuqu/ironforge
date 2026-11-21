//! MCP server manifest registry REST API: publish a `server.json`, and read it
//! back in shapes an agent can use directly.
//!
//! ```text
//! POST /repos/{o}/{n}/mcp-servers                                      publish a manifest
//! GET  /repos/{o}/{n}/mcp-servers                                      servers in this repository
//! GET  /repos/{o}/{n}/mcp-servers/{namespace}/{server}                 versions of one server
//! GET  /repos/{o}/{n}/mcp-servers/{namespace}/{server}/resolve         constraint -> exact version + digest
//! GET  /repos/{o}/{n}/mcp-servers/{namespace}/{server}/versions/{v}    the manifest
//! ```
//!
//! A server is named `namespace/server`, as in the official MCP Registry, so the
//! name is two path segments.
//!
//! The registry stores the manifest only. It does not host the server and never
//! fetches a URL in the manifest; every response says `ownership: "unverified"`
//! because who controls the name, the package or the remote is the publisher's
//! claim. Everything the publisher wrote is `origin: "user_content"`.
//!
//! Permission: reads need repository read access (and, for a CI job token,
//! `packages:read`), publishing needs repository write access. Publishing moves
//! no branch or tag, so a token limited to some branches is refused.

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use rg_core::package_registry::mcp_service::{self, PublishRequest};
use serde::Deserialize;

use super::skills::{app_error, require_read, storage};
use crate::error::AppError;
use crate::AppState;

fn server_name(namespace: &str, server: &str) -> String {
    format!("{namespace}/{server}")
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PublishMcpServerRequest {
    /// The `server.json` manifest.
    #[schema(value_type = Object)]
    pub server: serde_json::Value,
}

/// POST /repos/{owner}/{name}/mcp-servers
///
/// Publishes `server` as a new version of the server it names. Publishing the
/// same version again with the same manifest is a no-op (200); with a different
/// manifest it is refused (409 `VERSION_IMMUTABLE`). Key order and whitespace do
/// not matter: the manifest is stored in canonical form and compared by digest.
#[utoipa::path(
    post,
    path = "/repos/{owner}/{name}/mcp-servers",
    tag = "MCP servers",
    params(
        ("owner" = String, Path, description = "owner"),
        ("name" = String, Path, description = "repo name"),
    ),
    request_body = PublishMcpServerRequest,
    responses(
        (status = 201, description = "Published a new version", body = serde_json::Value),
        (status = 200, description = "That version already exists with the same manifest", body = serde_json::Value),
        (status = 400, description = "Not a valid manifest (a stable MCP_* code)", body = serde_json::Value),
        (status = 409, description = "VERSION_IMMUTABLE", body = serde_json::Value),
        (status = 413, description = "MCP_MANIFEST_TOO_LARGE", body = serde_json::Value),
    ),
)]
pub async fn publish(
    State(state): State<AppState>,
    Path((owner, name)): Path<(String, String)>,
    headers: HeaderMap,
    Json(body): Json<PublishMcpServerRequest>,
) -> Result<(StatusCode, Json<mcp_service::McpPublishOutcome>), AppError> {
    super::repo_content::validate_owner_repo(&owner, &name)?;
    let (repo, user_id) =
        super::repo_access::require_write(&state, &headers, &owner, &name).await?;

    let outcome = mcp_service::publish(
        &state.db,
        &storage(&state),
        PublishRequest {
            owner: &owner,
            repo_name: &name,
            repo_id: repo.id,
            manifest: &body.server,
            author_id: user_id,
        },
    )
    .await
    .map_err(app_error)?;

    if !outcome.existing {
        super::audit::record_write(
            &state.db,
            user_id,
            "mcp_server.publish",
            "mcp_server",
            Some(outcome.package_id),
            &format!("{owner}/{name}/{}@{}", outcome.name, outcome.version),
            &headers,
            serde_json::json!({
                "version": outcome.version,
                "digest": outcome.digest,
            }),
        )
        .await;
    }
    let status = if outcome.existing {
        StatusCode::OK
    } else {
        StatusCode::CREATED
    };
    Ok((status, Json(outcome)))
}

/// GET /repos/{owner}/{name}/mcp-servers
#[utoipa::path(
    get,
    path = "/repos/{owner}/{name}/mcp-servers",
    tag = "MCP servers",
    params(
        ("owner" = String, Path, description = "owner"),
        ("name" = String, Path, description = "repo name"),
    ),
    responses((status = 200, description = "MCP servers published in this repository", body = serde_json::Value)),
)]
pub async fn list_servers(
    State(state): State<AppState>,
    Path((owner, name)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, AppError> {
    let repo = require_read(&state, &headers, &owner, &name).await?;
    let servers = mcp_service::list(&state.db, repo.id)
        .await
        .map_err(app_error)?;
    Ok(Json(serde_json::json!({ "servers": servers })))
}

/// GET /repos/{owner}/{name}/mcp-servers/{namespace}/{server}
#[utoipa::path(
    get,
    path = "/repos/{owner}/{name}/mcp-servers/{namespace}/{server}",
    tag = "MCP servers",
    params(
        ("owner" = String, Path, description = "owner"),
        ("name" = String, Path, description = "repo name"),
        ("namespace" = String, Path, description = "namespace part of the server name"),
        ("server" = String, Path, description = "server part of the server name"),
    ),
    responses(
        (status = 200, description = "The server and its versions", body = serde_json::Value),
        (status = 404, description = "MCP_SERVER_NOT_FOUND", body = serde_json::Value),
    ),
)]
pub async fn get_server(
    State(state): State<AppState>,
    Path((owner, name, namespace, server)): Path<(String, String, String, String)>,
    headers: HeaderMap,
) -> Result<Json<mcp_service::McpServerDetail>, AppError> {
    let repo = require_read(&state, &headers, &owner, &name).await?;
    mcp_service::get(&state.db, repo.id, &server_name(&namespace, &server))
        .await
        .map(Json)
        .map_err(app_error)
}

#[derive(Deserialize, utoipa::IntoParams)]
pub struct ResolveQuery {
    /// Cargo-style semantic-version requirement: `^1.2`, `~1.2.3`, `>=1, <2`,
    /// `=1.2.3`. Empty or `latest` means any version.
    pub constraint: Option<String>,
}

/// GET /repos/{owner}/{name}/mcp-servers/{namespace}/{server}/resolve?constraint=
///
/// Resolves a constraint to one exact, non-yanked version and its manifest
/// digest, for writing to a lock file.
#[utoipa::path(
    get,
    path = "/repos/{owner}/{name}/mcp-servers/{namespace}/{server}/resolve",
    tag = "MCP servers",
    params(
        ("owner" = String, Path, description = "owner"),
        ("name" = String, Path, description = "repo name"),
        ("namespace" = String, Path, description = "namespace part of the server name"),
        ("server" = String, Path, description = "server part of the server name"),
        ResolveQuery,
    ),
    responses(
        (status = 200, description = "The chosen version", body = serde_json::Value),
        (status = 400, description = "MCP_CONSTRAINT_INVALID", body = serde_json::Value),
        (status = 404, description = "NO_MATCHING_VERSION, with `details.available`", body = serde_json::Value),
    ),
)]
pub async fn resolve(
    State(state): State<AppState>,
    Path((owner, name, namespace, server)): Path<(String, String, String, String)>,
    Query(query): Query<ResolveQuery>,
    headers: HeaderMap,
) -> Result<Json<mcp_service::ResolvedMcpServer>, AppError> {
    let repo = require_read(&state, &headers, &owner, &name).await?;
    mcp_service::resolve(
        &state.db,
        repo.id,
        &server_name(&namespace, &server),
        query.constraint.as_deref(),
    )
    .await
    .map(Json)
    .map_err(app_error)
}

/// GET /repos/{owner}/{name}/mcp-servers/{namespace}/{server}/versions/{version}
#[utoipa::path(
    get,
    path = "/repos/{owner}/{name}/mcp-servers/{namespace}/{server}/versions/{version}",
    tag = "MCP servers",
    params(
        ("owner" = String, Path, description = "owner"),
        ("name" = String, Path, description = "repo name"),
        ("namespace" = String, Path, description = "namespace part of the server name"),
        ("server" = String, Path, description = "server part of the server name"),
        ("version" = String, Path, description = "exact version"),
    ),
    responses(
        (status = 200, description = "The manifest, its digest and a summary of what it offers", body = serde_json::Value),
        (status = 404, description = "MCP_VERSION_NOT_FOUND", body = serde_json::Value),
    ),
)]
pub async fn get_version(
    State(state): State<AppState>,
    Path((owner, name, namespace, server, version)): Path<(String, String, String, String, String)>,
    headers: HeaderMap,
) -> Result<Json<mcp_service::McpVersionManifest>, AppError> {
    let repo = require_read(&state, &headers, &owner, &name).await?;
    mcp_service::get_version(
        &state.db,
        &storage(&state),
        repo.id,
        &server_name(&namespace, &server),
        &version,
    )
    .await
    .map(Json)
    .map_err(app_error)
}
