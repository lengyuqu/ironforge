//! OCI content-addressed blob storage.
//!
//! Layout:
//! ```text
//! {root}/{owner}/{repo}/oci/
//!   _blobs/{algo}/{hash}        — stored blobs (content-addressed)
//!   _uploads/{uuid}/            — in-progress upload temp dir
//!   _manifests/{algo}/{digest}  — cached manifest JSON
//! ```
//!
//! Blob paths use the first two chars of the hex digest as a sharding prefix:
//! sha256:abc123... → _blobs/sha256/ab/abc123...

use std::path::{Path, PathBuf};
use sha2::{Sha256, Digest};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct OciStorage {
    root: PathBuf,
}

impl OciStorage {
    pub fn new(root: &Path) -> Self {
        Self { root: root.to_path_buf() }
    }

    // ── path helpers ────────────────────────────────────────────

    /// Base path for an OCI namespace (owner/repo).
    fn namespace_base(&self, owner: &str, repo: &str) -> PathBuf {
        self.root.join(owner).join(repo).join("oci")
    }

    /// Blob directory: _blobs/{algo}/xx/
    fn blob_dir(&self, owner: &str, repo: &str, digest: &str) -> PathBuf {
        let parts: Vec<&str> = digest.splitn(2, ':').collect();
        let (algo, hash) = if parts.len() == 2 {
            (parts[0], parts[1])
        } else {
            ("sha256", parts[0])
        };
        self.namespace_base(owner, repo)
            .join("_blobs")
            .join(algo)
            .join(&hash[..2.min(hash.len())])
    }

    /// Blob file path: _blobs/{algo}/xx/{hash}
    fn blob_path(&self, owner: &str, repo: &str, digest: &str) -> PathBuf {
        let parts: Vec<&str> = digest.splitn(2, ':').collect();
        let (_algo, hash) = if parts.len() == 2 {
            (parts[0], parts[1])
        } else {
            ("sha256", parts[0])
        };
        self.blob_dir(owner, repo, digest).join(hash)
    }

    /// Manifest file path: _manifests/{algo}/{digest}
    fn manifest_path(&self, owner: &str, repo: &str, digest: &str) -> PathBuf {
        let algo = digest.split(':').next().unwrap_or("sha256");
        self.namespace_base(owner, repo)
            .join("_manifests")
            .join(algo)
            .join(digest.replace(':', "_"))
    }

    /// Upload temp directory: _uploads/{uuid}/
    fn upload_dir(&self, owner: &str, repo: &str, uuid: &str) -> PathBuf {
        self.namespace_base(owner, repo)
            .join("_uploads")
            .join(uuid)
    }

    /// Upload temp file: _uploads/{uuid}/data
    fn upload_file_path(&self, owner: &str, repo: &str, uuid: &str) -> PathBuf {
        self.upload_dir(owner, repo, uuid).join("data")
    }

    // ── blob operations ────────────────────────────────────────

    /// Check if a blob exists (by digest).
    pub fn blob_exists(&self, owner: &str, repo: &str, digest: &str) -> bool {
        self.blob_path(owner, repo, digest).exists()
    }

    /// Store a blob (content-addressed by digest).
    pub async fn store_blob(
        &self,
        owner: &str,
        repo: &str,
        digest: &str,
        data: &[u8],
    ) -> anyhow::Result<String> {
        // Verify digest matches
        let actual = format!("sha256:{}", hex::encode(Sha256::digest(data)));
        if actual != digest {
            anyhow::bail!(
                "digest mismatch: expected {}, got {}",
                digest, actual
            );
        }

        let dir = self.blob_dir(owner, repo, digest);
        tokio::fs::create_dir_all(&dir).await?;

        let path = self.blob_path(owner, repo, digest);
        // Skip if already exists (dedup)
        if path.exists() {
            return Ok(path.to_string_lossy().to_string());
        }

        tokio::fs::write(&path, data).await?;
        Ok(path.to_string_lossy().to_string())
    }

    /// Read a blob from storage.
    pub async fn read_blob(
        &self,
        owner: &str,
        repo: &str,
        digest: &str,
    ) -> anyhow::Result<Vec<u8>> {
        let path = self.blob_path(owner, repo, digest);
        if !path.exists() {
            anyhow::bail!("blob not found: {}", digest);
        }
        tokio::fs::read(&path).await.map_err(Into::into)
    }

    /// Get the file path to a blob for streaming.
    pub fn blob_file_path(&self, owner: &str, repo: &str, digest: &str) -> PathBuf {
        self.blob_path(owner, repo, digest)
    }

    /// Copy a blob file from another namespace (cross-repo mount).
    /// Uses hardlink when possible, falls back to `tokio::fs::copy` (streaming copy).
    pub async fn copy_blob_file(
        &self,
        src_owner: &str,
        src_repo: &str,
        dst_owner: &str,
        dst_repo: &str,
        digest: &str,
    ) -> anyhow::Result<String> {
        let src = self.blob_path(src_owner, src_repo, digest);
        let dst = self.blob_path(dst_owner, dst_repo, digest);

        if dst.exists() {
            return Ok(dst.to_string_lossy().to_string());
        }

        // Ensure target directory exists
        if let Some(parent) = dst.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        // Try hardlink first (instant, no data copy)
        if std::fs::hard_link(&src, &dst).is_err() {
            // Fall back to streaming copy
            tokio::fs::copy(&src, &dst).await?;
        }

        Ok(dst.to_string_lossy().to_string())
    }

