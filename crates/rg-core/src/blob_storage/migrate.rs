//! Copy every blob from one backend to another.
//!
//! This is the "we already have data on disk and just bought a bucket" path.
//! It is deliberately a one-shot operator tool rather than something the server
//! runs: moving data is a decision, not a background chore, and it wants to be
//! watched.
//!
//! # Resume and verification
//!
//! Existing objects on the target are skipped, so an interrupted run is just
//! restarted. Every object is read back against the size the source reported —
//! a copy that silently truncated is worse than one that failed.
//!
//! # Scope
//!
//! The local backend's root *is* the repository root, so an unscoped
//! `list(None)` would also walk every `{owner}/{repo}.git` object database.
//! The migration therefore walks [`BLOB_PREFIXES`] only, one subtree at a time
//! (which is also much faster than enumerating everything and filtering).

use super::{BlobKey, BlobStorage};

/// Top-level key namespaces owned by the blob store.
///
/// Anything outside these belongs to the git repository tree and must not be
/// copied into the object store.
pub const BLOB_PREFIXES: [&str; 7] = [
    "artifacts",
    "attachments",
    "lfs",
    "oci",
    "oci-uploads",
    "packages",
    "releases",
];

/// What a migration run did.
#[derive(Debug, Default)]
pub struct MigrationReport {
    /// Objects seen under the blob prefixes.
    pub scanned: usize,
    /// Objects written to the target.
    pub copied: usize,
    /// Objects already present on the target.
    pub skipped: usize,
    pub bytes_copied: u64,
    /// Objects that could not be copied: `(key, reason)`. The run continues
    /// past them, so one unreadable object does not abandon the rest.
    pub failed: Vec<(String, String)>,
}

impl MigrationReport {
    pub fn is_complete(&self) -> bool {
        self.failed.is_empty()
    }
}

