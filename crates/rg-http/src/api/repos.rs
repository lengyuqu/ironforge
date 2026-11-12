//! Repository REST API.
//!
//! POST /api/v1/repos              — create repo (auth required)
//! GET  /api/v1/repos/:owner       — list repos by owner (user or org)
//! GET  /api/v1/repos/:owner/:name — get single repo

use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use serde::Deserialize;
use utoipa::ToSchema;

use crate::error::AppError;
use crate::pagination::{PaginatedResponse, PaginationParams};
use crate::{
    api::auth::extract_bearer_claims,
    api::repo_access::{require_admin, require_authenticated_read, require_read, require_write},
    openapi::PaginatedRepoResponse,
    AppState,
};

/// Helper to record audit log (fire-and-forget).
#[allow(clippy::too_many_arguments)]
async fn record_audit(
    db: &sea_orm::DatabaseConnection,
    user_id: i64,
    username: &str,
    action: &str,
    resource_type: Option<&str>,
    resource_id: Option<i64>,
    resource_name: Option<&str>,
    headers: &HeaderMap,
    details: Option<serde_json::Value>,
) {
    let (ip_address, user_agent) = crate::api::audit::extract_ip_and_ua(headers);

    let entry = rg_db::entities::audit_log::ActiveModel {
        id: sea_orm::NotSet,
        user_id: sea_orm::Set(Some(user_id)),
        username: sea_orm::Set(Some(username.to_string())),
        action: sea_orm::Set(action.to_string()),
        resource_type: sea_orm::Set(resource_type.map(|s| s.to_string())),
        resource_id: sea_orm::Set(resource_id),
        resource_name: sea_orm::Set(resource_name.map(|s| s.to_string())),
        ip_address: sea_orm::Set(ip_address),
        user_agent: sea_orm::Set(user_agent),
        details: sea_orm::Set(
            crate::api::audit::with_access_token(headers, details).map(|v| v.to_string()),
        ),
        created_at: sea_orm::Set(chrono::Utc::now()),
    };

    if let Err(e) = rg_db::ops::audit_log_ops::insert(db, entry).await {
        tracing::error!(error = %e, "failed to record audit log");
    }
}

/// POST /api/v1/repos
#[derive(Deserialize, ToSchema)]
pub struct CreateRepoRequest {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_private: Option<bool>,
    /// Organization name — if provided, create repo under this org
    #[serde(skip_serializing_if = "Option::is_none")]
    pub org: Option<String>,
    /// Auto-initialize the repo with template files (README, LICENSE, .gitignore)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_init: Option<bool>,
    /// Default branch name (default: "main")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_branch: Option<String>,
    /// .gitignore template key (e.g., "go", "rust", "python")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gitignores: Option<String>,
    /// LICENSE template key (e.g., "mit", "apache-2.0", "gpl-3.0")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    /// README template key (e.g., "default")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub readme: Option<String>,
    /// Default issue label set ("none", "default", "scrum")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub issue_labels: Option<String>,
}

