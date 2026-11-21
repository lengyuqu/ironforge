//! MCP server manifest registry: publish a `server.json`, resolve it, read it.
//!
//! The registry keeps the manifest of an MCP server and nothing else: it does
//! not host the server, fetch any URL in the manifest, or look inside the
//! packages it names. So every response carries `ownership: "unverified"`: that
//! the publisher controls `name`, the npm package, or the remote URL is a claim,
//! not something this registry has checked. (The official MCP Registry verifies
//! namespace and package ownership; this one does not, yet.)
//!
//! Versions are immutable the way skill versions are: publishing the same
//! `(name, version)` again is a no-op only if the canonical manifest has the same
//! digest, and a conflict otherwise. Constraint resolution is shared with skills
//! ([`super::version_select`]).
//!
//! The package name stored for an entry is the manifest's `namespace/server`.

use rg_db::entities::{package, package_version};
use sea_orm::DatabaseConnection;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::mcp_manifest::{self, Analysis, EndpointSummary, ManifestError};
use super::service::{package_types, PublishInfo};
use super::skill_service::{SkillError as McpError, SkillErrorKind as McpErrorKind};
use super::storage::PackageStorage;
use super::version_select;

/// The one file stored per version.
const MANIFEST_FILE: &str = "server.json";
/// Schema version of [`McpVersionMetadata`].
const METADATA_SCHEMA: u32 = 1;
/// Always the answer, until the registry verifies something.
const OWNERSHIP_UNVERIFIED: &str = "unverified";
/// Publisher-written text.
const USER_CONTENT: &str = "user_content";

type Result<T> = std::result::Result<T, McpError>;

impl From<ManifestError> for McpError {
    fn from(error: ManifestError) -> Self {
        let kind = if error.too_large {
            McpErrorKind::TooLarge
        } else {
            McpErrorKind::BadRequest
        };
        McpError::new(kind, error.code, error.message)
    }
}

/// Stored in `package_versions.metadata` (a `TEXT` column: 64 KiB on MySQL), so
/// it holds only small fixed-size facts. The manifest itself is the stored file.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct McpVersionMetadata {
    schema: u32,
    /// `sha256:` of the canonical manifest bytes: what immutability is judged on
    /// and what a client downloading the file can check.
    digest: String,
    size: u64,
    declared_schema: Option<String>,
}

fn parse_metadata(version: &package_version::Model) -> Option<McpVersionMetadata> {
    serde_json::from_str(version.metadata.as_deref()?).ok()
}

