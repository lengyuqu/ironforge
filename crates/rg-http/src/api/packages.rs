//! Package Registry REST API.
//!
//! == Generic REST endpoints ==
//! POST   /api/v1/repos/:owner/:name/packages/:type/publish   — upload package
//! GET    /api/v1/repos/:owner/:name/packages/:type/list       — list packages
//! GET    /api/v1/repos/:owner/:name/packages/:type/:pkg       — get package detail
//! GET    /api/v1/repos/:owner/:name/packages/:type/:pkg/versions — list versions
//! GET    /api/v1/repos/:owner/:name/packages/:type/:pkg/:ver  — get version
//! DELETE /api/v1/repos/:owner/:name/packages/:type/:pkg/:ver  — delete version
//! PATCH  /api/v1/repos/:owner/:name/packages/:type/:pkg/:ver/yank — yank/unyank
//! GET    /api/v1/repos/:owner/:name/packages/:type/:pkg/:ver/:file — download file
//! GET    /api/v1/repos/{owner}/{repo}/packages                — list registries
//!
//! == Protocol-specific endpoints ==
//! GET    /api/v1/repos/{owner}/{repo}/packages/cargo/index/{pkg}  — Cargo sparse index
//! GET    /api/v1/repos/{owner}/{repo}/packages/npm/{pkg}          — npm registry metadata

use axum::{
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::api::auth::extract_user_id;
use crate::AppState;

// ── Request / Response types ─────────────────────────────

#[derive(Deserialize, ToSchema)]
pub struct PublishPackageQuery {
    /// Package name (can be auto-extracted by adapter if the file is a known format).
    #[serde(default)]
    pub name: Option<String>,
    /// Package version (can be auto-extracted by adapter if the file is a known format).
    #[serde(default)]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repository_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub semver: Option<String>,
}

#[derive(Deserialize, ToSchema)]
pub struct YankRequest {
    pub yank: bool,
}

#[derive(Serialize)]
pub struct PublishResponse {
    pub package_id: i64,
    pub version_id: i64,
    pub existing: bool,
}

#[derive(Serialize)]
pub struct PackageListResponse {
    pub packages: Vec<rg_core::package_registry::PackageSummary>,
}

#[derive(Serialize)]
pub struct VersionListResponse {
    pub versions: Vec<rg_core::package_registry::VersionDetail>,
}

#[derive(Serialize)]
pub struct RegistryListResponse {
    pub registries: Vec<RegistryEntry>,
}

#[derive(Serialize)]
pub struct RegistryEntry {
    pub package_type: String,
    pub enabled: bool,
}

/// Helper: generate a JSON error response.
fn err(status: StatusCode, msg: &str) -> axum::response::Response {
    (status, Json(serde_json::json!({"error": msg}))).into_response()
}

/// Helper: plain-text error response.
fn err_text(status: StatusCode, msg: &str) -> axum::response::Response {
    (status, [(header::CONTENT_TYPE, "text/plain; charset=utf-8")], msg.to_string()).into_response()
}

/// Helper: extract authenticated user from headers.
fn auth(headers: &axum::http::HeaderMap, secret: &str) -> Result<i64, axum::response::Response> {
    extract_user_id(headers, secret).ok_or_else(|| {
        err(StatusCode::UNAUTHORIZED, "authentication required")
    })
}

/// Resolve publish metadata: adapter-extracted fields take precedence, then
/// query-param overrides.
fn resolve_publish_info(
    query: &PublishPackageQuery,
    adapter_meta: Option<rg_core::package_registry::ExtractedMetadata>,
) -> Result<(String, String, Option<String>, Option<String>, Option<String>, Option<String>), String> {
    // If adapter extracted metadata, use it as base; query params override.
    if let Some(meta) = adapter_meta {
        let name = query.name.clone().unwrap_or(meta.name);
        let version = query.version.clone().unwrap_or(meta.version);
        let description = query.description.clone().or(meta.description);
        let homepage = query.homepage.clone().or(meta.homepage);
        let repository_url = query.repository_url.clone().or(meta.repository_url);
        let semver = query.semver.clone().or(meta.semver);
        if name.is_empty() || version.is_empty() {
            return Err("package name and version are required (could not be auto-extracted)".into());
        }
        return Ok((name, version, description, homepage, repository_url, semver));
    }

    // No adapter extraction — must be in query params.
    let name = query.name.clone().ok_or("package name is required")?;
    let version = query.version.clone().ok_or("package version is required")?;
    Ok((
        name,
        version,
        query.description.clone(),
        query.homepage.clone(),
        query.repository_url.clone(),
        query.semver.clone(),
    ))
}

// ── Generic REST route handlers ──────────────────────────

/// POST /api/v1/repos/:owner/:name/packages/:type/publish
/// Upload a package file.  Name/version are auto-extracted from known
/// package formats (Cargo, npm) if not provided in query params.
pub async fn publish(
    State(state): State<AppState>,
    Path((owner, name, pkg_type)): Path<(String, String, String)>,
    Query(query): Query<PublishPackageQuery>,
    headers: axum::http::HeaderMap,
    body: axum::body::Bytes,
) -> axum::response::Response {
    let user_id = match auth(&headers, &state.jwt_secret) {
        Ok(id) => id,
        Err(e) => return e,
    };

    if !rg_core::package_registry::package_types::is_valid(&pkg_type) {
        return err(StatusCode::BAD_REQUEST, &format!("unsupported package type: {}", pkg_type));
    }

    let filename = headers
        .get(header::CONTENT_DISPOSITION)
        .and_then(|v| v.to_str().ok())
        .and_then(parse_filename_from_disposition)
        .unwrap_or_else(|| "package".to_string());

    // Try to auto-extract metadata via the adapter
    let adapter = rg_core::package_registry::get_adapter(&pkg_type);
    let adapter_meta = if let Some(ref a) = adapter {
        a.extract_metadata(&filename, &body).ok()
    } else {
        None
    };

    let (pkg_name, pkg_version, description, homepage, repository_url, semver) =
        match resolve_publish_info(&query, adapter_meta) {
            Ok(v) => v,
            Err(msg) => return err(StatusCode::BAD_REQUEST, &msg),
        };

    let storage = rg_core::package_registry::PackageStorage::new(&*state.repo_root);

    let info = rg_core::package_registry::PublishInfo {
        owner,
        repo: name,
        package_type: pkg_type,
        name: pkg_name,
        version: pkg_version,
        semver,
        metadata: None,
        description,
        homepage,
        repository_url,
        author_id: user_id,
        files: vec![(filename, body.to_vec())],
    };

    match rg_core::package_registry::service::publish(&state.db, &storage, info).await {
        Ok(result) => {
            let status = if result.existing { StatusCode::OK } else { StatusCode::CREATED };
            (status, Json(PublishResponse {
                package_id: result.package_id,
                version_id: result.version_id,
                existing: result.existing,
            })).into_response()
        }
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, &format!("{e:#}")),
    }
}

