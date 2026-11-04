//! Database operations for the task dependency graph (`collab_dependency_graphs`,
//! `collab_task_dependencies`).
//!
//! Every edge change is serialised per repository by the graph version row: the
//! conditional bump of that row is the *first* statement of the transaction
//! (SQLite would otherwise fail concurrent writers with `SQLITE_BUSY`, see
//! CLAUDE.md pitfall 21), the cycle check and the edge write follow inside the
//! same transaction, and anything that goes wrong rolls the bump back. A client
//! that raced loses on the version, so two edges that together would close a
//! cycle can never both be committed from the same graph state.

use std::collections::HashMap;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use sea_orm::sea_query::Expr;
use sea_orm::*;

use crate::entities::collab_dependency_graph::{self, Entity as GraphEntity};
use crate::entities::collab_task::{self, Entity as TaskEntity, Model as Task};
use crate::entities::collab_task_dependency::{
    self, ActiveModel, Entity as EdgeEntity, Model as Edge,
};

/// Bounds of the cycle-check traversal and of a task's direct dependencies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GraphLimits {
    pub max_nodes: usize,
    pub max_edges: usize,
    pub max_direct: usize,
}

impl Default for GraphLimits {
    fn default() -> Self {
        Self {
            max_nodes: 1000,
            max_edges: 5000,
            max_direct: 100,
        }
    }
}

/// Which bound was hit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LimitKind {
    DirectDependencies,
    Nodes,
    Edges,
}

impl LimitKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::DirectDependencies => "direct_dependencies",
            Self::Nodes => "nodes",
            Self::Edges => "edges",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum AddOutcome {
    /// Edge inserted and the graph version bumped, in one transaction.
    Added { edge: Edge, graph_version: i64 },
    /// The idempotency key already owns an edge; compare `payload_hash`.
    Existing(Edge),
    /// The edge exists (created under another key).
    EdgeExists,
    /// The graph version no longer matches; `current_version` is the latest.
    VersionConflict { current_version: i64 },
    /// The edge would close a cycle; `path` runs from the new edge's target back
    /// to its source along existing edges.
    Cycle { path: Vec<String> },
    /// A bound was hit; nothing was written. Fail closed, never "assume acyclic".
    LimitExceeded { kind: LimitKind, max: usize },
}

#[derive(Debug, Clone, PartialEq)]
pub enum RemoveOutcome {
    Removed {
        graph_version: i64,
    },
    /// There is no such edge (already removed or never existed); nothing changed.
    Missing {
        graph_version: i64,
    },
    VersionConflict {
        current_version: i64,
    },
}

/// Current version of the repository's graph; 0 while no edge was ever written.
pub async fn graph_version<C: ConnectionTrait>(db: &C, repo_id: i64) -> Result<i64> {
    Ok(GraphEntity::find_by_id(repo_id)
        .one(db)
        .await
        .context("db: find dependency graph")?
        .map_or(0, |graph| graph.version))
}

pub async fn find_edge<C: ConnectionTrait>(
    db: &C,
    repo_id: i64,
    task_id: &str,
    depends_on_task_id: &str,
) -> Result<Option<Edge>> {
    EdgeEntity::find()
        .filter(collab_task_dependency::Column::RepoId.eq(repo_id))
        .filter(collab_task_dependency::Column::TaskId.eq(task_id))
        .filter(collab_task_dependency::Column::DependsOnTaskId.eq(depends_on_task_id))
        .one(db)
        .await
        .context("db: find dependency edge")
}

pub async fn find_by_client_id<C: ConnectionTrait>(
    db: &C,
    created_by_kind: &str,
    created_by_id: &str,
    client_dependency_id: &str,
) -> Result<Option<Edge>> {
    EdgeEntity::find()
        .filter(collab_task_dependency::Column::CreatedByKind.eq(created_by_kind))
        .filter(collab_task_dependency::Column::CreatedById.eq(created_by_id))
        .filter(collab_task_dependency::Column::ClientDependencyId.eq(client_dependency_id))
        .one(db)
        .await
        .context("db: find dependency by client id")
}

/// Direct dependencies of a task, oldest first.
pub async fn list_for_task<C: ConnectionTrait>(
    db: &C,
    repo_id: i64,
    task_id: &str,
) -> Result<Vec<Edge>> {
    EdgeEntity::find()
        .filter(collab_task_dependency::Column::RepoId.eq(repo_id))
        .filter(collab_task_dependency::Column::TaskId.eq(task_id))
        .order_by_asc(collab_task_dependency::Column::CreatedAt)
        .order_by_asc(collab_task_dependency::Column::DependsOnTaskId)
        .all(db)
        .await
        .context("db: list dependencies")
}

/// Tasks of this repository among `ids` (other repositories' ids read as absent).
pub async fn find_tasks<C: ConnectionTrait>(
    db: &C,
    repo_id: i64,
    ids: &[String],
) -> Result<Vec<Task>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    TaskEntity::find()
        .filter(collab_task::Column::RepoId.eq(repo_id))
        .filter(collab_task::Column::Id.is_in(ids.iter().cloned()))
        .all(db)
        .await
        .context("db: find dependency tasks")
}