/// Repository response (matches DB model fields exposed to API).
#[derive(serde::Serialize, ToSchema)]
pub struct RepoResponse {
    pub id: i64,
    pub owner_id: i64,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub is_private: bool,
    pub default_branch: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fork_id: Option<i64>,
    pub stars_count: i64,
    pub forks_count: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub org_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin_repo_id: Option<i64>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    /// Opaque version of the editable settings, present on repository reads.
    pub settings_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[utoipa::path(
    post,
    path = "/repos",
    tag = "Repositories",
    request_body = CreateRepoRequest,
    responses(
        (status = 201, description = "Repository created", body = RepoResponse),
        (status = 400, description = "Invalid input", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
        (status = 403, description = "Forbidden (org membership required)", body = serde_json::Value),
    )
)]
pub async fn create_repo(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<CreateRepoRequest>,
) -> impl IntoResponse {
    let claims = match extract_bearer_claims(&headers, &state.jwt_secret) {
        Some(c) => c,
        None => {
            return AppError::unauthorized("authentication required".to_string()).into_response()
        }
    };

    let owner_id: i64 = match claims.sub.parse::<i64>() {
        Ok(id) => id,
        Err(_) => {
            return AppError::unauthorized("invalid token subject".to_string()).into_response()
        }
    };
    let username = claims.username.clone();

    // Resolve org_id if org is specified
    let org_id = match &body.org {
        Some(org_name) => {
            match rg_db::ops::org_ops::get_org_by_name(&state.db, org_name).await {
                Ok(Some(org)) => {
                    // Verify the user is a member of this org
                    match rg_db::ops::org_ops::is_org_member(&state.db, org.id, owner_id).await {
                        Ok(true) => Some(org.id),
                        _ => {
                            return AppError::forbidden(
                                "you are not a member of this organization".to_string(),
                            )
                            .into_response()
                        }
                    }
                }
                Ok(None) => {
                    return AppError::not_found("organization not found".to_string())
                        .into_response()
                }
                Err(e) => return AppError::internal(e.to_string()).into_response(),
            }
        }
        None => None,
    };

    // Get owner identity for template substitution and initial commit author.
    let owner_user = match rg_db::ops::user_ops::find_by_id(&state.db, owner_id).await {
        Ok(Some(user)) => user,
        Ok(None) => {
            return AppError::unauthorized("invalid token subject".to_string()).into_response()
        }
        Err(e) => return AppError::internal(e.to_string()).into_response(),
    };
    let owner_display = owner_user
        .display_name
        .clone()
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| username.clone());

    let opts = rg_core::repo::service::CreateRepoOptions {
        owner_id,
        name: body.name.clone(),
        description: body.description.clone(),
        is_private: body.is_private.unwrap_or(false),
        org_id,
        default_branch: body.default_branch.clone(),
        auto_init: body.auto_init.unwrap_or(false),
        gitignores: body.gitignores.clone(),
        license: body.license.clone(),
        readme: body.readme.clone(),
        issue_labels: body.issue_labels.clone(),
        owner_display_name: owner_display,
        git_author_name: Some(username.clone()),
        git_author_email: Some(owner_user.email.clone()),
    };

    match rg_core::repo::service::create_repo_with_opts(&state.db, opts, state.repo_root.as_path())
        .await
    {
        Ok(repo) => {
            // Record audit log
            let details = serde_json::json!({
                "name": body.name,
                "is_private": body.is_private.unwrap_or(false),
                "org": body.org.as_deref(),
                "auto_init": body.auto_init.unwrap_or(false),
                "default_branch": body.default_branch.as_deref(),
                "gitignores": body.gitignores.as_deref(),
                "license": body.license.as_deref(),
                "readme": body.readme.as_deref(),
                "issue_labels": body.issue_labels.as_deref(),
            });
            let resource_name =
                format!("{}/{}", body.org.as_deref().unwrap_or(&username), body.name);
            record_audit(
                &state.db,
                owner_id,
                &username,
                "repo.create",
                Some("repo"),
                Some(repo.id),
                Some(&resource_name),
                &headers,
                Some(details),
            )
            .await;

            (StatusCode::CREATED, Json(serde_json::json!(repo))).into_response()
        }
        Err(e) => AppError::from(e).into_response(),
    }
}

/// GET /api/v1/repos/:owner
/// Lists repos for either a user or an organization.
#[derive(Deserialize)]
pub struct ListReposQuery {
    #[serde(flatten)]
    pub pagination: PaginationParams,
}

#[utoipa::path(
    get,
    path = "/repos/{owner}",
    tag = "Repositories",
    params(
        ("owner" = String, Path, description = "Username or organization name"),
        ("page" = Option<u64>, Query, description = "Page number (1-based)"),
        ("per_page" = Option<u64>, Query, description = "Items per page (1-100)"),
    ),
    responses(
        (status = 200, description = "List of repositories", body = PaginatedRepoResponse),
        (status = 404, description = "Owner not found", body = serde_json::Value),
        (status = 500, description = "Internal server error", body = serde_json::Value),
    )
)]
pub async fn list_repos(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(owner): Path<String>,
    Query(params): Query<ListReposQuery>,
) -> impl IntoResponse {
    let pagination = params.pagination.clamp();
    let offset = pagination.offset();
    let limit = pagination.limit();
    let actor_id = crate::api::auth::extract_user_id(&headers, &state.jwt_secret);

    // Try user first
    if let Some(user) = rg_db::ops::user_ops::find_by_username(&state.db, &owner)
        .await
        .ok()
        .flatten()
    {
        // Owner sees all repos; others see only public repos
        let include_private = actor_id == Some(user.id);
        let result = if include_private {
            rg_db::ops::repo_ops::list_by_owner_paginated(&state.db, user.id, offset, limit).await
        } else {
            rg_db::ops::repo_ops::list_public_by_owner_paginated(&state.db, user.id, offset, limit)
                .await
        };
        match result {
            Ok((data, total)) => {
                return (
                    StatusCode::OK,
                    Json(PaginatedResponse::new(data, &pagination, total as u64)),
                )
                    .into_response()
            }
            Err(e) => return AppError::internal(e.to_string()).into_response(),
        }
    }

    // Try organization
    if let Some(org) = rg_db::ops::org_ops::get_org_by_name(&state.db, &owner)
        .await
        .ok()
        .flatten()
    {
        // Org members see all repos; others see only public repos
        let include_private = match actor_id {
            // Effective membership: an agent sees an org's private repos only
            // if its owner could too.
            Some(id) => rg_core::org::is_effective_org_member(&state.db, org.id, id)
                .await
                .unwrap_or(false),
            None => false,
        };
        let result = if include_private {
            rg_db::ops::repo_ops::list_by_org_paginated(&state.db, org.id, offset, limit).await
        } else {
            rg_db::ops::repo_ops::list_public_by_org_paginated(&state.db, org.id, offset, limit)
                .await
        };
        match result {
            Ok((data, total)) => {
                return (
                    StatusCode::OK,
                    Json(PaginatedResponse::new(data, &pagination, total as u64)),
                )
                    .into_response()
            }
            Err(e) => return AppError::internal(e.to_string()).into_response(),
        }
    }

    AppError::not_found("owner not found (neither user nor organization)".to_string())
        .into_response()
}

