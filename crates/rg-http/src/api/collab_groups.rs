//! Cross-repository coordination groups (P2 X02).
//!
//! A group belongs to an *anchor* repository and links to tasks and commits of
//! other repositories by repository id. Permission model (design §2, §4):
//!
//! - create a group: write access to the anchor;
//! - add / remove a link: write access to **both** the anchor and the target;
//! - read a group: collaboration read access to the anchor; each link is shown
//!   only if the reader also has collaboration read access to the link's target.
//!
//! Every request decides from the database (the permission cache is bypassed
//! per repository, as everywhere in the collaboration API), authorizes
//! **before** it sorts or paginates, and answers "target repository missing"
//! and "target repository not visible to you" identically, so ids cannot be
//! probed. Nothing here grants Git, CI, merge or approval rights.
//!
//! **Warning for field `title` / `description`:** they are readable by everyone
//! who can read the anchor repository's collaboration data, including people who
//! cannot read the linked repositories. The server never copies content of the
//! linked repositories into them. Do not put confidential details of other
//! repositories there.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use chrono::Utc;
use rg_core::collaboration::{self, group as svc, CollabError, Actor};
use rg_db::entities::{collab_coordination_link, repository};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::collab::{
    commit_exists, cursor_key, is_collab_member, require_collab_read, require_collab_write,
    write_actor, ListQuery,
};
use crate::error::AppError;
use crate::AppState;

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateGroupRequest {
    /// Idempotency key, unique per creating session or user.
    pub client_request_id: String,
    /// 1–200 characters. **Readable by everyone who can read the anchor
    /// repository's collaboration data**, also people without access to the
    /// linked repositories; never put confidential details of other repositories
    /// here.
    pub title: String,
    /// Up to 16 KiB; same visibility warning as `title`.
    #[serde(default)]
    pub description: String,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AddLinkRequest {
    /// Idempotency key, unique per adding session or user.
    pub client_request_id: String,
    /// Stable id of the target repository (not its name).
    pub target_repo_id: i64,
    /// A collaboration task of the target repository.
    pub target_task_id: Option<String>,
    /// Full 40/64-digit commit id that exists in the target repository now.
    pub commit_sha: Option<String>,
    /// The group's `version` as last read.
    pub expected_group_version: i64,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RemoveLinkRequest {
    pub client_request_id: String,
    pub expected_group_version: i64,
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct GroupQuery {
    /// Size of the embedded first page of links (default 50, max 100).
    pub limit: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct GroupResponse {
    #[serde(flatten)]
    pub group: svc::GroupView,
    /// Reminder that title and description are anchor-repository data.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notice: Option<&'static str>,
}

#[derive(Debug, Serialize)]
pub struct GroupDetail {
    #[serde(flatten)]
    pub group: svc::GroupView,
    /// First page of the links the caller may see; continue with
    /// `GET .../links?cursor=`.
    pub links: svc::LinkPage,
}

// ── target access ───────────────────────────────────────────────────

/// A target repository the requester may see.
struct Target {
    repo: repository::Model,
    owner: String,
}

impl Target {
    fn repo_ref(&self) -> svc::RepoRef {
        svc::RepoRef { id: self.repo.id, owner: self.owner.clone(), name: self.repo.name.clone() }
    }
}

/// Per-request view of the target repositories as one account sees them. Each
/// repository is decided once per request, from the database.
struct TargetAccess<'a> {
    state: &'a AppState,
    user_id: i64,
    cache: Mutex<HashMap<i64, Option<Arc<Target>>>>,
}

impl<'a> TargetAccess<'a> {
    fn new(state: &'a AppState, user_id: i64) -> Self {
        Self { state, user_id, cache: Mutex::new(HashMap::new()) }
    }

    /// The repository if it exists (not soft-deleted) **and** the account is a
    /// collaboration reader of it; `None` otherwise, without telling which.
    async fn visible(&self, repo_id: i64) -> Result<Option<Arc<Target>>, AppError> {
        if let Some(hit) = self.cache.lock().unwrap_or_else(|e| e.into_inner()).get(&repo_id) {
            return Ok(hit.clone());
        }
        let found = match rg_db::ops::repo_ops::find_by_id(&self.state.db, repo_id)
            .await
            .map_err(|e| AppError::internal(e.to_string()))?
        {
            None => None,
            Some(repo) => {
                // Same rule as `require_collab_read`: decide from the database,
                // not from the 30 s process-local permission cache.
                rg_core::repo::service::invalidate_perm_cache_user(repo.id, self.user_id);
                if is_collab_member(self.state, &repo, self.user_id).await? {
                    let owner = rg_core::repo::service::repo_namespace(&self.state.db, &repo).await?;
                    Some(Arc::new(Target { repo, owner }))
                } else {
                    None
                }
            }
        };
        self.cache.lock().unwrap_or_else(|e| e.into_inner()).insert(repo_id, found.clone());
        Ok(found)
    }

    /// Like [`Self::visible`], plus write access (403 only for a repository the
    /// account can see anyway).
    async fn writable(&self, repo_id: i64, not_found: &'static str) -> Result<Arc<Target>, AppError> {
        let target = self.visible(repo_id).await?.ok_or_else(|| AppError::not_found(not_found))?;
        match rg_core::repo::service::can_write_repo(&self.state.db, &target.repo, Some(self.user_id)).await {
            Ok(true) => Ok(target),
            Ok(false) => Err(AppError::forbidden("write access to the target repository denied")),
            Err(e) => Err(AppError::from(e)),
        }
    }

    /// The link as the requester sees it, or `None` if they may not see it.
    async fn resolve_link(
        &self,
        row: collab_coordination_link::Model,
    ) -> Result<Option<svc::LinkView>, CollabError> {
        let target = match self.visible(row.target_repo_id).await {
            Ok(Some(target)) => target,
            Ok(None) => return Ok(None),
            Err(e) => return Err(CollabError::Internal(anyhow::anyhow!(e.to_string()))),
        };
        let task = match &row.target_task_id {
            None => None,
            Some(id) => {
                let found = rg_db::ops::collab_task_ops::find_in_repo(&self.state.db, target.repo.id, id)
                    .await
                    .map_err(CollabError::Internal)?;
                match found {
                    Some(t) => Some(svc::TaskRef {
                        id: t.id,
                        title: t.title,
                        status: t.status,
                        version: t.version,
                        origin: collaboration::ORIGIN_USER_CONTENT,
                    }),
                    // Raced with the task's removal: the link goes with it.
                    None => return Ok(None),
                }
            }
        };
        let commit_state = match &row.commit_sha {
            None => None,
            Some(sha) => {
                // A repository whose directory cannot be opened counts as unavailable.
                let present = commit_exists(self.state, &target.owner, &target.repo.name, sha)
                    .await
                    .unwrap_or(false);
                Some(if present { "available" } else { "unavailable" })
            }
        };
        Ok(Some(svc::LinkView::new(&row, target.repo_ref(), task, commit_state)))
    }
}

async fn audit(
    state: &AppState,
    headers: &HeaderMap,
    user_id: i64,
    action: &str,
    anchor_repo_id: i64,
    details: serde_json::Value,
) {
    // Ids only: the audit trail references objects, it never copies their content.
    // (`rg_core::audit::record` is not used: it inserts an explicit id 0, so only
    // the first row it ever writes succeeds.) An audit failure is logged and
    // never blocks the operation.
    let (ip_address, user_agent) = crate::api::audit::extract_ip_and_ua(headers);
    let entry = rg_db::entities::audit_log::ActiveModel {
        id: sea_orm::NotSet,
        user_id: sea_orm::Set(Some(user_id)),
        username: sea_orm::Set(None),
        action: sea_orm::Set(action.to_string()),
        resource_type: sea_orm::Set(Some("collab_group".to_string())),
        resource_id: sea_orm::Set(Some(anchor_repo_id)),
        resource_name: sea_orm::Set(None),
        ip_address: sea_orm::Set(ip_address),
        user_agent: sea_orm::Set(user_agent),
        details: sea_orm::Set(Some(details.to_string())),
        created_at: sea_orm::Set(Utc::now()),
    };
    if let Err(e) = rg_db::ops::audit_log_ops::insert(&state.db, entry).await {
        tracing::error!(%action, "failed to write audit log: {e}");
    }
}

fn is_task_row_id(id: &str) -> bool {
    uuid::Uuid::parse_str(id).is_ok()
}

// ── endpoints ───────────────────────────────────────────────────────

#[utoipa::path(
    post,
    path = "/repos/{owner}/{name}/collaboration/groups",
    tag = "Collaboration",
    params(("owner" = String, Path), ("name" = String, Path)),
    request_body = CreateGroupRequest,
    responses(
        (status = 201, description = "New group (version 1) owned by this repository, plus a `notice` about who can read title/description", body = serde_json::Value),
        (status = 200, description = "Identical earlier request replayed", body = serde_json::Value),
        (status = 400, description = "Invalid field"),
        (status = 403, description = "Write access to the anchor repository denied, or invalid session credentials"),
        (status = 409, description = "IDEMPOTENCY_KEY_REUSED or SESSION_EXPIRED")
    )
)]
pub async fn create_group(
    State(state): State<AppState>,
    Path((owner, name)): Path<(String, String)>,
    headers: HeaderMap,
    Json(req): Json<CreateGroupRequest>,
) -> Result<(StatusCode, Json<GroupResponse>), AppError> {
    let (repo, user_id) = require_collab_write(&state, &headers, &owner, &name).await?;
    let actor = write_actor(&state, &headers, repo.id, user_id).await?;
    let created = svc::create_group(
        &state.db,
        repo.id,
        user_id,
        &actor,
        svc::NewGroup {
            client_group_id: req.client_request_id,
            title: req.title,
            description: req.description,
        },
        Utc::now(),
    )
    .await?;
    if !created.replayed {
        audit(
            &state,
            &headers,
            user_id,
            "collab.group.create",
            repo.id,
            serde_json::json!({ "group_id": created.group.id }),
        )
        .await;
    }
    let status = if created.replayed { StatusCode::OK } else { StatusCode::CREATED };
    Ok((
        status,
        Json(GroupResponse {
            group: svc::GroupView::new(created.group, true),
            notice: Some(svc::SHARING_NOTICE),
        }),
    ))
}

#[utoipa::path(
    get,
    path = "/repos/{owner}/{name}/collaboration/groups/{group_id}",
    tag = "Collaboration",
    params(("owner" = String, Path), ("name" = String, Path), ("group_id" = String, Path), GroupQuery),
    responses(
        (status = 200, description = "Group fields and the first page `links` {items, next_cursor, has_more, as_of} of the links the caller may see; `version` only with write access to the anchor. Links to repositories the caller cannot see are omitted without a trace.", body = serde_json::Value),
        (status = 403, description = "Collaboration read access to the anchor denied"),
        (status = 404, description = "No such group in this repository")
    )
)]
pub async fn get_group(
    State(state): State<AppState>,
    Path((owner, name, group_id)): Path<(String, String, String)>,
    Query(query): Query<GroupQuery>,
    headers: HeaderMap,
) -> Result<Json<GroupDetail>, AppError> {
    let (repo, user_id) = require_collab_read(&state, &headers, &owner, &name).await?;
    let group = svc::find_group(&state.db, repo.id, &group_id).await?;
    let can_write = rg_core::repo::service::can_write_repo(&state.db, &repo, Some(user_id))
        .await
        .map_err(AppError::from)?;
    let access = TargetAccess::new(&state, user_id);
    let links = svc::page_links(
        &state.db,
        cursor_key(&state),
        repo.id,
        &group.id,
        user_id,
        collaboration::clamp_limit(query.limit),
        None,
        Utc::now(),
        |row| access.resolve_link(row),
    )
    .await?;
    Ok(Json(GroupDetail { group: svc::GroupView::new(group, can_write), links }))
}

