//! Cross-repository coordination groups (P2 X02).
//!
//! Design: `ironforge-docs/architecture/agent-collaboration-p2-design-2026-11.md`
//! §2–§4. A group is owned by an *anchor* repository and links to tasks and
//! commits of other repositories **by repository id**. Nothing here grants Git,
//! CI, merge or approval rights, moves a task, or copies target-repository
//! content into the group.
//!
//! ## Authorization is the caller's, and comes first
//!
//! This module does not know who is asking. [`page_links`] therefore takes a
//! `resolve` callback that returns `None` for every link the requester may not
//! see; the listing authorizes **first**, then orders/paginates the visible
//! rows, so that `has_more`, the cursor and the page length are functions of
//! the visible set only and nothing reports "N hidden". When filtering leaves a
//! page short the scan simply continues until the page is full or the group is
//! exhausted.
//!
//! ## Pagination is a re-authorizing keyset, not a snapshot
//!
//! The design suggests a short-lived shared snapshot of visible link ids. This
//! implementation has no snapshot table: a cursor is the HMAC-protected
//! `(created_at, id)` position of the last returned link, bound to the account,
//! anchor and group, and **every page re-authorizes every link it looks at**.
//! Consequences (see the verification note): there is no `CURSOR_EXPIRED`;
//! links *behind* the cursor are never added later, but a link that becomes
//! visible *ahead* of it (a re-granted permission, a restored repository, a
//! newly added link) is picked up by the following pages.

use chrono::{DateTime, Utc};
use rg_db::entities::{collab_coordination_group as group_entity, collab_coordination_link as link_entity};
use rg_db::ops::collab_group_ops::{self, AddLinkOutcome, CreateGroupOutcome, RemoveInfo, RemoveLinkOutcome};
use sea_orm::{DatabaseConnection, Set};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::future::Future;

use super::cursor::{self, KeysetPosition};
use super::error::{CollabError, CollabResult};
use super::message::normalize_commit_sha;
use super::{validation, Actor, ORIGIN_USER_CONTENT};

pub const MAX_TITLE_CHARS: usize = 200;
pub const MAX_DESCRIPTION_BYTES: usize = 16 * 1024;
/// Upper bound of *active* links per group (hidden ones included). Bounds the
/// work of one authorized scan.
pub const MAX_ACTIVE_LINKS: u64 = 1000;
/// Rows read from the database per scan step.
const SCAN_BATCH: u64 = 100;

/// Shown to creators and returned with a new group: a group is data of the
/// anchor repository, whatever the linked repositories' own visibility is.
pub const SHARING_NOTICE: &str = "title and description are readable by everyone who can read the anchor \
repository's collaboration data, including people who cannot read the linked repositories. The server \
never copies content of linked repositories into them; do not put confidential details of other \
repositories there.";

// ── views ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct GroupView {
    pub id: String,
    pub anchor_repo_id: i64,
    /// Client-written; readable by all readers of the anchor's collaboration
    /// data (see [`SHARING_NOTICE`]).
    pub title: String,
    pub description: String,
    pub created_by_kind: String,
    pub created_by_id: String,
    /// CAS counter for link changes. Only sent to callers with write access to
    /// the anchor: it also moves for links to repositories the reader cannot
    /// see, so readers get no such signal.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<i64>,
    pub created_at: DateTime<Utc>,
    pub origin: &'static str,
}

