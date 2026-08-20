//! Shared repository-scoped authorization helpers for REST handlers.

use axum::http::HeaderMap;

use crate::error::AppError;
use crate::AppState;

/// Resolve a repository from its route owner/name pair.
pub(crate) async fn resolve_repo(
    state: &AppState,
    owner: &str,
    name: &str,
) -> Result<rg_db::entities::repository::Model, AppError> {
    rg_core::repo::service::find_repo_by_owner_name(&state.db, owner, name)
        .await
        .map_err(AppError::internal)?
        .ok_or_else(|| AppError::not_found("repository not found"))
}

/// Require repository read access, while retaining anonymous access to public repos.
///
/// A valid CI job token with `repo:read` scope for this repo also grants read
/// access (anonymous CI path, e.g. artifact/cache fetch inside a job).
pub(crate) async fn require_read(
    state: &AppState,
    headers: &HeaderMap,
    owner: &str,
    name: &str,
) -> Result<rg_db::entities::repository::Model, AppError> {
    require_read_with_ci_scope(state, headers, owner, name, "repo:read").await
}

/// Same as [`require_read`], but the anonymous CI job token must carry the
/// given domain-specific scope (e.g. `packages:read`) instead of `repo:read`.
pub(crate) async fn require_read_with_ci_scope(
    state: &AppState,
    headers: &HeaderMap,
    owner: &str,
    name: &str,
    ci_scope: &str,
) -> Result<rg_db::entities::repository::Model, AppError> {
    let repo = resolve_repo(state, owner, name).await?;
    let actor_id = super::auth::extract_user_id(headers, &state.jwt_secret);

    match rg_core::repo::service::can_read_repo(&state.db, &repo, actor_id).await {
        Ok(true) => Ok(repo),
        Ok(false)
            if actor_id.is_none()
                && super::auth::extract_ci_job_claims(
                    headers,
                    &state.jwt_secret,
                    repo.id,
                    ci_scope,
                )
                .is_some() =>
        {
            Ok(repo)
        }
        Ok(false) if repo.is_private && actor_id.is_none() => {
            Err(AppError::unauthorized("authentication required"))
        }
        Ok(false) => Err(AppError::forbidden("access denied")),
        Err(e) => Err(AppError::internal(e)),
    }
}

/// Require an authenticated user who can read the repository.
pub(crate) async fn require_authenticated_read(
    state: &AppState,
    headers: &HeaderMap,
    owner: &str,
    name: &str,
) -> Result<(rg_db::entities::repository::Model, i64), AppError> {
    let actor_id = super::auth::extract_user_id(headers, &state.jwt_secret)
        .ok_or_else(|| AppError::unauthorized("authentication required"))?;
    let repo = resolve_repo(state, owner, name).await?;

    match rg_core::repo::service::can_read_repo(&state.db, &repo, Some(actor_id)).await {
        Ok(true) => Ok((repo, actor_id)),
        Ok(false) => Err(AppError::forbidden("access denied")),
        Err(e) => Err(AppError::internal(e)),
    }
}

/// Require an authenticated user with repository write access.
pub(crate) async fn require_write(
    state: &AppState,
    headers: &HeaderMap,
    owner: &str,
    name: &str,
) -> Result<(rg_db::entities::repository::Model, i64), AppError> {
    let actor_id = super::auth::extract_user_id(headers, &state.jwt_secret)
        .ok_or_else(|| AppError::unauthorized("authentication required"))?;
    let repo = resolve_repo(state, owner, name).await?;

    match rg_core::repo::service::can_write_repo(&state.db, &repo, Some(actor_id)).await {
        Ok(true) => Ok((repo, actor_id)),
        Ok(false) => Err(AppError::forbidden("write access denied")),
        Err(e) => Err(AppError::internal(e)),
    }
}

/// Require an authenticated repository administrator.
pub(crate) async fn require_admin(
    state: &AppState,
    headers: &HeaderMap,
    owner: &str,
    name: &str,
) -> Result<(rg_db::entities::repository::Model, i64), AppError> {
    let actor_id = super::auth::extract_user_id(headers, &state.jwt_secret)
        .ok_or_else(|| AppError::unauthorized("authentication required"))?;
    let repo = resolve_repo(state, owner, name).await?;
    match rg_core::repo::service::can_admin_repo(&state.db, &repo, Some(actor_id)).await {
        Ok(true) => Ok((repo, actor_id)),
        Ok(false) => Err(AppError::forbidden("repository admin access required")),
        Err(error) => Err(AppError::internal(error)),
    }
}
