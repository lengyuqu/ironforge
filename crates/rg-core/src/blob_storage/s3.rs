//! S3-compatible object storage backend (AWS S3, MinIO, R2, Ceph, ...).
//!
//! The contract is the same [`BlobStorage`] every other backend implements;
//! only the durability story changes. Keys keep the exact shape they have on
//! the local backend (`lfs/ab/cd/…`), so a later migration is a copy, not a
//! rewrite.
//!
//! # Why presigned URLs
//!
//! `rusty-s3` signs a request by producing a URL that already carries the
//! signature. That keeps this file out of the SigV4 business and means the
//! request carries no secret beyond the short-lived URL itself. Uploads are
//! `PUT` to a signed URL, downloads `GET` to one.
//!
//! # What is deliberately not here
//!
//! * **Streaming uploads via a single request.** Objects past
//!   [`MULTIPART_THRESHOLD`] are uploaded in [`PART_SIZE`] chunks, so peak
//!   memory is one part rather than the whole file; smaller ones still go in a
//!   single PUT, which is simpler and atomic.
//! * **Redirects for downloads.** Callers that want the client to fetch
//!   directly from S3 need a presigned GET; the trait returns bytes, and adding
//!   `presigned_get` here without a caller would be speculative.

use super::{BlobKey, BlobMetadata, BlobStorage, BlobStorageError, Result};
use futures::future::BoxFuture;
use rusty_s3::{Bucket, Credentials, S3Action, UrlStyle};
use std::path::Path;
use std::time::{Duration, SystemTime};

/// How long a signed URL stays valid.
///
/// Long enough for a slow upload of a large artifact on a bad link; short enough
/// that a URL leaked into a log is useless by the time anyone reads it.
const PRESIGN_TTL: Duration = Duration::from_secs(15 * 60);

/// Below this, a single PUT is simpler and cheaper than a multipart dance.
const MULTIPART_THRESHOLD: u64 = 16 * 1024 * 1024;
/// S3 requires every part except the last to be at least 5 MiB; 8 leaves
/// headroom and keeps the part count far below the 10 000 limit.
const PART_SIZE: usize = 8 * 1024 * 1024;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);

/// Connection settings for an S3-compatible endpoint.
#[derive(Clone, Debug)]
pub struct S3Config {
    /// `https://s3.amazonaws.com`, `http://minio:9000`, ...
    pub endpoint: String,
    pub bucket: String,
    /// SigV4 region. MinIO accepts anything non-empty; AWS requires the real one.
    pub region: String,
    pub access_key: String,
    pub secret_key: String,
    /// Path-style addressing (`endpoint/bucket/key`). S3-compatible servers that
    /// are not addressed by a bucket-per-subdomain DNS name need this; AWS is
    /// moving away from it.
    pub path_style: bool,
    /// Optional key prefix, so one bucket can be shared with other applications.
    pub prefix: Option<String>,
}

/// S3-backed implementation of [`BlobStorage`].
pub struct S3BlobStorage {
    client: reqwest::Client,
    bucket: Bucket,
    credentials: Credentials,
    prefix: Option<String>,
}

impl S3BlobStorage {
    pub fn new(config: &S3Config) -> Result<Self> {
        let endpoint: reqwest::Url = config
            .endpoint
            .parse()
            .map_err(|e| BlobStorageError::InvalidKey(format!("invalid S3 endpoint: {e}")))?;
        let style = if config.path_style {
            UrlStyle::Path
        } else {
            UrlStyle::VirtualHost
        };
        let bucket = Bucket::new(
            endpoint,
            style,
            config.bucket.clone(),
            config.region.clone(),
        )
        .map_err(|e| {
            BlobStorageError::InvalidKey(format!("invalid S3 bucket configuration: {e}"))
        })?;

        let client = reqwest::Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(REQUEST_TIMEOUT)
            .build()
            .map_err(transport_error)?;

        Ok(Self {
            client,
            bucket,
            credentials: Credentials::new(config.access_key.as_str(), config.secret_key.as_str()),
            prefix: config
                .prefix
                .as_deref()
                .map(|p| p.trim_matches('/'))
                .filter(|p| !p.is_empty())
                .map(str::to_string),
        })
    }

