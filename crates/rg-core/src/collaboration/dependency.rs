//! Task dependencies: "task A waits for task B" edges per repository.
//!
//! An edge is a *declaration* by a session or a user door. It blocks nothing and
//! grants no right: the dependency's status shown next to it is the status that
//! task's participants declared, not evidence that the work is done or verified.
//! `blocked_by` is derived at read time from those declarations.
//!
//! Every change goes through the repository's graph version (optimistic lock);
//! the cycle check and the edge write run in the same transaction as the version
//! bump (see `rg_db::ops::collab_dependency_ops`).

use chrono::{DateTime, Utc};
use rg_db::entities::collab_task_dependency;
use rg_db::ops::collab_dependency_ops::{self, AddOutcome, RemoveOutcome};
use sea_orm::{DatabaseConnection, Set};
use serde::Serialize;
use sha2::{Digest, Sha256};

use super::error::{CollabError, CollabResult};
use super::task;
use super::{validation, Actor, ORIGIN_USER_CONTENT};

pub use rg_db::ops::collab_dependency_ops::GraphLimits;

/// The only status that ends a wait; everything else is "not done".
pub const DONE_STATUS: &str = "done";

#[derive(Debug, Clone)]
pub struct AddRequest {
    pub client_request_id: String,
    pub depends_on_task_id: String,
    pub expected_graph_version: i64,
}