    // ── manifest operations ─────────────────────────────────────

    /// Store manifest JSON.
    pub async fn store_manifest(
        &self,
        owner: &str,
        repo: &str,
        digest: &str,
        data: &[u8],
    ) -> anyhow::Result<String> {
        let path = self.manifest_path(owner, repo, digest);
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tokio::fs::write(&path, data).await?;
        Ok(path.to_string_lossy().to_string())
    }

    /// Read manifest JSON.
    pub async fn read_manifest(
        &self,
        owner: &str,
        repo: &str,
        digest: &str,
    ) -> anyhow::Result<Vec<u8>> {
        let path = self.manifest_path(owner, repo, digest);
        tokio::fs::read(&path).await.map_err(Into::into)
    }

    // ── upload operations ───────────────────────────────────────

    /// Create a new upload session, returning the UUID and temp file path.
    pub async fn create_upload(
        &self,
        owner: &str,
        repo: &str,
    ) -> anyhow::Result<(String, String)> {
        let uuid = Uuid::new_v4().to_string();
        let dir = self.upload_dir(owner, repo, &uuid);
        tokio::fs::create_dir_all(&dir).await?;

        let file_path = self.upload_file_path(owner, repo, &uuid);
        // Create empty file
        tokio::fs::write(&file_path, &[]).await?;

        Ok((uuid, file_path.to_string_lossy().to_string()))
    }

    /// Append chunk data to an upload.
    pub async fn append_to_upload(
        &self,
        owner: &str,
        repo: &str,
        uuid: &str,
        data: &[u8],
    ) -> anyhow::Result<i64> {
        let path = self.upload_file_path(owner, repo, uuid);
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;
        file.write_all(data)?;
        let size = file.metadata()?.len() as i64;
        Ok(size)
    }

    /// Get current upload size.
    pub fn upload_size(&self, owner: &str, repo: &str, uuid: &str) -> i64 {
        let path = self.upload_file_path(owner, repo, uuid);
        std::fs::metadata(&path)
            .map(|m| m.len() as i64)
            .unwrap_or(0)
    }

    /// Finalize an upload: verify digest, copy to blob storage, return storage path.
    /// Streams the upload file in chunks—never loads the entire file into memory.
    /// Returns (digest, size, storage_path).
    pub async fn finalize_upload(
        &self,
        owner: &str,
        repo: &str,
        uuid: &str,
        expected_digest: &str,
    ) -> anyhow::Result<(String, i64, String)> {
        let upload_path = self.upload_file_path(owner, repo, uuid);
        let blob_dst = self.blob_path(owner, repo, expected_digest);

        // Dedup: already exists
        if blob_dst.exists() {
            let size = blob_dst.metadata().map(|m| m.len() as i64).unwrap_or(0);
            let dir = self.upload_dir(owner, repo, uuid);
            let _ = tokio::fs::remove_dir_all(&dir).await;
            return Ok((expected_digest.to_string(), size, blob_dst.to_string_lossy().to_string()));
        }

        // Ensure blob directory exists
        if let Some(parent) = blob_dst.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        // Stream copy + hash: read upload file in chunks, pipe to blob file
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let mut src = tokio::fs::File::open(&upload_path).await?;
        let mut dst = tokio::fs::File::create(&blob_dst).await?;
        let mut hasher = Sha256::new();

        let mut buf = vec![0u8; 64 * 1024]; // 64 KiB chunks
        loop {
            let n = src.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
            dst.write_all(&buf[..n]).await?;
        }

        let size = dst.metadata().await?.len() as i64;
        let actual = format!("sha256:{}", hex::encode(hasher.finalize()));
        if actual != expected_digest {
            // Clean up partial blob on digest mismatch
            let _ = tokio::fs::remove_file(&blob_dst).await;
            anyhow::bail!(
                "digest mismatch: expected {}, got {}",
                expected_digest, actual
            );
        }

        // Clean up upload temp
        let dir = self.upload_dir(owner, repo, uuid);
        let _ = tokio::fs::remove_dir_all(&dir).await;

        Ok((expected_digest.to_string(), size, blob_dst.to_string_lossy().to_string()))
    }

    /// Delete upload temp files.
    pub async fn delete_upload(&self, owner: &str, repo: &str, uuid: &str) -> anyhow::Result<()> {
        let dir = self.upload_dir(owner, repo, uuid);
        if dir.exists() {
            tokio::fs::remove_dir_all(&dir).await?;
        }
        Ok(())
    }

    /// Read upload file content for streaming during PATCH.
    pub fn upload_file(&self, owner: &str, repo: &str, uuid: &str) -> PathBuf {
        self.upload_file_path(owner, repo, uuid)
    }
}
