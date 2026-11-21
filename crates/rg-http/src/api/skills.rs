//! Skill registry REST API: publish a skill from a Git ref, and read it back in
//! shapes an agent can use directly.
//!
//! ```text
//! POST /repos/{o}/{n}/skills/publish                                  build + publish from a ref
//! GET  /repos/{o}/{n}/skills                                          skills in this repository
//! GET  /repos/{o}/{n}/skills/{skill}                                  versions of one skill
//! GET  /repos/{o}/{n}/skills/{skill}/resolve?constraint=^1.2          constraint -> exact version + digests
//! GET  /repos/{o}/{n}/skills/{skill}/versions/{v}                     manifest
//! GET  /repos/{o}/{n}/skills/{skill}/versions/{v}/content             SKILL.md text
//! GET  /repos/{o}/{n}/skills/{skill}/versions/{v}/files/{*path}       one text file
//! GET  /repos/{o}/{n}/skills/{skill}/versions/{v}/archive             the .tar.gz
//! ```
//!
//! Every text a publisher wrote (`description`, `frontmatter`, file content) is
//! returned under `origin: "user_content"`: it is data to show or to be
//! reviewed, never an instruction to the registry or to the client's runtime.
//!
//! Permission: reads need repository read access (and, for a CI job token,
//! `packages:read`), publishing needs repository write access. Publishing moves
//! no branch or tag, so it carries no branch declaration in the route metadata:
//! a token limited to some branches is refused.

use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use rg_core::package_registry::skill_service::{self, PublishRequest, SkillError, SkillErrorKind};
use rg_core::package_registry::PackageStorage;
use serde::Deserialize;

use crate::error::AppError;
use crate::AppState;

/// Map a registry failure to an HTTP error. A server-side failure keeps its
/// detail in the log only: it can carry storage paths.
pub(super) fn app_error(error: SkillError) -> AppError {
    let status = match error.kind {
        SkillErrorKind::BadRequest => StatusCode::BAD_REQUEST,
        SkillErrorKind::NotFound => StatusCode::NOT_FOUND,
        SkillErrorKind::Conflict => StatusCode::CONFLICT,
        SkillErrorKind::TooLarge => StatusCode::PAYLOAD_TOO_LARGE,
        SkillErrorKind::UnsupportedMedia => StatusCode::UNSUPPORTED_MEDIA_TYPE,
        SkillErrorKind::Internal => {
            tracing::error!(code = error.code, detail = %error.message, "skill registry failure");
            return AppError::internal("internal error");
        }
    };
    let conflict = error.kind == SkillErrorKind::Conflict;
    let mut mapped = AppError::coded(status, error.code, error.message);
    if let Some(details) = error.details {
        mapped = mapped.with_details(details);
    }
    if conflict {
        mapped = mapped.with_reason(
            error.code,
            false,
            "publish the changed content under a new version",
        );
    }
    mapped
}

pub(super) async fn require_read(
    state: &AppState,
    headers: &HeaderMap,
    owner: &str,
    name: &str,
) -> Result<rg_db::entities::repository::Model, AppError> {
    super::repo_content::validate_owner_repo(owner, name)?;
    super::repo_access::require_read_with_ci_scope(state, headers, owner, name, "packages:read")
        .await
}

pub(super) fn storage(state: &AppState) -> PackageStorage {
    PackageStorage::from_backend(state.blob_storage.clone())
}

// ── Publish ──────────────────────────────────────────────────────────────

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PublishSkillRequest {
    /// Directory holding `SKILL.md`; omit or `""` for the repository root.
    #[serde(default)]
    pub path: String,
    /// Branch, tag or commit id to build from. The commit it resolves to is
    /// what is recorded; the name is kept only as a label.
    #[serde(rename = "ref")]
    pub git_ref: String,
    /// Semantic version. Defaults to `metadata.version` in `SKILL.md`; if both
    /// are given they must agree.
    #[serde(default)]
    pub version: Option<String>,
}