#[utoipa::path(
    get,
    path = "/repos/{owner}/{name}",
    tag = "Repositories",
    params(
        ("owner" = String, Path, description = "Username or organization name"),
        ("name" = String, Path, description = "Repository name"),
    ),
    responses(
        (status = 200, description = "Repository details", body = RepoResponse),
        (status = 404, description = "Repository not found", body = serde_json::Value),
        (status = 500, description = "Internal server error", body = serde_json::Value),
    )
)]
/// GET /api/v1/repos/:owner/:name
/// Gets a single repo, supporting both user and org owners.
pub async fn get_repo(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((owner, name)): Path<(String, String)>,
) -> impl IntoResponse {
    match require_read(&state, &headers, &owner, &name).await {
        Ok(repo) => (StatusCode::OK, Json(repo_with_settings_version(&repo))).into_response(),
        Err(e) => e.into_response(),
    }
}

/// Settings use a representation-based token. A client must resend the token
/// from GET; changing settings away and back produces the same token.
fn settings_version(repo: &rg_db::entities::repository::Model) -> String {
    use sha2::{Digest, Sha256};
    let value = serde_json::json!([
        repo.id,
        repo.description,
        repo.is_private,
        repo.default_branch
    ]);
    hex::encode(Sha256::digest(value.to_string().as_bytes()))
}

fn repo_with_settings_version(repo: &rg_db::entities::repository::Model) -> serde_json::Value {
    let mut value = serde_json::json!(repo);
    value["settings_version"] = serde_json::json!(settings_version(repo));
    value
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateRepoSettingsRequest {
    /// Token from the repository GET response.
    pub expected_version: String,
    /// Omit to leave unchanged, null to clear.
    #[serde(default, deserialize_with = "super::issues::de_clearable_str")]
    pub description: Option<Option<String>>,
    pub is_private: Option<bool>,
    /// Existing Git branch to use as the default.
    pub default_branch: Option<String>,
}

/// Change the bare repository's symbolic HEAD. Return an error if its current
/// target does not match the database, so an external change is not overwritten.
fn set_default_head(path: &std::path::Path, old: &str, new: &str) -> Result<(), String> {
    use gix::refs::transaction::{Change, LogChange, PreviousValue, RefEdit, RefLog};
    let branch_ref = format!("refs/heads/{new}");
    let branch: gix::refs::FullName = branch_ref
        .clone()
        .try_into()
        .map_err(|_| "invalid branch name".to_string())?;
    let repo = gix::open(path).map_err(|_| "repository storage is unavailable".to_string())?;
    if repo.rev_parse_single(branch_ref.as_str()).is_err() {
        return Err("default branch does not exist".into());
    }
    let current = std::fs::read_to_string(path.join("HEAD"))
        .map_err(|_| "cannot read repository HEAD".to_string())?;
    if current.trim() != format!("ref: refs/heads/{old}") {
        return Err("repository HEAD differs from the configured default branch".into());
    }
    let head: gix::refs::FullName = "HEAD".try_into().map_err(|_| "invalid HEAD".to_string())?;
    let previous: gix::refs::FullName = format!("refs/heads/{old}")
        .try_into()
        .map_err(|_| "invalid existing default branch".to_string())?;
    repo.edit_reference(RefEdit {
        change: Change::Update {
            log: LogChange {
                mode: RefLog::AndReference,
                force_create_reflog: false,
                message: "update default branch".into(),
            },
            expected: PreviousValue::MustExistAndMatch(gix::refs::Target::Symbolic(previous)),
            new: gix::refs::Target::Symbolic(branch),
        },
        name: head,
        deref: false,
    })
    .map_err(|_| "cannot update repository HEAD".to_string())?;
    Ok(())
}

#[utoipa::path(
    patch, path = "/repos/{owner}/{name}", tag = "Repositories",
    params(("owner" = String, Path), ("name" = String, Path)),
    request_body = UpdateRepoSettingsRequest,
    responses(
        (status = 200, description = "Updated settings", body = RepoResponse),
        (status = 400, description = "Invalid input or branch", body = crate::error::ErrorResponse),
        (status = 401, description = "Authentication required", body = crate::error::ErrorResponse),
        (status = 403, description = "Repository administrator required", body = crate::error::ErrorResponse),
        (status = 404, description = "Repository not found", body = crate::error::ErrorResponse),
        (status = 409, description = "Settings changed; reload", body = crate::error::ErrorResponse),
        (status = 415, description = "Expected application/json content type"),
        (status = 422, description = "Missing or malformed request fields")
    )
)]
pub async fn update_repo_settings(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((owner, name)): Path<(String, String)>,
    Json(body): Json<UpdateRepoSettingsRequest>,
) -> impl IntoResponse {
    match update_repo_settings_inner(&state, &headers, &owner, &name, body).await {
        Ok(repo) => (StatusCode::OK, Json(repo_with_settings_version(&repo))).into_response(),
        Err(err) => err.into_response(),
    }
}

