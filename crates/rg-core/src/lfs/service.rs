//! Git LFS service — implements the LFS batch API.
//!
//! Git LFS (Large File Storage) replaces large files with pointer files in Git,
//! while storing the actual content separately. This service implements the
//! LFS batch API for upload/download operations.
//!
//! Storage layout: `<repo_root>/<owner>/<repo>.lfs/<oid_prefix>/<oid>`
//!
//! ## Compression
//!
//! LFS objects are compressed using zstd by default. Storage format:
//! - Compressed: `<oid>.zst` (zstd compressed)
//! - Uncompressed (legacy): `<oid>` (raw)
//!
//! The `compression` field in DB tracks the algorithm used.

use anyhow::{Context, Result};
use chrono::Utc;
use sea_orm::{ActiveModelTrait, DatabaseConnection};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::PathBuf;

use rg_db::entities::lfs_object;
use rg_db::ops::lfs_object_ops;

/// Compression level for zstd (1-22, default 3)
const ZSTD_LEVEL: i32 = 3;

/// Compression algorithm name
const COMPRESSION_ALGO: &str = "zstd";

// ── LFS API types ─────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct LfsBatchRequest {
    pub operation: String,  // "upload" or "download"
    pub objects: Vec<LfsObjectRequest>,
    pub transfers: Option<Vec<String>>, // e.g. ["basic"]
    pub refname: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LfsObjectRequest {
    pub oid: String,
    pub size: i64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LfsBatchResponse {
    pub transfer: String,
    pub objects: Vec<LfsObjectResponse>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LfsObjectResponse {
    pub oid: String,
    pub size: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actions: Option<LfsActions>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<LfsError>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LfsActions {
    pub download: Option<LfsAction>,
    pub upload: Option<LfsAction>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LfsAction {
    pub href: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub header: Option<std::collections::HashMap<String, String>>,
    #[serde(rename = "expires_in")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_in: Option<i64>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LfsError {
    pub code: i32,
    pub message: String,
}

// ── LFS service ───────────────────────────────────────────────────────────

/// Get the storage path for an LFS object.
fn lfs_object_path(lfs_root: &std::path::Path, oid: &str) -> PathBuf {
    // Shard by first 2 hex chars: <lfs_root>/ab/<full-oid>
    let prefix = &oid[..2];
    lfs_root.join(prefix).join(oid)
}

/// Get the LFS root directory for a repository.
pub fn lfs_root(repo_root: &std::path::Path, owner: &str, repo: &str) -> PathBuf {
    repo_root.join(format!("{}.lfs", owner)).join(repo)
}

/// Handle a batch upload/download request.
pub async fn batch(
    db: &DatabaseConnection,
    repo_id: i64,
    lfs_root: &std::path::Path,
    base_url: &str,
    owner: &str,
    repo: &str,
    req: &LfsBatchRequest,
) -> Result<LfsBatchResponse> {
    let transfer = req
        .transfers
        .as_ref()
        .and_then(|t| t.first().cloned())
        .unwrap_or_else(|| "basic".to_string());

    let mut objects = Vec::new();

    for obj_req in &req.objects {
        let obj_resp = match req.operation.as_str() {
            "upload" => {
                handle_upload(db, repo_id, lfs_root, base_url, owner, repo, &obj_req.oid, obj_req.size)
                    .await?
            }
            "download" => {
                handle_download(db, repo_id, lfs_root, base_url, owner, repo, &obj_req.oid, obj_req.size)
                    .await?
            }
            _ => LfsObjectResponse {
                oid: obj_req.oid.clone(),
                size: obj_req.size,
                actions: None,
                error: Some(LfsError {
                    code: 422,
                    message: format!("unsupported operation: {}", req.operation),
                }),
            },
        };
        objects.push(obj_resp);
    }

    Ok(LfsBatchResponse { transfer, objects })
}

async fn handle_upload(
    db: &DatabaseConnection,
    repo_id: i64,
    lfs_root: &std::path::Path,
    base_url: &str,
    owner: &str,
    repo: &str,
    oid: &str,
    size: i64,
) -> Result<LfsObjectResponse> {
    // Check if object already exists
    let existing = lfs_object_ops::find_by_repo_and_oid(db, repo_id, oid).await?;

    if let Some(obj) = &existing {
        if obj.uploaded {
            let obj_path = lfs_object_path(lfs_root, oid);
            if obj_path.exists() {
                // Already uploaded — no action needed
                return Ok(LfsObjectResponse {
                    oid: oid.to_string(),
                    size,
                    actions: None,
                    error: None,
                });
            }
        }
    }

    // Register object if not yet tracked
    if existing.is_none() {
        let model = lfs_object::ActiveModel {
            id: sea_orm::NotSet,
            repo_id: sea_orm::Set(repo_id),
            oid: sea_orm::Set(oid.to_string()),
            size: sea_orm::Set(size),
            uploaded: sea_orm::Set(false),
            compression: sea_orm::Set(None),
            compressed_size: sea_orm::Set(None),
            created_at: sea_orm::Set(Utc::now()),
        };
        lfs_object_ops::create(db, model).await?;
    }

    // Return upload URL
    let upload_href = format!(
        "{}/api/v1/repos/{}/{}/lfs/objects/{}",
        base_url, owner, repo, oid
    );

    Ok(LfsObjectResponse {
        oid: oid.to_string(),
        size,
        actions: Some(LfsActions {
            download: None,
            upload: Some(LfsAction {
                href: upload_href,
                header: None,
                expires_in: None,
            }),
        }),
        error: None,
    })
}

async fn handle_download(
    db: &DatabaseConnection,
    repo_id: i64,
    lfs_root: &std::path::Path,
    base_url: &str,
    owner: &str,
    repo: &str,
    oid: &str,
    size: i64,
) -> Result<LfsObjectResponse> {
    let existing = lfs_object_ops::find_by_repo_and_oid(db, repo_id, oid)
        .await?
        .ok_or_else(|| anyhow::anyhow!("LFS object {} not found", oid))?;

    if !existing.uploaded {
        return Ok(LfsObjectResponse {
            oid: oid.to_string(),
            size,
            actions: None,
            error: Some(LfsError {
                code: 404,
                message: "object not uploaded yet".to_string(),
            }),
        });
    }

    let download_href = format!(
        "{}/api/v1/repos/{}/{}/lfs/objects/{}",
        base_url, owner, repo, oid
    );

    Ok(LfsObjectResponse {
        oid: oid.to_string(),
        size,
        actions: Some(LfsActions {
            download: Some(LfsAction {
                href: download_href,
                header: None,
                expires_in: None,
            }),
            upload: None,
        }),
        error: None,
    })
}

/// Store an uploaded LFS object to disk and mark as uploaded in DB.
pub async fn store_object(
    db: &DatabaseConnection,
    repo_id: i64,
    lfs_root: &std::path::Path,
    oid: &str,
    data: &[u8],
) -> Result<()> {
    let obj_path = lfs_object_path(lfs_root, oid);

    // Create parent directory
    if let Some(parent) = obj_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create LFS directory {:?}", parent))?;
    }

    // Find or create the DB record first
    let existing = lfs_object_ops::find_by_repo_and_oid(db, repo_id, oid).await?;
    let obj_id = if let Some(obj) = existing {
        obj.id
    } else {
        let model = lfs_object::ActiveModel {
            id: sea_orm::NotSet,
            repo_id: sea_orm::Set(repo_id),
            oid: sea_orm::Set(oid.to_string()),
            size: sea_orm::Set(data.len() as i64),
            uploaded: sea_orm::Set(false),
            compression: sea_orm::Set(None),
            compressed_size: sea_orm::Set(None),
            created_at: sea_orm::Set(Utc::now()),
        };
        let new_obj = lfs_object_ops::create(db, model).await?;
        new_obj.id
    };

    // Compress data with zstd
    let compressed = compress_data(data)?;
    let compressed_size = compressed.len() as i64;

    // Write compressed file with .zst extension
    let compressed_path = obj_path.with_extension("zst");
    std::fs::write(&compressed_path, &compressed)
        .with_context(|| format!("write compressed LFS object {:?}", compressed_path))?;

    tracing::info!(
        oid = %oid,
        original_size = data.len(),
        compressed_size = compressed_size,
        ratio = format!("{:.1}%", (compressed_size as f64 / data.len() as f64) * 100.0),
        "LFS object compressed and stored"
    );

    // Update DB with compression info and mark as uploaded
    let obj = lfs_object_ops::find_by_repo_and_oid(db, repo_id, oid)
        .await?
        .ok_or_else(|| anyhow::anyhow!("LFS object {} not found after create", oid))?;

    let mut model: lfs_object::ActiveModel = obj.into();
    model.uploaded = sea_orm::Set(true);
    model.compression = sea_orm::Set(Some(COMPRESSION_ALGO.to_string()));
    model.compressed_size = sea_orm::Set(Some(compressed_size));
    model.update(db).await.context("db: update LFS object after store")?;

    Ok(())
}

/// Read an LFS object from disk.
pub async fn read_object(
    lfs_root: &std::path::Path,
    oid: &str,
) -> Result<Vec<u8>> {
    let obj_path = lfs_object_path(lfs_root, oid);

    // Try compressed version first (.zst)
    let compressed_path = obj_path.with_extension("zst");
    if compressed_path.exists() {
        let compressed = std::fs::read(&compressed_path)
            .with_context(|| format!("read compressed LFS object {:?}", compressed_path))?;
        return decompress_data(&compressed);
    }

    // Fallback to uncompressed (legacy)
    if obj_path.exists() {
        return std::fs::read(&obj_path)
            .with_context(|| format!("read LFS object {:?}", obj_path));
    }

    anyhow::bail!("LFS object {} not found", oid)
}

// ── Compression helpers ───────────────────────────────────────────────────────

/// Compress data using zstd.
fn compress_data(data: &[u8]) -> Result<Vec<u8>> {
    let mut compressed = Vec::with_capacity(data.len());
    let mut encoder = zstd::Encoder::new(&mut compressed, ZSTD_LEVEL)
        .context("failed to create zstd encoder")?;
    encoder.write_all(data)
        .context("failed to write data to zstd encoder")?;
    encoder.finish()
        .context("failed to finish zstd encoding")?;
    Ok(compressed)
}

/// Decompress zstd data.
fn decompress_data(compressed: &[u8]) -> Result<Vec<u8>> {
    let mut decompressed = Vec::new();
    let mut decoder = zstd::Decoder::new(compressed)
        .context("failed to create zstd decoder")?;
    std::io::copy(&mut decoder, &mut decompressed)
        .context("failed to decompress zstd data")?;
    Ok(decompressed)
}

// ── Lazy compression utility ──────────────────────────────────────────────────

/// Compress existing uncompressed LFS objects in a repository.
/// Returns the number of objects compressed.
pub async fn compress_existing(
    db: &DatabaseConnection,
    repo_id: i64,
    lfs_root: &std::path::Path,
    batch_size: u64,
) -> Result<usize> {
    let uncompressed = lfs_object_ops::list_uncompressed(db, repo_id, batch_size).await?;
    let mut count = 0;

    for obj in uncompressed {
        let obj_path = lfs_object_path(lfs_root, &obj.oid);

        // Skip if already compressed
        if obj_path.with_extension("zst").exists() {
            continue;
        }

        // Skip if original file doesn't exist
        if !obj_path.exists() {
            tracing::warn!(oid = %obj.oid, "LFS object file not found, skipping");
            continue;
        }

        // Read and compress
        match std::fs::read(&obj_path) {
            Ok(data) => {
                let compressed = match compress_data(&data) {
                    Ok(c) => c,
                    Err(e) => {
                        tracing::error!(oid = %obj.oid, err = %e, "failed to compress LFS object");
                        continue;
                    }
                };

                let compressed_path = obj_path.with_extension("zst");
                if let Err(e) = std::fs::write(&compressed_path, &compressed) {
                    tracing::error!(oid = %obj.oid, err = %e, "failed to write compressed file");
                    continue;
                }

                // Update DB
                if let Err(e) = lfs_object_ops::update_compression(
                    db,
                    obj.id,
                    COMPRESSION_ALGO,
                    compressed.len() as i64,
                ).await {
                    tracing::error!(oid = %obj.oid, err = %e, "failed to update DB");
                    // Clean up compressed file on DB error
                    let _ = std::fs::remove_file(&compressed_path);
                    continue;
                }

                // Remove original uncompressed file
                if let Err(e) = std::fs::remove_file(&obj_path) {
                    tracing::warn!(oid = %obj.oid, err = %e, "failed to remove original file");
                }

                tracing::info!(
                    oid = %obj.oid,
                    original = data.len(),
                    compressed = compressed.len(),
                    ratio = format!("{:.1}%", (compressed.len() as f64 / data.len() as f64) * 100.0),
                    "compressed existing LFS object"
                );
                count += 1;
            }
            Err(e) => {
                tracing::error!(oid = %obj.oid, err = %e, "failed to read original file");
            }
        }
    }

    Ok(count)
}

/// Delete an LFS object from disk and DB.
pub async fn delete_object(
    db: &DatabaseConnection,
    repo_id: i64,
    lfs_root: &std::path::Path,
    oid: &str,
) -> Result<()> {
    let obj_path = lfs_object_path(lfs_root, oid);

    // Delete compressed version
    let compressed_path = obj_path.with_extension("zst");
    if compressed_path.exists() {
        std::fs::remove_file(&compressed_path)
            .with_context(|| format!("delete compressed LFS object {:?}", compressed_path))?;
    }

    // Delete uncompressed version (legacy)
    if obj_path.exists() {
        std::fs::remove_file(&obj_path)
            .with_context(|| format!("delete LFS object {:?}", obj_path))?;
    }

    // Delete from DB
    if let Some(obj) = lfs_object_ops::find_by_repo_and_oid(db, repo_id, oid).await? {
        lfs_object_ops::delete_by_id(db, obj.id).await?;
    }

    Ok(())
}
