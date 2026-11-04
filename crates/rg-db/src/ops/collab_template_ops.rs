//! Database operations for task templates: `collab_task_templates`,
//! `collab_task_template_versions` and `collab_task_template_links`.
//!
//! Versions are insert-only. The only mutable template column is
//! `latest_version`, moved by a conditional `UPDATE` that arbitrates concurrent
//! "new version" requests. On SQLite a transaction must begin with its write
//! (a read first takes a snapshot that cannot be upgraded when another writer
//! commits), so every replay `SELECT` here runs before `BEGIN`.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use sea_orm::sea_query::Expr;
use sea_orm::*;

use crate::entities::collab_task::{self, ActiveModel as TaskActive, Model as Task};
use crate::entities::collab_task_template::{
    self, ActiveModel as TemplateActive, Model as Template,
};
use crate::entities::collab_task_template_link::{self, ActiveModel as LinkActive, Model as Link};
use crate::entities::collab_task_template_version::{
    self, ActiveModel as VersionActive, Model as Version,
};

fn is_unique_violation(err: &DbErr) -> bool {
    matches!(err.sql_err(), Some(SqlErr::UniqueConstraintViolation(_)))
}

pub async fn find_template<C: ConnectionTrait>(
    db: &C,
    repo_id: i64,
    id: &str,
) -> Result<Option<Template>> {
    collab_task_template::Entity::find_by_id(id.to_string())
        .filter(collab_task_template::Column::RepoId.eq(repo_id))
        .one(db)
        .await
        .context("db: find task template")
}

pub async fn find_template_by_client_id<C: ConnectionTrait>(
    db: &C,
    user_id: i64,
    client_template_id: &str,
) -> Result<Option<Template>> {
    collab_task_template::Entity::find()
        .filter(collab_task_template::Column::CreatedByUserId.eq(user_id))
        .filter(collab_task_template::Column::ClientTemplateId.eq(client_template_id))
        .one(db)
        .await
        .context("db: find task template by client id")
}

pub async fn find_version<C: ConnectionTrait>(
    db: &C,
    template_id: &str,
    version: i64,
) -> Result<Option<Version>> {
    collab_task_template_version::Entity::find()
        .filter(collab_task_template_version::Column::TemplateId.eq(template_id))
        .filter(collab_task_template_version::Column::Version.eq(version))
        .one(db)
        .await
        .context("db: find task template version")
}

pub async fn find_version_by_client_id<C: ConnectionTrait>(
    db: &C,
    user_id: i64,
    client_version_id: &str,
) -> Result<Option<Version>> {
    collab_task_template_version::Entity::find()
        .filter(collab_task_template_version::Column::CreatedByUserId.eq(user_id))
        .filter(collab_task_template_version::Column::ClientVersionId.eq(client_version_id))
        .one(db)
        .await
        .context("db: find task template version by client id")
}

/// Versions `ids`' templates have at the given numbers, for list rendering.
pub async fn versions_of<C: ConnectionTrait>(
    db: &C,
    template_ids: &[String],
) -> Result<Vec<Version>> {
    if template_ids.is_empty() {
        return Ok(Vec::new());
    }
    collab_task_template_version::Entity::find()
        .filter(
            collab_task_template_version::Column::TemplateId.is_in(template_ids.iter().cloned()),
        )
        .all(db)
        .await
        .context("db: list template versions")
}

#[derive(Debug, Clone, PartialEq)]
#[allow(clippy::large_enum_variant)]
pub enum CreateTemplateOutcome {
    Created {
        template: Template,
        version: Version,
    },
    /// `(created_by_user_id, client_template_id)` already exists.
    Existing {
        template: Template,
        version: Version,
    },
    NameTaken,
}

async fn existing_template(
    db: &DatabaseConnection,
    user_id: i64,
    client_id: &str,
) -> Result<Option<CreateTemplateOutcome>> {
    let Some(template) = find_template_by_client_id(db, user_id, client_id).await? else {
        return Ok(None);
    };
    let version = find_version(db, &template.id, 1)
        .await?
        .context("db: template without version 1")?;
    Ok(Some(CreateTemplateOutcome::Existing { template, version }))
}