/// GET /api/v1/repos/:owner/:name/packages
pub async fn list_registries(
    State(state): State<AppState>,
    Path((owner, name)): Path<(String, String)>,
) -> axum::response::Response {
    let repo = match rg_core::repo::service::find_repo_by_owner_name(&state.db, &owner, &name).await {
        Ok(Some(r)) => r,
        Ok(None) => return err(StatusCode::NOT_FOUND, "repository not found"),
        Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, &format!("{e:#}")),
    };

    match rg_db::ops::package_registry_ops::list_by_repo(&state.db, repo.id).await {
        Ok(registries) => Json(RegistryListResponse {
            registries: registries
                .into_iter()
                .map(|r| RegistryEntry {
                    package_type: r.package_type,
                    enabled: r.enabled,
                })
                .collect(),
        }).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, &format!("{e:#}")),
    }
}

/// GET /api/v1/repos/:owner/:name/packages/:type/list
pub async fn list_packages(
    State(state): State<AppState>,
    Path((owner, name, pkg_type)): Path<(String, String, String)>,
) -> axum::response::Response {
    match rg_core::package_registry::service::list_packages(&state.db, &owner, &name, &pkg_type).await {
        Ok(packages) => Json(PackageListResponse { packages }).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, &format!("{e:#}")),
    }
}

