//! Database operations for completion conditions (O04): condition set
//! versions, manual confirmations and evaluation snapshots.

use anyhow::{Context, Result};
use sea_orm::*;

use crate::entities::collab_completion_condition_set::{self as sets, Entity as SetEntity};
use crate::entities::collab_completion_confirmation::{self as confs, Entity as ConfEntity};
use crate::entities::collab_completion_snapshot::{self as snaps, Entity as SnapEntity};

pub type ConditionSet = sets::Model;
pub type Confirmation = confs::Model;
pub type Snapshot = snaps::Model;

#[derive(Debug, Clone, PartialEq)]
pub enum InsertOutcome<T> {
    Inserted(T),
    /// The client request id was already used; the row that owns it.
    Replay(T),
    /// Another writer took the same `version` first.
    VersionTaken,
}

fn is_unique(err: &DbErr) -> bool {
    matches!(err.sql_err(), Some(SqlErr::UniqueConstraintViolation(_)))
}

/// The newest version of the task's condition set.
pub async fn current_set<C: ConnectionTrait>(
    db: &C,
    task_id: &str,
) -> Result<Option<ConditionSet>> {
    SetEntity::find()
        .filter(sets::Column::TaskId.eq(task_id))
        .order_by_desc(sets::Column::Version)
        .one(db)
        .await
        .context("db: find current condition set")
}

pub async fn find_set_by_request<C: ConnectionTrait>(
    db: &C,
    task_id: &str,
    client_request_id: &str,
) -> Result<Option<ConditionSet>> {
    SetEntity::find()
        .filter(sets::Column::TaskId.eq(task_id))
        .filter(sets::Column::ClientRequestId.eq(client_request_id))
        .one(db)
        .await
        .context("db: find condition set by request id")
}

/// Condition set versions, newest first, older than `before_version`.
pub async fn list_sets<C: ConnectionTrait>(
    db: &C,
    task_id: &str,
    before_version: Option<i64>,
    limit: u64,
) -> Result<Vec<ConditionSet>> {
    let mut query = SetEntity::find().filter(sets::Column::TaskId.eq(task_id));
    if let Some(before) = before_version {
        query = query.filter(sets::Column::Version.lt(before));
    }
    query
        .order_by_desc(sets::Column::Version)
        .limit(limit)
        .all(db)
        .await
        .context("db: list condition sets")
}

pub async fn get_set<C: ConnectionTrait>(
    db: &C,
    task_id: &str,
    version: i64,
) -> Result<Option<ConditionSet>> {
    SetEntity::find()
        .filter(sets::Column::TaskId.eq(task_id))
        .filter(sets::Column::Version.eq(version))
        .one(db)
        .await
        .context("db: get condition set")
}

/// Insert the next version. The unique `(task_id, version)` key arbitrates two
/// writers that read the same current version: exactly one inserts.
pub async fn insert_set(
    db: &DatabaseConnection,
    model: sets::ActiveModel,
    task_id: &str,
    client_request_id: &str,
) -> Result<InsertOutcome<ConditionSet>> {
    match model.insert(db).await {
        Ok(row) => Ok(InsertOutcome::Inserted(row)),
        Err(err) if is_unique(&err) => {
            match find_set_by_request(db, task_id, client_request_id).await? {
                Some(row) => Ok(InsertOutcome::Replay(row)),
                None => Ok(InsertOutcome::VersionTaken),
            }
        }
        Err(err) => Err(err).context("db: insert condition set"),
    }
}

pub async fn find_confirmation_by_request<C: ConnectionTrait>(
    db: &C,
    task_id: &str,
    client_request_id: &str,
) -> Result<Option<Confirmation>> {
    ConfEntity::find()
        .filter(confs::Column::TaskId.eq(task_id))
        .filter(confs::Column::ClientRequestId.eq(client_request_id))
        .one(db)
        .await
        .context("db: find confirmation by request id")
}

pub async fn insert_confirmation(
    db: &DatabaseConnection,
    model: confs::ActiveModel,
    task_id: &str,
    client_request_id: &str,
) -> Result<InsertOutcome<Confirmation>> {
    match model.insert(db).await {
        Ok(row) => Ok(InsertOutcome::Inserted(row)),
        Err(err) if is_unique(&err) => {
            match find_confirmation_by_request(db, task_id, client_request_id).await? {
                Some(row) => Ok(InsertOutcome::Replay(row)),
                None => Err(err).context("db: confirmation unique violation without owner"),
            }
        }
        Err(err) => Err(err).context("db: insert confirmation"),
    }
}

/// The newest confirmation of one condition, by creation time then id.
pub async fn latest_confirmation<C: ConnectionTrait>(
    db: &C,
    task_id: &str,
    condition_id: &str,
) -> Result<Option<Confirmation>> {
    ConfEntity::find()
        .filter(confs::Column::TaskId.eq(task_id))
        .filter(confs::Column::ConditionId.eq(condition_id))
        .order_by_desc(confs::Column::CreatedAt)
        .order_by_desc(confs::Column::Id)
        .one(db)
        .await
        .context("db: latest confirmation")
}

pub async fn count_snapshots<C: ConnectionTrait>(db: &C, task_id: &str) -> Result<u64> {
    SnapEntity::find()
        .filter(snaps::Column::TaskId.eq(task_id))
        .count(db)
        .await
        .context("db: count snapshots")
}

pub async fn insert_snapshot(
    db: &DatabaseConnection,
    model: snaps::ActiveModel,
) -> Result<Snapshot> {
    model.insert(db).await.context("db: insert snapshot")
}

/// Newest snapshots of a task first.
pub async fn list_snapshots<C: ConnectionTrait>(
    db: &C,
    task_id: &str,
    limit: u64,
) -> Result<Vec<Snapshot>> {
    SnapEntity::find()
        .filter(snaps::Column::TaskId.eq(task_id))
        .order_by_desc(snaps::Column::CreatedAt)
        .order_by_desc(snaps::Column::Id)
        .limit(limit)
        .all(db)
        .await
        .context("db: list snapshots")
}

pub async fn get_snapshot<C: ConnectionTrait>(
    db: &C,
    repo_id: i64,
    task_id: &str,
    id: &str,
) -> Result<Option<Snapshot>> {
    SnapEntity::find_by_id(id.to_string())
        .filter(snaps::Column::RepoId.eq(repo_id))
        .filter(snaps::Column::TaskId.eq(task_id))
        .one(db)
        .await
        .context("db: get snapshot")
}