    /// Object name in the bucket: `prefix/key`, or just `key`.
    fn object_name(&self, key: &BlobKey) -> String {
        match &self.prefix {
            Some(prefix) => format!("{prefix}/{}", key.as_str()),
            None => key.as_str().to_string(),
        }
    }

    /// Reverse of [`Self::object_name`] — `None` when the object lives outside
    /// this backend's prefix and therefore is not ours to list.
    fn key_from_object(&self, object: &str) -> Option<BlobKey> {
        let relative = match &self.prefix {
            Some(prefix) => object.strip_prefix(prefix.as_str())?.strip_prefix('/')?,
            None => object,
        };
        BlobKey::new(relative).ok()
    }

    fn signed_put(&self, key: &BlobKey) -> reqwest::Url {
        let object = self.object_name(key);
        self.bucket
            .put_object(Some(&self.credentials), &object)
            .sign(PRESIGN_TTL)
    }

    fn signed_get(&self, key: &BlobKey) -> reqwest::Url {
        let object = self.object_name(key);
        self.bucket
            .get_object(Some(&self.credentials), &object)
            .sign(PRESIGN_TTL)
    }

    fn signed_head(&self, key: &BlobKey) -> reqwest::Url {
        let object = self.object_name(key);
        self.bucket
            .head_object(Some(&self.credentials), &object)
            .sign(PRESIGN_TTL)
    }

    fn signed_delete(&self, key: &BlobKey) -> reqwest::Url {
        let object = self.object_name(key);
        self.bucket
            .delete_object(Some(&self.credentials), &object)
            .sign(PRESIGN_TTL)
    }

    /// `CreateMultipartUpload` → the upload id that ties the parts together.
    async fn create_multipart(&self, object: &str) -> Result<String> {
        let action = self
            .bucket
            .create_multipart_upload(Some(&self.credentials), object);
        let url = action.sign(PRESIGN_TTL);
        let response = self
            .client
            .post(url)
            .send()
            .await
            .map_err(transport_error)?;
        let status = response.status();
        if !status.is_success() {
            return Err(BlobStorageError::Io(std::io::Error::other(format!(
                "S3 CreateMultipartUpload failed with HTTP {status}"
            ))));
        }
        let body = response.text().await.map_err(transport_error)?;
        let parsed =
            rusty_s3::actions::CreateMultipartUpload::parse_response(&body).map_err(|e| {
                BlobStorageError::Io(std::io::Error::other(format!(
                    "S3 CreateMultipartUpload response could not be parsed: {e}"
                )))
            })?;
        Ok(parsed.upload_id().to_string())
    }

    /// Upload every part, in order. Returns the ETags the completion needs.
    async fn upload_parts(
        &self,
        object: &str,
        upload_id: &str,
        source: &Path,
    ) -> Result<Vec<String>> {
        use tokio::io::AsyncReadExt;

        let mut file = tokio::fs::File::open(source).await?;
        let mut etags: Vec<String> = Vec::new();
        let mut part_number: u16 = 1;

        loop {
            // Read exactly one part, or whatever is left at the end.
            let mut buffer = vec![0u8; PART_SIZE];
            let mut filled = 0usize;
            while filled < PART_SIZE {
                let read = file.read(&mut buffer[filled..]).await?;
                if read == 0 {
                    break;
                }
                filled += read;
            }
            if filled == 0 {
                break;
            }
            buffer.truncate(filled);

            let action =
                self.bucket
                    .upload_part(Some(&self.credentials), object, part_number, upload_id);
            let url = action.sign(PRESIGN_TTL);
            let response = self
                .client
                .put(url)
                .body(buffer)
                .send()
                .await
                .map_err(transport_error)?;
            let status = response.status();
            if !status.is_success() {
                return Err(BlobStorageError::Io(std::io::Error::other(format!(
                    "S3 UploadPart {part_number} failed with HTTP {status}"
                ))));
            }
            // The ETag is what proves the part arrived intact; without it the
            // completion cannot name the part.
            let etag = response
                .headers()
                .get(reqwest::header::ETAG)
                .and_then(|value| value.to_str().ok())
                .map(str::to_string)
                .ok_or_else(|| {
                    BlobStorageError::Io(std::io::Error::other(format!(
                        "S3 UploadPart {part_number} returned no ETag"
                    )))
                })?;
            etags.push(etag);
            part_number = part_number.checked_add(1).ok_or_else(|| {
                BlobStorageError::Io(std::io::Error::other(
                    "S3 multipart upload exceeded 65535 parts".to_string(),
                ))
            })?;
        }

        if etags.is_empty() {
            return Err(BlobStorageError::Io(std::io::Error::other(
                "S3 multipart upload read no data",
            )));
        }
        Ok(etags)
    }

