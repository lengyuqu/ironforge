//! Read-only previews of what a deletion would take from the collaboration data.
//! They authorize and delete nothing; see `rg_core::collaboration::retention`.

use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::Json;
use chrono::Utc;
use rg_core::collaboration::retention as svc;

use super::repo_access::require_admin;
use crate::error::AppError;
use crate::AppState;

#[utoipa::path(
    get,
    path = "/repos/{owner}/{name}/collaboration/deletion-impact",
    tag = "Collaboration",
    params(("owner" = String, Path), ("name" = String, Path)),
    responses(
        (status = 200, description = "Collaboration records of the repository and what deleting it does to them (point-in-time counts)", body = serde_json::Value),
        (status = 401, description = "Authentication required"),
        (status = 403, description = "Repository admin access required")
    )
)]
pub async fn repo_deletion_impact(
    State(state): State<AppState>,
    Path((owner, name)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<Json<svc::RepoImpact>, AppError> {
    let (repo, _user) = require_admin(&state, &headers, &owner, &name).await?;
    Ok(Json(
        svc::repo_impact(&state.db, repo.id, Utc::now()).await?,
    ))
}

#[utoipa::path(
    get,
    path = "/admin/users/{id}/collaboration-impact",
    tag = "Admin",
    params(("id" = i64, Path, description = "user id")),
    responses(
        (status = 200, description = "Collaboration records removed if the account is deleted (point-in-time counts)", body = serde_json::Value),
        (status = 403, description = "Site admin required"),
        (status = 404, description = "User not found")
    )
)]
pub async fn user_deletion_impact(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(user_id): Path<i64>,
) -> Result<Json<svc::UserImpact>, AppError> {
    if super::admin::require_admin(&state, &headers)
        .await
        .is_none()
    {
        return Err(AppError::forbidden("admin required"));
    }
    if rg_db::ops::user_ops::find_by_id(&state.db, user_id)
        .await
        .map_err(|e| AppError::internal(e.to_string()))?
        .is_none()
    {
        return Err(AppError::not_found("user not found"));
    }
    Ok(Json(
        svc::user_impact(&state.db, user_id, Utc::now()).await?,
    ))
}
