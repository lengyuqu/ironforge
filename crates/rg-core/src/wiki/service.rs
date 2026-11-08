//! Wiki service — CRUD operations for repository wiki pages.
//!
//! Each repository can have an associated wiki. Wiki pages are stored in the
//! database for fast querying and optionally mirrored to a `.wiki.git` bare
//! repository on disk for version control.

use anyhow::Context;
use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, DatabaseConnection, EntityTrait, QueryFilter,
    QueryOrder, TransactionTrait,
};

use rg_db::entities::wiki_page;
use rg_db::entities::wiki_revision;
use rg_db::ops::wiki_page_ops;
use rg_db::ops::wiki_revision_ops;

use crate::error::{CoreError, CoreResult};
use crate::search::dialect::metadata_fts_upsert_sql;

/// Create a new wiki page.
pub async fn create_page(
    db: &DatabaseConnection,
    repo_id: i64,
    title: &str,
    content: &str,
    message: Option<&str>,
    author_id: Option<i64>,
) -> CoreResult<wiki_page::Model> {
    // Check for duplicate title
    if wiki_page_ops::find_by_repo_and_title(db, repo_id, title)
        .await
        .context("check existing wiki page")?
        .is_some()
    {
        return Err(CoreError::conflict(format!(
            "wiki page '{}' already exists in this repository",
            title
        )));
    }

    let now = Utc::now();
    let model = wiki_page::ActiveModel {
        id: sea_orm::NotSet,
        repo_id: sea_orm::Set(repo_id),
        title: sea_orm::Set(title.to_string()),
        content: sea_orm::Set(content.to_string()),
        message: sea_orm::Set(message.map(|s| s.to_string())),
        author_id: sea_orm::Set(author_id),
        sha: sea_orm::Set(None),
        created_at: sea_orm::Set(now),
        updated_at: sea_orm::Set(now),
    };

    let page = wiki_page_ops::create(db, model).await?;

    // Keep the metadata FTS table in sync (non-fatal). Database triggers also
    // maintain it; the upsert makes the explicit write safe on every backend.
    let page_id = page.id;
    let page_title = page.title.clone();
    let page_content = page.content.clone();
    let backend = db.get_database_backend();
    let sql = metadata_fts_upsert_sql(backend, "wiki_pages_fts", "title, content");
    if let Err(e) = db
        .execute(sea_orm::Statement::from_sql_and_values(
            backend,
            rg_db::prepare_sql(backend, &sql),
            [page_id.into(), page_title.into(), page_content.into()],
        ))
        .await
    {
        tracing::warn!(error = %e, page_id = %page_id, "failed to update wiki_pages_fts index");
    }

    Ok(page)
}

/// Get a wiki page by repo and title.
pub async fn get_page(
    db: &DatabaseConnection,
    repo_id: i64,
    title: &str,
) -> CoreResult<Option<wiki_page::Model>> {
    Ok(wiki_page_ops::find_by_repo_and_title(db, repo_id, title).await?)
}

/// List all wiki pages for a repo (title + updated_at only for index).
pub async fn list_pages(
    db: &DatabaseConnection,
    repo_id: i64,
) -> CoreResult<Vec<wiki_page::Model>> {
    Ok(wiki_page_ops::list_by_repo(db, repo_id).await?)
}

/// Read a page together with an opaque generation derived from its latest
/// committed revision. Re-read both to avoid pairing a stale body with a newer
/// generation when an edit commits between queries.
pub async fn get_page_with_version(
    db: &DatabaseConnection,
    repo_id: i64,
    title: &str,
) -> CoreResult<Option<(wiki_page::Model, String)>> {
    for _ in 0..8 {
        let Some(first) = wiki_page_ops::find_by_repo_and_title(db, repo_id, title).await? else {
            return Ok(None);
        };
        let rev1 = wiki_revision::Entity::find()
            .filter(wiki_revision::Column::WikiPageId.eq(first.id))
            .order_by_desc(wiki_revision::Column::Id)
            .one(db)
            .await?;
        let second = wiki_page_ops::find_by_repo_and_title(db, repo_id, title).await?;
        let rev2 = wiki_revision::Entity::find()
            .filter(wiki_revision::Column::WikiPageId.eq(first.id))
            .order_by_desc(wiki_revision::Column::Id)
            .one(db)
            .await?;
        let id1 = rev1.as_ref().map_or(0, |r| r.id);
        let id2 = rev2.as_ref().map_or(0, |r| r.id);
        if second.as_ref() == Some(&first) && id1 == id2 {
            return Ok(Some((first.clone(), format!("{}:{}", first.id, id1))));
        }
    }
    Err(CoreError::conflict(
        "wiki page changed while it was being read; retry",
    ))
}

