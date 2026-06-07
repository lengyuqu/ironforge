//! Package Registry storage layer.
//!
//! Manages file-system storage for package blobs.
//! Directory layout: `{root}/{owner}/{repo}/packages/{type}/{name}/{version}/{filename}`

use std::path::{Path, PathBuf};
use sha2::{Sha256, Digest};
use tokio::io::AsyncWriteExt;

#[derive(Debug, Clone)]
pub struct PackageStorage {
    root: PathBuf,
}

impl PackageStorage {
    pub fn new(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
        }
    }

    /// Build the base path for a package type in a specific repo.
    pub fn base_path(
        &self,
        owner: &str,
        repo: &str,
        package_type: &str,
    ) -> PathBuf {
        self.root
            .join(owner)
            .join(repo)
            .join("packages")
            .join(package_type)
    }

    /// Build the path for a specific package name.
    pub fn package_path(
        &self,
        owner: &str,
        repo: &str,
        package_type: &str,
        name: &str,
    ) -> PathBuf {
        self.base_path(owner, repo, package_type).join(name)
    }

    /// Build the path for a specific package version.
    pub fn version_path(
        &self,
        owner: &str,
        repo: &str,
        package_type: &str,
        name: &str,
        version: &str,
    ) -> PathBuf {
        self.package_path(owner, repo, package_type, name).join(version)
    }

    /// Store a file returning its storage path and sha256.
    pub async fn store_file(
        &self,
        owner: &str,
        repo: &str,
        package_type: &str,
        name: &str,
        version: &str,
        filename: &str,
        data: &[u8],
    ) -> Result<StoredFile> {
        let dir = self.version_path(owner, repo, package_type, name, version);
        tokio::fs::create_dir_all(&dir).await?;

        let file_path = dir.join(filename);
        let mut f = tokio::fs::File::create(&file_path).await?;
        f.write_all(data).await?;
        f.flush().await?;

        let sha256 = hex::encode(Sha256::digest(data));

        Ok(StoredFile {
            filename: filename.to_string(),
            size: data.len() as i64,
            sha256,
            storage_path: file_path.to_string_lossy().to_string(),
        })
    }

    /// Read a file from storage.
    pub async fn read_file(&self, storage_path: &str) -> Result<Vec<u8>> {
        tokio::fs::read(storage_path).await.map_err(Into::into)
    }

    /// Stream a file from storage (returns the file path for serving).
    pub fn file_path(&self, storage_path: &str) -> PathBuf {
        PathBuf::from(storage_path)
    }

    /// Delete a version directory and all its files.
    pub async fn delete_version(
        &self,
        owner: &str,
        repo: &str,
        package_type: &str,
        name: &str,
        version: &str,
    ) -> Result<()> {
        let dir = self.version_path(owner, repo, package_type, name, version);
        if dir.exists() {
            tokio::fs::remove_dir_all(&dir).await?;
        }
        Ok(())
    }

    /// Delete a file by storage path.
    pub async fn delete_file(&self, storage_path: &str) -> Result<()> {
        let path = Path::new(storage_path);
        if path.exists() {
            tokio::fs::remove_file(path).await?;
        }
        Ok(())
    }

    /// Check if a version directory has any files (synchronous check for simplicity).
    pub async fn has_files(
        &self,
        owner: &str,
        repo: &str,
        package_type: &str,
        name: &str,
        version: &str,
    ) -> bool {
        let dir = self.version_path(owner, repo, package_type, name, version);
        if let Ok(entries) = std::fs::read_dir(&dir) {
            entries.count() > 0
        } else {
            false
        }
    }
}

#[derive(Debug, Clone)]
pub struct StoredFile {
    pub filename: String,
    pub size: i64,
    pub sha256: String,
    pub storage_path: String,
}

/// Error type for storage operations.
pub type Error = anyhow::Error;
pub type Result<T> = std::result::Result<T, Error>;
