//! Release service — business logic for releases.

use anyhow::Result;
use chrono::Utc;
use sea_orm::{ActiveValue::Set, DatabaseConnection};

use rg_db::{
    entities::release::{ActiveModel as ReleaseActiveModel, Model as Release},
    entities::release_asset::{ActiveModel as AssetActiveModel, Model as Asset},
};

/// Create a new release.
pub async fn create_release(
    db: &DatabaseConnection,
    repo_id: i64,
    author_id: i64,
    tag_name: &str,
    title: &str,
    body: Option<&str>,
    target_commitish: &str,
    is_draft: bool,
    is_prerelease: bool,
    _repo_path: &std::path::Path,
) -> Result<Release> {
    // Validate inputs
    if tag_name.is_empty() {
        anyhow::bail!("tag_name cannot be empty");
    }
    if title.is_empty() {
        anyhow::bail!("title cannot be empty");
    }

    // Check for duplicate tag
    if rg_db::ops::release_ops::find_by_repo_and_tag(db, repo_id, tag_name)
        .await?
        .is_some()
    {
        anyhow::bail!("release with tag '{}' already exists", tag_name);
    }

    let now = Utc::now();
    let model = ReleaseActiveModel {
        repo_id: Set(repo_id),
        author_id: Set(author_id),
        tag_name: Set(tag_name.to_string()),
        title: Set(title.to_string()),
        body: Set(body.map(str::to_string)),
        target_commitish: Set(target_commitish.to_string()),
        is_draft: Set(is_draft),
        is_prerelease: Set(is_prerelease),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    };

    let release = rg_db::ops::release_ops::create(db, model).await?;

    // Trigger release.created webhook
    let payload = serde_json::json!({
        "id": release.id,
        "repo_id": release.repo_id,
        "tag_name": release.tag_name,
        "title": release.title,
        "body": release.body,
        "is_draft": release.is_draft,
        "is_prerelease": release.is_prerelease,
        "author_id": release.author_id,
    });
    let _ = crate::webhook::service::trigger_release_created(db, repo_id, &payload).await;

    Ok(release)
}

/// List releases for a repository.
pub async fn list_releases(
    db: &DatabaseConnection,
    repo_id: i64,
    offset: u64,
    limit: u64,
) -> Result<(Vec<Release>, i64)> {
    rg_db::ops::release_ops::list_by_repo(db, repo_id, offset, limit).await
}

/// Get a release by ID.
pub async fn get_release(db: &DatabaseConnection, id: i64) -> Result<Release> {
    rg_db::ops::release_ops::find_by_id(db, id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("release not found"))
}

/// Update a release.
pub async fn update_release(
    db: &DatabaseConnection,
    id: i64,
    title: Option<&str>,
    body: Option<&str>,
    is_draft: Option<bool>,
    is_prerelease: Option<bool>,
) -> Result<Release> {
    let existing = rg_db::ops::release_ops::find_by_id(db, id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("release not found"))?;

    let mut model: ReleaseActiveModel = existing.into();
    if let Some(t) = title {
        model.title = Set(t.to_string());
    }
    if let Some(b) = body {
        model.body = Set(Some(b.to_string()));
    }
    if let Some(d) = is_draft {
        model.is_draft = Set(d);
    }
    if let Some(p) = is_prerelease {
        model.is_prerelease = Set(p);
    }
    model.updated_at = Set(Utc::now());

    rg_db::ops::release_ops::update(db, model).await
}

/// Delete a release.
pub async fn delete_release(db: &DatabaseConnection, id: i64) -> Result<()> {
    // Get release info for webhook before deleting
    let release = rg_db::ops::release_ops::find_by_id(db, id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("release not found"))?;
    let repo_id = release.repo_id;

    rg_db::ops::release_ops::delete_by_id(db, id).await?;

    // Trigger release.deleted webhook
    let payload = serde_json::json!({
        "id": release.id,
        "repo_id": release.repo_id,
        "tag_name": release.tag_name,
        "title": release.title,
    });
    let _ = crate::webhook::service::trigger_release_deleted(db, repo_id, &payload).await;

    Ok(())
}

/// Upload a release asset.
pub async fn upload_asset(
    db: &DatabaseConnection,
    release_id: i64,
    filename: &str,
    size: i64,
    content_type: &str,
    uploader_id: i64,
) -> Result<Asset> {
    // Verify release exists
    let _ = get_release(db, release_id).await?;

    let model = AssetActiveModel {
        release_id: Set(release_id),
        filename: Set(filename.to_string()),
        size: Set(size),
        content_type: Set(content_type.to_string()),
        download_count: Set(0),
        uploader_id: Set(uploader_id),
        created_at: Set(Utc::now()),
        ..Default::default()
    };

    rg_db::ops::release_ops::create_asset(db, model).await
}

/// Download a release asset (increments download count).
pub async fn download_asset(db: &DatabaseConnection, asset_id: i64) -> Result<Asset> {
    let asset = rg_db::ops::release_ops::find_asset_by_id(db, asset_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("asset not found"))?;

    // Increment download count
    rg_db::ops::release_ops::increment_download_count(db, asset_id).await?;

    Ok(asset)
}

/// List assets for a release.
pub async fn list_assets(
    db: &DatabaseConnection,
    release_id: i64,
) -> Result<Vec<Asset>> {
    rg_db::ops::release_ops::list_assets(db, release_id).await
}