/// POST /repos/{owner}/{name}/skills/publish
///
/// Builds a deterministic bundle from the tree of the named ref and publishes
/// it as a new version. Publishing the same version again with the same content
/// is a no-op (200); with different content it is refused (409
/// `VERSION_IMMUTABLE`).
#[utoipa::path(
    post,
    path = "/repos/{owner}/{name}/skills/publish",
    tag = "Skills",
    params(
        ("owner" = String, Path, description = "owner"),
        ("name" = String, Path, description = "repo name"),
    ),
    request_body = PublishSkillRequest,
    responses(
        (status = 201, description = "Published a new version", body = serde_json::Value),
        (status = 200, description = "That version already exists with the same content", body = serde_json::Value),
        (status = 400, description = "Not a valid skill", body = serde_json::Value),
        (status = 404, description = "Ref or path not found", body = serde_json::Value),
        (status = 409, description = "VERSION_IMMUTABLE", body = serde_json::Value),
        (status = 413, description = "Over the size limits", body = serde_json::Value),
    ),
)]
pub async fn publish(
    State(state): State<AppState>,
    Path((owner, name)): Path<(String, String)>,
    headers: HeaderMap,
    Json(body): Json<PublishSkillRequest>,
) -> Result<(StatusCode, Json<skill_service::PublishOutcome>), AppError> {
    super::repo_content::validate_owner_repo(&owner, &name)?;
    let (repo, user_id) =
        super::repo_access::require_write(&state, &headers, &owner, &name).await?;

    let repo_path = state.repo_root.join(format!("{owner}/{name}.git"));
    if !repo_path.exists() {
        tracing::error!(path = %repo_path.display(), "repository storage missing on disk");
        return Err(AppError::internal("internal error"));
    }

    let outcome = skill_service::publish_from_ref(
        &state.db,
        &storage(&state),
        PublishRequest {
            repo_id: repo.id,
            owner: &owner,
            repo_name: &name,
            repo_path: &repo_path,
            path: &body.path,
            git_ref: &body.git_ref,
            version: body.version.as_deref(),
            author_id: user_id,
        },
    )
    .await
    .map_err(app_error)?;

    if !outcome.existing {
        super::audit::record_write(
            &state.db,
            user_id,
            "skill.publish",
            "skill",
            Some(outcome.package_id),
            &format!("{owner}/{name}/{}@{}", outcome.name, outcome.version),
            &headers,
            serde_json::json!({
                "version": outcome.version,
                "commit": outcome.commit,
                "content_digest": outcome.content_digest,
                "path": body.path,
                "ref": body.git_ref,
            }),
        )
        .await;
    }
    let status = if outcome.existing {
        StatusCode::OK
    } else {
        StatusCode::CREATED
    };
    Ok((status, Json(outcome)))
}

// ── Reads ────────────────────────────────────────────────────────────────

/// GET /repos/{owner}/{name}/skills
#[utoipa::path(
    get,
    path = "/repos/{owner}/{name}/skills",
    tag = "Skills",
    params(
        ("owner" = String, Path, description = "owner"),
        ("name" = String, Path, description = "repo name"),
    ),
    responses((status = 200, description = "Skills published in this repository", body = serde_json::Value)),
)]
pub async fn list_skills(
    State(state): State<AppState>,
    Path((owner, name)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, AppError> {
    let repo = require_read(&state, &headers, &owner, &name).await?;
    let skills = skill_service::list_skills(&state.db, repo.id)
        .await
        .map_err(app_error)?;
    Ok(Json(serde_json::json!({ "skills": skills })))
}

/// GET /repos/{owner}/{name}/skills/{skill}
#[utoipa::path(
    get,
    path = "/repos/{owner}/{name}/skills/{skill}",
    tag = "Skills",
    params(
        ("owner" = String, Path, description = "owner"),
        ("name" = String, Path, description = "repo name"),
        ("skill" = String, Path, description = "skill name"),
    ),
    responses(
        (status = 200, description = "The skill and its versions", body = serde_json::Value),
        (status = 404, description = "SKILL_NOT_FOUND", body = serde_json::Value),
    ),
)]
pub async fn get_skill(
    State(state): State<AppState>,
    Path((owner, name, skill)): Path<(String, String, String)>,
    headers: HeaderMap,
) -> Result<Json<skill_service::SkillDetail>, AppError> {
    let repo = require_read(&state, &headers, &owner, &name).await?;
    skill_service::get_skill(&state.db, repo.id, &skill)
        .await
        .map(Json)
        .map_err(app_error)
}