    async fn complete_multipart(
        &self,
        object: &str,
        upload_id: &str,
        etags: &[String],
    ) -> Result<()> {
        // `body()` consumes the action, and the signature covers the body, so
        // the XML is built once and attached to a separately signed request.
        let body = self
            .bucket
            .complete_multipart_upload(
                Some(&self.credentials),
                object,
                upload_id,
                etags.iter().map(String::as_str),
            )
            .body();
        let action = self.bucket.complete_multipart_upload(
            Some(&self.credentials),
            object,
            upload_id,
            etags.iter().map(String::as_str),
        );
        let url = action.sign(PRESIGN_TTL);
        let response = self
            .client
            .post(url)
            .body(body)
            .send()
            .await
            .map_err(transport_error)?;
        let status = response.status();
        if !status.is_success() {
            let detail = response.text().await.unwrap_or_default();
            return Err(BlobStorageError::Io(std::io::Error::other(format!(
                "S3 CompleteMultipartUpload failed with HTTP {status}: {detail}"
            ))));
        }
        Ok(())
    }

    /// Best effort: an abandoned upload keeps costing storage and is invisible
    /// in a bucket listing, so a failed migration must not leave one behind.
    async fn abort_multipart(&self, object: &str, upload_id: &str) {
        let action = self
            .bucket
            .abort_multipart_upload(Some(&self.credentials), object, upload_id);
        let url = action.sign(PRESIGN_TTL);
        if let Err(error) = self.client.delete(url).send().await {
            tracing::warn!(%error, object, "could not abort a failed multipart upload");
        }
    }

    /// Chunked upload for objects above [`MULTIPART_THRESHOLD`].
    async fn send_multipart(
        &self,
        key: &BlobKey,
        source: &Path,
        size: u64,
    ) -> Result<BlobMetadata> {
        let object = self.object_name(key);
        let upload_id = self.create_multipart(&object).await?;
        match self.upload_parts(&object, &upload_id, source).await {
            Ok(etags) => {
                self.complete_multipart(&object, &upload_id, &etags).await?;
                Ok(BlobMetadata {
                    key: key.clone(),
                    size,
                    modified: Some(SystemTime::now()),
                })
            }
            Err(error) => {
                self.abort_multipart(&object, &upload_id).await;
                Err(error)
            }
        }
    }

    async fn send_put(&self, key: &BlobKey, body: Vec<u8>) -> Result<BlobMetadata> {
        let response = self
            .client
            .put(self.signed_put(key))
            .body(body.clone())
            .send()
            .await
            .map_err(transport_error)?;
        ensure_success(response.status(), key, "PUT")?;
        Ok(BlobMetadata {
            key: key.clone(),
            size: body.len() as u64,
            modified: Some(SystemTime::now()),
        })
    }
}

/// Transport-level failure. `BlobStorageError` predates this backend and has no
/// variant for "the HTTP client gave up"; folding it into `Io` keeps every
/// existing `match` exhaustive.
fn transport_error(error: reqwest::Error) -> BlobStorageError {
    BlobStorageError::Io(std::io::Error::other(format!(
        "S3 transport error: {error}"
    )))
}

