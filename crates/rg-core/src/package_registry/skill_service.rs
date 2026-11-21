//! Skill registry service: publish a skill from a Git ref, and read it back.
//!
//! Everything a skill *is* (frontmatter rules, path safety, the deterministic
//! tar) lives in [`super::skill_bundle`]; storage and the package tables are the
//! ordinary package registry's. This module adds the three things that are
//! specific to skills as a registry entry:
//!
//! * **Source.** A version is built **by the server** from the tree of one
//!   commit, so "this version is that commit's `path`" is a fact the server
//!   checked, not something the publisher claimed. The commit, the ref the
//!   publisher named, and the path are recorded in the version's metadata.
//! * **Immutability.** Publishing the same `(skill, version)` again is
//!   idempotent only if it rebuilds to the same content digest; otherwise it is
//!   a conflict. (The generic package `publish` silently returns the existing
//!   version without comparing content, which is wrong for a thing agents will
//!   pin by digest.)
//! * **Resolution.** `resolve` turns a semver constraint into one exact
//!   version plus its digests, ready to be written to a lock file.
//!
//! Nothing here executes or interprets a skill. Text that came from a
//! publisher (`description`, frontmatter, file content) is data; the HTTP layer
//! labels it `origin: user_content`.

use std::path::Path;

use rg_db::entities::{package, package_version};
use rg_git::ops::{TreeFileEntry, TreeFileKind};
use sea_orm::DatabaseConnection;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::service::{package_types, PublishInfo};
use super::skill_bundle::{
    self, Bundle, BundleError, BundleFile, FileEntry, MAX_FILES, MAX_PATH_BYTES,
    MAX_UNPACKED_BYTES, SKILL_MD,
};
use super::storage::PackageStorage;
use super::version_select;

/// Largest single file the text endpoints will return inline. Bigger files are
/// still in the archive.
pub const MAX_INLINE_FILE_BYTES: usize = 1024 * 1024;
/// Names a skill cannot take because they are path segments of the registry's
/// own routes (`…/skills/publish`).
const RESERVED_NAMES: &[&str] = &["publish"];
/// Largest metadata JSON stored with a version, with headroom under MySQL's
/// 65,535-byte `TEXT`.
const MAX_METADATA_BYTES: usize = 48 * 1024;
/// Schema version of [`SkillVersionMetadata`].
const METADATA_SCHEMA: u32 = 1;

// ── Errors ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillErrorKind {
    BadRequest,
    NotFound,
    Conflict,
    TooLarge,
    UnsupportedMedia,
    Internal,
}

/// A failed skill operation with a stable machine-readable `code`.
#[derive(Debug, Clone)]
pub struct SkillError {
    pub kind: SkillErrorKind,
    pub code: &'static str,
    pub message: String,
    pub details: Option<Value>,
}

impl SkillError {
    pub(super) fn new(
        kind: SkillErrorKind,
        code: &'static str,
        message: impl Into<String>,
    ) -> Self {
        Self {
            kind,
            code,
            message: message.into(),
            details: None,
        }
    }

    pub(super) fn with_details(mut self, details: Value) -> Self {
        self.details = Some(details);
        self
    }

    pub(super) fn internal(message: impl std::fmt::Display) -> Self {
        Self::new(SkillErrorKind::Internal, "INTERNAL", message.to_string())
    }
}

impl std::fmt::Display for SkillError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for SkillError {}

impl From<BundleError> for SkillError {
    fn from(error: BundleError) -> Self {
        let kind = match error.code {
            "SKILL_PACK_FAILED" => SkillErrorKind::Internal,
            "SKILL_TOO_LARGE" | "SKILL_TOO_MANY_FILES" => SkillErrorKind::TooLarge,
            _ => SkillErrorKind::BadRequest,
        };
        Self::new(kind, error.code, error.message)
    }
}

impl From<anyhow::Error> for SkillError {
    fn from(error: anyhow::Error) -> Self {
        Self::internal(format!("{error:#}"))
    }
}

impl From<sea_orm::DbErr> for SkillError {
    fn from(error: sea_orm::DbErr) -> Self {
        Self::internal(error)
    }
}

type Result<T> = std::result::Result<T, SkillError>;

// ── Stored shape ─────────────────────────────────────────────────────────

/// Where a version came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillSource {
    /// `"server"`: the registry built the bundle from `commit`'s tree itself.
    /// (Reserved: `"publisher"` for an uploaded bundle; P1 has no upload.)
    pub built_by: String,
    pub repo_id: i64,
    /// The ref the publisher named, verbatim. Only `commit` is authoritative.
    pub git_ref: String,
    /// The commit the ref resolved to when the version was built.
    pub commit: String,
    /// Directory in the tree the skill was built from (`""` = repository root).
    pub path: String,
}