#[derive(Deserialize, utoipa::IntoParams)]
pub struct ResolveQuery {
    /// Cargo-style semantic-version requirement: `^1.2`, `~1.2.3`, `>=1, <2`,
    /// `=1.2.3`. Empty or `latest` means any version.
    pub constraint: Option<String>,
}

/// GET /repos/{owner}/{name}/skills/{skill}/resolve?constraint=
///
/// Resolves a constraint to one exact, non-yanked version and its digests, for
/// writing to a lock file.
#[utoipa::path(
    get,
    path = "/repos/{owner}/{name}/skills/{skill}/resolve",
    tag = "Skills",
    params(
        ("owner" = String, Path, description = "owner"),
        ("name" = String, Path, description = "repo name"),
        ("skill" = String, Path, description = "skill name"),
        ResolveQuery,
    ),
    responses(
        (status = 200, description = "The chosen version", body = serde_json::Value),
        (status = 400, description = "SKILL_CONSTRAINT_INVALID", body = serde_json::Value),
        (status = 404, description = "NO_MATCHING_VERSION, with `details.available`", body = serde_json::Value),
    ),
)]
pub async fn resolve(
    State(state): State<AppState>,
    Path((owner, name, skill)): Path<(String, String, String)>,
    Query(query): Query<ResolveQuery>,
    headers: HeaderMap,
) -> Result<Json<skill_service::ResolvedSkill>, AppError> {
    let repo = require_read(&state, &headers, &owner, &name).await?;
    skill_service::resolve(&state.db, repo.id, &skill, query.constraint.as_deref())
        .await
        .map(Json)
        .map_err(app_error)
}

/// GET /repos/{owner}/{name}/skills/{skill}/versions/{version}
#[utoipa::path(
    get,
    path = "/repos/{owner}/{name}/skills/{skill}/versions/{version}",
    tag = "Skills",
    params(
        ("owner" = String, Path, description = "owner"),
        ("name" = String, Path, description = "repo name"),
        ("skill" = String, Path, description = "skill name"),
        ("version" = String, Path, description = "exact version"),
    ),
    responses(
        (status = 200, description = "Manifest: frontmatter, file list, digests, source", body = serde_json::Value),
        (status = 404, description = "SKILL_VERSION_NOT_FOUND", body = serde_json::Value),
    ),
)]
pub async fn get_version(
    State(state): State<AppState>,
    Path((owner, name, skill, version)): Path<(String, String, String, String)>,
    headers: HeaderMap,
) -> Result<Json<skill_service::SkillVersionManifest>, AppError> {
    let repo = require_read(&state, &headers, &owner, &name).await?;
    skill_service::get_version_manifest(&state.db, &storage(&state), repo.id, &skill, &version)
        .await
        .map(Json)
        .map_err(app_error)
}

/// GET /repos/{owner}/{name}/skills/{skill}/versions/{version}/content
///
/// `SKILL.md` of that version as JSON text — no download or unpacking needed.
#[utoipa::path(
    get,
    path = "/repos/{owner}/{name}/skills/{skill}/versions/{version}/content",
    tag = "Skills",
    params(
        ("owner" = String, Path, description = "owner"),
        ("name" = String, Path, description = "repo name"),
        ("skill" = String, Path, description = "skill name"),
        ("version" = String, Path, description = "exact version"),
    ),
    responses(
        (status = 200, description = "SKILL.md text, origin user_content", body = serde_json::Value),
        (status = 404, description = "SKILL_VERSION_NOT_FOUND", body = serde_json::Value),
    ),
)]
pub async fn get_content(
    State(state): State<AppState>,
    Path((owner, name, skill, version)): Path<(String, String, String, String)>,
    headers: HeaderMap,
) -> Result<Json<skill_service::SkillTextFile>, AppError> {
    let repo = require_read(&state, &headers, &owner, &name).await?;
    skill_service::read_skill_md(&state.db, &storage(&state), repo.id, &skill, &version)
        .await
        .map(Json)
        .map_err(app_error)
}