async fn update_repo_settings_inner(
    state: &AppState,
    headers: &HeaderMap,
    owner: &str,
    name: &str,
    body: UpdateRepoSettingsRequest,
) -> Result<rg_db::entities::repository::Model, AppError> {
    use rg_db::entities::repository;
    use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, TransactionTrait};
    let (repo, _) = require_admin(state, headers, owner, name).await?;
    if let Some(Some(description)) = &body.description {
        super::validation::validate_length(
            description,
            super::validation::MAX_BODY_LEN,
            "description",
        )?;
        super::validation::reject_control_chars(description, "description")?;
    }
    if let Some(branch) = &body.default_branch {
        super::validation::validate_length(
            branch,
            super::validation::MAX_BRANCH_NAME_LEN,
            "default_branch",
        )?;
        if branch.is_empty()
            || gix::refs::FullName::try_from(format!("refs/heads/{branch}")).is_err()
        {
            return Err(AppError::bad_request("invalid default branch"));
        }
    }
    let storage_owner = super::ci::resolve_repo_storage_owner(state, &repo, owner).await?;
    rg_core::platform::validate_repo_path(&storage_owner)
        .map_err(|e| AppError::bad_request(e.to_string()))?;
    rg_core::platform::validate_repo_path(name)
        .map_err(|e| AppError::bad_request(e.to_string()))?;
    let path = state.repo_root.join(format!("{storage_owner}/{name}.git"));
    // A retry is safe here: the lock statement has not changed a value or
    // performed any external action, and a failed transaction is rolled back.
    let txn = {
        let mut attempts = 0;
        loop {
            let txn = state.db.begin().await.map_err(AppError::from)?;
            let locked = repository::Entity::update_many()
                .col_expr(
                    repository::Column::Id,
                    sea_orm::sea_query::Expr::col(repository::Column::Id).into(),
                )
                .filter(repository::Column::Id.eq(repo.id))
                .filter(repository::Column::DeletedAt.is_null())
                .exec(&txn)
                .await;
            match locked {
                Ok(_) => break txn,
                Err(error) if rg_db::is_sqlite_busy(&error) && attempts < 50 => {
                    txn.rollback().await.map_err(AppError::from)?;
                    attempts += 1;
                    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                }
                Err(error) => return Err(AppError::from(error)),
            }
        }
    };
    let current = repository::Entity::find_by_id(repo.id)
        .filter(repository::Column::DeletedAt.is_null())
        .one(&txn)
        .await
        .map_err(AppError::from)?
        .ok_or_else(|| AppError::not_found("repository not found"))?;
    // The route and administrator were resolved before the lock. A concurrent
    // transfer or rename must not let the old route modify the new owner's repo.
    if current.owner_id != repo.owner_id
        || current.org_id != repo.org_id
        || current.name != repo.name
    {
        return Err(AppError::conflict(
            "repository identity changed; resolve it again",
        ));
    }
    if body.expected_version != settings_version(&current) {
        return Err(AppError::conflict(
            "repository settings changed; reload before saving",
        ));
    }
    let old_branch = current.default_branch.clone();
    let new_branch = body
        .default_branch
        .clone()
        .unwrap_or_else(|| old_branch.clone());
    let mut active: repository::ActiveModel = current.into();
    if let Some(description) = body.description {
        active.description = sea_orm::Set(description);
    }
    if let Some(private) = body.is_private {
        active.is_private = sea_orm::Set(private);
    }
    active.default_branch = sea_orm::Set(new_branch.clone());
    active.updated_at = sea_orm::Set(chrono::Utc::now());
    let updated = active.update(&txn).await.map_err(AppError::from)?;
    if new_branch != old_branch {
        let path2 = path.clone();
        let old2 = old_branch.clone();
        let new2 = new_branch.clone();
        tokio::task::spawn_blocking(move || set_default_head(&path2, &old2, &new2))
            .await
            .map_err(|_| AppError::internal("default branch task failed"))?
            .map_err(AppError::bad_request)?;
    }
    if let Err(error) = txn.commit().await {
        if new_branch != old_branch {
            let path2 = path.clone();
            let old2 = old_branch.clone();
            let new2 = new_branch.clone();
            if !matches!(
                tokio::task::spawn_blocking(move || set_default_head(&path2, &new2, &old2)).await,
                Ok(Ok(()))
            ) {
                tracing::error!(
                    repo_id = repo.id,
                    "failed to roll back HEAD after settings commit failed"
                );
            }
        }
        return Err(AppError::from(error));
    }
    rg_core::repo::service::invalidate_perm_cache_repo(repo.id);
    Ok(updated)
}

// ── Star/Watch/Delete handlers ───────────────────────────────────────────────

/// Request body for watch state.
#[derive(serde::Deserialize, ToSchema)]
pub struct WatchRequest {
    pub state: String,
}

