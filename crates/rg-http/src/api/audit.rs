//! Admin-only audit log query endpoints.

use std::str::FromStr;

use axum::{
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::{api::admin::require_admin, AppState};

#[derive(Debug, Deserialize)]
pub struct AuditLogQuery {
    page: Option<u64>,
    page_size: Option<u64>,
    user_id: Option<i64>,
    action: Option<String>,
    resource_type: Option<String>,
    start_time: Option<String>, // ISO 8601
    end_time: Option<String>,   // ISO 8601
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AuditLogEntry {
    id: i64,
    user_id: Option<i64>,
    username: Option<String>,
    action: String,
    resource_type: Option<String>,
    resource_id: Option<i64>,
    resource_name: Option<String>,
    ip_address: Option<String>,
    details: Option<String>,
    created_at: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AuditLogResponse {
    total: u64,
    page: u64,
    page_size: u64,
    logs: Vec<AuditLogEntry>,
}

/// GET /admin/audit/logs
/// List audit logs with optional filters (admin only).
#[utoipa::path(
    get,
    path = "/admin/audit/logs",
    tag = "Audit",
    params(
        ("page" = Option<u64>, Query, description = "Page number (0-based)"),
        ("page_size" = Option<u64>, Query, description = "Items per page (1-100)"),
        ("user_id" = Option<i64>, Query, description = "Filter by user ID"),
        ("action" = Option<String>, Query, description = "Filter by action"),
        ("resource_type" = Option<String>, Query, description = "Filter by resource type"),
        ("start_time" = Option<String>, Query, description = "Filter after start time (ISO 8601)"),
        ("end_time" = Option<String>, Query, description = "Filter before end time (ISO 8601)"),
    ),
    responses(
        (status = 200, description = "Paginated audit log entries", body = AuditLogResponse),
        (status = 401, description = "Admin access required"),
    ),
)]
pub async fn list_audit_logs(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<AuditLogQuery>,
) -> Result<Json<AuditLogResponse>, (StatusCode, String)> {
    if require_admin(&state, &headers).await.is_none() {
        return Err((StatusCode::UNAUTHORIZED, "admin required".into()));
    }

    let page = q.page.unwrap_or(0);
    let page_size = q.page_size.unwrap_or(20).clamp(1, 100);

    let start_time = q
        .start_time
        .as_deref()
        .and_then(|s| chrono::DateTime::<chrono::Utc>::from_str(s).ok());
    let end_time = q
        .end_time
        .as_deref()
        .and_then(|s| chrono::DateTime::<chrono::Utc>::from_str(s).ok());

    let (logs, total) = rg_db::ops::audit_log_ops::list_paginated(
        &state.db,
        page,
        page_size,
        q.user_id,
        q.action.as_deref(),
        q.resource_type.as_deref(),
        start_time,
        end_time,
    )
    .await
    .map_err(|e| {
        tracing::error!("audit list error: {}", e);
        (StatusCode::INTERNAL_SERVER_ERROR, "database error".into())
    })?;

    let logs = logs
        .into_iter()
        .map(|m| AuditLogEntry {
            id: m.id,
            user_id: m.user_id,
            username: m.username,
            action: m.action,
            resource_type: m.resource_type,
            resource_id: m.resource_id,
            resource_name: m.resource_name,
            ip_address: m.ip_address,
            details: m.details,
            created_at: m.created_at.to_rfc3339(),
        })
        .collect();

    Ok(Json(AuditLogResponse {
        total,
        page,
        page_size,
        logs,
    }))
}

/// GET /admin/audit/logs/{id}
/// Fetch a single audit log entry by id (admin only).
#[utoipa::path(
    get,
    path = "/admin/audit/logs/{id}",
    tag = "Audit",
    params(
        ("id" = i64, Path, description = "Audit log entry ID"),
    ),
    responses(
        (status = 200, description = "Audit log entry details", body = AuditLogEntry),
        (status = 401, description = "Admin access required"),
        (status = 404, description = "Audit log entry not found"),
    ),
)]
pub async fn get_audit_log(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(id): axum::extract::Path<i64>,
) -> Result<Json<AuditLogEntry>, (StatusCode, String)> {
    if require_admin(&state, &headers).await.is_none() {
        return Err((StatusCode::UNAUTHORIZED, "admin required".into()));
    }

    let log = rg_db::ops::audit_log_ops::find_by_id(&state.db, id)
        .await
        .map_err(|e| {
            tracing::error!("audit get error: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "database error".into())
        })?
        .ok_or((StatusCode::NOT_FOUND, "audit log not found".into()))?;

    Ok(Json(AuditLogEntry {
        id: log.id,
        user_id: log.user_id,
        username: log.username,
        action: log.action,
        resource_type: log.resource_type,
        resource_id: log.resource_id,
        resource_name: log.resource_name,
        ip_address: log.ip_address,
        details: log.details,
        created_at: log.created_at.to_rfc3339(),
    }))
}

/// Extract client IP and User-Agent from request headers.
pub(crate) fn extract_ip_and_ua(headers: &HeaderMap) -> (Option<String>, Option<String>) {
    use axum::http::header;

    let ip_address = headers
        .get("X-Forwarded-For")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.split(',').next())
        .map(|s| s.trim().to_string())
        .or_else(|| {
            headers
                .get("X-Real-IP")
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string())
        });

    let user_agent = headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    (ip_address, user_agent)
}
