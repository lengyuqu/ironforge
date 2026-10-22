//! Release service — business logic for releases.

use chrono::Utc;
use sea_orm::{ActiveValue::Set, DatabaseConnection};
use std::path::{Path, PathBuf};

use crate::error::{CoreContext, CoreError, CoreResult};

use rg_db::{
    entities::release::{ActiveModel as ReleaseActiveModel, Model as Release},
    entities::release_asset::{ActiveModel as AssetActiveModel, Model as Asset},
};

/// Create a new release.
#[allow(clippy::too_many_arguments)]
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
) -> CoreResult<Release> {
    // Validate inputs
    if tag_name.is_empty() {
        return Err(CoreError::invalid_input("tag_name cannot be empty"));
    }
    if title.is_empty() {
        return Err(CoreError::invalid_input("title cannot be empty"));
    }

    // Check for duplicate tag
    if rg_db::ops::release_ops::find_by_repo_and_tag(db, repo_id, tag_name)
        .await?
        .is_some()
    {
        return Err(CoreError::conflict(format!(
            "release with tag '{}' already exists",
            tag_name
        )));
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
    if let Err(e) = crate::webhook::service::trigger_release_created(db, repo_id, &payload).await {
        tracing::warn!("Failed to trigger release.created webhook: {e}");
    }

    Ok(release)
}

/// List releases for a repository.
pub async fn list_releases(
    db: &DatabaseConnection,
    repo_id: i64,
    offset: u64,
    limit: u64,
) -> CoreResult<(Vec<Release>, i64)> {
    Ok(rg_db::ops::release_ops::list_by_repo(db, repo_id, offset, limit).await?)
}

/// Get a release by ID.
pub async fn get_release(db: &DatabaseConnection, id: i64) -> CoreResult<Release> {
    rg_db::ops::release_ops::find_by_id(db, id)
        .await?
        .ok_or_else(|| CoreError::not_found("release not found"))
}

/// Update a release.
#[allow(clippy::too_many_arguments)]
pub async fn update_release(
    db: &DatabaseConnection,
    id: i64,
    title: Option<&str>,
    body: Option<&str>,
    is_draft: Option<bool>,
    is_prerelease: Option<bool>,
) -> CoreResult<Release> {
    let existing = rg_db::ops::release_ops::find_by_id(db, id)
        .await?
        .ok_or_else(|| CoreError::not_found("release not found"))?;

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

    Ok(rg_db::ops::release_ops::update(db, model).await?)
}

/// Delete a release.
pub async fn delete_release(db: &DatabaseConnection, id: i64) -> CoreResult<()> {
    // Get release info for webhook before deleting
    let release = rg_db::ops::release_ops::find_by_id(db, id)
        .await?
        .ok_or_else(|| CoreError::not_found("release not found"))?;
    let repo_id = release.repo_id;

    rg_db::ops::release_ops::delete_by_id(db, id).await?;

    // Trigger release.deleted webhook
    let payload = serde_json::json!({
        "id": release.id,
        "repo_id": release.repo_id,
        "tag_name": release.tag_name,
        "title": release.title,
    });
    if let Err(e) = crate::webhook::service::trigger_release_deleted(db, repo_id, &payload).await {
        tracing::warn!("Failed to trigger release.deleted webhook: {e}");
    }

    Ok(())
}

/// Upload a release asset (saves file to disk + creates DB record).
#[allow(clippy::too_many_arguments)]
pub async fn upload_asset(
    db: &DatabaseConnection,
    release_id: i64,
    storage: &dyn crate::blob_storage::BlobStorage,
    repo_root: &Path,
    owner: &str,
    repo_name: &str,
    filename: &str,
    size: i64,
    content_type: &str,
    uploader_id: i64,
    data: &[u8],
) -> CoreResult<Asset> {
    // Verify release exists and get repo info
    let _release = get_release(db, release_id).await?;

    // Create DB record first to get asset ID
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
    let asset = rg_db::ops::release_ops::create_asset(db, model).await?;

    let key = asset_blob_key(owner, repo_name, &asset)?;
    if let Err(error) = storage.put(&key, data).await {
        let _ = rg_db::ops::release_ops::delete_asset_by_id(db, asset.id).await;
        return Err(blob_error(error, "failed to write release asset"));
    }

    // Keep the parameter during the compatibility window: old assets are read
    // from this root, while all new writes use backend-neutral keys.
    let _ = repo_root;

    Ok(asset)
}

/// Download a release asset (increments download count, returns file bytes).
pub async fn download_asset(
    db: &DatabaseConnection,
    asset_id: i64,
    storage: &dyn crate::blob_storage::BlobStorage,
    repo_root: &Path,
    owner: &str,
    repo_name: &str,
) -> CoreResult<(Asset, Vec<u8>)> {
    let asset = rg_db::ops::release_ops::find_asset_by_id(db, asset_id)
        .await?
        .ok_or_else(|| CoreError::not_found("asset not found"))?;

    // Increment download count
    rg_db::ops::release_ops::increment_download_count(db, asset_id).await?;

    let key = asset_blob_key(owner, repo_name, &asset)?;
    let data = match storage.get(&key).await {
        Ok(data) => data,
        Err(crate::blob_storage::BlobStorageError::NotFound(_)) => {
            let file_path = asset_file_path(repo_root, owner, repo_name, &asset);
            match tokio::fs::read(&file_path).await {
                Ok(data) => data,
                // Neither the current backend nor the legacy location has the
                // bytes: the asset's content is unavailable. Do not echo the
                // storage path to the client.
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    tracing::warn!(
                        asset_id,
                        path = %file_path.display(),
                        "release asset content missing from storage"
                    );
                    return Err(CoreError::not_found("asset content not found"));
                }
                Err(error) => {
                    return Err(error).context("failed to read legacy release asset")
                }
            }
        }
        Err(error) => return Err(blob_error(error, "failed to read release asset")),
    };

    Ok((asset, data))
}