/// PUT /api/v1/repos/:owner/:name/star
#[utoipa::path(
    put,
    path = "/repos/{owner}/{name}/star",
    tag = "Repositories",
    params(
        ("owner" = String, Path, description = "owner"),
        ("name" = String, Path, description = "name"),
    ),
    responses(
        (status = 200, description = "Updated", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
    ),
)]
pub async fn star_repo(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((owner, name)): Path<(String, String)>,
) -> impl IntoResponse {
    let (repo, user_id) = match require_authenticated_read(&state, &headers, &owner, &name).await {
        Ok(pair) => pair,
        Err(e) => return e.into_response(),
    };

    match rg_core::repo::service::toggle_star(&state.db, user_id, repo.id).await {
        Ok(starred) => (
            StatusCode::OK,
            Json(serde_json::json!({ "starred": starred })),
        )
            .into_response(),
        Err(e) => AppError::internal(e.to_string()).into_response(),
    }
}

/// GET /api/v1/repos/:owner/:name/starred
#[utoipa::path(
    get,
    path = "/repos/{owner}/{name}/starred",
    tag = "Repositories",
    params(
        ("owner" = String, Path, description = "owner"),
        ("name" = String, Path, description = "name"),
    ),
    responses(
        (status = 200, description = "Success", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
    ),
)]
pub async fn get_starred_status(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((owner, name)): Path<(String, String)>,
) -> impl IntoResponse {
    let (repo, user_id) = match require_authenticated_read(&state, &headers, &owner, &name).await {
        Ok(pair) => pair,
        Err(e) => return e.into_response(),
    };

    match rg_core::repo::service::is_starred(&state.db, user_id, repo.id).await {
        Ok(starred) => (
            StatusCode::OK,
            Json(serde_json::json!({ "starred": starred })),
        )
            .into_response(),
        Err(e) => AppError::internal(e.to_string()).into_response(),
    }
}

/// GET /api/v1/repos/:owner/:name/stargazers
#[utoipa::path(
    get,
    path = "/repos/{owner}/{name}/stargazers",
    tag = "Repositories",
    params(
        ("owner" = String, Path, description = "owner"),
        ("name" = String, Path, description = "name"),
    ),
    responses(
        (status = 200, description = "Success", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
    ),
)]
pub async fn get_stargazers(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((owner, name)): Path<(String, String)>,
    Query(params): Query<PaginationParams>,
) -> impl IntoResponse {
    let pagination = params.clamp();
    let offset = pagination.offset();
    let limit = pagination.limit();

    let repo = match require_read(&state, &headers, &owner, &name).await {
        Ok(r) => r,
        Err(e) => return e.into_response(),
    };

    match rg_core::repo::service::list_stargazers(&state.db, repo.id, offset, limit).await {
        Ok((stargazers, total)) => (
            StatusCode::OK,
            Json(PaginatedResponse::new(
                stargazers,
                &pagination,
                total as u64,
            )),
        )
            .into_response(),
        Err(e) => AppError::internal(e.to_string()).into_response(),
    }
}

/// GET /api/v1/repos/:owner/:name/watch
#[utoipa::path(
    get,
    path = "/repos/{owner}/{name}/watch",
    tag = "Repositories",
    params(
        ("owner" = String, Path, description = "owner"),
        ("name" = String, Path, description = "name"),
    ),
    responses(
        (status = 200, description = "Success", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
    ),
)]
pub async fn get_watch_status(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((owner, name)): Path<(String, String)>,
) -> impl IntoResponse {
    let (repo, user_id) = match require_authenticated_read(&state, &headers, &owner, &name).await {
        Ok(pair) => pair,
        Err(e) => return e.into_response(),
    };

    match rg_core::repo::service::get_watch(&state.db, user_id, repo.id).await {
        Ok(watch_state) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "watch_state": watch_state.unwrap_or_else(|| "not_watching".to_string())
            })),
        )
            .into_response(),
        Err(e) => AppError::internal(e.to_string()).into_response(),
    }
}

/// PUT /api/v1/repos/:owner/:name/watch
#[utoipa::path(
    put,
    path = "/repos/{owner}/{name}/watch",
    tag = "Repositories",
    params(
        ("owner" = String, Path, description = "owner"),
        ("name" = String, Path, description = "name"),
    ),
    request_body(content = serde_json::Value),
    responses(
        (status = 200, description = "Updated", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
    ),
)]
pub async fn watch_repo(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((owner, name)): Path<(String, String)>,
    Json(body): Json<WatchRequest>,
) -> impl IntoResponse {
    let (repo, user_id) = match require_authenticated_read(&state, &headers, &owner, &name).await {
        Ok(pair) => pair,
        Err(e) => return e.into_response(),
    };

    match rg_core::repo::service::set_watch(&state.db, user_id, repo.id, &body.state).await {
        Ok(watch_state) => (
            StatusCode::OK,
            Json(serde_json::json!({ "watch_state": watch_state })),
        )
            .into_response(),
        Err(e) => AppError::internal(e.to_string()).into_response(),
    }
}