/// Copy every blob under [`BLOB_PREFIXES`] from `source` to `target`.
///
/// `dry_run` counts what would be copied and writes nothing.
pub async fn copy_all(
    source: &dyn BlobStorage,
    target: &dyn BlobStorage,
    dry_run: bool,
) -> MigrationReport {
    let mut report = MigrationReport::default();

    for namespace in BLOB_PREFIXES {
        let Ok(prefix) = BlobKey::new(namespace) else {
            // A hard-coded constant that fails validation is a bug, not a
            // runtime condition; record it and keep the rest of the run going.
            report
                .failed
                .push((namespace.to_string(), "invalid prefix constant".to_string()));
            continue;
        };

        let entries = match source.list(Some(&prefix)).await {
            Ok(entries) => entries,
            Err(error) => {
                // A missing namespace is normal (an instance may never have
                // published a package); anything else is worth reporting.
                tracing::debug!(prefix = namespace, %error, "blob migration: namespace not listed");
                continue;
            }
        };

        for entry in entries {
            report.scanned += 1;
            let key = entry.key;

            match target.exists(&key).await {
                Ok(true) => {
                    report.skipped += 1;
                    continue;
                }
                Ok(false) => {}
                Err(error) => {
                    report
                        .failed
                        .push((key.as_str().to_string(), format!("exists: {error}")));
                    continue;
                }
            }

            if dry_run {
                report.copied += 1;
                report.bytes_copied += entry.size;
                continue;
            }

            let data = match source.get(&key).await {
                Ok(data) => data,
                Err(error) => {
                    report
                        .failed
                        .push((key.as_str().to_string(), format!("read: {error}")));
                    continue;
                }
            };

            // Verify before writing: a truncated read would otherwise be
            // indistinguishable from a truncated blob.
            if data.len() as u64 != entry.size {
                report.failed.push((
                    key.as_str().to_string(),
                    format!(
                        "size mismatch while reading: expected {} bytes, got {}",
                        entry.size,
                        data.len()
                    ),
                ));
                continue;
            }

            match target.put(&key, &data).await {
                Ok(written) => {
                    if written.size != entry.size {
                        // The target accepted the write but stored a different
                        // length: surface it rather than claiming success.
                        report.failed.push((
                            key.as_str().to_string(),
                            format!(
                                "size mismatch after write: expected {} bytes, stored {}",
                                entry.size, written.size
                            ),
                        ));
                        continue;
                    }
                    report.copied += 1;
                    report.bytes_copied += entry.size;
                }
                Err(error) => {
                    report
                        .failed
                        .push((key.as_str().to_string(), format!("write: {error}")));
                }
            }
        }
    }

    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blob_storage::LocalBlobStorage;

    fn local(dir: &std::path::Path) -> LocalBlobStorage {
        LocalBlobStorage::new(dir)
    }

    fn key(segments: &[&str]) -> BlobKey {
        BlobKey::from_segments(segments.iter().copied()).expect("valid key")
    }

    /// Everything under a blob prefix is copied; a git object database sharing
    /// the same root is not.
    #[tokio::test]
    async fn copies_blob_namespaces_and_ignores_the_repository_tree() {
        let src = tempfile::tempdir().unwrap();
        let dst = tempfile::tempdir().unwrap();
        let source = local(src.path());
        let target = local(dst.path());

        let lfs = key(&["lfs", "acme", "repo", "aa", "blob"]);
        let pkg = key(&["packages", "acme", "npm", "thing", "1.0.0"]);
        let artifact = key(&["artifacts", "jobs", "42", "build.log"]);
        for (k, body) in [
            (&lfs, b"lfs-bytes".as_slice()),
            (&pkg, b"package-bytes".as_slice()),
            (&artifact, b"log-bytes".as_slice()),
        ] {
            source.put(k, body).await.unwrap();
        }

        // A repository that happens to live under the same root.
        std::fs::create_dir_all(src.path().join("acme/demo.git/objects")).unwrap();
        std::fs::write(
            src.path().join("acme/demo.git/objects/pack"),
            b"object data",
        )
        .unwrap();

        let report = copy_all(&source, &target, false).await;
        assert!(report.is_complete(), "failures: {:?}", report.failed);
        assert_eq!(report.copied, 3);
        assert_eq!(report.scanned, 3);

        for k in [&lfs, &pkg, &artifact] {
            assert!(target.exists(k).await.unwrap(), "missing {}", k.as_str());
        }
        assert!(
            !dst.path().join("acme/demo.git").exists(),
            "the git object database must not be copied"
        );
    }

    /// A second run copies nothing: the target already has every object.
    #[tokio::test]
    async fn a_second_run_skips_what_is_already_there() {
        let src = tempfile::tempdir().unwrap();
        let dst = tempfile::tempdir().unwrap();
        let source = local(src.path());
        let target = local(dst.path());
        source
            .put(&key(&["lfs", "a", "b"]), b"payload")
            .await
            .unwrap();

        let first = copy_all(&source, &target, false).await;
        assert_eq!(first.copied, 1);

        let second = copy_all(&source, &target, false).await;
        assert_eq!(second.copied, 0);
        assert_eq!(second.skipped, 1);
    }

    /// `dry_run` reports the same numbers without writing anything.
    #[tokio::test]
    async fn a_dry_run_writes_nothing() {
        let src = tempfile::tempdir().unwrap();
        let dst = tempfile::tempdir().unwrap();
        let source = local(src.path());
        let target = local(dst.path());
        let k = key(&["lfs", "x", "y"]);
        source.put(&k, b"0123456789").await.unwrap();

        let report = copy_all(&source, &target, true).await;
        assert_eq!(report.copied, 1);
        assert_eq!(report.bytes_copied, 10);
        assert!(!target.exists(&k).await.unwrap(), "dry run must not write");
    }

    /// Byte counts add up, so the operator can size the bucket beforehand.
    #[tokio::test]
    async fn byte_totals_match_what_was_written() {
        let src = tempfile::tempdir().unwrap();
        let dst = tempfile::tempdir().unwrap();
        let source = local(src.path());
        let target = local(dst.path());
        for (i, size) in [3usize, 7, 11].into_iter().enumerate() {
            let k = key(&["lfs", "sizes", &format!("{i}")]);
            source.put(&k, &vec![b'x'; size]).await.unwrap();
        }

        let report = copy_all(&source, &target, false).await;
        assert!(report.is_complete(), "failures: {:?}", report.failed);
        assert_eq!(report.bytes_copied, 21);
    }
}