/// Bump the version row from `expected` to `expected + 1`; `false` = someone
/// else got there first (or, for `expected == 0`, the row already exists).
async fn bump_version(
    txn: &DatabaseTransaction,
    repo_id: i64,
    expected: i64,
    now: DateTime<Utc>,
    allow_create: bool,
) -> Result<bool> {
    if expected == 0 {
        if !allow_create {
            return Ok(false);
        }
        let row = collab_dependency_graph::ActiveModel {
            repo_id: Set(repo_id),
            version: Set(1),
            updated_at: Set(now),
        };
        return match row.insert(txn).await {
            Ok(_) => Ok(true),
            Err(err) if matches!(err.sql_err(), Some(SqlErr::UniqueConstraintViolation(_))) => {
                Ok(false)
            }
            Err(err) => Err(err).context("db: create dependency graph"),
        };
    }
    let res = GraphEntity::update_many()
        .col_expr(
            collab_dependency_graph::Column::Version,
            Expr::value(expected + 1),
        )
        .col_expr(collab_dependency_graph::Column::UpdatedAt, Expr::value(now))
        .filter(collab_dependency_graph::Column::RepoId.eq(repo_id))
        .filter(collab_dependency_graph::Column::Version.eq(expected))
        .exec(txn)
        .await
        .context("db: bump dependency graph version")?;
    Ok(res.rows_affected == 1)
}

enum Reach {
    Found(Vec<String>),
    NotFound,
    Limit(LimitKind, usize),
}

/// Bounded breadth-first search along `task -> dependency` edges from `from`
/// looking for `target`. `Found` carries the path `from .. target`.
async fn reach<C: ConnectionTrait>(
    db: &C,
    repo_id: i64,
    from: &str,
    target: &str,
    limits: &GraphLimits,
) -> Result<Reach> {
    // Keeps SQL parameter lists far below every backend's bind limit.
    const CHUNK: usize = 200;
    let mut parent: HashMap<String, Option<String>> = HashMap::new();
    parent.insert(from.to_string(), None);
    let mut frontier = vec![from.to_string()];
    let mut edges_seen = 0usize;
    while !frontier.is_empty() {
        let mut next = Vec::new();
        for chunk in frontier.chunks(CHUNK) {
            let remaining = limits.max_edges - edges_seen;
            let rows = EdgeEntity::find()
                .filter(collab_task_dependency::Column::RepoId.eq(repo_id))
                .filter(collab_task_dependency::Column::TaskId.is_in(chunk.iter().cloned()))
                .limit(remaining as u64 + 1)
                .all(db)
                .await
                .context("db: traverse dependency graph")?;
            edges_seen += rows.len();
            if edges_seen > limits.max_edges {
                return Ok(Reach::Limit(LimitKind::Edges, limits.max_edges));
            }
            for edge in rows {
                if edge.depends_on_task_id == target {
                    let mut path = vec![edge.depends_on_task_id.clone()];
                    let mut cursor = Some(edge.task_id.clone());
                    while let Some(node) = cursor {
                        cursor = parent.get(&node).cloned().flatten();
                        path.push(node);
                    }
                    path.reverse();
                    return Ok(Reach::Found(path));
                }
                if !parent.contains_key(&edge.depends_on_task_id) {
                    if parent.len() >= limits.max_nodes {
                        return Ok(Reach::Limit(LimitKind::Nodes, limits.max_nodes));
                    }
                    parent.insert(edge.depends_on_task_id.clone(), Some(edge.task_id.clone()));
                    next.push(edge.depends_on_task_id);
                }
            }
        }
        frontier = next;
    }
    Ok(Reach::NotFound)
}

/// The outcome for an edge that is already stored: a replay when this very
/// request (same actor, same key) created it, otherwise a plain duplicate.
async fn existing_edge<C: ConnectionTrait>(
    db: &C,
    repo_id: i64,
    task_id: &str,
    depends_on_task_id: &str,
    created_by_kind: &str,
    created_by_id: &str,
    client_dependency_id: &str,
) -> Result<Option<AddOutcome>> {
    Ok(find_edge(db, repo_id, task_id, depends_on_task_id)
        .await?
        .map(|edge| {
            if edge.created_by_kind == created_by_kind
                && edge.created_by_id == created_by_id
                && edge.client_dependency_id == client_dependency_id
            {
                AddOutcome::Existing(edge)
            } else {
                AddOutcome::EdgeExists
            }
        }))
}