/// Update a page and append its old body to history under a database row lock.
/// Existing revision ids are used as a generation, avoiding a schema change.
pub async fn update_page(
    db: &DatabaseConnection,
    repo_id: i64,
    title: &str,
    content: &str,
    message: Option<&str>,
    author_id: Option<i64>,
    expected_version: &str,
) -> CoreResult<(wiki_page::Model, String)> {
    // The first statement locks the row. Retrying a SQLite BUSY here is safe:
    // no revision or body has been written, and the transaction is rolled back.
    let txn = {
        let mut attempts = 0;
        loop {
            let txn = db.begin().await?;
            let locked = wiki_page::Entity::update_many()
                .col_expr(
                    wiki_page::Column::Id,
                    sea_orm::sea_query::Expr::col(wiki_page::Column::Id).into(),
                )
                .filter(wiki_page::Column::RepoId.eq(repo_id))
                .filter(wiki_page::Column::Title.eq(title))
                .exec(&txn)
                .await;
            match locked {
                Ok(_) => break txn,
                Err(error) if rg_db::is_sqlite_busy(&error) && attempts < 50 => {
                    txn.rollback().await?;
                    attempts += 1;
                    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                }
                Err(error) => return Err(error.into()),
            }
        }
    };
    let existing = wiki_page::Entity::find()
        .filter(wiki_page::Column::RepoId.eq(repo_id))
        .filter(wiki_page::Column::Title.eq(title))
        .one(&txn)
        .await?
        .ok_or_else(|| CoreError::not_found(format!("wiki page '{}' not found", title)))?;
    let latest = wiki_revision::Entity::find()
        .filter(wiki_revision::Column::WikiPageId.eq(existing.id))
        .order_by_desc(wiki_revision::Column::Id)
        .one(&txn)
        .await?;
    let generation = format!("{}:{}", existing.id, latest.as_ref().map_or(0, |r| r.id));
    if expected_version != generation {
        return Err(CoreError::conflict(
            "wiki version changed; reload before saving",
        ));
    }
    let version = latest
        .as_ref()
        .map_or(0, |r| r.version)
        .checked_add(1)
        .ok_or_else(|| CoreError::internal("wiki revision version overflow"))?;
    let revision = wiki_revision::ActiveModel {
        id: sea_orm::NotSet,
        wiki_page_id: sea_orm::Set(existing.id),
        content: sea_orm::Set(existing.content.clone()),
        message: sea_orm::Set(existing.message.clone()),
        author_id: sea_orm::Set(existing.author_id),
        version: sea_orm::Set(version),
        created_at: sea_orm::Set(Utc::now()),
    }
    .insert(&txn)
    .await?;
    let mut model: wiki_page::ActiveModel = existing.into();
    model.content = sea_orm::Set(content.to_string());
    model.message = sea_orm::Set(message.map(str::to_owned));
    if let Some(author_id) = author_id {
        model.author_id = sea_orm::Set(Some(author_id));
    }
    model.sha = sea_orm::Set(None);
    model.updated_at = sea_orm::Set(Utc::now());
    let updated = model.update(&txn).await?;
    txn.commit().await?;
    // Database triggers update the search index in the same transaction.
    let generation = format!("{}:{}", updated.id, revision.id);
    Ok((updated, generation))
}

/// List all revisions for a wiki page (newest first).
pub async fn list_revisions(
    db: &DatabaseConnection,
    repo_id: i64,
    title: &str,
) -> CoreResult<Vec<wiki_revision::Model>> {
    let page = wiki_page_ops::find_by_repo_and_title(db, repo_id, title)
        .await
        .context("find wiki page for revisions")?
        .ok_or_else(|| CoreError::not_found(format!("wiki page '{}' not found", title)))?;
    Ok(wiki_revision_ops::list_by_page(db, page.id).await?)
}

/// Get a revision only when it belongs to the named page in this repository.
/// Missing pages and revisions, including revisions from another page or
/// repository, all return None to avoid exposing their existence.
pub async fn get_revision(
    db: &DatabaseConnection,
    repo_id: i64,
    title: &str,
    revision_id: i64,
) -> CoreResult<Option<wiki_revision::Model>> {
    let Some(page) = wiki_page_ops::find_by_repo_and_title(db, repo_id, title).await? else {
        return Ok(None);
    };
    Ok(wiki_revision_ops::find_by_id(db, revision_id)
        .await?
        .filter(|revision| revision.wiki_page_id == page.id))
}

/// Delete a wiki page.
pub async fn delete_page(db: &DatabaseConnection, repo_id: i64, title: &str) -> CoreResult<()> {
    let existing = wiki_page_ops::find_by_repo_and_title(db, repo_id, title)
        .await
        .context("find wiki page for delete")?
        .ok_or_else(|| CoreError::not_found(format!("wiki page '{}' not found", title)))?;

    let page_id = existing.id;

    // Delete from the cross-backend metadata FTS index (non-fatal).
    let backend = db.get_database_backend();
    if let Err(e) = db
        .execute(sea_orm::Statement::from_sql_and_values(
            backend,
            rg_db::prepare_sql(backend, "DELETE FROM wiki_pages_fts WHERE rowid = ?"),
            [page_id.into()],
        ))
        .await
    {
        tracing::warn!(error = %e, page_id = %page_id, "failed to delete from wiki_pages_fts index");
    }

    wiki_page_ops::delete_by_id(db, page_id).await?;
    Ok(())
}