/// Insert a template together with its version 1. The template insert is the
/// transaction's first statement.
pub async fn create_template(
    db: &DatabaseConnection,
    template: TemplateActive,
    version: VersionActive,
) -> Result<CreateTemplateOutcome> {
    let user_id = template.created_by_user_id.clone().unwrap();
    let client_id = template.client_template_id.clone().unwrap();
    let repo_id = template.repo_id.clone().unwrap();
    let name = template.name.clone().unwrap();

    if let Some(found) = existing_template(db, user_id, &client_id).await? {
        return Ok(found);
    }

    let txn = db.begin().await.context("db: begin create template")?;
    match template.insert(&txn).await {
        Ok(t) => {
            let v = version
                .insert(&txn)
                .await
                .context("db: insert template version")?;
            txn.commit().await.context("db: commit create template")?;
            Ok(CreateTemplateOutcome::Created {
                template: t,
                version: v,
            })
        }
        Err(err) if is_unique_violation(&err) => {
            drop(txn);
            // Either the same client id raced with itself, or the name is taken.
            if let Some(found) = existing_template(db, user_id, &client_id).await? {
                return Ok(found);
            }
            let taken = collab_task_template::Entity::find()
                .filter(collab_task_template::Column::RepoId.eq(repo_id))
                .filter(collab_task_template::Column::Name.eq(name))
                .one(db)
                .await
                .context("db: check template name")?
                .is_some();
            if taken {
                Ok(CreateTemplateOutcome::NameTaken)
            } else {
                Err(err).context("db: insert template")
            }
        }
        Err(err) => Err(err).context("db: insert template"),
    }
}

#[derive(Debug, Clone, PartialEq)]
#[allow(clippy::large_enum_variant)]
pub enum AddVersionOutcome {
    Created {
        template: Template,
        version: Version,
    },
    /// `(created_by_user_id, client_version_id)` already exists.
    Existing(Version),
    /// `latest_version` is no longer the expected one.
    Conflict {
        current: Template,
    },
    NotFound,
}

/// Append version `expected_latest + 1` iff the template is still at
/// `expected_latest`. The conditional `UPDATE` is the arbiter: of two racing
/// writers with the same expectation exactly one matches a row.
pub async fn add_version(
    db: &DatabaseConnection,
    repo_id: i64,
    template_id: &str,
    expected_latest: i64,
    make: impl Fn(i64) -> VersionActive,
    now: DateTime<Utc>,
) -> Result<AddVersionOutcome> {
    let probe = make(expected_latest + 1);
    let user_id = probe.created_by_user_id.clone().unwrap();
    let client_id = probe.client_version_id.clone().unwrap();
    if let Some(existing) = find_version_by_client_id(db, user_id, &client_id).await? {
        return Ok(AddVersionOutcome::Existing(existing));
    }

    let txn = db.begin().await.context("db: begin add template version")?;
    let res = collab_task_template::Entity::update_many()
        .col_expr(
            collab_task_template::Column::LatestVersion,
            Expr::col(collab_task_template::Column::LatestVersion).add(1),
        )
        .col_expr(collab_task_template::Column::UpdatedAt, Expr::value(now))
        .filter(collab_task_template::Column::Id.eq(template_id))
        .filter(collab_task_template::Column::RepoId.eq(repo_id))
        .filter(collab_task_template::Column::LatestVersion.eq(expected_latest))
        .exec(&txn)
        .await
        .context("db: bump template version")?;
    if res.rows_affected == 0 {
        let current = find_template(&txn, repo_id, template_id).await?;
        txn.commit()
            .await
            .context("db: commit add template version")?;
        return Ok(match current {
            Some(current) => AddVersionOutcome::Conflict { current },
            None => AddVersionOutcome::NotFound,
        });
    }
    match make(expected_latest + 1).insert(&txn).await {
        Ok(version) => {
            let template = find_template(&txn, repo_id, template_id)
                .await?
                .context("db: template vanished while adding a version")?;
            txn.commit()
                .await
                .context("db: commit add template version")?;
            Ok(AddVersionOutcome::Created { template, version })
        }
        Err(err) if is_unique_violation(&err) => {
            // The same client id committed concurrently: undo the bump.
            drop(txn);
            let existing = find_version_by_client_id(db, user_id, &client_id)
                .await?
                .context("db: template version client id vanished after conflict")?;
            Ok(AddVersionOutcome::Existing(existing))
        }
        Err(err) => Err(err).context("db: insert template version"),
    }
}

