//! Collaboration rules discovery endpoint (O01).
//!
//! Tells an agent which `AGENTS.md`-style files govern a path and at which
//! commit. Content is never inlined or interpreted: every entry is
//! `repository_content` with a pinned `read_url`.

use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::Json;
use rg_core::collaboration::rules::{self, ManifestStatus, RuleEntry, SkippedRule};
use serde::{Deserialize, Serialize};

use super::collab::require_collab_read;
use crate::error::AppError;
use crate::AppState;

#[derive(Debug, Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct RulesQuery {
    /// Branch, tag or commit; default `HEAD`. Resolved once to `resolved_commit`.
    pub r#ref: Option<String>,
    /// Repository-relative file or directory the rules should govern; default
    /// the repository root.
    pub path: Option<String>,
}

#[derive(Debug, Serialize)]
struct RuleEntryView {
    #[serde(flatten)]
    entry: RuleEntry,
    /// Blob endpoint pinned to `resolved_commit` (not to the moving ref).
    read_url: String,
}

#[derive(Debug, Serialize)]
pub struct RulesResponse {
    requested_ref: String,
    resolved_commit: Option<String>,
    path: String,
    discovery_version: u32,
    manifest: ManifestStatus,
    entries: Vec<RuleEntryView>,
    skipped: Vec<SkippedRule>,
    truncated: bool,
    /// The list is bounded (50 entries), so there is no continuation cursor.
    next_cursor: Option<String>,
}

fn encode_path(path: &str) -> String {
    path.split('/')
        .map(|segment| {
            segment
                .bytes()
                .map(|b| match b {
                    b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                        (b as char).to_string()
                    }
                    _ => format!("%{b:02X}"),
                })
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("/")
}

#[utoipa::path(
    get,
    path = "/repos/{owner}/{name}/collaboration/rules",
    tag = "Collaboration",
    params(("owner" = String, Path), ("name" = String, Path), RulesQuery),
    responses(
        (status = 200, description = "Rule files that govern `path` at `resolved_commit`, shallowest first; empty for a repository without commits", body = serde_json::Value),
        (status = 400, description = "Invalid path"),
        (status = 401, description = "Authentication required"),
        (status = 403, description = "Read access denied"),
        (status = 404, description = "Ref not found")
    )
)]
pub async fn get_rules(
    State(state): State<AppState>,
    Path((owner, name)): Path<(String, String)>,
    Query(query): Query<RulesQuery>,
    headers: HeaderMap,
) -> Result<Json<RulesResponse>, AppError> {
    require_collab_read(&state, &headers, &owner, &name).await?;
    let repo_path = state.repo_root.join(format!("{owner}/{name}.git"));
    let git_ref = query.r#ref.filter(|r| !r.is_empty());
    let path = query.path.unwrap_or_default();
    let found =
        tokio::task::spawn_blocking(move || rules::discover(&repo_path, git_ref.as_deref(), &path))
            .await
            .map_err(|e| AppError::internal(format!("rules discovery task failed: {e}")))??;

    let commit = found.resolved_commit.clone();
    let entries = found
        .entries
        .into_iter()
        .map(|entry| {
            let read_url = format!(
                "/api/v1/repos/{owner}/{name}/blob/{}?ref={}",
                encode_path(&entry.path),
                commit.as_deref().unwrap_or_default()
            );
            RuleEntryView { entry, read_url }
        })
        .collect();
    Ok(Json(RulesResponse {
        requested_ref: found.requested_ref,
        resolved_commit: found.resolved_commit,
        path: found.path,
        discovery_version: found.discovery_version,
        manifest: found.manifest,
        entries,
        skipped: found.skipped,
        truncated: found.truncated,
        next_cursor: None,
    }))
}