/// Map a non-success HTTP status onto the storage error the trait speaks.
fn ensure_success(status: reqwest::StatusCode, key: &BlobKey, action: &str) -> Result<()> {
    if status.is_success() {
        return Ok(());
    }
    if status == reqwest::StatusCode::NOT_FOUND {
        return Err(BlobStorageError::NotFound(key.clone()));
    }
    Err(BlobStorageError::Io(std::io::Error::other(format!(
        "S3 {action} failed with HTTP {status}"
    ))))
}

/// HTTP-date (`Wed, 21 Oct 2015 07:28:00 GMT`) → `SystemTime`.
fn parse_http_date(value: &str) -> Option<SystemTime> {
    chrono::DateTime::parse_from_rfc2822(value)
        .ok()
        .map(|parsed| SystemTime::from(parsed.with_timezone(&chrono::Utc)))
}

impl BlobStorage for S3BlobStorage {
    fn backend_name(&self) -> &'static str {
        "s3"
    }

    fn put<'a>(&'a self, key: &'a BlobKey, data: &'a [u8]) -> BoxFuture<'a, Result<BlobMetadata>> {
        Box::pin(self.send_put(key, data.to_vec()))
    }

    fn put_file<'a>(
        &'a self,
        key: &'a BlobKey,
        source: &'a Path,
    ) -> BoxFuture<'a, Result<BlobMetadata>> {
        Box::pin(async move {
            let size = tokio::fs::metadata(source).await?.len();
            if size < MULTIPART_THRESHOLD {
                // Small objects: one PUT is atomic, which satisfies the trait's
                // "never observe a partial object" rule without a temporary.
                let body = tokio::fs::read(source).await?;
                return self.send_put(key, body).await;
            }
            // Large objects (LFS, release assets, OCI blobs): chunked, so peak
            // memory stays at one part instead of the whole file.
            self.send_multipart(key, source, size).await
        })
    }

    fn get<'a>(&'a self, key: &'a BlobKey) -> BoxFuture<'a, Result<Vec<u8>>> {
        Box::pin(async move {
            let response = self
                .client
                .get(self.signed_get(key))
                .send()
                .await
                .map_err(transport_error)?;
            ensure_success(response.status(), key, "GET")?;
            Ok(response.bytes().await.map_err(transport_error)?.to_vec())
        })
    }

    fn metadata<'a>(&'a self, key: &'a BlobKey) -> BoxFuture<'a, Result<BlobMetadata>> {
        Box::pin(async move {
            let response = self
                .client
                .head(self.signed_head(key))
                .send()
                .await
                .map_err(transport_error)?;
            ensure_success(response.status(), key, "HEAD")?;
            let headers = response.headers();
            let size = headers
                .get(reqwest::header::CONTENT_LENGTH)
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse::<u64>().ok())
                .ok_or_else(|| {
                    BlobStorageError::Io(std::io::Error::other(
                        "S3 HEAD response carried no usable Content-Length",
                    ))
                })?;
            let modified = headers
                .get(reqwest::header::LAST_MODIFIED)
                .and_then(|v| v.to_str().ok())
                .and_then(parse_http_date);
            Ok(BlobMetadata {
                key: key.clone(),
                size,
                modified,
            })
        })
    }

    fn exists<'a>(&'a self, key: &'a BlobKey) -> BoxFuture<'a, Result<bool>> {
        Box::pin(async move {
            match self.metadata(key).await {
                Ok(_) => Ok(true),
                Err(BlobStorageError::NotFound(_)) => Ok(false),
                Err(error) => Err(error),
            }
        })
    }

    fn delete<'a>(&'a self, key: &'a BlobKey) -> BoxFuture<'a, Result<bool>> {
        Box::pin(async move {
            // S3 answers 204 for a delete whether or not the key existed, so
            // "deleted" and "was never there" are indistinguishable afterwards.
            // The trait distinguishes them, so existence is settled first.
            match self.metadata(key).await {
                Ok(_) => {}
                Err(BlobStorageError::NotFound(_)) => return Ok(false),
                Err(error) => return Err(error),
            }
            let response = self
                .client
                .delete(self.signed_delete(key))
                .send()
                .await
                .map_err(transport_error)?;
            ensure_success(response.status(), key, "DELETE")?;
            Ok(true)
        })
    }

    fn list<'a>(&'a self, prefix: Option<&'a BlobKey>) -> BoxFuture<'a, Result<Vec<BlobMetadata>>> {
        Box::pin(async move {
            let prefix = prefix.map(|key| self.object_name(key));
            let mut collected = Vec::new();
            let mut continuation: Option<String> = None;

            loop {
                let mut action = self.bucket.list_objects_v2(Some(&self.credentials));
                if let Some(prefix) = prefix.as_deref() {
                    action.with_prefix(prefix.to_string());
                }
                if let Some(token) = continuation.as_deref() {
                    action.with_continuation_token(token.to_string());
                }
                let url = action.sign(PRESIGN_TTL);

                let response = self.client.get(url).send().await.map_err(transport_error)?;
                let status = response.status();
                if !status.is_success() {
                    return Err(BlobStorageError::Io(std::io::Error::other(format!(
                        "S3 LIST failed with HTTP {status}"
                    ))));
                }
                let body = response.text().await.map_err(transport_error)?;
                let parsed =
                    rusty_s3::actions::ListObjectsV2::parse_response(&body).map_err(|e| {
                        BlobStorageError::Io(std::io::Error::other(format!(
                            "S3 LIST response could not be parsed: {e}"
                        )))
                    })?;

                for object in parsed.contents {
                    // Objects outside this backend's prefix are somebody else's.
                    let Some(key) = self.key_from_object(&object.key) else {
                        continue;
                    };
                    collected.push(BlobMetadata {
                        key,
                        size: object.size.unwrap_or(0),
                        modified: object.last_modified.as_deref().and_then(parse_http_date),
                    });
                }

                match parsed.next_continuation_token {
                    Some(token) if !token.is_empty() => continuation = Some(token),
                    _ => break,
                }
            }

            collected.sort_by(|a, b| a.key.as_str().cmp(b.key.as_str()));
            Ok(collected)
        })
    }

    fn presigned_url(&self, key: &BlobKey, ttl: Duration) -> Option<String> {
        let object = self.object_name(key);
        // Same signing path as every other request; only the method differs, so
        // a signed GET is a signed GET whatever the caller means to do with it.
        Some(
            self.bucket
                .get_object(Some(&self.credentials), &object)
                .sign(ttl)
                .to_string(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(path_style: bool) -> S3Config {
        S3Config {
            endpoint: "http://127.0.0.1:9000".to_string(),
            bucket: "ironforge".to_string(),
            region: "us-east-1".to_string(),
            access_key: "minioadmin".to_string(),
            secret_key: "minioadmin".to_string(),
            path_style,
            prefix: None,
        }
    }

    /// Path style keeps the bucket in the path and works with an IP endpoint
    /// (MinIO on localhost); virtual host puts it in the hostname and therefore
    /// needs a DNS name. Getting this backwards is the classic
    /// "works against AWS, fails against MinIO" bug.
    #[test]
    fn a_bucket_is_built_for_both_url_styles() {
        let path = S3BlobStorage::new(&config(true)).expect("path style accepts an IP endpoint");
        assert!(
            path.bucket.base_url().as_str().contains("/ironforge"),
            "path style base: {}",
            path.bucket.base_url()
        );

        let mut dns = config(false);
        dns.endpoint = "https://s3.example.com".to_string();
        let virtual_host = S3BlobStorage::new(&dns).expect("virtual host accepts a DNS endpoint");
        assert!(
            virtual_host
                .bucket
                .base_url()
                .as_str()
                .contains("ironforge."),
            "virtual host base: {}",
            virtual_host.bucket.base_url()
        );

        // The failure is explicit rather than a silently broken URL.
        let mut ip = config(false);
        ip.endpoint = "http://127.0.0.1:9000".to_string();
        assert!(
            S3BlobStorage::new(&ip).is_err(),
            "virtual host cannot be addressed by an IP endpoint"
        );
    }

    #[test]
    fn a_bad_endpoint_is_rejected_before_any_request() {
        let mut bad = config(true);
        bad.endpoint = "not a url".to_string();
        assert!(S3BlobStorage::new(&bad).is_err());
    }

    #[test]
    fn a_prefix_is_normalised_and_applied() {
        let mut with_prefix = config(true);
        with_prefix.prefix = Some("/tenant-a/".to_string());
        let storage = S3BlobStorage::new(&with_prefix).expect("valid configuration");
        let key = BlobKey::from_segments(["lfs", "ab", "cd", "blob"]).expect("valid key");
        assert_eq!(storage.object_name(&key), "tenant-a/lfs/ab/cd/blob");
        assert_eq!(
            storage.key_from_object("tenant-a/lfs/ab/cd/blob"),
            Some(key.clone())
        );
        // Another tenant's object is not ours to list.
        assert!(storage.key_from_object("tenant-b/lfs/ab/cd/blob").is_none());
    }

    #[test]
    fn without_a_prefix_objects_keep_the_local_layout() {
        let storage = S3BlobStorage::new(&config(true)).expect("valid configuration");
        let key = BlobKey::from_segments(["packages", "acme", "pkg.tgz"]).expect("valid key");
        assert_eq!(storage.object_name(&key), "packages/acme/pkg.tgz");
        assert_eq!(storage.key_from_object("packages/acme/pkg.tgz"), Some(key));
    }

    #[test]
    fn http_dates_are_parsed() {
        let parsed = parse_http_date("Wed, 21 Oct 2015 07:28:00 GMT").expect("valid HTTP date");
        let chrono_time: chrono::DateTime<chrono::Utc> = parsed.into();
        assert_eq!(chrono_time.timestamp(), 1_445_412_480);
        assert!(parse_http_date("not a date").is_none());
    }

    #[test]
    fn a_404_becomes_not_found() {
        let key = BlobKey::new("lfs/aa/bb").unwrap();
        let error = ensure_success(reqwest::StatusCode::NOT_FOUND, &key, "GET").unwrap_err();
        assert!(matches!(error, BlobStorageError::NotFound(_)));
        assert!(ensure_success(reqwest::StatusCode::FORBIDDEN, &key, "GET").is_err());
        assert!(ensure_success(reqwest::StatusCode::OK, &key, "GET").is_ok());
    }

    /// Optional live check against a real S3-compatible endpoint.
    ///
    /// Set `S3_TEST_ENDPOINT` (e.g. `http://127.0.0.1:14566`) plus
    /// `S3_TEST_BUCKET`/`S3_TEST_ACCESS_KEY`/`S3_TEST_SECRET_KEY` to run it.
    /// Without them the test returns immediately, so the default suite never
    /// needs a container — but the contract is exercised against something that
    /// actually speaks S3 before this backend is trusted.
    #[tokio::test]
    async fn round_trips_against_a_real_endpoint() {
        let Ok(endpoint) = std::env::var("S3_TEST_ENDPOINT") else {
            return;
        };
        let config = S3Config {
            endpoint,
            bucket: std::env::var("S3_TEST_BUCKET").unwrap_or_else(|_| "ironforge".to_string()),
            region: "us-east-1".to_string(),
            access_key: std::env::var("S3_TEST_ACCESS_KEY").unwrap_or_else(|_| "test".to_string()),
            secret_key: std::env::var("S3_TEST_SECRET_KEY").unwrap_or_else(|_| "test".to_string()),
            path_style: true,
            prefix: Some("rg-test".to_string()),
        };
        let storage = S3BlobStorage::new(&config).expect("live configuration");

        let key = BlobKey::from_segments(["lfs", "live", "round-trip"]).unwrap();
        let _ = storage.delete(&key).await;

        // put -> get
        let payload = b"the quick brown fox jumps over the lazy dog";
        let written = storage.put(&key, payload).await.expect("put");
        assert_eq!(written.size as usize, payload.len());
        assert_eq!(storage.get(&key).await.expect("get"), payload);

        // exists / metadata
        assert!(storage.exists(&key).await.expect("exists"));
        let meta = storage.metadata(&key).await.expect("metadata");
        assert_eq!(meta.size as usize, payload.len());

        // list sees it, and only our prefix
        let listed = storage.list(None).await.expect("list");
        assert!(
            listed.iter().any(|item| item.key == key),
            "listed: {:?}",
            listed.iter().map(|i| i.key.as_str()).collect::<Vec<_>>()
        );

        // overwrite is a replace, not an append
        storage.put(&key, b"second").await.expect("overwrite");
        assert_eq!(storage.get(&key).await.expect("get"), b"second");

        // A large object goes through multipart (8 MiB parts); the payload must
        // come back byte-identical, which is what proves the parts were
        // assembled in order with the right ETags.
        let big_key = BlobKey::from_segments(["lfs", "live", "multipart"]).unwrap();
        let _ = storage.delete(&big_key).await;
        let payload: Vec<u8> = (0..(17 * 1024 * 1024u32))
            .map(|i| (i % 251) as u8)
            .collect();
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("big.bin");
        std::fs::write(&path, &payload).expect("write fixture");
        let written = storage
            .put_file(&big_key, &path)
            .await
            .expect("multipart put");
        assert_eq!(written.size, payload.len() as u64);
        let read_back = storage.get(&big_key).await.expect("get multipart");
        assert_eq!(read_back.len(), payload.len());
        assert_eq!(read_back, payload, "multipart assembly changed the bytes");
        assert!(storage.delete(&big_key).await.expect("delete multipart"));

        // delete reports existence correctly
        assert!(storage.delete(&key).await.expect("delete"));
        assert!(!storage.exists(&key).await.expect("gone"));
        assert!(!storage.delete(&key).await.expect("already gone"));
        assert!(matches!(
            storage.get(&key).await,
            Err(BlobStorageError::NotFound(_))
        ));
    }

    /// A presigned URL must carry a signature and point at the object; both
    /// are what makes it usable without credentials.
    #[test]
    fn a_presigned_url_is_signed_and_names_the_object() {
        let storage = S3BlobStorage::new(&config(true)).expect("valid configuration");
        let key = BlobKey::from_segments(["releases", "acme", "v1", "app.tar.gz"]).unwrap();
        let url = storage
            .presigned_url(&key, Duration::from_secs(300))
            .expect("S3 can presign");
        assert!(
            url.contains("releases/acme/v1/app.tar.gz"),
            "url must name the object: {url}"
        );
        assert!(
            url.contains("X-Amz-Signature="),
            "url must be signed: {url}"
        );
        assert!(
            url.contains("X-Amz-Expires=300"),
            "ttl must be honoured: {url}"
        );
    }

    /// The local backend has no such thing, and says so instead of returning a
    /// URL that would not work.
    #[test]
    fn a_local_backend_offers_no_presigned_url() {
        let dir = tempfile::tempdir().unwrap();
        let storage = crate::blob_storage::LocalBlobStorage::new(dir.path());
        let key = BlobKey::new("releases/a/b").unwrap();
        assert!(storage
            .presigned_url(&key, Duration::from_secs(60))
            .is_none());
    }

    #[test]
    fn the_backend_reports_its_name() {
        let storage = S3BlobStorage::new(&config(true)).expect("valid configuration");
        assert_eq!(storage.backend_name(), "s3");
        // Portable callers must be ready for a store with no local path.
        let key = BlobKey::new("lfs/aa/bb").unwrap();
        assert!(storage.local_path(&key).is_none());
    }
}