/// Add edge `model.task_id -> model.depends_on_task_id` iff the graph is still at
/// `expected_version`, the edge closes no cycle, and the bounds hold — all in one
/// transaction. On any refusal nothing is written (the version bump rolls back).
pub async fn add_edge(
    db: &DatabaseConnection,
    repo_id: i64,
    expected_version: i64,
    mut model: ActiveModel,
    limits: &GraphLimits,
    now: DateTime<Utc>,
) -> Result<AddOutcome> {
    let task_id = model.task_id.clone().unwrap();
    let dep_id = model.depends_on_task_id.clone().unwrap();
    let kind = model.created_by_kind.clone().unwrap();
    let by = model.created_by_id.clone().unwrap();
    let client_id = model.client_dependency_id.clone().unwrap();

    // Replay checks outside the transaction: its first statement must be the
    // write (SQLite cannot upgrade a read snapshot once another writer commits).
    if let Some(existing) = find_by_client_id(db, &kind, &by, &client_id).await? {
        return Ok(AddOutcome::Existing(existing));
    }
    if let Some(outcome) =
        existing_edge(db, repo_id, &task_id, &dep_id, &kind, &by, &client_id).await?
    {
        return Ok(outcome);
    }

    let txn = db.begin().await.context("db: begin add dependency")?;
    if !bump_version(&txn, repo_id, expected_version, now, true).await? {
        txn.rollback()
            .await
            .context("db: rollback add dependency")?;
        let current_version = graph_version(db, repo_id).await?;
        return Ok(AddOutcome::VersionConflict { current_version });
    }

    let direct = EdgeEntity::find()
        .filter(collab_task_dependency::Column::RepoId.eq(repo_id))
        .filter(collab_task_dependency::Column::TaskId.eq(task_id.as_str()))
        .count(&txn)
        .await
        .context("db: count direct dependencies")? as usize;
    if direct >= limits.max_direct {
        txn.rollback()
            .await
            .context("db: rollback add dependency")?;
        return Ok(AddOutcome::LimitExceeded {
            kind: LimitKind::DirectDependencies,
            max: limits.max_direct,
        });
    }

    match reach(&txn, repo_id, &dep_id, &task_id, limits).await? {
        Reach::Found(path) => {
            txn.rollback()
                .await
                .context("db: rollback add dependency")?;
            return Ok(AddOutcome::Cycle { path });
        }
        Reach::Limit(kind, max) => {
            txn.rollback()
                .await
                .context("db: rollback add dependency")?;
            return Ok(AddOutcome::LimitExceeded { kind, max });
        }
        Reach::NotFound => {}
    }

    let graph_version = expected_version + 1;
    model.graph_version = Set(graph_version);
    match model.insert(&txn).await {
        Ok(edge) => {
            txn.commit().await.context("db: commit add dependency")?;
            Ok(AddOutcome::Added {
                edge,
                graph_version,
            })
        }
        Err(err) if matches!(err.sql_err(), Some(SqlErr::UniqueConstraintViolation(_))) => {
            // Same key or same edge committed concurrently under a different
            // graph state: undo the bump and report what is there.
            txn.rollback()
                .await
                .context("db: rollback add dependency")?;
            if let Some(existing) = find_by_client_id(db, &kind, &by, &client_id).await? {
                return Ok(AddOutcome::Existing(existing));
            }
            Ok(
                existing_edge(db, repo_id, &task_id, &dep_id, &kind, &by, &client_id)
                    .await?
                    .unwrap_or(AddOutcome::EdgeExists),
            )
        }
        Err(err) => Err(err).context("db: insert dependency"),
    }
}

/// Remove an edge iff the graph is still at `expected_version`. A missing edge is
/// not an error (removal is idempotent) and is answered before the version is
/// looked at.
pub async fn remove_edge(
    db: &DatabaseConnection,
    repo_id: i64,
    task_id: &str,
    depends_on_task_id: &str,
    expected_version: i64,
    now: DateTime<Utc>,
) -> Result<RemoveOutcome> {
    if find_edge(db, repo_id, task_id, depends_on_task_id)
        .await?
        .is_none()
    {
        let graph_version = graph_version(db, repo_id).await?;
        return Ok(RemoveOutcome::Missing { graph_version });
    }
    let txn = db.begin().await.context("db: begin remove dependency")?;
    if !bump_version(&txn, repo_id, expected_version, now, false).await? {
        txn.rollback()
            .await
            .context("db: rollback remove dependency")?;
        let current_version = graph_version(db, repo_id).await?;
        return Ok(RemoveOutcome::VersionConflict { current_version });
    }
    let res = EdgeEntity::delete_many()
        .filter(collab_task_dependency::Column::RepoId.eq(repo_id))
        .filter(collab_task_dependency::Column::TaskId.eq(task_id))
        .filter(collab_task_dependency::Column::DependsOnTaskId.eq(depends_on_task_id))
        .exec(&txn)
        .await
        .context("db: delete dependency")?;
    if res.rows_affected == 0 {
        txn.rollback()
            .await
            .context("db: rollback remove dependency")?;
        let graph_version = graph_version(db, repo_id).await?;
        return Ok(RemoveOutcome::Missing { graph_version });
    }
    txn.commit().await.context("db: commit remove dependency")?;
    Ok(RemoveOutcome::Removed {
        graph_version: expected_version + 1,
    })
}