/// Stored in `package_versions.metadata`, a `TEXT` column that holds 64 KiB on
/// MySQL. It therefore carries no per-file data: the file list of a skill with
/// 200 long paths would not fit, and it is cheap to rebuild from the archive,
/// which is the source of truth for it anyway.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillVersionMetadata {
    pub schema: u32,
    /// The whole parsed frontmatter, unknown keys included.
    pub frontmatter: Value,
    /// `sha256:` of the uncompressed tar — what immutability is judged on.
    pub content_digest: String,
    /// `sha256:` of the stored `.tar.gz`, the bytes a client downloads.
    pub archive_digest: String,
    pub unpacked_size: u64,
    pub source: SkillSource,
}

fn parse_metadata(version: &package_version::Model) -> Option<SkillVersionMetadata> {
    serde_json::from_str(version.metadata.as_deref()?).ok()
}

fn archive_filename(name: &str, version: &str) -> String {
    format!("{name}-{version}.tar.gz")
}

// ── Output shapes ────────────────────────────────────────────────────────

/// What the HTTP layer returns. Every text field a publisher wrote
/// (`description`, `frontmatter`, file content) is under `origin: user_content`.
const USER_CONTENT: &str = "user_content";

#[derive(Debug, Clone, Serialize)]
pub struct SkillSummary {
    pub name: String,
    pub description: String,
    pub latest_version: Option<String>,
    pub version_count: usize,
    pub updated_at: String,
    pub origin: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct SkillVersionSummary {
    pub version: String,
    pub content_digest: Option<String>,
    pub archive_digest: Option<String>,
    pub commit: Option<String>,
    pub yanked: bool,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SkillDetail {
    pub name: String,
    pub description: String,
    pub latest_version: Option<String>,
    pub versions: Vec<SkillVersionSummary>,
    pub origin: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct SkillVersionManifest {
    pub name: String,
    pub version: String,
    pub description: String,
    pub frontmatter: Value,
    pub files: Vec<FileEntry>,
    pub content_digest: String,
    pub archive_digest: String,
    pub unpacked_size: u64,
    pub source: SkillSource,
    /// `true` when the registry built this version from the commit itself.
    pub source_verified: bool,
    pub yanked: bool,
    pub created_at: String,
    pub origin: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct ResolvedSkill {
    pub name: String,
    pub constraint: String,
    pub version: String,
    pub content_digest: String,
    pub archive_digest: String,
    pub commit: String,
    pub yanked: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct SkillTextFile {
    pub path: String,
    pub content: String,
    pub size: usize,
    pub sha256: String,
    pub content_digest: String,
    pub origin: &'static str,
}

/// A stored archive and what is recorded about it.
#[derive(Debug, Clone)]
pub struct SkillArchive {
    pub filename: String,
    pub bytes: Vec<u8>,
    pub archive_digest: String,
    pub content_digest: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PublishOutcome {
    pub name: String,
    pub version: String,
    pub content_digest: String,
    pub archive_digest: String,
    pub commit: String,
    /// `true` when this version already existed with the same content.
    pub existing: bool,
    pub package_id: i64,
    pub version_id: i64,
}

// ── Publishing ───────────────────────────────────────────────────────────

pub struct PublishRequest<'a> {
    pub repo_id: i64,
    pub owner: &'a str,
    pub repo_name: &'a str,
    /// The repository's directory on disk.
    pub repo_path: &'a Path,
    /// Directory holding `SKILL.md`; `""` or `"."` for the repository root.
    pub path: &'a str,
    pub git_ref: &'a str,
    /// Defaults to the frontmatter's `metadata.version`; if both are given they
    /// must agree.
    pub version: Option<&'a str>,
    pub author_id: i64,
}

fn valid_git_ref(git_ref: &str) -> bool {
    // A branch, tag or commit id. Deliberately narrower than git's revision
    // syntax: no `^`, `~`, `:`, `@{`, so a ref cannot select a different object
    // than the one named.
    let bytes = git_ref.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 255
        && bytes[0].is_ascii_alphanumeric()
        && bytes
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'/' | b'-'))
        && !git_ref.contains("..")
}

fn normalise_dir(path: &str) -> Result<String> {
    let trimmed = path.trim_matches('/');
    if trimmed.is_empty() || trimmed == "." {
        return Ok(String::new());
    }
    skill_bundle::validate_entry_path(trimmed).map_err(|e| {
        SkillError::new(SkillErrorKind::BadRequest, "SKILL_PATH_INVALID", e.message)
    })?;
    Ok(trimmed.to_string())
}

/// The canonical semver for a version string, or why it is refused.
pub fn canonical_version(version: &str) -> Result<semver::Version> {
    let parsed = semver::Version::parse(version).map_err(|error| {
        SkillError::new(
            SkillErrorKind::BadRequest,
            "SKILL_VERSION_INVALID",
            format!("version {version:?} is not a semantic version: {error}"),
        )
    })?;
    if !parsed.build.is_empty() {
        return Err(SkillError::new(
            SkillErrorKind::BadRequest,
            "SKILL_VERSION_INVALID",
            "build metadata (+...) is not allowed in a skill version: it does not take part in \
             version ordering, so two versions could not be told apart",
        ));
    }
    if parsed.to_string() != version {
        return Err(SkillError::new(
            SkillErrorKind::BadRequest,
            "SKILL_VERSION_INVALID",
            format!("write the version as {parsed}, not {version:?}"),
        ));
    }
    Ok(parsed)
}

fn choose_version(requested: Option<&str>, declared: Option<&str>) -> Result<String> {
    match (requested, declared) {
        (Some(a), Some(b)) if a != b => Err(SkillError::new(
            SkillErrorKind::BadRequest,
            "SKILL_VERSION_MISMATCH",
            format!("the request says version {a:?} but SKILL.md metadata.version is {b:?}"),
        )),
        (Some(v), _) | (None, Some(v)) => {
            canonical_version(v)?;
            Ok(v.to_string())
        }
        (None, None) => Err(SkillError::new(
            SkillErrorKind::BadRequest,
            "SKILL_VERSION_REQUIRED",
            "give a version in the request or set metadata.version in SKILL.md",
        )),
    }
}

/// Read the files under `dir` of `commit`, refusing anything that is not a
/// plain file and anything over the limits *before* reading content.
fn collect_files(repo_path: &Path, commit: &str, dir: &str) -> Result<Vec<BundleFile>> {
    let listing =
        rg_git::ops::list_tree_files(repo_path, commit, dir, MAX_FILES + 1)?.ok_or_else(|| {
            SkillError::new(
                SkillErrorKind::NotFound,
                "SKILL_PATH_NOT_FOUND",
                format!("{dir:?} is not a directory in commit {commit}"),
            )
        })?;
    if listing.truncated || listing.entries.len() > MAX_FILES {
        return Err(SkillError::new(
            SkillErrorKind::TooLarge,
            "SKILL_TOO_MANY_FILES",
            format!("a skill may hold at most {MAX_FILES} files"),
        ));
    }
    let mut total: u64 = 0;
    for entry in &listing.entries {
        match entry.kind {
            TreeFileKind::Symlink | TreeFileKind::Submodule => {
                return Err(SkillError::new(
                    SkillErrorKind::BadRequest,
                    "SKILL_UNSAFE_ENTRY",
                    format!(
                        "{:?} is a {}; a skill may contain only regular files",
                        entry.path,
                        if entry.kind == TreeFileKind::Symlink {
                            "symbolic link"
                        } else {
                            "submodule"
                        }
                    ),
                ));
            }
            TreeFileKind::File | TreeFileKind::Executable => {}
        }
        if entry.path.len() > MAX_PATH_BYTES {
            return Err(SkillError::new(
                SkillErrorKind::BadRequest,
                "SKILL_UNSAFE_ENTRY",
                format!(
                    "path {:?} is longer than {MAX_PATH_BYTES} bytes",
                    entry.path
                ),
            ));
        }
        total = total.saturating_add(entry.size);
        if total > MAX_UNPACKED_BYTES {
            return Err(SkillError::new(
                SkillErrorKind::TooLarge,
                "SKILL_TOO_LARGE",
                format!("a skill may hold at most {MAX_UNPACKED_BYTES} bytes of files"),
            ));
        }
    }
    if !listing.entries.iter().any(|e| e.path == SKILL_MD) {
        return Err(SkillError::new(
            SkillErrorKind::BadRequest,
            "SKILL_MISSING_SKILL_MD",
            format!("{dir:?} has no SKILL.md at its root"),
        ));
    }
    let shas: Vec<String> = listing.entries.iter().map(|e| e.blob_sha.clone()).collect();
    let blobs = rg_git::ops::read_blobs(repo_path, &shas)?;
    Ok(listing
        .entries
        .into_iter()
        .zip(blobs)
        .map(|(entry, data): (TreeFileEntry, Vec<u8>)| BundleFile {
            executable: entry.kind == TreeFileKind::Executable,
            path: entry.path,
            data,
        })
        .collect())
}

/// Build a skill from `path` at `git_ref` and publish it as a new version.
///
/// * Same `(skill, version)` and same content digest → `existing: true`.
/// * Same `(skill, version)` and a different digest → `VERSION_IMMUTABLE`.
pub async fn publish_from_ref(
    db: &DatabaseConnection,
    storage: &PackageStorage,
    request: PublishRequest<'_>,
) -> Result<PublishOutcome> {
    if !valid_git_ref(request.git_ref) {
        return Err(SkillError::new(
            SkillErrorKind::BadRequest,
            "SKILL_REF_INVALID",
            "ref must be a branch name, tag name or commit id",
        ));
    }
    let dir = normalise_dir(request.path)?;

    // Git work is synchronous and can read up to the size limit: keep it off
    // the async workers.
    let repo_path = request.repo_path.to_path_buf();
    let git_ref = request.git_ref.to_string();
    let build_dir = dir.clone();
    let (commit, files) =
        tokio::task::spawn_blocking(move || -> Result<(String, Vec<BundleFile>)> {
            let commit = rg_git::ops::try_rev_parse(&repo_path, &format!("{git_ref}^{{commit}}"))?
                .ok_or_else(|| {
                    SkillError::new(
                        SkillErrorKind::NotFound,
                        "SKILL_REF_NOT_FOUND",
                        format!("ref {git_ref:?} does not name a commit in this repository"),
                    )
                })?;
            let files = collect_files(&repo_path, &commit, &build_dir)?;
            Ok((commit, files))
        })
        .await
        .map_err(|error| SkillError::internal(format!("build task failed: {error}")))??;

    let packed = skill_bundle::pack(files)?;
    let name = packed.frontmatter.name.clone();
    if RESERVED_NAMES.contains(&name.as_str()) {
        return Err(SkillError::new(
            SkillErrorKind::BadRequest,
            "SKILL_NAME_RESERVED",
            format!("{name:?} is reserved by the registry's own routes; choose another skill name"),
        ));
    }
    let version = choose_version(
        request.version,
        packed
            .frontmatter
            .metadata
            .get("version")
            .map(String::as_str),
    )?;

    let metadata = SkillVersionMetadata {
        schema: METADATA_SCHEMA,
        frontmatter: packed.frontmatter.raw.clone(),
        content_digest: packed.content_digest.clone(),
        archive_digest: packed.archive_digest.clone(),
        unpacked_size: packed.unpacked_size,
        source: SkillSource {
            built_by: "server".into(),
            repo_id: request.repo_id,
            git_ref: request.git_ref.to_string(),
            commit: commit.clone(),
            path: dir,
        },
    };
    let metadata_json = serde_json::to_string(&metadata)
        .map_err(|error| SkillError::internal(format!("metadata did not serialize: {error}")))?;
    if metadata_json.len() > MAX_METADATA_BYTES {
        return Err(SkillError::new(
            SkillErrorKind::BadRequest,
            "SKILL_INVALID_FRONTMATTER",
            "the frontmatter is too large to store with the version",
        ));
    }

    let info = PublishInfo {
        owner: request.owner.to_string(),
        repo: request.repo_name.to_string(),
        package_type: package_types::SKILL.to_string(),
        name: name.clone(),
        version: version.clone(),
        semver: Some(version.clone()),
        metadata: Some(metadata_json),
        description: Some(packed.frontmatter.description.clone()),
        homepage: None,
        repository_url: None,
        author_id: request.author_id,
        files: vec![(archive_filename(&name, &version), packed.archive.clone())],
    };

    let outcome = |package_id, version_id, existing| PublishOutcome {
        name: name.clone(),
        version: version.clone(),
        content_digest: packed.content_digest.clone(),
        archive_digest: packed.archive_digest.clone(),
        commit: commit.clone(),
        existing,
        package_id,
        version_id,
    };

    match super::service::publish(db, storage, info).await {
        Ok(result) if !result.existing => Ok(outcome(result.package_id, result.version_id, false)),
        Ok(result) => {
            // The version was already there; the generic publish returned it
            // without looking at content. Look now.
            let stored = rg_db::ops::package_version_ops::find_by_id(db, result.version_id)
                .await?
                .ok_or_else(|| SkillError::internal("the existing version vanished"))?;
            same_content_or_conflict(&stored, &packed.content_digest, &name, &version)?;
            Ok(outcome(result.package_id, result.version_id, true))
        }
        Err(error) => {
            // A concurrent publish of the same version can win the unique index;
            // if so the version exists now and is judged like any other.
            let racing = find_stored_version(db, request.repo_id, &name, &version).await;
            match racing {
                Ok(Some((pkg, stored))) => {
                    same_content_or_conflict(&stored, &packed.content_digest, &name, &version)?;
                    Ok(outcome(pkg.id, stored.id, true))
                }
                _ => Err(SkillError::internal(format!("{error:#}"))),
            }
        }
    }
}

fn same_content_or_conflict(
    stored: &package_version::Model,
    new_digest: &str,
    name: &str,
    version: &str,
) -> Result<()> {
    let existing = parse_metadata(stored);
    if existing.as_ref().map(|m| m.content_digest.as_str()) == Some(new_digest) {
        return Ok(());
    }
    Err(SkillError::new(
        SkillErrorKind::Conflict,
        "VERSION_IMMUTABLE",
        format!(
            "{name} {version} is already published with different content; \
             a published version never changes, publish a new version"
        ),
    )
    .with_details(serde_json::json!({
        "existing_digest": existing.as_ref().map(|m| m.content_digest.clone()),
        "new_digest": new_digest,
        "existing_commit": existing.as_ref().map(|m| m.source.commit.clone()),
    })))
}

// ── Reading ──────────────────────────────────────────────────────────────

async fn find_skill_package(
    db: &DatabaseConnection,
    repo_id: i64,
    name: &str,
) -> Result<Option<package::Model>> {
    let Some(registry) =
        rg_db::ops::package_registry_ops::find_by_repo_and_type(db, repo_id, package_types::SKILL)
            .await?
    else {
        return Ok(None);
    };
    Ok(rg_db::ops::package_ops::find_by_registry_and_name(db, registry.id, name).await?)
}

async fn find_stored_version(
    db: &DatabaseConnection,
    repo_id: i64,
    name: &str,
    version: &str,
) -> Result<Option<(package::Model, package_version::Model)>> {
    let Some(pkg) = find_skill_package(db, repo_id, name).await? else {
        return Ok(None);
    };
    let found =
        rg_db::ops::package_version_ops::find_by_package_and_version(db, pkg.id, version).await?;
    Ok(found.map(|v| (pkg, v)))
}

fn skill_not_found(name: &str) -> SkillError {
    SkillError::new(
        SkillErrorKind::NotFound,
        "SKILL_NOT_FOUND",
        format!("no skill named {name:?} in this repository"),
    )
}

fn version_not_found(name: &str, version: &str) -> SkillError {
    SkillError::new(
        SkillErrorKind::NotFound,
        "SKILL_VERSION_NOT_FOUND",
        format!("skill {name:?} has no version {version:?}"),
    )
}

fn description_of(pkg: &package::Model, latest: Option<&package_version::Model>) -> String {
    latest
        .and_then(parse_metadata)
        .and_then(|m| {
            m.frontmatter
                .get("description")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .or_else(|| pkg.description.clone())
        .unwrap_or_default()
}

/// What a cross-repository listing shows of one version: the digests of its
/// content and of its archive, and whether the server built it from the commit.
pub(super) struct DiscoveryView {
    pub content_digest: String,
    pub archive_digest: String,
    pub source_verified: bool,
    pub description: String,
}

pub(super) fn discovery_view(version: &package_version::Model) -> Option<DiscoveryView> {
    let meta = parse_metadata(version)?;
    Some(DiscoveryView {
        description: meta
            .frontmatter
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        source_verified: meta.source.built_by == "server",
        content_digest: meta.content_digest,
        archive_digest: meta.archive_digest,
    })
}

fn version_summary(v: &package_version::Model) -> SkillVersionSummary {
    let meta = parse_metadata(v);
    SkillVersionSummary {
        version: v.version.clone(),
        content_digest: meta.as_ref().map(|m| m.content_digest.clone()),
        archive_digest: meta.as_ref().map(|m| m.archive_digest.clone()),
        commit: meta.as_ref().map(|m| m.source.commit.clone()),
        yanked: v.is_yanked,
        created_at: v.created_at.to_rfc3339(),
    }
}

/// The skills published in one repository, by name.
pub async fn list_skills(db: &DatabaseConnection, repo_id: i64) -> Result<Vec<SkillSummary>> {
    let Some(registry) =
        rg_db::ops::package_registry_ops::find_by_repo_and_type(db, repo_id, package_types::SKILL)
            .await?
    else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for pkg in rg_db::ops::package_ops::list_by_registry(db, registry.id).await? {
        let versions = rg_db::ops::package_version_ops::list_by_package(db, pkg.id).await?;
        let latest = version_select::pick_latest(&versions);
        out.push(SkillSummary {
            description: description_of(&pkg, latest),
            latest_version: latest.map(|v| v.version.clone()),
            version_count: versions.len(),
            updated_at: versions
                .iter()
                .map(|v| v.created_at)
                .max()
                .unwrap_or(pkg.updated_at)
                .to_rfc3339(),
            name: pkg.name,
            origin: USER_CONTENT,
        });
    }
    Ok(out)
}

/// One skill: its versions, newest first.
pub async fn get_skill(db: &DatabaseConnection, repo_id: i64, name: &str) -> Result<SkillDetail> {
    let pkg = find_skill_package(db, repo_id, name)
        .await?
        .ok_or_else(|| skill_not_found(name))?;
    let versions = rg_db::ops::package_version_ops::list_by_package(db, pkg.id).await?;
    let latest = version_select::pick_latest(&versions);
    Ok(SkillDetail {
        description: description_of(&pkg, latest),
        latest_version: latest.map(|v| v.version.clone()),
        versions: versions.iter().map(version_summary).collect(),
        name: pkg.name,
        origin: USER_CONTENT,
    })
}

async fn stored_version(
    db: &DatabaseConnection,
    repo_id: i64,
    name: &str,
    version: &str,
) -> Result<(package_version::Model, SkillVersionMetadata)> {
    let (_pkg, stored) = find_stored_version(db, repo_id, name, version)
        .await?
        .ok_or_else(|| version_not_found(name, version))?;
    let meta = parse_metadata(&stored).ok_or_else(|| {
        SkillError::internal(format!(
            "skill {name} {version} has no readable skill metadata"
        ))
    })?;
    Ok((stored, meta))
}

/// The manifest of one version. The file list is rebuilt from the archive (the
/// source of truth), which also checks that the stored bundle still matches the
/// digests recorded for it.
pub async fn get_version_manifest(
    db: &DatabaseConnection,
    storage: &PackageStorage,
    repo_id: i64,
    name: &str,
    version: &str,
) -> Result<SkillVersionManifest> {
    let (stored, meta, bundle) = open_bundle(db, storage, repo_id, name, version).await?;
    Ok(SkillVersionManifest {
        name: name.to_string(),
        version: stored.version.clone(),
        description: meta
            .frontmatter
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        frontmatter: meta.frontmatter,
        files: skill_bundle::file_entries(&bundle.files),
        content_digest: meta.content_digest,
        archive_digest: meta.archive_digest,
        unpacked_size: meta.unpacked_size,
        source_verified: meta.source.built_by == "server",
        source: meta.source,
        yanked: stored.is_yanked,
        created_at: stored.created_at.to_rfc3339(),
        origin: USER_CONTENT,
    })
}

/// Turn a constraint into one exact, non-yanked version.
///
/// `constraint` uses Cargo's semantic-version requirement syntax: `^1.2`,
/// `~1.2.3`, `>=1, <2`, `=1.2.3` (a bare `1.2.3` means `^1.2.3`). Empty or
/// `latest` means any version. Pre-releases match only a constraint that names
/// a pre-release itself. Yanked versions are never chosen.
pub async fn resolve(
    db: &DatabaseConnection,
    repo_id: i64,
    name: &str,
    constraint: Option<&str>,
) -> Result<ResolvedSkill> {
    let (text, requirement) =
        version_select::parse_constraint(constraint).map_err(|(text, error)| {
            SkillError::new(
                SkillErrorKind::BadRequest,
                "SKILL_CONSTRAINT_INVALID",
                format!("constraint {text:?} is not a semantic-version requirement: {error}"),
            )
        })?;

    let pkg = find_skill_package(db, repo_id, name)
        .await?
        .ok_or_else(|| skill_not_found(name))?;
    let versions = rg_db::ops::package_version_ops::list_by_package(db, pkg.id).await?;

    let Some(chosen) = version_select::highest_matching(&versions, &requirement) else {
        return Err(SkillError::new(
            SkillErrorKind::NotFound,
            "NO_MATCHING_VERSION",
            format!("no published, non-yanked version of {name:?} satisfies {text:?}"),
        )
        .with_details(serde_json::json!({ "available": version_select::available(&versions) })));
    };
    let meta = parse_metadata(chosen).ok_or_else(|| {
        SkillError::internal(format!(
            "skill {name} {} has no readable skill metadata",
            chosen.version
        ))
    })?;
    Ok(ResolvedSkill {
        name: name.to_string(),
        constraint: text,
        version: chosen.version.clone(),
        content_digest: meta.content_digest,
        archive_digest: meta.archive_digest,
        commit: meta.source.commit,
        yanked: chosen.is_yanked,
    })
}

/// Load a stored archive and check it still matches what was recorded.
async fn load_archive(
    db: &DatabaseConnection,
    storage: &PackageStorage,
    repo_id: i64,
    name: &str,
    version: &str,
) -> Result<(package_version::Model, SkillVersionMetadata, SkillArchive)> {
    let (stored, meta) = stored_version(db, repo_id, name, version).await?;
    let files = rg_db::ops::package_file_ops::list_by_version(db, stored.id).await?;
    let wanted = archive_filename(name, version);
    let file = files.iter().find(|f| f.filename == wanted).ok_or_else(|| {
        SkillError::internal(format!("the archive of {name} {version} is missing"))
    })?;
    let bytes = storage.read_file(&file.storage_path).await?;

    use sha2::{Digest, Sha256};
    let archive_digest = format!("sha256:{}", hex::encode(Sha256::digest(&bytes)));
    if archive_digest != meta.archive_digest {
        return Err(SkillError::internal(format!(
            "the stored archive of {name} {version} does not match its recorded digest"
        )));
    }
    Ok((
        stored,
        meta.clone(),
        SkillArchive {
            filename: wanted,
            bytes,
            archive_digest,
            content_digest: meta.content_digest,
        },
    ))
}

/// The archive of one version, for download. Counts as a download.
pub async fn read_archive(
    db: &DatabaseConnection,
    storage: &PackageStorage,
    repo_id: i64,
    name: &str,
    version: &str,
) -> Result<SkillArchive> {
    let (stored, _meta, archive) = load_archive(db, storage, repo_id, name, version).await?;
    let _ = rg_db::ops::package_version_ops::increment_download_count(db, stored.id).await;
    let _ = rg_db::ops::package_ops::increment_download_count(db, stored.package_id).await;
    Ok(archive)
}

/// Open one version's bundle, checking the unpacked content against the
/// recorded digest as well.
async fn open_bundle(
    db: &DatabaseConnection,
    storage: &PackageStorage,
    repo_id: i64,
    name: &str,
    version: &str,
) -> Result<(package_version::Model, SkillVersionMetadata, Bundle)> {
    let (stored, meta, archive) = load_archive(db, storage, repo_id, name, version).await?;
    let bundle = skill_bundle::read_bundle(&archive.bytes)?;
    if bundle.content_digest != meta.content_digest {
        return Err(SkillError::internal(format!(
            "the unpacked content of {name} {version} does not match its recorded digest"
        )));
    }
    Ok((stored, meta, bundle))
}

fn text_file(bundle: &Bundle, path: &str) -> Result<SkillTextFile> {
    let file = bundle
        .files
        .iter()
        .find(|f| f.path == path)
        .ok_or_else(|| {
            SkillError::new(
                SkillErrorKind::NotFound,
                "SKILL_FILE_NOT_FOUND",
                format!("the bundle has no file {path:?}"),
            )
        })?;
    if file.data.len() > MAX_INLINE_FILE_BYTES {
        return Err(SkillError::new(
            SkillErrorKind::TooLarge,
            "SKILL_FILE_TOO_LARGE",
            format!(
                "{path:?} is larger than {MAX_INLINE_FILE_BYTES} bytes; fetch the archive instead"
            ),
        ));
    }
    let content = std::str::from_utf8(&file.data).map_err(|_| {
        SkillError::new(
            SkillErrorKind::UnsupportedMedia,
            "SKILL_FILE_NOT_TEXT",
            format!("{path:?} is not UTF-8 text; fetch the archive instead"),
        )
    })?;
    use sha2::{Digest, Sha256};
    Ok(SkillTextFile {
        path: file.path.clone(),
        content: content.to_string(),
        size: file.data.len(),
        sha256: hex::encode(Sha256::digest(&file.data)),
        content_digest: bundle.content_digest.clone(),
        origin: USER_CONTENT,
    })
}

/// `SKILL.md` of one version.
pub async fn read_skill_md(
    db: &DatabaseConnection,
    storage: &PackageStorage,
    repo_id: i64,
    name: &str,
    version: &str,
) -> Result<SkillTextFile> {
    let (_, _, bundle) = open_bundle(db, storage, repo_id, name, version).await?;
    text_file(&bundle, SKILL_MD)
}

/// One text file of one version.
pub async fn read_text_file(
    db: &DatabaseConnection,
    storage: &PackageStorage,
    repo_id: i64,
    name: &str,
    version: &str,
    path: &str,
) -> Result<SkillTextFile> {
    skill_bundle::validate_entry_path(path).map_err(|e| {
        SkillError::new(SkillErrorKind::BadRequest, "SKILL_PATH_INVALID", e.message)
    })?;
    let (_, _, bundle) = open_bundle(db, storage, repo_id, name, version).await?;
    text_file(&bundle, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_must_be_canonical_semver_without_build_metadata() {
        assert!(canonical_version("1.2.3").is_ok());
        assert!(canonical_version("1.2.3-rc.1").is_ok());
        for bad in [
            "1.2",
            "v1.2.3",
            "1.2.3+build",
            "01.2.3",
            "",
            "latest",
            " 1.2.3",
        ] {
            assert_eq!(
                canonical_version(bad).unwrap_err().code,
                "SKILL_VERSION_INVALID",
                "{bad:?}"
            );
        }
    }

    #[test]
    fn version_choice_rules() {
        assert_eq!(choose_version(Some("1.0.0"), None).unwrap(), "1.0.0");
        assert_eq!(choose_version(None, Some("2.0.0")).unwrap(), "2.0.0");
        assert_eq!(
            choose_version(Some("1.0.0"), Some("1.0.0")).unwrap(),
            "1.0.0"
        );
        assert_eq!(
            choose_version(Some("1.0.0"), Some("1.0.1"))
                .unwrap_err()
                .code,
            "SKILL_VERSION_MISMATCH"
        );
        assert_eq!(
            choose_version(None, None).unwrap_err().code,
            "SKILL_VERSION_REQUIRED"
        );
    }

    #[test]
    fn refs_cannot_name_a_different_object() {
        for ok in ["main", "v1.2.0", "feature/x-y", "refs/tags/v1", "0123abcd"] {
            assert!(valid_git_ref(ok), "{ok}");
        }
        for bad in [
            "",
            "-x",
            "a^",
            "a~1",
            "a:b",
            "a@{1}",
            "a..b",
            "a b",
            "HEAD^{tree}",
            "/x",
        ] {
            assert!(!valid_git_ref(bad), "{bad:?}");
        }
    }

    /// A repository holding `skill/` built from `files`, `(path, content, mode)`,
    /// committed with the git CLI (identity set locally, none read from the host).
    fn repo_with(dir: &std::path::Path, files: &[(&str, &str, u32)], symlinks: &[(&str, &str)]) {
        use rg_git::cli_gateway::GitCommandGateway;
        let git = GitCommandGateway::new().expect("git CLI available");
        let run = |args: &[&str]| {
            git.run(args, Some(dir)).unwrap().ensure_success().unwrap();
        };
        run(&["init", "-b", "main"]);
        run(&["config", "user.email", "tester@example.com"]);
        run(&["config", "user.name", "Tester"]);
        for (path, content, mode) in files {
            let full = dir.join(path);
            std::fs::create_dir_all(full.parent().unwrap()).unwrap();
            std::fs::write(&full, content).unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&full, std::fs::Permissions::from_mode(*mode)).unwrap();
            }
            let _ = mode;
        }
        #[cfg(unix)]
        for (path, target) in symlinks {
            std::os::unix::fs::symlink(target, dir.join(path)).unwrap();
        }
        let _ = symlinks;
        run(&["add", "-A"]);
        run(&["commit", "-m", "seed"]);
    }

    const MD: &str = "---\nname: demo\ndescription: A demo.\n---\nbody\n";

    #[test]
    fn building_from_a_tree_keeps_the_executable_bit() {
        let temp = tempfile::tempdir().unwrap();
        repo_with(
            temp.path(),
            &[
                ("skill/SKILL.md", MD, 0o644),
                ("skill/run.sh", "#!/bin/sh\n", 0o755),
            ],
            &[],
        );
        let files = collect_files(temp.path(), "main", "skill").unwrap();
        let by = |p: &str| files.iter().find(|f| f.path == p).unwrap();
        assert!(by("run.sh").executable);
        assert!(!by("SKILL.md").executable);
        // Packing it twice from the same tree gives the same digest.
        let a = skill_bundle::pack(files.clone()).unwrap();
        let b = skill_bundle::pack(collect_files(temp.path(), "main", "skill").unwrap()).unwrap();
        assert_eq!(a.content_digest, b.content_digest);
    }

    #[cfg(unix)]
    #[test]
    fn a_symbolic_link_in_the_tree_is_refused_not_followed() {
        let temp = tempfile::tempdir().unwrap();
        repo_with(
            temp.path(),
            &[("skill/SKILL.md", MD, 0o644)],
            &[("skill/secrets", "/etc/passwd")],
        );
        let error = collect_files(temp.path(), "main", "skill").unwrap_err();
        assert_eq!(error.code, "SKILL_UNSAFE_ENTRY");
        assert!(error.message.contains("secrets"), "{}", error.message);
    }

    #[test]
    fn a_directory_without_skill_md_and_a_missing_directory_are_told_apart() {
        let temp = tempfile::tempdir().unwrap();
        repo_with(temp.path(), &[("skill/notes.md", "x", 0o644)], &[]);
        assert_eq!(
            collect_files(temp.path(), "main", "skill")
                .unwrap_err()
                .code,
            "SKILL_MISSING_SKILL_MD"
        );
        assert_eq!(
            collect_files(temp.path(), "main", "elsewhere")
                .unwrap_err()
                .code,
            "SKILL_PATH_NOT_FOUND"
        );
    }

    #[test]
    fn too_many_files_in_the_tree_is_refused() {
        let temp = tempfile::tempdir().unwrap();
        let mut files: Vec<(String, String, u32)> =
            vec![("skill/SKILL.md".into(), MD.into(), 0o644)];
        for i in 0..=MAX_FILES {
            files.push((format!("skill/f{i}.txt"), "x".into(), 0o644));
        }
        let borrowed: Vec<(&str, &str, u32)> = files
            .iter()
            .map(|(a, b, c)| (a.as_str(), b.as_str(), *c))
            .collect();
        repo_with(temp.path(), &borrowed, &[]);
        assert_eq!(
            collect_files(temp.path(), "main", "skill")
                .unwrap_err()
                .code,
            "SKILL_TOO_MANY_FILES"
        );
    }

    #[test]
    fn directory_paths_are_normalised_and_checked() {
        assert_eq!(normalise_dir("").unwrap(), "");
        assert_eq!(normalise_dir(".").unwrap(), "");
        assert_eq!(normalise_dir("/skills/pdf/").unwrap(), "skills/pdf");
        for bad in ["../x", "a/../b", "a//b", ".git/x"] {
            assert_eq!(
                normalise_dir(bad).unwrap_err().code,
                "SKILL_PATH_INVALID",
                "{bad}"
            );
        }
    }
}