/// DELETE /api/v1/repos/:owner/:name/watch
#[utoipa::path(
    delete,
    path = "/repos/{owner}/{name}/watch",
    tag = "Repositories",
    params(
        ("owner" = String, Path, description = "owner"),
        ("name" = String, Path, description = "name"),
    ),
    responses(
        (status = 200, description = "Deleted", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
    ),
)]
pub async fn unwatch_repo(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((owner, name)): Path<(String, String)>,
) -> impl IntoResponse {
    let (repo, user_id) = match require_authenticated_read(&state, &headers, &owner, &name).await {
        Ok(pair) => pair,
        Err(e) => return e.into_response(),
    };

    match rg_core::repo::service::set_watch(&state.db, user_id, repo.id, "not_watching").await {
        Ok(_) => (
            StatusCode::OK,
            Json(serde_json::json!({ "watch_state": "not_watching" })),
        )
            .into_response(),
        Err(e) => AppError::internal(e.to_string()).into_response(),
    }
}

/// DELETE /api/v1/repos/:owner/:name
#[utoipa::path(
    delete,
    path = "/repos/{owner}/{name}",
    tag = "Repositories",
    params(
        ("owner" = String, Path, description = "owner"),
        ("name" = String, Path, description = "name"),
    ),
    responses(
        (status = 200, description = "Deleted", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
    ),
)]
pub async fn delete_repo_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((owner, name)): Path<(String, String)>,
) -> impl IntoResponse {
    let (repo, user_id) = match require_admin(&state, &headers, &owner, &name).await {
        Ok(pair) => pair,
        Err(e) => return e.into_response(),
    };
    // Extract claims for audit log username
    let claims = match extract_bearer_claims(&headers, &state.jwt_secret) {
        Some(c) => c,
        None => {
            return AppError::unauthorized("authentication required".to_string()).into_response()
        }
    };

    match rg_core::repo::service::delete_repo(&state.db, &state.repo_root, repo.id).await {
        Ok(()) => {
            // Record audit log
            let resource_name = format!("{}/{}", owner, name);
            let details = serde_json::json!({
                "owner": owner,
                "name": name
            });
            record_audit(
                &state.db,
                user_id,
                &claims.username,
                "repo.delete",
                Some("repo"),
                Some(repo.id),
                Some(&resource_name),
                &headers,
                Some(details),
            )
            .await;

            (StatusCode::OK, Json(serde_json::json!({ "deleted": true }))).into_response()
        }
        Err(e) => AppError::internal(e.to_string()).into_response(),
    }
}

// ── Fork handlers ──────────────────────────────────────────────────────

#[derive(serde::Deserialize, ToSchema)]
pub struct ForkRequest {
    pub org: Option<String>,
}

/// POST /api/v1/repos/:owner/:name/fork
#[utoipa::path(
    post,
    path = "/repos/{owner}/{name}/fork",
    tag = "Repositories",
    params(
        ("owner" = String, Path, description = "owner"),
        ("name" = String, Path, description = "name"),
    ),
    responses(
        (status = 201, description = "Created", body = serde_json::Value),
        (status = 400, description = "Bad request", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
    ),
)]
pub async fn fork_repo_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((owner, name)): Path<(String, String)>,
) -> impl IntoResponse {
    let (_repo, user_id) = match require_authenticated_read(&state, &headers, &owner, &name).await {
        Ok(pair) => pair,
        Err(e) => return e.into_response(),
    };
    // Extract claims for audit log username
    let claims = match extract_bearer_claims(&headers, &state.jwt_secret) {
        Some(c) => c,
        None => {
            return AppError::unauthorized("authentication required".to_string()).into_response();
        }
    };

    match rg_core::repo::service::fork_repo(&state.db, user_id, &owner, &name, &state.repo_root)
        .await
    {
        Ok(repo) => {
            // Record audit log
            let details = serde_json::json!({
                "source_owner": owner,
                "source_name": name,
                "fork_owner": claims.username
            });
            let resource_name = format!("{}/{}", claims.username, name);
            record_audit(
                &state.db,
                user_id,
                &claims.username,
                "repo.fork",
                Some("repo"),
                Some(repo.id),
                Some(&resource_name),
                &headers,
                Some(details),
            )
            .await;

            (StatusCode::ACCEPTED, Json(serde_json::json!(repo))).into_response()
        }
        Err(e) => AppError::from(e).into_response(),
    }
}

/// GET /api/v1/repos/:owner/:name/forks
#[utoipa::path(
    get,
    path = "/repos/{owner}/{name}/forks",
    tag = "Repositories",
    params(
        ("owner" = String, Path, description = "owner"),
        ("name" = String, Path, description = "name"),
    ),
    responses(
        (status = 200, description = "Success", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
    ),
)]
pub async fn list_forks_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((owner, name)): Path<(String, String)>,
    Query(params): Query<PaginationParams>,
) -> impl IntoResponse {
    if let Err(e) = require_read(&state, &headers, &owner, &name).await {
        return e.into_response();
    }

    let pagination = params.clamp();
    let offset = pagination.offset();
    let limit = pagination.limit();

    match rg_core::repo::service::list_forks(&state.db, &owner, &name, offset, limit).await {
        Ok((forks, total)) => (
            StatusCode::OK,
            Json(PaginatedResponse::new(forks, &pagination, total as u64)),
        )
            .into_response(),
        Err(e) => AppError::internal(e.to_string()).into_response(),
    }
}