#[utoipa::path(
    get,
    path = "/repos/{owner}/{name}/collaboration/groups/{group_id}/links",
    tag = "Collaboration",
    params(("owner" = String, Path), ("name" = String, Path), ("group_id" = String, Path), ListQuery),
    responses(
        (status = 200, description = "{items, next_cursor, has_more, as_of}: only links whose target repository the caller can see, resolved with the repository's current name. Authorization happens first, then ordering and paging, on every page; there is no total and no marker for hidden links.", body = serde_json::Value),
        (status = 400, description = "Invalid cursor"),
        (status = 403, description = "Collaboration read access to the anchor denied"),
        (status = 404, description = "No such group in this repository")
    )
)]
pub async fn list_group_links(
    State(state): State<AppState>,
    Path((owner, name, group_id)): Path<(String, String, String)>,
    Query(query): Query<ListQuery>,
    headers: HeaderMap,
) -> Result<Json<svc::LinkPage>, AppError> {
    let (repo, user_id) = require_collab_read(&state, &headers, &owner, &name).await?;
    let group = svc::find_group(&state.db, repo.id, &group_id).await?;
    let access = TargetAccess::new(&state, user_id);
    let page = svc::page_links(
        &state.db,
        cursor_key(&state),
        repo.id,
        &group.id,
        user_id,
        collaboration::clamp_limit(query.limit),
        query.cursor.as_deref(),
        Utc::now(),
        |row| access.resolve_link(row),
    )
    .await?;
    Ok(Json(page))
}

