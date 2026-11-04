//! Capability discovery of the collaboration contract (design R14 / X01).
//!
//! The answer comes from one static registry
//! (`rg_core::collaboration::capabilities`), so it lists exactly what this
//! build serves and nothing else. The instance endpoint carries no repository
//! information; the repository endpoint adds the caller's access and is
//! guarded like every other collaboration read.

use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::Json;
use rg_core::collaboration::{
    self, capabilities as registry, handoff, message, question, session, task, work_package,
};
use serde::Serialize;
use utoipa::ToSchema;

use super::collab::require_collab_read;
use super::collab::{HEADER_SESSION, HEADER_SESSION_SECRET};
use crate::error::AppError;
use crate::AppState;

#[derive(Debug, Serialize, ToSchema)]
pub struct CapabilityEntry {
    pub name: String,
    /// `major.minor` of this capability.
    pub version: String,
    /// `stable` | `experimental`. Clients treat an unknown value as `experimental`.
    pub status: String,
    /// What a caller needs: any of `account`, `repo_collab_read`, `repo_write`,
    /// `session_credentials`, `repo_admin`, `site_admin`. Clients ignore names they do not know
    /// but must not assume an unknown requirement is satisfied.
    pub requires: Vec<String>,
    /// MCP tools adapting the capability (empty when REST-only).
    pub mcp_tools: Vec<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AuthMode {
    pub name: String,
    pub description: String,
    pub headers: Vec<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct McpTransport {
    /// Only `stdio` exists; there is no HTTP/SSE MCP endpoint.
    pub transports: Vec<String>,
    pub binary: String,
    /// Number of `collab_*` tools. The authoritative list is MCP `tools/list`.
    pub tool_count: usize,
    pub discovery: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct DeprecationEntry {
    pub capability: String,
    pub what: String,
    pub replacement: String,
    pub announced: String,
    pub earliest_removal: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct CapabilitiesResponse {
    /// `major.minor`. Same major = compatible; a higher minor only adds optional things.
    pub contract_version: String,
    pub server_version: String,
    pub capabilities: Vec<CapabilityEntry>,
    pub limits: serde_json::Value,
    pub auth_modes: Vec<AuthMode>,
    pub mcp_transport: McpTransport,
    pub deprecations: Vec<DeprecationEntry>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct CallerAccess {
    pub repo_collab_read: bool,
    pub repo_write: bool,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct RepoCapabilitiesResponse {
    #[serde(flatten)]
    pub instance: CapabilitiesResponse,
    pub repo: String,
    /// The caller's access to this repository, from the database (not the
    /// 30s permission cache). Admin-only routes need more than this reports.
    pub caller: CallerAccess,
}

fn instance_response() -> CapabilitiesResponse {
    CapabilitiesResponse {
        contract_version: registry::contract_version(),
        server_version: env!("CARGO_PKG_VERSION").to_string(),
        capabilities: registry::all()
            .iter()
            .map(|c| CapabilityEntry {
                name: c.name.to_string(),
                version: c.version.to_string(),
                status: match c.status {
                    registry::Stability::Stable => "stable",
                    registry::Stability::Experimental => "experimental",
                }
                .to_string(),
                requires: c.requires().into_iter().map(str::to_string).collect(),
                mcp_tools: c.mcp_tools.iter().map(|t| t.to_string()).collect(),
            })
            .collect(),
        limits: limits(),
        auth_modes: vec![
            AuthMode {
                name: "bearer_token".into(),
                description: "Account JWT or personal access token; authorizes every request against the account's repository permission.".into(),
                headers: vec!["Authorization".into()],
            },
            AuthMode {
                name: "session_credentials".into(),
                description: "Acting session id and secret, required by session-bound writes in addition to the bearer token. A session is a label under the account, not a security boundary.".into(),
                headers: vec![HEADER_SESSION.into(), HEADER_SESSION_SECRET.into()],
            },
        ],
        mcp_transport: McpTransport {
            transports: vec!["stdio".into()],
            binary: "ironforge-mcp".into(),
            tool_count: registry::mcp_tools().len(),
            discovery: "MCP tools/list; the application contract version is not the MCP protocol version".into(),
        },
        deprecations: registry::DEPRECATIONS
            .iter()
            .map(|d| DeprecationEntry {
                capability: d.capability.into(),
                what: d.what.into(),
                replacement: d.replacement.into(),
                announced: d.announced.into(),
                earliest_removal: d.earliest_removal.into(),
            })
            .collect(),
    }
}

/// Published limits, taken from the constants the services enforce.
fn limits() -> serde_json::Value {
    serde_json::json!({
        "page_size_default": collaboration::DEFAULT_LIMIT,
        "page_size_max": collaboration::MAX_LIMIT,
        "session_ttl_seconds": session::SESSION_TTL.num_seconds(),
        "session_online_window_seconds": session::ONLINE_WINDOW.num_seconds(),
        "session_display_name_chars": session::MAX_DISPLAY_NAME_CHARS,
        "task_title_chars": task::MAX_TITLE_CHARS,
        "task_description_bytes": task::MAX_DESCRIPTION_BYTES,
        "task_paths": task::MAX_PATHS,
        "task_path_bytes": task::MAX_PATH_BYTES,
        "message_body_bytes": message::MAX_BODY_BYTES,
        "handoff_section_bytes": handoff::MAX_SECTION_BYTES,
        "handoff_evidence_items": handoff::MAX_EVIDENCE,
        "question_title_chars": question::MAX_TITLE_CHARS,
        "question_body_bytes": question::MAX_BODY_BYTES,
        "question_options": question::MAX_OPTIONS,
        "work_package_items": work_package::MAX_ITEMS,
        "work_package_item_bytes": work_package::MAX_ITEM_BYTES,
    })
}

#[utoipa::path(
    get,
    path = "/collaboration/capabilities",
    tag = "Collaboration",
    responses(
        (status = 200, description = "What this server's collaboration contract offers; no repository information", body = CapabilitiesResponse),
        (status = 401, description = "Authentication required")
    )
)]
pub async fn instance_capabilities(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<CapabilitiesResponse>, AppError> {
    crate::api::auth::extract_user_id(&headers, &state.jwt_secret)
        .ok_or_else(|| AppError::unauthorized("authentication required"))?;
    Ok(Json(instance_response()))
}

#[utoipa::path(
    get,
    path = "/repos/{owner}/{name}/collaboration/capabilities",
    tag = "Collaboration",
    params(("owner" = String, Path), ("name" = String, Path)),
    responses(
        (status = 200, description = "The contract plus the caller's access to this repository", body = RepoCapabilitiesResponse),
        (status = 401, description = "Authentication required"),
        (status = 403, description = "Public repository, but the caller is not part of its working group"),
        (status = 404, description = "No such repository, or a private one the caller cannot see (indistinguishable)")
    )
)]
pub async fn repo_capabilities(
    State(state): State<AppState>,
    Path((owner, name)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<Json<RepoCapabilitiesResponse>, AppError> {
    let (repo, actor_id) = match require_collab_read(&state, &headers, &owner, &name).await {
        Ok(ok) => ok,
        Err(e) if e.status() == axum::http::StatusCode::FORBIDDEN => {
            // A private repository must look exactly like a missing one.
            let repo = super::repo_access::resolve_repo(&state, &owner, &name).await?;
            return Err(if repo.is_private {
                AppError::not_found("repository not found")
            } else {
                e
            });
        }
        Err(e) => return Err(e),
    };
    let repo_write = rg_core::repo::service::can_write_repo(&state.db, &repo, Some(actor_id))
        .await
        .map_err(AppError::from)?;
    Ok(Json(RepoCapabilitiesResponse {
        instance: instance_response(),
        repo: format!("{owner}/{name}"),
        caller: CallerAccess {
            repo_collab_read: true,
            repo_write,
        },
    }))
}