impl GroupView {
    pub fn new(row: group_entity::Model, include_version: bool) -> Self {
        Self {
            id: row.id,
            anchor_repo_id: row.anchor_repo_id,
            title: row.title,
            description: row.description,
            created_by_kind: row.actor_kind,
            created_by_id: row.actor_id,
            version: include_version.then_some(row.version),
            created_at: row.created_at,
            origin: ORIGIN_USER_CONTENT,
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct RepoRef {
    pub id: i64,
    /// Namespace (user or organization) *now*, resolved at read time.
    pub owner: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct TaskRef {
    pub id: String,
    /// Resolved at read time from the target repository; a client-written claim.
    pub title: String,
    pub status: String,
    pub version: i64,
    pub origin: &'static str,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct CommitRef {
    pub sha: String,
    /// `available` (the object is readable now) or `unavailable` (it is gone,
    /// e.g. after garbage collection). The sha is never replaced by another.
    pub commit_state: &'static str,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct LinkView {
    pub id: String,
    pub group_id: String,
    pub target_repo: RepoRef,
    pub task: Option<TaskRef>,
    pub commit: Option<CommitRef>,
    pub created_by_kind: String,
    pub created_by_id: String,
    pub created_at: DateTime<Utc>,
    pub removed_at: Option<DateTime<Utc>>,
}

impl LinkView {
    pub fn new(
        row: &link_entity::Model,
        target_repo: RepoRef,
        task: Option<TaskRef>,
        commit_state: Option<&'static str>,
    ) -> Self {
        Self {
            id: row.id.clone(),
            group_id: row.group_id.clone(),
            target_repo,
            task,
            commit: row.commit_sha.clone().zip(commit_state).map(|(sha, commit_state)| CommitRef { sha, commit_state }),
            created_by_kind: row.actor_kind.clone(),
            created_by_id: row.actor_id.clone(),
            created_at: row.created_at,
            removed_at: row.removed_at,
        }
    }
}

/// A page of the links the requester may see. There is deliberately no total.
#[derive(Debug, Clone, Serialize)]
pub struct LinkPage {
    pub items: Vec<LinkView>,
    pub next_cursor: Option<String>,
    pub has_more: bool,
    pub as_of: DateTime<Utc>,
}

// ── groups ──────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct NewGroup {
    pub client_group_id: String,
    pub title: String,
    pub description: String,
}

pub struct CreatedGroup {
    pub group: group_entity::Model,
    pub replayed: bool,
}

fn group_hash(anchor_repo_id: i64, title: &str, description: &str) -> String {
    let canonical = serde_json::json!([anchor_repo_id, title, description]);
    hex::encode(Sha256::digest(canonical.to_string().as_bytes()))
}

/// Create a group owned by `anchor_repo_id`. The caller has verified write
/// access to the anchor.
pub async fn create_group(
    db: &DatabaseConnection,
    anchor_repo_id: i64,
    user_id: i64,
    actor: &Actor,
    input: NewGroup,
    now: DateTime<Utc>,
) -> CollabResult<CreatedGroup> {
    validation::client_key("client_request_id", &input.client_group_id)?;
    let title = validation::label("title", &input.title, MAX_TITLE_CHARS)?;
    validation::text("description", &input.description, 0, MAX_DESCRIPTION_BYTES)?;
    let hash = group_hash(anchor_repo_id, &title, &input.description);
    let outcome = collab_group_ops::create_group(
        db,
        group_entity::ActiveModel {
            id: Set(uuid::Uuid::new_v4().to_string()),
            anchor_repo_id: Set(anchor_repo_id),
            title: Set(title),
            description: Set(input.description),
            created_by_user_id: Set(user_id),
            actor_kind: Set(actor.kind.to_string()),
            actor_id: Set(actor.id.clone()),
            version: Set(1),
            client_group_id: Set(input.client_group_id),
            payload_hash: Set(hash.clone()),
            created_at: Set(now),
            updated_at: Set(now),
        },
    )
    .await?;
    match outcome {
        CreateGroupOutcome::Created(group) => Ok(CreatedGroup { group, replayed: false }),
        CreateGroupOutcome::Existing(group) if group.payload_hash == hash => {
            Ok(CreatedGroup { group, replayed: true })
        }
        CreateGroupOutcome::Existing(_) => Err(CollabError::IdempotencyKeyReused),
    }
}

/// A group of the anchor repository; any other id is "not found".
pub async fn find_group(
    db: &DatabaseConnection,
    anchor_repo_id: i64,
    id: &str,
) -> CollabResult<group_entity::Model> {
    if !validation::is_uuid(id) {
        return Err(CollabError::not_found("group not found"));
    }
    collab_group_ops::find_group_in_anchor(db, anchor_repo_id, id)
        .await?
        .ok_or_else(|| CollabError::not_found("group not found"))
}

// ── adding and removing links ───────────────────────────────────────

#[derive(Debug, Clone)]
pub struct NewLink {
    pub client_link_id: String,
    pub target_repo_id: i64,
    pub target_task_id: Option<String>,
    pub commit_sha: Option<String>,
    pub expected_group_version: i64,
}

impl NewLink {
    /// Validate and normalize (lower-case commit id). Pure; no repository access.
    pub fn validated(mut self) -> CollabResult<Self> {
        validation::client_key("client_request_id", &self.client_link_id)?;
        if self.target_repo_id <= 0 {
            return Err(CollabError::bad_request("target_repo_id must be a repository id"));
        }
        if self.target_task_id.is_none() && self.commit_sha.is_none() {
            return Err(CollabError::bad_request(
                "a link needs a target_task_id and/or a commit_sha",
            ));
        }
        self.commit_sha = self.commit_sha.as_deref().map(normalize_commit_sha).transpose()?;
        Ok(self)
    }

    fn hash(&self, group_id: &str) -> String {
        let canonical = serde_json::json!([
            group_id,
            self.target_repo_id,
            self.target_task_id,
            self.commit_sha,
            self.expected_group_version
        ]);
        hex::encode(Sha256::digest(canonical.to_string().as_bytes()))
    }

    fn active_key(&self, group_id: &str) -> String {
        let canonical = serde_json::json!([
            group_id,
            self.target_repo_id,
            self.target_task_id,
            self.commit_sha
        ]);
        hex::encode(Sha256::digest(canonical.to_string().as_bytes()))
    }
}

pub struct AddedLink {
    pub link: link_entity::Model,
    pub replayed: bool,
}

/// The earlier result of this very request, if any. Call it **before** the
/// preconditions that depend on current state (does the task / commit still
/// exist?): a successful request must stay replayable. The caller has
/// re-authorized the request.
pub async fn replay_add(
    db: &DatabaseConnection,
    group_id: &str,
    actor: &Actor,
    input: &NewLink,
) -> CollabResult<Option<AddedLink>> {
    let Some(existing) =
        collab_group_ops::find_link_by_client_id(db, actor.kind, &actor.id, &input.client_link_id).await?
    else {
        return Ok(None);
    };
    if existing.payload_hash == input.hash(group_id) {
        Ok(Some(AddedLink { link: existing, replayed: true }))
    } else {
        Err(CollabError::IdempotencyKeyReused)
    }
}

/// Add a link and bump the group version in one transaction. The caller has
/// authorized write access to the anchor and the target repository and has
/// verified that the task / commit exist there.
pub async fn add_link(
    db: &DatabaseConnection,
    anchor_repo_id: i64,
    group_id: &str,
    user_id: i64,
    actor: &Actor,
    input: &NewLink,
    now: DateTime<Utc>,
) -> CollabResult<AddedLink> {
    let hash = input.hash(group_id);
    let outcome = collab_group_ops::add_link_cas(
        db,
        anchor_repo_id,
        group_id,
        input.expected_group_version,
        link_entity::ActiveModel {
            id: Set(uuid::Uuid::new_v4().to_string()),
            group_id: Set(group_id.to_string()),
            anchor_repo_id: Set(anchor_repo_id),
            target_repo_id: Set(input.target_repo_id),
            target_task_id: Set(input.target_task_id.clone()),
            commit_sha: Set(input.commit_sha.clone()),
            active_key: Set(Some(input.active_key(group_id))),
            created_by_user_id: Set(user_id),
            actor_kind: Set(actor.kind.to_string()),
            actor_id: Set(actor.id.clone()),
            client_link_id: Set(input.client_link_id.clone()),
            payload_hash: Set(hash.clone()),
            created_at: Set(now),
            removed_at: Set(None),
            removed_by_user_id: Set(None),
            removed_actor_kind: Set(None),
            removed_actor_id: Set(None),
            remove_client_id: Set(None),
            remove_payload_hash: Set(None),
        },
        MAX_ACTIVE_LINKS,
        now,
    )
    .await?;
    match outcome {
        AddLinkOutcome::Added { link, .. } => Ok(AddedLink { link, replayed: false }),
        AddLinkOutcome::Existing(link) if link.payload_hash == hash => {
            Ok(AddedLink { link, replayed: true })
        }
        AddLinkOutcome::Existing(_) => Err(CollabError::IdempotencyKeyReused),
        AddLinkOutcome::Conflict { current_version } => Err(CollabError::VersionConflict { current_version }),
        AddLinkOutcome::Duplicate { link_id } => Err(CollabError::coded_conflict(
            "LINK_ALREADY_EXISTS",
            "an active link to this target already exists in the group",
        )
        .with_details(serde_json::json!({ "link_id": link_id }))),
        AddLinkOutcome::LimitReached => Err(CollabError::coded_conflict(
            "GROUP_LINK_LIMIT_REACHED",
            format!("a group holds at most {MAX_ACTIVE_LINKS} active links"),
        )),
        AddLinkOutcome::GroupNotFound => Err(CollabError::not_found("group not found")),
    }
}

#[derive(Debug, Clone)]
pub struct LinkRemoval {
    pub link_id: String,
    pub client_request_id: String,
    pub expected_group_version: i64,
}

impl LinkRemoval {
    pub fn validated(self) -> CollabResult<Self> {
        validation::client_key("client_request_id", &self.client_request_id)?;
        Ok(self)
    }

    fn hash(&self, group_id: &str) -> String {
        let canonical = serde_json::json!([group_id, self.link_id, self.expected_group_version]);
        hex::encode(Sha256::digest(canonical.to_string().as_bytes()))
    }
}

pub struct RemovedLink {
    pub link: link_entity::Model,
    pub replayed: bool,
}

/// A link of the group, active or removed (`None` for a foreign or malformed id).
pub async fn find_link(
    db: &DatabaseConnection,
    group_id: &str,
    link_id: &str,
) -> CollabResult<Option<link_entity::Model>> {
    if !validation::is_uuid(link_id) {
        return Ok(None);
    }
    Ok(collab_group_ops::find_link_in_group(db, group_id, link_id).await?)
}

/// Whether `link` was already removed by exactly this request. Like
/// [`replay_add`], consult it before preconditions about the link's state.
pub fn replay_remove(
    link: &link_entity::Model,
    group_id: &str,
    actor: &Actor,
    input: &LinkRemoval,
) -> CollabResult<Option<RemovedLink>> {
    let same_key = link.removed_actor_kind.as_deref() == Some(actor.kind)
        && link.removed_actor_id.as_deref() == Some(actor.id.as_str())
        && link.remove_client_id.as_deref() == Some(input.client_request_id.as_str());
    if !same_key {
        return Ok(None);
    }
    if link.remove_payload_hash.as_deref() == Some(input.hash(group_id).as_str()) {
        Ok(Some(RemovedLink { link: link.clone(), replayed: true }))
    } else {
        Err(CollabError::IdempotencyKeyReused)
    }
}

/// Soft-remove a link and bump the group version in one transaction. The
/// caller has authorized write access to the anchor and the link's target.
pub async fn remove_link(
    db: &DatabaseConnection,
    anchor_repo_id: i64,
    group_id: &str,
    user_id: i64,
    actor: &Actor,
    input: &LinkRemoval,
    now: DateTime<Utc>,
) -> CollabResult<RemovedLink> {
    let hash = input.hash(group_id);
    let outcome = collab_group_ops::remove_link_cas(
        db,
        anchor_repo_id,
        group_id,
        &input.link_id,
        input.expected_group_version,
        &RemoveInfo {
            user_id,
            actor_kind: actor.kind.to_string(),
            actor_id: actor.id.clone(),
            client_id: input.client_request_id.clone(),
            payload_hash: hash.clone(),
        },
        now,
    )
    .await?;
    match outcome {
        RemoveLinkOutcome::Removed { link, .. } => Ok(RemovedLink { link, replayed: false }),
        RemoveLinkOutcome::Existing(link) if link.remove_payload_hash.as_deref() == Some(hash.as_str()) => {
            Ok(RemovedLink { link, replayed: true })
        }
        RemoveLinkOutcome::Existing(_) => Err(CollabError::IdempotencyKeyReused),
        RemoveLinkOutcome::Conflict { current_version } => Err(CollabError::VersionConflict { current_version }),
        RemoveLinkOutcome::NotActive => Err(CollabError::coded_conflict(
            "LINK_ALREADY_REMOVED",
            "the link is not active (it was removed by another request)",
        )),
        RemoveLinkOutcome::GroupNotFound => Err(CollabError::not_found("group not found")),
    }
}

// ── listing ─────────────────────────────────────────────────────────

fn links_scope(anchor_repo_id: i64, group_id: &str, principal_id: i64) -> String {
    format!("group-links:v1:repo={anchor_repo_id}:group={group_id}:user={principal_id}:order=created_asc,id_asc")
}

/// One page of the links `resolve` lets the requester see.
///
/// `resolve` is called for stored links in order and returns the view of a link
/// the requester is authorized to see, `None` for any other (no access to the
/// target, repository deleted, ...) — silently, with no reason. The scan goes
/// on, batch after batch, until the page is full and one more visible link
/// proved `has_more`, or the group is exhausted.
#[allow(clippy::too_many_arguments)]
pub async fn page_links<F, Fut>(
    db: &DatabaseConnection,
    cursor_key: &[u8],
    anchor_repo_id: i64,
    group_id: &str,
    principal_id: i64,
    limit: u64,
    cursor_token: Option<&str>,
    now: DateTime<Utc>,
    resolve: F,
) -> CollabResult<LinkPage>
where
    F: Fn(link_entity::Model) -> Fut,
    Fut: Future<Output = CollabResult<Option<LinkView>>>,
{
    let scope = links_scope(anchor_repo_id, group_id, principal_id);
    let mut after = cursor_token
        .map(|token| cursor::decode(cursor_key, &scope, token))
        .transpose()?
        .map(|p| (p.at, p.id));

    let mut items: Vec<LinkView> = Vec::new();
    let mut has_more = false;
    'scan: loop {
        let rows = collab_group_ops::list_active_batch(db, group_id, after.clone(), SCAN_BATCH).await?;
        let exhausted = (rows.len() as u64) < SCAN_BATCH;
        for row in rows {
            after = Some((row.created_at, row.id.clone()));
            if let Some(view) = resolve(row).await? {
                if items.len() as u64 == limit {
                    has_more = true;
                    break 'scan;
                }
                items.push(view);
            }
        }
        if exhausted {
            break;
        }
    }
    let next_cursor = has_more
        .then(|| items.last())
        .flatten()
        .map(|last| {
            cursor::encode(
                cursor_key,
                &scope,
                &KeysetPosition { at: last.created_at, id: last.id.clone() },
            )
        });
    Ok(LinkPage { items, next_cursor, has_more, as_of: now })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn link(kind: Option<&str>, sha: Option<&str>) -> NewLink {
        NewLink {
            client_link_id: "k".into(),
            target_repo_id: 7,
            target_task_id: kind.map(str::to_string),
            commit_sha: sha.map(str::to_string),
            expected_group_version: 1,
        }
    }

    #[test]
    fn a_link_needs_a_task_or_a_commit_and_a_full_sha() {
        assert!(link(None, None).validated().is_err());
        assert!(link(Some("t"), None).validated().is_ok());
        assert!(link(None, Some("abc123")).validated().is_err(), "short sha");
        let upper = "A".repeat(40);
        let ok = link(None, Some(&upper)).validated().unwrap();
        assert_eq!(ok.commit_sha.unwrap(), "a".repeat(40), "normalized to lower case");
        let mut bad_repo = link(Some("t"), None);
        bad_repo.target_repo_id = 0;
        assert!(bad_repo.validated().is_err());
    }

    #[test]
    fn hashes_separate_bodies_but_not_the_key_of_active_links() {
        let a = link(Some("t"), None);
        let mut b = a.clone();
        b.expected_group_version = 2;
        assert_ne!(a.hash("g"), b.hash("g"), "version is part of the request body");
        assert_eq!(a.active_key("g"), b.active_key("g"), "same target, same active key");
        assert_ne!(a.active_key("g"), a.active_key("h"), "keys are per group");
        let mut c = a.clone();
        c.commit_sha = Some("a".repeat(40));
        assert_ne!(a.active_key("g"), c.active_key("g"));
    }
}