/// Get a release asset by ID (without incrementing download count).
pub async fn get_asset(db: &DatabaseConnection, asset_id: i64) -> CoreResult<Asset> {
    rg_db::ops::release_ops::find_asset_by_id(db, asset_id)
        .await?
        .ok_or_else(|| CoreError::not_found("asset not found"))
}

/// List assets for a release.
pub async fn list_assets(db: &DatabaseConnection, release_id: i64) -> CoreResult<Vec<Asset>> {
    Ok(rg_db::ops::release_ops::list_assets(db, release_id).await?)
}

/// Delete a release asset (removes DB record + file from disk).
pub async fn delete_asset(
    db: &DatabaseConnection,
    asset_id: i64,
    storage: &dyn crate::blob_storage::BlobStorage,
    repo_root: &Path,
    owner: &str,
    repo_name: &str,
) -> CoreResult<()> {
    let asset = get_asset(db, asset_id).await?;

    let key = asset_blob_key(owner, repo_name, &asset)?;
    if let Err(error) = storage.delete(&key).await {
        tracing::warn!(%key, %error, "failed to remove release asset blob");
    }

    // Remove file from disk (ignore errors if file doesn't exist)
    let file_path = asset_file_path(repo_root, owner, repo_name, &asset);
    if let Err(e) = tokio::fs::remove_file(&file_path).await {
        tracing::warn!("Failed to remove asset file {}: {e}", file_path.display());
    }

    // Remove parent directory if empty
    if let Some(parent) = file_path.parent() {
        if let Err(e) = tokio::fs::remove_dir(parent).await {
            tracing::warn!("Failed to remove asset directory {}: {e}", parent.display());
        }
    }

    // Delete DB record
    rg_db::ops::release_ops::delete_asset_by_id(db, asset_id).await?;

    Ok(())
}

/// Get the storage directory for release assets.
fn asset_storage_dir(repo_root: &Path, owner: &str, repo_name: &str) -> PathBuf {
    repo_root.join(format!("{}/{}.releases/assets", owner, repo_name))
}

/// Get the file path for a specific asset.
fn asset_file_path(repo_root: &Path, owner: &str, repo_name: &str, asset: &Asset) -> PathBuf {
    asset_storage_dir(repo_root, owner, repo_name)
        .join(asset.id.to_string())
        .join(&asset.filename)
}

/// Classify a blob-storage failure: an invalid key is a caller mistake (for
/// example an empty filename), everything else is an internal storage fault.
fn blob_error(error: crate::blob_storage::BlobStorageError, context: &str) -> CoreError {
    match error {
        crate::blob_storage::BlobStorageError::InvalidKey(_) => {
            CoreError::invalid_input("invalid release asset name")
        }
        other => CoreError::Internal(anyhow::Error::new(other).context(context.to_string())),
    }
}

fn asset_blob_key(
    owner: &str,
    repo_name: &str,
    asset: &Asset,
) -> CoreResult<crate::blob_storage::BlobKey> {
    let release_id = asset.release_id.to_string();
    let asset_id = asset.id.to_string();
    crate::blob_storage::BlobKey::from_segments([
        "releases",
        owner,
        repo_name,
        &release_id,
        &asset_id,
        &asset.filename,
    ])
    .map_err(|error| blob_error(error, "failed to build release asset key"))
}