/// GET /repos/{owner}/{name}/skills/{skill}/versions/{version}/files/{*path}
///
/// One UTF-8 text file of that version (at most 1 MiB). Larger or binary files
/// are refused with a code that says to fetch the archive.
#[utoipa::path(
    get,
    path = "/repos/{owner}/{name}/skills/{skill}/versions/{version}/files/{path}",
    tag = "Skills",
    params(
        ("owner" = String, Path, description = "owner"),
        ("name" = String, Path, description = "repo name"),
        ("skill" = String, Path, description = "skill name"),
        ("version" = String, Path, description = "exact version"),
        ("path" = String, Path, description = "file path inside the bundle"),
    ),
    responses(
        (status = 200, description = "File text, origin user_content", body = serde_json::Value),
        (status = 404, description = "SKILL_FILE_NOT_FOUND", body = serde_json::Value),
        (status = 413, description = "SKILL_FILE_TOO_LARGE", body = serde_json::Value),
        (status = 415, description = "SKILL_FILE_NOT_TEXT", body = serde_json::Value),
    ),
)]
pub async fn get_file(
    State(state): State<AppState>,
    Path((owner, name, skill, version, path)): Path<(String, String, String, String, String)>,
    headers: HeaderMap,
) -> Result<Json<skill_service::SkillTextFile>, AppError> {
    let repo = require_read(&state, &headers, &owner, &name).await?;
    skill_service::read_text_file(
        &state.db,
        &storage(&state),
        repo.id,
        &skill,
        &version,
        &path,
    )
    .await
    .map(Json)
    .map_err(app_error)
}

/// GET /repos/{owner}/{name}/skills/{skill}/versions/{version}/archive
///
/// The stored `.tar.gz`. `x-checksum-sha256` is the digest of exactly these
/// bytes; `x-content-digest` is the digest of the uncompressed tar.
#[utoipa::path(
    get,
    path = "/repos/{owner}/{name}/skills/{skill}/versions/{version}/archive",
    tag = "Skills",
    params(
        ("owner" = String, Path, description = "owner"),
        ("name" = String, Path, description = "repo name"),
        ("skill" = String, Path, description = "skill name"),
        ("version" = String, Path, description = "exact version"),
    ),
    responses(
        (status = 200, description = "The bundle (application/gzip)", body = String),
        (status = 404, description = "SKILL_VERSION_NOT_FOUND", body = serde_json::Value),
    ),
)]
pub async fn get_archive(
    State(state): State<AppState>,
    Path((owner, name, skill, version)): Path<(String, String, String, String)>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let repo = require_read(&state, &headers, &owner, &name).await?;
    let archive =
        skill_service::read_archive(&state.db, &storage(&state), repo.id, &skill, &version)
            .await
            .map_err(app_error)?;

    let hex_digest = archive
        .archive_digest
        .strip_prefix("sha256:")
        .unwrap_or(&archive.archive_digest)
        .to_string();
    let mut response = (StatusCode::OK, archive.bytes).into_response();
    let out = response.headers_mut();
    out.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/gzip"),
    );
    out.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    // The filename is built from a validated skill name and a canonical
    // semantic version, so it is plain ASCII with no quoting hazards.
    if let Ok(value) =
        HeaderValue::from_str(&format!("attachment; filename=\"{}\"", archive.filename))
    {
        out.insert(header::CONTENT_DISPOSITION, value);
    }
    if let Ok(value) = HeaderValue::from_str(&hex_digest) {
        out.insert("x-checksum-sha256", value);
    }
    if let Ok(value) = HeaderValue::from_str(&archive.content_digest) {
        out.insert("x-content-digest", value);
    }
    Ok(response)
}
