//! Database operations for `collab_work_packages`.

use anyhow::{Context, Result};
use sea_orm::*;

use crate::entities::collab_work_package::{
    self, ActiveModel, Entity as PackageEntity, Model as Package,
};

#[derive(Debug, Clone, PartialEq)]
pub enum CreateOutcome {
    Created(Package),
    /// `(author_kind, author_id, client_package_id)` already exists.
    Existing(Package),
}

pub async fn find_by_client_id<C: ConnectionTrait>(
    db: &C,
    author_kind: &str,
    author_id: &str,
    client_package_id: &str,
) -> Result<Option<Package>> {
    PackageEntity::find()
        .filter(collab_work_package::Column::AuthorKind.eq(author_kind))
        .filter(collab_work_package::Column::AuthorId.eq(author_id))
        .filter(collab_work_package::Column::ClientPackageId.eq(client_package_id))
        .one(db)
        .await
        .context("db: find work package by client id")
}

pub async fn find_in_repo<C: ConnectionTrait>(
    db: &C,
    repo_id: i64,
    id: &str,
) -> Result<Option<Package>> {
    PackageEntity::find_by_id(id.to_string())
        .filter(collab_work_package::Column::RepoId.eq(repo_id))
        .one(db)
        .await
        .context("db: find work package")
}

/// Highest revision of the task (0 when it has none).
pub async fn max_revision<C: ConnectionTrait>(db: &C, task_id: &str) -> Result<i64> {
    Ok(PackageEntity::find()
        .filter(collab_work_package::Column::TaskId.eq(task_id))
        .order_by_desc(collab_work_package::Column::Revision)
        .one(db)
        .await
        .context("db: max work package revision")?
        .map_or(0, |p| p.revision))
}

/// Insert `make(revision)` as the next revision of the task.
///
/// The revision is `max + 1`; two concurrent writers may compute the same one,
/// and the `(task_id, revision)` unique key rejects the loser, who simply
/// retries with a fresh maximum. The same client key racing with itself ends in
/// [`CreateOutcome::Existing`].
pub async fn create_next_revision(
    db: &DatabaseConnection,
    task_id: &str,
    author_kind: &str,
    author_id: &str,
    client_package_id: &str,
    make: impl Fn(i64) -> ActiveModel,
) -> Result<CreateOutcome> {
    for _ in 0..8 {
        if let Some(existing) =
            find_by_client_id(db, author_kind, author_id, client_package_id).await?
        {
            return Ok(CreateOutcome::Existing(existing));
        }
        let revision = max_revision(db, task_id).await? + 1;
        match make(revision).insert(db).await {
            Ok(created) => return Ok(CreateOutcome::Created(created)),
            Err(err) if matches!(err.sql_err(), Some(SqlErr::UniqueConstraintViolation(_))) => {
                continue;
            }
            Err(err) => return Err(err).context("db: create work package"),
        }
    }
    anyhow::bail!("db: could not allocate a work package revision under contention")
}

/// Keyset page of a task's packages ordered by `revision` descending.
///
/// `revision` is unique and increasing within a task, so it is a total order even
/// when timestamps tie (MySQL stores whole seconds).
pub async fn list_page<C: ConnectionTrait>(
    db: &C,
    repo_id: i64,
    task_id: &str,
    after_revision: Option<i64>,
    limit: u64,
) -> Result<Vec<Package>> {
    let mut query = PackageEntity::find()
        .filter(collab_work_package::Column::RepoId.eq(repo_id))
        .filter(collab_work_package::Column::TaskId.eq(task_id));
    if let Some(revision) = after_revision {
        query = query.filter(collab_work_package::Column::Revision.lt(revision));
    }
    query
        .order_by_desc(collab_work_package::Column::Revision)
        .limit(limit)
        .all(db)
        .await
        .context("db: list work packages")
}