#[utoipa::path(
    post,
    path = "/repos/{owner}/{name}/collaboration/groups/{group_id}/links",
    tag = "Collaboration",
    params(("owner" = String, Path), ("name" = String, Path), ("group_id" = String, Path)),
    request_body = AddLinkRequest,
    responses(
        (status = 201, description = "Link added; the group version moved by one", body = serde_json::Value),
        (status = 200, description = "Identical earlier request replayed", body = serde_json::Value),
        (status = 400, description = "Invalid field, or the commit is not in the target repository"),
        (status = 403, description = "Write access to the anchor or the (visible) target denied"),
        (status = 404, description = "No such group, target repository (or not visible to the caller), or task"),
        (status = 409, description = "VERSION_CONFLICT, LINK_ALREADY_EXISTS, GROUP_LINK_LIMIT_REACHED, IDEMPOTENCY_KEY_REUSED or SESSION_EXPIRED")
    )
)]
pub async fn add_group_link(
    State(state): State<AppState>,
    Path((owner, name, group_id)): Path<(String, String, String)>,
    headers: HeaderMap,
    Json(req): Json<AddLinkRequest>,
) -> Result<(StatusCode, Json<svc::LinkView>), AppError> {
    let (anchor, user_id) = require_collab_write(&state, &headers, &owner, &name).await?;
    let actor = write_actor(&state, &headers, anchor.id, user_id).await?;
    let group = svc::find_group(&state.db, anchor.id, &group_id).await?;
    let input = svc::NewLink {
        client_link_id: req.client_request_id,
        target_repo_id: req.target_repo_id,
        target_task_id: req.target_task_id,
        commit_sha: req.commit_sha,
        expected_group_version: req.expected_group_version,
    }
    .validated()?;

    let access = TargetAccess::new(&state, user_id);
    let target = access.writable(input.target_repo_id, "target repository not found").await?;

    // A retry of a request that succeeded must not fail because the world moved
    // on (the task or the commit may be gone since).
    if let Some(done) = svc::replay_add(&state.db, &group.id, &actor, &input).await? {
        return respond_link(&access, done.link, true).await;
    }
    if let Some(task_id) = &input.target_task_id {
        let found = is_task_row_id(task_id)
            && rg_db::ops::collab_task_ops::find_in_repo(&state.db, target.repo.id, task_id)
                .await
                .map_err(|e| AppError::internal(e.to_string()))?
                .is_some();
        if !found {
            return Err(AppError::not_found("task not found in the target repository"));
        }
    }
    if let Some(sha) = &input.commit_sha {
        if !commit_exists(&state, &target.owner, &target.repo.name, sha).await? {
            return Err(AppError::bad_request(
                "commit_sha does not name a commit in the target repository",
            ));
        }
    }
    let added = svc::add_link(&state.db, anchor.id, &group.id, user_id, &actor, &input, Utc::now()).await?;
    if !added.replayed {
        audit(
            &state,
            &headers,
            user_id,
            "collab.group.link_add",
            anchor.id,
            serde_json::json!({
                "group_id": group.id,
                "link_id": added.link.id,
                "target_repo_id": added.link.target_repo_id,
            }),
        )
        .await;
    }
    respond_link(&access, added.link, added.replayed).await
}