// ── Transfer handler ──────────────────────────────────────────────────

#[derive(serde::Deserialize, ToSchema)]
pub struct TransferRequest {
    pub new_owner: String,
}

/// POST /api/v1/repos/:owner/:name/transfer
#[utoipa::path(
    post,
    path = "/repos/{owner}/{name}/transfer",
    tag = "Repositories",
    params(
        ("owner" = String, Path, description = "owner"),
        ("name" = String, Path, description = "name"),
    ),
    request_body(content = serde_json::Value),
    responses(
        (status = 201, description = "Created", body = serde_json::Value),
        (status = 400, description = "Bad request", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
    ),
)]
pub async fn transfer_repo_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((owner, name)): Path<(String, String)>,
    Json(body): Json<TransferRequest>,
) -> impl IntoResponse {
    let (_repo, user_id) = match require_admin(&state, &headers, &owner, &name).await {
        Ok(pair) => pair,
        Err(e) => return e.into_response(),
    };
    // Extract claims for audit log username
    let claims = match extract_bearer_claims(&headers, &state.jwt_secret) {
        Some(c) => c,
        None => {
            return AppError::unauthorized("authentication required".to_string()).into_response();
        }
    };

    match rg_core::repo::service::transfer_repo(
        &state.db,
        user_id,
        &owner,
        &name,
        &body.new_owner,
        &state.repo_root,
    )
    .await
    {
        Ok(repo) => {
            // Record audit log
            let details = serde_json::json!({
                "old_owner": owner,
                "new_owner": body.new_owner,
                "name": name
            });
            let resource_name = format!("{}/{}", body.new_owner, name);
            record_audit(
                &state.db,
                user_id,
                &claims.username,
                "repo.transfer",
                Some("repo"),
                Some(repo.id),
                Some(&resource_name),
                &headers,
                Some(details),
            )
            .await;

            (StatusCode::OK, Json(serde_json::json!(repo))).into_response()
        }
        Err(e) => AppError::from(e).into_response(),
    }
}

// ── Commit Status handlers ─────────────────────────────────────────────

#[derive(serde::Deserialize, ToSchema)]
pub struct CreateCommitStatusRequest {
    pub state: String,
    pub context: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_url: Option<String>,
}

/// POST /api/v1/repos/:owner/:name/statuses/:sha
#[utoipa::path(
    post,
    path = "/repos/{owner}/{name}/statuses/{sha}",
    tag = "Repositories",
    params(
        ("owner" = String, Path, description = "owner"),
        ("name" = String, Path, description = "name"),
        ("sha" = String, Path, description = "sha"),
    ),
    request_body(content = serde_json::Value),
    responses(
        (status = 201, description = "Created", body = serde_json::Value),
        (status = 400, description = "Bad request", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
    ),
)]
pub async fn create_commit_status(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((owner, name, sha)): Path<(String, String, String)>,
    Json(body): Json<CreateCommitStatusRequest>,
) -> impl IntoResponse {
    let (repo, user_id) = match require_write(&state, &headers, &owner, &name).await {
        Ok(pair) => pair,
        Err(e) => return e.into_response(),
    };

    match rg_core::repo::service::create_commit_status(
        &state.db,
        repo.id,
        &sha,
        &body.state,
        &body.context,
        body.description.as_deref(),
        body.target_url.as_deref(),
        user_id,
    )
    .await
    {
        Ok(status) => (StatusCode::CREATED, Json(serde_json::json!(status))).into_response(),
        Err(e) => AppError::bad_request(e.to_string()).into_response(),
    }
}

/// GET /api/v1/repos/:owner/:name/commits/:sha/statuses
#[utoipa::path(
    get,
    path = "/repos/{owner}/{name}/commits/{sha}/statuses",
    tag = "Repositories",
    params(
        ("owner" = String, Path, description = "owner"),
        ("name" = String, Path, description = "name"),
        ("sha" = String, Path, description = "sha"),
    ),
    responses(
        (status = 200, description = "Success", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
    ),
)]
pub async fn list_commit_statuses(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((owner, name, sha)): Path<(String, String, String)>,
) -> impl IntoResponse {
    if let Err(e) = require_read(&state, &headers, &owner, &name).await {
        return e.into_response();
    }

    match rg_core::repo::service::list_commit_statuses(&state.db, &owner, &name, &sha).await {
        Ok(statuses) => (StatusCode::OK, Json(serde_json::json!(statuses))).into_response(),
        Err(e) => AppError::internal(e.to_string()).into_response(),
    }
}

