//! Wiki service — CRUD operations for repository wiki pages.
//!
//! Each repository can have an associated wiki. Wiki pages are stored in the
//! database for fast querying and optionally mirrored to a `.wiki.git` bare
//! repository on disk for version control.

use anyhow::{Context, Result};
use chrono::Utc;
use sea_orm::DatabaseConnection;

use rg_db::entities::wiki_page;
use rg_db::entities::wiki_revision;
use rg_db::ops::wiki_page_ops;
use rg_db::ops::wiki_revision_ops;

/// Create a new wiki page.
pub async fn create_page(
    db: &DatabaseConnection,
    repo_id: i64,
    title: &str,
    content: &str,
    message: Option<&str>,
    author_id: Option<i64>,
) -> Result<wiki_page::Model> {
    // Check for duplicate title
    if wiki_page_ops::find_by_repo_and_title(db, repo_id, title)
        .await
        .context("check existing wiki page")?
        .is_some()
    {
        anyhow::bail!("wiki page '{}' already exists in this repository", title);
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

    wiki_page_ops::create(db, model).await
}

/// Get a wiki page by repo and title.
pub async fn get_page(
    db: &DatabaseConnection,
    repo_id: i64,
    title: &str,
) -> Result<Option<wiki_page::Model>> {
    wiki_page_ops::find_by_repo_and_title(db, repo_id, title).await
}

/// List all wiki pages for a repo (title + updated_at only for index).
pub async fn list_pages(
    db: &DatabaseConnection,
    repo_id: i64,
) -> Result<Vec<wiki_page::Model>> {
    wiki_page_ops::list_by_repo(db, repo_id).await
}

/// Update a wiki page.
pub async fn update_page(
    db: &DatabaseConnection,
    repo_id: i64,
    title: &str,
    content: &str,
    message: Option<&str>,
    author_id: Option<i64>,
) -> Result<wiki_page::Model> {
    let existing = wiki_page_ops::find_by_repo_and_title(db, repo_id, title)
        .await
        .context("find wiki page for update")?
        .ok_or_else(|| anyhow::anyhow!("wiki page '{}' not found", title))?;

    // Save the current content as a revision before overwriting.
    let next_version = wiki_revision_ops::latest_version(db, existing.id).await
        .unwrap_or(0) + 1;
    let rev = wiki_revision::ActiveModel {
        id: sea_orm::NotSet,
        wiki_page_id: sea_orm::Set(existing.id),
        content: sea_orm::Set(existing.content.clone()),
        message: sea_orm::Set(existing.message.clone()),
        author_id: sea_orm::Set(existing.author_id),
        version: sea_orm::Set(next_version),
        created_at: sea_orm::Set(Utc::now()),
    };
    let _ = wiki_revision_ops::create(db, rev).await; // non-fatal: revision save failure doesn't block page update

    let model = wiki_page::ActiveModel {
        id: sea_orm::Set(existing.id),
        repo_id: sea_orm::Set(existing.repo_id),
        title: sea_orm::Set(existing.title),
        content: sea_orm::Set(content.to_string()),
        message: sea_orm::Set(message.map(|s| s.to_string())),
        author_id: sea_orm::Set(author_id.or(existing.author_id)),
        sha: sea_orm::Set(None),
        created_at: sea_orm::Set(existing.created_at),
        updated_at: sea_orm::Set(Utc::now()),
    };

    wiki_page_ops::update(db, model).await
}

/// List all revisions for a wiki page (newest first).
pub async fn list_revisions(
    db: &DatabaseConnection,
    repo_id: i64,
    title: &str,
) -> Result<Vec<wiki_revision::Model>> {
    let page = wiki_page_ops::find_by_repo_and_title(db, repo_id, title)
        .await
        .context("find wiki page for revisions")?
        .ok_or_else(|| anyhow::anyhow!("wiki page '{}' not found", title))?;
    wiki_revision_ops::list_by_page(db, page.id).await
}

/// Get a specific revision's content.
pub async fn get_revision(
    db: &DatabaseConnection,
    revision_id: i64,
) -> Result<Option<wiki_revision::Model>> {
    wiki_revision_ops::find_by_id(db, revision_id).await
}

/// Delete a wiki page.
pub async fn delete_page(
    db: &DatabaseConnection,
    repo_id: i64,
    title: &str,
) -> Result<()> {
    let existing = wiki_page_ops::find_by_repo_and_title(db, repo_id, title)
        .await
        .context("find wiki page for delete")?
        .ok_or_else(|| anyhow::anyhow!("wiki page '{}' not found", title))?;

    wiki_page_ops::delete_by_id(db, existing.id).await
}