async fn respond_link(
    access: &TargetAccess<'_>,
    link: collab_coordination_link::Model,
    replayed: bool,
) -> Result<(StatusCode, Json<svc::LinkView>), AppError> {
    // The caller has just proven write access to the target, so it is visible.
    let view = access
        .resolve_link(link)
        .await?
        .ok_or_else(|| AppError::not_found("link not found"))?;
    let status = if replayed { StatusCode::OK } else { StatusCode::CREATED };
    Ok((status, Json(view)))
}

#[utoipa::path(
    delete,
    path = "/repos/{owner}/{name}/collaboration/groups/{group_id}/links/{link_id}",
    tag = "Collaboration",
    params(("owner" = String, Path), ("name" = String, Path), ("group_id" = String, Path), ("link_id" = String, Path)),
    request_body = RemoveLinkRequest,
    responses(
        (status = 200, description = "Link soft-removed (`removed_at` set) or an identical earlier removal replayed; the group version moved by one", body = serde_json::Value),
        (status = 400, description = "Invalid field"),
        (status = 403, description = "Write access to the anchor or the (visible) target denied"),
        (status = 404, description = "No such group or link, or the link's target is not visible to the caller"),
        (status = 409, description = "VERSION_CONFLICT, LINK_ALREADY_REMOVED, IDEMPOTENCY_KEY_REUSED or SESSION_EXPIRED")
    )
)]
pub async fn remove_group_link(
    State(state): State<AppState>,
    Path((owner, name, group_id, link_id)): Path<(String, String, String, String)>,
    headers: HeaderMap,
    Json(req): Json<RemoveLinkRequest>,
) -> Result<Json<svc::LinkView>, AppError> {
    let (anchor, user_id) = require_collab_write(&state, &headers, &owner, &name).await?;
    let actor: Actor = write_actor(&state, &headers, anchor.id, user_id).await?;
    let group = svc::find_group(&state.db, anchor.id, &group_id).await?;
    let input = svc::LinkRemoval {
        link_id,
        client_request_id: req.client_request_id,
        expected_group_version: req.expected_group_version,
    }
    .validated()?;
    let link = svc::find_link(&state.db, &group.id, &input.link_id)
        .await?
        .ok_or_else(|| AppError::not_found("link not found"))?;

    // Whoever cannot see the target gets the answer for a link that does not exist.
    let access = TargetAccess::new(&state, user_id);
    access.writable(link.target_repo_id, "link not found").await?;

    if let Some(done) = svc::replay_remove(&link, &group.id, &actor, &input)? {
        return finish_removal(&access, done.link).await;
    }
    if link.removed_at.is_some() {
        return Err(CollabError::coded_conflict(
            "LINK_ALREADY_REMOVED",
            "the link is not active (it was removed by another request)",
        )
        .into());
    }
    let removed =
        svc::remove_link(&state.db, anchor.id, &group.id, user_id, &actor, &input, Utc::now()).await?;
    if !removed.replayed {
        audit(
            &state,
            &headers,
            user_id,
            "collab.group.link_remove",
            anchor.id,
            serde_json::json!({
                "group_id": group.id,
                "link_id": removed.link.id,
                "target_repo_id": removed.link.target_repo_id,
            }),
        )
        .await;
    }
    finish_removal(&access, removed.link).await
}

async fn finish_removal(
    access: &TargetAccess<'_>,
    link: collab_coordination_link::Model,
) -> Result<Json<svc::LinkView>, AppError> {
    let view = access
        .resolve_link(link)
        .await?
        .ok_or_else(|| AppError::not_found("link not found"))?;
    Ok(Json(view))
}