/// Keyset page of a repository's templates ordered by `(created_at, id)`
/// descending.
pub async fn list_templates<C: ConnectionTrait>(
    db: &C,
    repo_id: i64,
    after: Option<(DateTime<Utc>, String)>,
    limit: u64,
) -> Result<Vec<Template>> {
    let mut query = collab_task_template::Entity::find()
        .filter(collab_task_template::Column::RepoId.eq(repo_id));
    if let Some((created, id)) = after {
        query = query.filter(
            Condition::any()
                .add(collab_task_template::Column::CreatedAt.lt(created))
                .add(
                    Condition::all()
                        .add(collab_task_template::Column::CreatedAt.eq(created))
                        .add(collab_task_template::Column::Id.lt(id)),
                ),
        );
    }
    query
        .order_by_desc(collab_task_template::Column::CreatedAt)
        .order_by_desc(collab_task_template::Column::Id)
        .limit(limit)
        .all(db)
        .await
        .context("db: list task templates")
}

pub async fn find_link<C: ConnectionTrait>(
    db: &C,
    repo_id: i64,
    task_id: &str,
) -> Result<Option<Link>> {
    collab_task_template_link::Entity::find_by_id(task_id.to_string())
        .filter(collab_task_template_link::Column::RepoId.eq(repo_id))
        .one(db)
        .await
        .context("db: find template link")
}

pub async fn find_link_by_client_id<C: ConnectionTrait>(
    db: &C,
    creator_kind: &str,
    creator_id: &str,
    client_task_id: &str,
) -> Result<Option<Link>> {
    collab_task_template_link::Entity::find()
        .filter(collab_task_template_link::Column::CreatorKind.eq(creator_kind))
        .filter(collab_task_template_link::Column::CreatorId.eq(creator_id))
        .filter(collab_task_template_link::Column::ClientTaskId.eq(client_task_id))
        .one(db)
        .await
        .context("db: find template link by client id")
}

#[derive(Debug, Clone, PartialEq)]
#[allow(clippy::large_enum_variant)]
pub enum CreateFromTemplateOutcome {
    Created {
        task: Task,
        link: Link,
    },
    /// `(creator, client_task_id)` already exists.
    Existing(Link),
}

/// Insert the task and its template link in one transaction (the task insert is
/// the first statement), so a task is never left without its pin.
pub async fn create_task_with_link(
    db: &DatabaseConnection,
    task: TaskActive,
    link: LinkActive,
) -> Result<CreateFromTemplateOutcome> {
    let kind = link.creator_kind.clone().unwrap();
    let creator = link.creator_id.clone().unwrap();
    let client_id = link.client_task_id.clone().unwrap();
    if let Some(existing) = find_link_by_client_id(db, &kind, &creator, &client_id).await? {
        return Ok(CreateFromTemplateOutcome::Existing(existing));
    }
    let txn = db.begin().await.context("db: begin task from template")?;
    let task = task
        .insert(&txn)
        .await
        .context("db: insert task from template")?;
    match link.insert(&txn).await {
        Ok(link) => {
            txn.commit()
                .await
                .context("db: commit task from template")?;
            Ok(CreateFromTemplateOutcome::Created { task, link })
        }
        Err(err) if is_unique_violation(&err) => {
            // The same client id committed concurrently: drop our task with the
            // rolled-back transaction and answer with the winner's link.
            drop(txn);
            let existing = find_link_by_client_id(db, &kind, &creator, &client_id)
                .await?
                .context("db: template link client id vanished after conflict")?;
            Ok(CreateFromTemplateOutcome::Existing(existing))
        }
        Err(err) => Err(err).context("db: insert template link"),
    }
}

/// Task of this repository, for rebuilding a replayed response.
pub async fn find_task<C: ConnectionTrait>(
    db: &C,
    repo_id: i64,
    task_id: &str,
) -> Result<Option<Task>> {
    collab_task::Entity::find_by_id(task_id.to_string())
        .filter(collab_task::Column::RepoId.eq(repo_id))
        .one(db)
        .await
        .context("db: find task")
}
