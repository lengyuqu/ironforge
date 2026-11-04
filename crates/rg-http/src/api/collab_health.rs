//! Collaboration health metrics of one repository (R13), repository admins only.
//!
//! The figures are computed from the platform event log, never from client
//! clocks; task status itself is a client declaration and is labelled as such.
//! There is no per-session or per-agent view, and no message body is returned.
//! See `rg_core::collaboration::health`.

use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::Json;
use chrono::{DateTime, Utc};
use rg_core::collaboration::health as svc;
use serde::Deserialize;
use utoipa::IntoParams;

use super::collab::cursor_key;
use super::repo_access::{require_admin, resolve_repo};
use crate::error::AppError;
use crate::AppState;

#[derive(Debug, Deserialize, IntoParams)]
pub struct HealthQuery {
    /// Inclusive start, RFC 3339 (default: `to` minus 7 days).
    pub from: Option<String>,
    /// Exclusive end, RFC 3339, not in the future (default: now).
    pub to: Option<String>,
    /// Comma-separated: `blocked_tasks`, `blocked_duration`, `handoff_waiting`,
    /// `open_questions`, `stale_path_claims` (default: all).
    pub metrics: Option<String>,
}

#[derive(Debug, Deserialize, IntoParams)]
pub struct HealthTasksQuery {
    /// `blocked_at_to`, `entered_blocked`, `blocked_duration`, `handoff_waiting`,
    /// `open_questions` or `unknown_state`.
    pub metric: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub limit: Option<u64>,
    pub cursor: Option<String>,
}

fn parse_time(field: &str, raw: Option<&str>) -> Result<Option<DateTime<Utc>>, AppError> {
    raw.map(|value| {
        DateTime::parse_from_rfc3339(value.trim())
            .map(|t| t.with_timezone(&Utc))
            .map_err(|_| AppError::bad_request(format!("`{field}` must be an RFC 3339 timestamp")))
    })
    .transpose()
}

/// Repository administrator; the permission cache is bypassed so a revoked
/// admin is refused on the very next request.
async fn admin_repo(
    state: &AppState,
    headers: &HeaderMap,
    owner: &str,
    name: &str,
) -> Result<rg_db::entities::repository::Model, AppError> {
    if let Some(user_id) = crate::api::auth::extract_user_id(headers, &state.jwt_secret) {
        if let Ok(repo) = resolve_repo(state, owner, name).await {
            rg_core::repo::service::invalidate_perm_cache_user(repo.id, user_id);
        }
    }
    Ok(require_admin(state, headers, owner, name).await?.0)
}

fn api_base(owner: &str, name: &str) -> String {
    format!("/api/v1/repos/{owner}/{name}/collaboration")
}

#[utoipa::path(
    get,
    path = "/repos/{owner}/{name}/collaboration/health",
    tag = "Collaboration",
    params(("owner" = String, Path), ("name" = String, Path), HealthQuery),
    responses(
        (status = 200, description = "Health metrics for the window [from, to) with metric_version, coverage_start, partial, computed_at, per-metric source/status and data_quality. Data before coverage_start is never counted as zero.", body = serde_json::Value),
        (status = 400, description = "Invalid window or metric id"),
        (status = 401, description = "Authentication required"),
        (status = 403, description = "Repository admin access required"),
        (status = 409, description = "Too many events to compute the window (HEALTH_EVENT_LIMIT)")
    )
)]
pub async fn get_health(
    State(state): State<AppState>,
    Path((owner, name)): Path<(String, String)>,
    Query(query): Query<HealthQuery>,
    headers: HeaderMap,
) -> Result<Json<svc::Health>, AppError> {
    let repo = admin_repo(&state, &headers, &owner, &name).await?;
    let now = Utc::now();
    let window = svc::resolve_window(
        parse_time("from", query.from.as_deref())?,
        parse_time("to", query.to.as_deref())?,
        now,
    )?;
    let wanted = svc::parse_metrics(query.metrics.as_deref())?;
    Ok(Json(
        svc::health(&state.db, repo.id, &api_base(&owner, &name), &window, &wanted, now).await?,
    ))
}

#[utoipa::path(
    get,
    path = "/repos/{owner}/{name}/collaboration/health/tasks",
    tag = "Collaboration",
    params(("owner" = String, Path), ("name" = String, Path), HealthTasksQuery),
    responses(
        (status = 200, description = "Tasks behind one metric with the figures each contributes, ordered by task id (keyset pagination)", body = serde_json::Value),
        (status = 400, description = "Invalid window, metric or cursor"),
        (status = 401, description = "Authentication required"),
        (status = 403, description = "Repository admin access required")
    )
)]
pub async fn get_health_tasks(
    State(state): State<AppState>,
    Path((owner, name)): Path<(String, String)>,
    Query(query): Query<HealthTasksQuery>,
    headers: HeaderMap,
) -> Result<Json<svc::DrillPage>, AppError> {
    let repo = admin_repo(&state, &headers, &owner, &name).await?;
    let now = Utc::now();
    let window = svc::resolve_window(
        parse_time("from", query.from.as_deref())?,
        parse_time("to", query.to.as_deref())?,
        now,
    )?;
    let metric = query
        .metric
        .as_deref()
        .ok_or_else(|| AppError::bad_request("`metric` is required"))?;
    Ok(Json(
        svc::drill_down(
            &state.db,
            cursor_key(&state),
            repo.id,
            &api_base(&owner, &name),
            &window,
            metric,
            query.limit,
            query.cursor.as_deref(),
            now,
        )
        .await?,
    ))
}

#[utoipa::path(
    post,
    path = "/repos/{owner}/{name}/collaboration/health/baseline",
    tag = "Collaboration",
    params(("owner" = String, Path), ("name" = String, Path)),
    responses(
        (status = 200, description = "Objects without any recorded event were recorded as 'observed now'. Repeating the call adds nothing for objects that already have events.", body = serde_json::Value),
        (status = 401, description = "Authentication required"),
        (status = 403, description = "Repository admin access required")
    )
)]
pub async fn take_baseline(
    State(state): State<AppState>,
    Path((owner, name)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<Json<svc::BaselineReport>, AppError> {
    let repo = admin_repo(&state, &headers, &owner, &name).await?;
    Ok(Json(svc::take_baseline(&state.db, repo.id, Utc::now()).await?))
}
