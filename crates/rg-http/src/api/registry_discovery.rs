//! Cross-repository discovery of skills and MCP server manifests.
//!
//! ```text
//! GET /skills?q=&limit=&cursor=          skills in repositories the caller may read
//! GET /mcp-servers?q=&limit=&cursor=     MCP server manifests, likewise
//! ```
//!
//! Reachable anonymously (public repositories only); a signed-in caller also
//! sees what they may read. Authorization is applied **before** a page is cut,
//! each entry's repository is checked for the caller, and no total is returned:
//! see `rg_core::package_registry::discovery` for what that promises and what a
//! short page means.
//!
//! These routes belong to no single repository, so a token limited to some
//! repositories is refused (the allow-list only admits `/repos/{owner}/{name}`
//! paths); such a token uses the per-repository routes instead.
//!
//! An entry's text is what its publisher wrote: `origin: "user_content"`.

use axum::extract::{Query, State};
use axum::http::HeaderMap;
use axum::Json;
use rg_core::package_registry::discovery::{self, Kind, Page};
use serde::Deserialize;

use crate::error::AppError;
use crate::AppState;

#[derive(Deserialize, utoipa::IntoParams)]
pub struct DiscoveryParams {
    /// Case-insensitive substring of the name or description (at most 200
    /// characters). Omit to list everything the caller may read.
    pub q: Option<String>,
    /// Entries per page: 1 to 100, default 20. Over 100 is an error, not a cut.
    pub limit: Option<usize>,
    /// The `next_cursor` of the previous page.
    pub cursor: Option<String>,
}

async fn discover(
    state: &AppState,
    headers: &HeaderMap,
    kind: Kind,
    params: DiscoveryParams,
) -> Result<Json<Page>, AppError> {
    // Anonymous is allowed: it sees public repositories only.
    let actor_id = super::auth::extract_user_id(headers, &state.jwt_secret);
    let page = discovery::discover(
        &state.db,
        discovery::Query {
            kind,
            text: params.q.as_deref(),
            limit: params.limit.unwrap_or(discovery::DEFAULT_LIMIT),
            cursor: params.cursor.as_deref(),
            actor_id,
        },
    )
    .await?;
    Ok(Json(page))
}

/// GET /api/v1/skills
///
/// Skills published in repositories the caller may read, oldest first.
/// `next_cursor` is present while there may be more; a page can hold fewer than
/// `limit` entries and still carry one (the scan limit was reached).
#[utoipa::path(
    get,
    path = "/skills",
    tag = "Skills",
    params(DiscoveryParams),
    responses(
        (status = 200, description = "A page of skills, each with owner/repo/name, latest version, digests, publisher", body = serde_json::Value),
        (status = 400, description = "INVALID_LIMIT, INVALID_QUERY or INVALID_CURSOR", body = serde_json::Value),
    ),
)]
pub async fn discover_skills(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<DiscoveryParams>,
) -> Result<Json<Page>, AppError> {
    discover(&state, &headers, Kind::Skill, params).await
}

/// GET /api/v1/mcp-servers
///
/// MCP server manifests published in repositories the caller may read, oldest
/// first. Every entry says `ownership: "unverified"`.
#[utoipa::path(
    get,
    path = "/mcp-servers",
    tag = "MCP servers",
    params(DiscoveryParams),
    responses(
        (status = 200, description = "A page of MCP server manifests, each with owner/repo/name, latest version, digest, publisher", body = serde_json::Value),
        (status = 400, description = "INVALID_LIMIT, INVALID_QUERY or INVALID_CURSOR", body = serde_json::Value),
    ),
)]
pub async fn discover_mcp_servers(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<DiscoveryParams>,
) -> Result<Json<Page>, AppError> {
    discover(&state, &headers, Kind::McpServer, params).await
}