#[derive(Debug, Clone)]
pub struct RemoveRequest {
    pub client_request_id: String,
    pub expected_graph_version: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct DependencyView {
    pub task_id: String,
    pub depends_on_task_id: String,
    /// `session` or `user`: the door used, not proof of who used it.
    pub created_by_kind: String,
    pub created_by_id: String,
    /// Graph version this edge's insertion produced.
    pub graph_version: i64,
    pub created_at: DateTime<Utc>,
    pub origin: &'static str,
}

fn view(row: collab_task_dependency::Model) -> DependencyView {
    DependencyView {
        task_id: row.task_id,
        depends_on_task_id: row.depends_on_task_id,
        created_by_kind: row.created_by_kind,
        created_by_id: row.created_by_id,
        graph_version: row.graph_version,
        created_at: row.created_at,
        origin: ORIGIN_USER_CONTENT,
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct AddResult {
    pub dependency: DependencyView,
    /// Version after the change (the current one when `replayed`).
    pub graph_version: i64,
    /// The same client id was already applied; nothing changed this time.
    pub replayed: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct RemoveResult {
    /// `false` when the edge did not exist (already removed): idempotent success.
    pub removed: bool,
    pub graph_version: i64,
}

/// A dependency with the status its task declares.
#[derive(Debug, Clone, Serialize)]
pub struct DependencyItem {
    #[serde(flatten)]
    pub edge: DependencyView,
    pub title: String,
    /// Declared by the dependency task's participants; not verified.
    pub status: String,
    pub task_version: i64,
    pub task_updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DependencyList {
    pub task_id: String,
    pub graph_version: i64,
    pub dependencies: Vec<DependencyItem>,
    /// Ids of dependencies whose declared status is not `done`.
    pub blocked_by: Vec<String>,
    /// Statuses are declarations by task participants, not evidence.
    pub status_source: &'static str,
    pub origin: &'static str,
}

fn add_hash(task_id: &str, req: &AddRequest) -> String {
    let canonical = serde_json::json!([
        "add",
        task_id,
        req.depends_on_task_id,
        req.expected_graph_version
    ]);
    hex::encode(Sha256::digest(canonical.to_string().as_bytes()))
}

fn limit_error(kind: collab_dependency_ops::LimitKind, max: usize) -> CollabError {
    CollabError::coded_conflict(
        "GRAPH_LIMIT_EXCEEDED",
        "the dependency graph is too large to check safely; nothing was changed",
    )
    .with_details(serde_json::json!({ "limit": kind.as_str(), "max": max }))
}

/// Direct dependencies of a task with their declared status.
pub async fn list(
    db: &DatabaseConnection,
    repo_id: i64,
    task_id: &str,
) -> CollabResult<DependencyList> {
    let task = task::find(db, repo_id, task_id).await?;
    // Version first, edges second: a concurrent change makes the version older
    // than the edges (a stale expected version then loses), never newer.
    let graph_version = collab_dependency_ops::graph_version(db, repo_id).await?;
    let edges = collab_dependency_ops::list_for_task(db, repo_id, &task.id).await?;
    let ids: Vec<String> = edges.iter().map(|e| e.depends_on_task_id.clone()).collect();
    let tasks = collab_dependency_ops::find_tasks(db, repo_id, &ids).await?;
    let mut dependencies = Vec::with_capacity(edges.len());
    let mut blocked_by = Vec::new();
    for edge in edges {
        // A dependency task cannot vanish (foreign key); skip defensively.
        let Some(dep) = tasks.iter().find(|t| t.id == edge.depends_on_task_id) else {
            continue;
        };
        if dep.status != DONE_STATUS {
            blocked_by.push(dep.id.clone());
        }
        dependencies.push(DependencyItem {
            edge: view(edge),
            title: dep.title.clone(),
            status: dep.status.clone(),
            task_version: dep.version,
            task_updated_at: dep.updated_at,
        });
    }
    Ok(DependencyList {
        task_id: task.id,
        graph_version,
        dependencies,
        blocked_by,
        status_source: "declared",
        origin: ORIGIN_USER_CONTENT,
    })
}

pub async fn add(
    db: &DatabaseConnection,
    repo_id: i64,
    task_id: &str,
    actor: &Actor,
    req: AddRequest,
    limits: &GraphLimits,
    now: DateTime<Utc>,
) -> CollabResult<AddResult> {
    validation::client_key("client_request_id", &req.client_request_id)?;
    if req.expected_graph_version < 0 {
        return Err(CollabError::bad_request(
            "expected_graph_version must not be negative",
        ));
    }
    let task = task::find(db, repo_id, task_id).await?;
    let hash = add_hash(&task.id, &req);

    // A retry of a request that already succeeded is a replay, whatever the graph
    // looks like now (and a key reused for another request is refused).
    if let Some(row) =
        collab_dependency_ops::find_by_client_id(db, actor.kind, &actor.id, &req.client_request_id)
            .await?
    {
        if row.payload_hash != hash {
            return Err(CollabError::IdempotencyKeyReused);
        }
        let graph_version = collab_dependency_ops::graph_version(db, repo_id).await?;
        return Ok(AddResult {
            dependency: view(row),
            graph_version,
            replayed: true,
        });
    }

    if task.id == req.depends_on_task_id {
        return Err(CollabError::bad_request("a task cannot depend on itself"));
    }
    // Another repository's task reads as absent, like a nonexistent one.
    let dep = task::find(db, repo_id, &req.depends_on_task_id)
        .await
        .map_err(|e| match e {
            CollabError::NotFound(_) => CollabError::not_found("dependency task not found"),
            other => other,
        })?;

    let model = collab_task_dependency::ActiveModel {
        repo_id: Set(repo_id),
        task_id: Set(task.id.clone()),
        depends_on_task_id: Set(dep.id.clone()),
        created_by_kind: Set(actor.kind.to_string()),
        created_by_id: Set(actor.id.clone()),
        client_dependency_id: Set(req.client_request_id.clone()),
        payload_hash: Set(hash.clone()),
        graph_version: Set(0),
        created_at: Set(now),
    };
    match collab_dependency_ops::add_edge(
        db,
        repo_id,
        req.expected_graph_version,
        model,
        limits,
        now,
    )
    .await?
    {
        AddOutcome::Added {
            edge,
            graph_version,
        } => Ok(AddResult {
            dependency: view(edge),
            graph_version,
            replayed: false,
        }),
        AddOutcome::Existing(row) if row.payload_hash == hash => {
            let graph_version = collab_dependency_ops::graph_version(db, repo_id).await?;
            Ok(AddResult {
                dependency: view(row),
                graph_version,
                replayed: true,
            })
        }
        AddOutcome::Existing(_) => Err(CollabError::IdempotencyKeyReused),
        AddOutcome::EdgeExists => Err(CollabError::coded_conflict(
            "DEPENDENCY_EXISTS",
            "this task already depends on that task",
        )),
        AddOutcome::VersionConflict { current_version } => {
            Err(CollabError::VersionConflict { current_version })
        }
        AddOutcome::Cycle { path } => Err(CollabError::coded_conflict(
            "DEPENDENCY_CYCLE",
            "the dependency would make tasks wait for each other",
        )
        .with_details(serde_json::json!({ "cycle_path": path }))),
        AddOutcome::LimitExceeded { kind, max } => Err(limit_error(kind, max)),
    }
}

/// Remove `task_id -> depends_on_task_id`. A missing edge is answered with
/// `removed: false` before the graph version is compared: a retry of a removal
/// that succeeded still succeeds, and if the edge was re-added since, the retry's
/// stale version makes it a `VERSION_CONFLICT` instead of removing it again.
pub async fn remove(
    db: &DatabaseConnection,
    repo_id: i64,
    task_id: &str,
    depends_on_task_id: &str,
    req: RemoveRequest,
    now: DateTime<Utc>,
) -> CollabResult<RemoveResult> {
    validation::client_key("client_request_id", &req.client_request_id)?;
    if req.expected_graph_version < 0 {
        return Err(CollabError::bad_request(
            "expected_graph_version must not be negative",
        ));
    }
    let task = task::find(db, repo_id, task_id).await?;
    if !validation::is_uuid(depends_on_task_id) {
        return Ok(RemoveResult {
            removed: false,
            graph_version: collab_dependency_ops::graph_version(db, repo_id).await?,
        });
    }
    match collab_dependency_ops::remove_edge(
        db,
        repo_id,
        &task.id,
        depends_on_task_id,
        req.expected_graph_version,
        now,
    )
    .await?
    {
        RemoveOutcome::Removed { graph_version } => Ok(RemoveResult {
            removed: true,
            graph_version,
        }),
        RemoveOutcome::Missing { graph_version } => Ok(RemoveResult {
            removed: false,
            graph_version,
        }),
        RemoveOutcome::VersionConflict { current_version } => {
            Err(CollabError::VersionConflict { current_version })
        }
    }
}