/// GET /api/v1/repos/:owner/:name/commits/:sha/status
#[utoipa::path(
    get,
    path = "/repos/{owner}/{name}/commits/{sha}/status",
    tag = "Repositories",
    params(
        ("owner" = String, Path, description = "owner"),
        ("name" = String, Path, description = "name"),
        ("sha" = String, Path, description = "sha"),
    ),
    responses(
        (status = 200, description = "Success", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
    ),
)]
pub async fn get_combined_status(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((owner, name, sha)): Path<(String, String, String)>,
) -> impl IntoResponse {
    if let Err(e) = require_read(&state, &headers, &owner, &name).await {
        return e.into_response();
    }

    match rg_core::repo::service::get_combined_status(&state.db, &owner, &name, &sha).await {
        Ok(combined) => (StatusCode::OK, Json(combined)).into_response(),
        Err(e) => AppError::internal(e.to_string()).into_response(),
    }
}

// ── Template listing endpoints ─────────────────────────────────────────

/// GET /api/v1/repos/templates/gitignores
/// List available .gitignore templates.
#[utoipa::path(
    get,
    path = "/repos/templates/gitignores",
    tag = "Repositories",
    responses(
        (status = 200, description = "List of .gitignore template options", body = serde_json::Value),
    )
)]
pub async fn list_gitignore_templates() -> impl IntoResponse {
    let options = rg_core::repo::templates::gitignore_options();
    (StatusCode::OK, Json(serde_json::json!({ "data": options }))).into_response()
}

/// GET /api/v1/repos/templates/licenses
/// List available LICENSE templates.
#[utoipa::path(
    get,
    path = "/repos/templates/licenses",
    tag = "Repositories",
    responses(
        (status = 200, description = "List of LICENSE template options", body = serde_json::Value),
    )
)]
pub async fn list_license_templates() -> impl IntoResponse {
    let options = rg_core::repo::templates::license_options();
    (StatusCode::OK, Json(serde_json::json!({ "data": options }))).into_response()
}

/// GET /api/v1/repos/templates/readmes
/// List available README templates.
#[utoipa::path(
    get,
    path = "/repos/templates/readmes",
    tag = "Repositories",
    responses(
        (status = 200, description = "List of README template options", body = serde_json::Value),
    )
)]
pub async fn list_readme_templates() -> impl IntoResponse {
    let options = rg_core::repo::templates::readme_options();
    (StatusCode::OK, Json(serde_json::json!({ "data": options }))).into_response()
}

/// GET /api/v1/repos/templates/labels
/// List available default label sets.
#[utoipa::path(
    get,
    path = "/repos/templates/labels",
    tag = "Repositories",
    responses(
        (status = 200, description = "List of label set options", body = serde_json::Value),
    )
)]
pub async fn list_label_sets() -> impl IntoResponse {
    let options = rg_core::repo::templates::label_set_options();
    (StatusCode::OK, Json(serde_json::json!({ "data": options }))).into_response()
}

// ── Public repository listing (Explore) ───────────────────────────────

#[derive(Deserialize)]
pub struct ExploreQuery {
    pub page: Option<u64>,
    pub per_page: Option<u64>,
}

/// GET /api/v1/repos/explore
/// List public repositories for the explore page.
#[utoipa::path(
    get,
    path = "/repos/explore",
    tag = "Repositories",
    params(
        ("page" = Option<u64>, Query, description = "Page number (1-based)"),
        ("per_page" = Option<u64>, Query, description = "Items per page (1-100)"),
    ),
    responses(
        (status = 200, description = "Paginated list of public repositories", body = PaginatedRepoResponse),
    )
)]
pub async fn explore(
    State(state): State<AppState>,
    Query(params): Query<ExploreQuery>,
) -> impl IntoResponse {
    let pagination = PaginationParams {
        page: params.page.unwrap_or(1),
        per_page: params.per_page.unwrap_or(20),
    }
    .clamp();
    let offset = pagination.offset();
    let limit = pagination.limit();

    match rg_db::ops::repo_ops::list_public_paginated(&state.db, offset, limit).await {
        Ok((data, total)) => {
            // Enrich with owner names
            let enriched: Vec<serde_json::Value> =
                futures::future::join_all(data.iter().map(|repo| async {
                    let owner_name = rg_db::ops::user_ops::find_by_id(&state.db, repo.owner_id)
                        .await
                        .ok()
                        .flatten()
                        .map(|u| u.username)
                        .unwrap_or_else(|| "unknown".to_string());
                    serde_json::json!({
                        "id": repo.id,
                        "owner_id": repo.owner_id,
                        "owner_name": owner_name,
                        "name": repo.name,
                        "description": repo.description,
                        "stars_count": repo.stars_count,
                        "forks_count": repo.forks_count,
                        "updated_at": repo.updated_at,
                    })
                }))
                .await;

            (
                StatusCode::OK,
                Json(PaginatedResponse::new(enriched, &pagination, total as u64)),
            )
                .into_response()
        }
        Err(e) => AppError::internal(e.to_string()).into_response(),
    }
}