// ── Output shapes ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct McpServerSummary {
    pub name: String,
    pub description: String,
    pub latest_version: Option<String>,
    pub version_count: usize,
    pub updated_at: String,
    pub ownership: &'static str,
    pub origin: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct McpVersionSummary {
    pub version: String,
    pub digest: Option<String>,
    pub yanked: bool,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct McpServerDetail {
    pub name: String,
    pub description: String,
    pub latest_version: Option<String>,
    pub versions: Vec<McpVersionSummary>,
    pub ownership: &'static str,
    pub origin: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct McpVersionManifest {
    pub name: String,
    pub version: String,
    pub description: String,
    /// The manifest as stored: the canonical form of what was published.
    pub manifest: Value,
    pub digest: String,
    pub size: u64,
    pub declared_schema: Option<String>,
    /// What a client can run or call, read off the manifest. `None` only if a
    /// manifest stored under older rules no longer passes today's checks; the
    /// manifest itself is still returned, digest-checked.
    pub endpoints: Option<EndpointSummary>,
    pub ownership: &'static str,
    pub yanked: bool,
    pub created_at: String,
    pub origin: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct ResolvedMcpServer {
    pub name: String,
    pub constraint: String,
    pub version: String,
    pub digest: String,
    pub yanked: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct McpPublishOutcome {
    pub name: String,
    pub version: String,
    pub digest: String,
    /// `true` when this version already existed with the same manifest.
    pub existing: bool,
    pub ownership: &'static str,
    pub package_id: i64,
    pub version_id: i64,
}

// ── Publishing ───────────────────────────────────────────────────────────

pub struct PublishRequest<'a> {
    pub owner: &'a str,
    pub repo_name: &'a str,
    pub repo_id: i64,
    pub manifest: &'a Value,
    pub author_id: i64,
}

/// Publish `manifest` as a new version of the server it names.
///
/// * Same `(name, version)` and same digest → `existing: true`.
/// * Same `(name, version)` and a different digest → `VERSION_IMMUTABLE`.
pub async fn publish(
    db: &DatabaseConnection,
    storage: &PackageStorage,
    request: PublishRequest<'_>,
) -> Result<McpPublishOutcome> {
    let analysis = mcp_manifest::analyze(request.manifest)?;
    let Analysis {
        name,
        version,
        description,
        declared_schema,
        canonical,
        digest,
        ..
    } = analysis;

    let metadata = McpVersionMetadata {
        schema: METADATA_SCHEMA,
        digest: digest.clone(),
        size: canonical.len() as u64,
        declared_schema,
    };
    let metadata_json = serde_json::to_string(&metadata)
        .map_err(|error| McpError::internal(format!("metadata did not serialize: {error}")))?;

    let info = PublishInfo {
        owner: request.owner.to_string(),
        repo: request.repo_name.to_string(),
        package_type: package_types::MCP_SERVER.to_string(),
        name: name.clone(),
        version: version.clone(),
        semver: Some(version.clone()),
        metadata: Some(metadata_json),
        description: Some(description),
        homepage: None,
        repository_url: None,
        author_id: request.author_id,
        files: vec![(MANIFEST_FILE.to_string(), canonical)],
    };

    let outcome = |package_id, version_id, existing| McpPublishOutcome {
        name: name.clone(),
        version: version.clone(),
        digest: digest.clone(),
        existing,
        ownership: OWNERSHIP_UNVERIFIED,
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
                .ok_or_else(|| McpError::internal("the existing version vanished"))?;
            same_content_or_conflict(&stored, &digest, &name, &version)?;
            Ok(outcome(result.package_id, result.version_id, true))
        }
        Err(error) => {
            // A concurrent publish of the same version can win the unique index;
            // if so the version exists now and is judged like any other.
            match find_stored_version(db, request.repo_id, &name, &version).await {
                Ok(Some((pkg, stored))) => {
                    same_content_or_conflict(&stored, &digest, &name, &version)?;
                    Ok(outcome(pkg.id, stored.id, true))
                }
                _ => Err(McpError::internal(format!("{error:#}"))),
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
    if existing.as_ref().map(|m| m.digest.as_str()) == Some(new_digest) {
        return Ok(());
    }
    Err(McpError::new(
        McpErrorKind::Conflict,
        "VERSION_IMMUTABLE",
        format!(
            "{name} {version} is already published with a different manifest; \
             a published version never changes, publish a new version"
        ),
    )
    .with_details(serde_json::json!({
        "existing_digest": existing.as_ref().map(|m| m.digest.clone()),
        "new_digest": new_digest,
    })))
}

// ── Reading ──────────────────────────────────────────────────────────────

async fn find_package(
    db: &DatabaseConnection,
    repo_id: i64,
    name: &str,
) -> Result<Option<package::Model>> {
    let Some(registry) = rg_db::ops::package_registry_ops::find_by_repo_and_type(
        db,
        repo_id,
        package_types::MCP_SERVER,
    )
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
    let Some(pkg) = find_package(db, repo_id, name).await? else {
        return Ok(None);
    };
    let found =
        rg_db::ops::package_version_ops::find_by_package_and_version(db, pkg.id, version).await?;
    Ok(found.map(|v| (pkg, v)))
}

fn not_found(name: &str) -> McpError {
    McpError::new(
        McpErrorKind::NotFound,
        "MCP_SERVER_NOT_FOUND",
        format!("no MCP server named {name:?} in this repository"),
    )
}

fn version_not_found(name: &str, version: &str) -> McpError {
    McpError::new(
        McpErrorKind::NotFound,
        "MCP_VERSION_NOT_FOUND",
        format!("MCP server {name:?} has no version {version:?}"),
    )
}

fn version_summary(v: &package_version::Model) -> McpVersionSummary {
    McpVersionSummary {
        version: v.version.clone(),
        digest: parse_metadata(v).map(|m| m.digest),
        yanked: v.is_yanked,
        created_at: v.created_at.to_rfc3339(),
    }
}

/// The MCP servers published in one repository, by name.
pub async fn list(db: &DatabaseConnection, repo_id: i64) -> Result<Vec<McpServerSummary>> {
    let Some(registry) = rg_db::ops::package_registry_ops::find_by_repo_and_type(
        db,
        repo_id,
        package_types::MCP_SERVER,
    )
    .await?
    else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for pkg in rg_db::ops::package_ops::list_by_registry(db, registry.id).await? {
        let versions = rg_db::ops::package_version_ops::list_by_package(db, pkg.id).await?;
        let latest = version_select::pick_latest(&versions);
        out.push(McpServerSummary {
            description: pkg.description.clone().unwrap_or_default(),
            latest_version: latest.map(|v| v.version.clone()),
            version_count: versions.len(),
            updated_at: versions
                .iter()
                .map(|v| v.created_at)
                .max()
                .unwrap_or(pkg.updated_at)
                .to_rfc3339(),
            name: pkg.name,
            ownership: OWNERSHIP_UNVERIFIED,
            origin: USER_CONTENT,
        });
    }
    Ok(out)
}

/// One server: its versions.
pub async fn get(db: &DatabaseConnection, repo_id: i64, name: &str) -> Result<McpServerDetail> {
    let pkg = find_package(db, repo_id, name)
        .await?
        .ok_or_else(|| not_found(name))?;
    let versions = rg_db::ops::package_version_ops::list_by_package(db, pkg.id).await?;
    let latest = version_select::pick_latest(&versions);
    Ok(McpServerDetail {
        description: pkg.description.clone().unwrap_or_default(),
        latest_version: latest.map(|v| v.version.clone()),
        versions: versions.iter().map(version_summary).collect(),
        name: pkg.name,
        ownership: OWNERSHIP_UNVERIFIED,
        origin: USER_CONTENT,
    })
}

/// The manifest of one version, read from storage and checked against the
/// digest recorded when it was published.
pub async fn get_version(
    db: &DatabaseConnection,
    storage: &PackageStorage,
    repo_id: i64,
    name: &str,
    version: &str,
) -> Result<McpVersionManifest> {
    let (_pkg, stored) = find_stored_version(db, repo_id, name, version)
        .await?
        .ok_or_else(|| version_not_found(name, version))?;
    let meta = parse_metadata(&stored).ok_or_else(|| {
        McpError::internal(format!(
            "MCP server {name} {version} has no readable metadata"
        ))
    })?;

    let files = rg_db::ops::package_file_ops::list_by_version(db, stored.id).await?;
    let file = files
        .iter()
        .find(|f| f.filename == MANIFEST_FILE)
        .ok_or_else(|| {
            McpError::internal(format!("the manifest of {name} {version} is missing"))
        })?;
    let bytes = storage.read_file(&file.storage_path).await?;
    let digest = format!("sha256:{}", hex::encode(Sha256::digest(&bytes)));
    if digest != meta.digest {
        return Err(McpError::internal(format!(
            "the stored manifest of {name} {version} does not match its recorded digest"
        )));
    }
    let manifest: Value = serde_json::from_slice(&bytes).map_err(|error| {
        McpError::internal(format!("the stored manifest of {name} {version}: {error}"))
    })?;
    let description = manifest
        .get("description")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let endpoints = mcp_manifest::analyze(&manifest).ok().map(|a| a.endpoints);

    Ok(McpVersionManifest {
        name: name.to_string(),
        version: stored.version.clone(),
        description,
        manifest,
        digest: meta.digest,
        size: meta.size,
        declared_schema: meta.declared_schema,
        endpoints,
        ownership: OWNERSHIP_UNVERIFIED,
        yanked: stored.is_yanked,
        created_at: stored.created_at.to_rfc3339(),
        origin: USER_CONTENT,
    })
}

/// Turn a constraint into one exact, non-yanked version, with its digest.
///
/// `constraint` uses Cargo's semantic-version requirement syntax; empty or
/// `latest` means any version. Pre-releases match only a constraint that names
/// one. Yanked versions are never chosen.
pub async fn resolve(
    db: &DatabaseConnection,
    repo_id: i64,
    name: &str,
    constraint: Option<&str>,
) -> Result<ResolvedMcpServer> {
    let (text, requirement) =
        version_select::parse_constraint(constraint).map_err(|(text, error)| {
            McpError::new(
                McpErrorKind::BadRequest,
                "MCP_CONSTRAINT_INVALID",
                format!("constraint {text:?} is not a semantic-version requirement: {error}"),
            )
        })?;
    let pkg = find_package(db, repo_id, name)
        .await?
        .ok_or_else(|| not_found(name))?;
    let versions = rg_db::ops::package_version_ops::list_by_package(db, pkg.id).await?;

    let Some(chosen) = version_select::highest_matching(&versions, &requirement) else {
        return Err(McpError::new(
            McpErrorKind::NotFound,
            "NO_MATCHING_VERSION",
            format!("no published, non-yanked version of {name:?} satisfies {text:?}"),
        )
        .with_details(serde_json::json!({ "available": version_select::available(&versions) })));
    };
    let meta = parse_metadata(chosen).ok_or_else(|| {
        McpError::internal(format!(
            "MCP server {name} {} has no readable metadata",
            chosen.version
        ))
    })?;
    Ok(ResolvedMcpServer {
        name: name.to_string(),
        constraint: text,
        version: chosen.version.clone(),
        digest: meta.digest,
        yanked: chosen.is_yanked,
    })
}
