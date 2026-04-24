//! Git LFS service — implements the LFS batch API.
//!
//! Git LFS (Large File Storage) replaces large files with pointer files in Git,
//! while storing the actual content separately. This service implements the
//! LFS batch API for upload/download operations.
//!
//! Storage layout: `<repo_root>/<owner>/<repo>.lfs/<oid_prefix>/<oid>`

use anyhow::{Context, Result};
use chrono::Utc;
use sea_orm::DatabaseConnection;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use rg_db::entities::lfs_object;
use rg_db::ops::lfs_object_ops;

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

    // Write file
    std::fs::write(&obj_path, data)
        .with_context(|| format!("write LFS object {:?}", obj_path))?;

    // Mark as uploaded in DB
    let existing = lfs_object_ops::find_by_repo_and_oid(db, repo_id, oid)
        .await?
        .ok_or_else(|| anyhow::anyhow!("LFS object {} not registered", oid))?;

    lfs_object_ops::mark_uploaded(db, existing.id).await?;

    Ok(())
}

/// Read an LFS object from disk.
pub async fn read_object(
    lfs_root: &std::path::Path,
    oid: &str,
) -> Result<Vec<u8>> {
    let obj_path = lfs_object_path(lfs_root, oid);
    std::fs::read(&obj_path).with_context(|| format!("read LFS object {:?}", obj_path))
}