/// GET /api/v1/repos/:owner/:name/packages/:type/:pkg
pub async fn get_package(
    State(state): State<AppState>,
    Path((owner, name, pkg_type, pkg_name)): Path<(String, String, String, String)>,
) -> axum::response::Response {
    match rg_core::package_registry::service::get_package(&state.db, &owner, &name, &pkg_type, &pkg_name).await {
        Ok(detail) => Json(detail).into_response(),
        Err(e) => err(StatusCode::NOT_FOUND, &format!("{e:#}")),
    }
}

/// GET /api/v1/repos/:owner/:name/packages/:type/:pkg/versions
pub async fn list_versions(
    State(state): State<AppState>,
    Path((owner, name, pkg_type, pkg_name)): Path<(String, String, String, String)>,
) -> axum::response::Response {
    match rg_core::package_registry::service::list_versions(&state.db, &owner, &name, &pkg_type, &pkg_name).await {
        Ok(versions) => Json(VersionListResponse { versions }).into_response(),
        Err(e) => err(StatusCode::NOT_FOUND, &format!("{e:#}")),
    }
}

/// GET /api/v1/repos/:owner/:name/packages/:type/:pkg/:ver
pub async fn get_version(
    State(state): State<AppState>,
    Path((owner, name, pkg_type, pkg_name, version)): Path<(String, String, String, String, String)>,
) -> axum::response::Response {
    match rg_core::package_registry::service::get_version(&state.db, &owner, &name, &pkg_type, &pkg_name, &version).await {
        Ok(detail) => Json(detail).into_response(),
        Err(e) => err(StatusCode::NOT_FOUND, &format!("{e:#}")),
    }
}

/// DELETE /api/v1/repos/:owner/:name/packages/:type/:pkg/:ver
pub async fn delete_version(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path((owner, name, pkg_type, pkg_name, version)): Path<(String, String, String, String, String)>,
) -> axum::response::Response {
    let _need_auth = match auth(&headers, &state.jwt_secret) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let storage = rg_core::package_registry::PackageStorage::new(&*state.repo_root);

    match rg_core::package_registry::service::delete_version(
        &state.db, &storage, &owner, &name, &pkg_type, &pkg_name, &version,
    ).await {
        Ok(_) => (StatusCode::NO_CONTENT,).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, &format!("{e:#}")),
    }
}

/// PATCH /api/v1/repos/:owner/:name/packages/:type/:pkg/:ver/yank
pub async fn yank_version(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path((owner, name, pkg_type, pkg_name, version)): Path<(String, String, String, String, String)>,
    Json(body): Json<YankRequest>,
) -> axum::response::Response {
    let _need_auth = match auth(&headers, &state.jwt_secret) {
        Ok(c) => c,
        Err(e) => return e,
    };

    match rg_core::package_registry::service::yank_version(
        &state.db, &owner, &name, &pkg_type, &pkg_name, &version, body.yank,
    ).await {
        Ok(_) => (StatusCode::OK, Json(serde_json::json!({"yanked": body.yank}))).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, &format!("{e:#}")),
    }
}

/// GET /api/v1/repos/:owner/:name/packages/:type/:pkg/:ver/:file
pub async fn download_file(
    State(state): State<AppState>,
    Path((owner, name, pkg_type, pkg_name, version, filename)): Path<(String, String, String, String, String, String)>,
) -> axum::response::Response {
    let storage = rg_core::package_registry::PackageStorage::new(&*state.repo_root);

    match rg_core::package_registry::service::download_file(
        &state.db, &storage, &owner, &name, &pkg_type, &pkg_name, &version, &filename,
    ).await {
        Ok((data, content_type, _size)) => {
            (
                StatusCode::OK,
                [
                    (header::CONTENT_TYPE, content_type),
                    (
                        header::CONTENT_DISPOSITION,
                        format!("attachment; filename=\"{}\"", filename),
                    ),
                ],
                data,
            ).into_response()
        }
        Err(e) => err(StatusCode::NOT_FOUND, &format!("{e:#}")),
    }
}

// ── Protocol-specific endpoints ──────────────────────────

/// GET /api/v1/repos/:owner/:name/packages/cargo/index/:pkg
///
/// Cargo sparse index protocol (RFC 2789 / Cargo ≥ 1.68).
/// Returns line-delimited JSON, one line per version.
pub async fn cargo_sparse_index(
    State(state): State<AppState>,
    Path((owner, name, pkg_name)): Path<(String, String, String)>,
) -> axum::response::Response {
    let versions = match rg_core::package_registry::service::list_versions(
        &state.db, &owner, &name, "cargo", &pkg_name,
    ).await {
        Ok(v) => v,
        Err(e) => return err_text(StatusCode::NOT_FOUND, &format!("{e:#}")),
    };

    let entries: Vec<(&str, Option<&str>, bool)> = versions
        .iter()
        .map(|v| (v.version.as_str(), v.sha256.as_deref(), v.is_yanked))
        .collect();

    let body = rg_core::package_registry::build_sparse_index(&pkg_name, &entries);

    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "text/plain; charset=utf-8"),
            ("x-cargo-registry-type".parse().unwrap(), "sparse"),
        ],
        body,
    ).into_response()
}

/// GET /api/v1/repos/:owner/:name/packages/npm/:pkg_name
///
/// npm registry "abbreviated" metadata protocol.
/// Returns JSON with dist-tags and versions.
pub async fn npm_registry_metadata(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path((owner, name, pkg_name)): Path<(String, String, String)>,
) -> axum::response::Response {
    let versions = match rg_core::package_registry::service::list_versions(
        &state.db, &owner, &name, "npm", &pkg_name,
    ).await {
        Ok(v) => v,
        Err(e) => return err(StatusCode::NOT_FOUND, &format!("{e:#}")),
    };

    // Determine base URL from request host header
    let base_url = headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .map(|host| {
            let scheme = if host.starts_with("localhost") || host.starts_with("127.") {
                "http"
            } else {
                "https"
            };
            format!("{}://{}", scheme, host)
        })
        .unwrap_or_else(|| "http://localhost".into());

    let npm_versions: Vec<rg_core::package_registry::NpmVersionInfo> = versions
        .iter()
        .map(|v| {
            // Find the tgz file
            let tgz_file = v.files.iter().find(|f| {
                f.filename.ends_with(".tgz") || f.filename.ends_with(".tar.gz")
            });

            rg_core::package_registry::NpmVersionInfo {
                version: v.version.clone(),
                description: None, // Version-level descriptions come from package detail
                sha256: v.sha256.clone(),
                filename: tgz_file.map(|f| f.filename.clone()),
                yanked: v.is_yanked,
            }
        })
        .collect();

    let metadata = rg_core::package_registry::build_npm_metadata(
        &pkg_name,
        &npm_versions,
        &base_url,
        &owner,
        &name,
    );

    (StatusCode::OK, Json(metadata)).into_response()
}

// ── helpers ───────────────────────────────────────────────

fn parse_filename_from_disposition(disposition: &str) -> Option<String> {
    for part in disposition.split(';') {
        let part = part.trim();
        if let Some(val) = part.strip_prefix("filename=") {
            return Some(val.trim_matches('"').to_string());
        }
        if let Some(val) = part.strip_prefix("filename*=") {
            if let Some(idx) = val.find("''") {
                let encoded = &val[idx + 2..];
                if let Ok(decoded) = percent_decode(encoded) {
                    return Some(decoded);
                }
            }
        }
    }
    None
}

fn percent_decode(s: &str) -> Result<String, ()> {
    let mut result = Vec::with_capacity(s.len());
    let mut chars = s.bytes();
    while let Some(b) = chars.next() {
        if b == b'%' {
            let hi = chars.next().ok_or(())?;
            let lo = chars.next().ok_or(())?;
            let hi = hex_val(hi)?;
            let lo = hex_val(lo)?;
            result.push((hi << 4) | lo);
        } else {
            result.push(b);
        }
    }
    String::from_utf8(result).map_err(|_| ())
}

fn hex_val(b: u8) -> Result<u8, ()> {
    match b {
        b'0'..=b'9' => Ok(b - b'0'),
        b'A'..=b'F' => Ok(b - b'A' + 10),
        b'a'..=b'f' => Ok(b - b'a' + 10),
        _ => Err(()),
    }
}
