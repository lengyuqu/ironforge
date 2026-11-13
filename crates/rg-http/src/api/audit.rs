//! Admin-only audit log query endpoints.

use std::str::FromStr;

use axum::{
    extract::{Query, State},
    http::HeaderMap,
    Json,
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::{api::admin::require_admin, error::AppError, AppState};

#[derive(Debug, Deserialize)]
pub struct AuditLogQuery {
    page: Option<u64>,
    /// L-4: Standardized to `per_page` (alias `page_size` for backward compat).
    #[serde(default, alias = "page_size")]
    per_page: Option<u64>,
    user_id: Option<i64>,
    on_behalf_of_user_id: Option<i64>,
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
    /// For an action by an agent, the person who owns that agent; null for a
    /// person's own action and for rows written before this field existed.
    on_behalf_of_user_id: Option<i64>,
    /// Who `user_id` is now: person or agent, and an agent's owner. Null when
    /// the row has no user or the account was deleted. `username` above is the
    /// name recorded when the row was written; this is the live account.
    #[schema(value_type = Option<crate::api::actor_ref::ActorRefSchema>)]
    user_ref: Option<rg_core::agent::ActorRef>,
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
    per_page: u64,
    logs: Vec<AuditLogEntry>,
}

#[derive(Debug, Deserialize)]
pub struct LoginAttemptQuery {
    page: Option<u64>,
    #[serde(default, alias = "page_size")]
    per_page: Option<u64>,
    username: Option<String>,
    auth_provider: Option<String>,
    success: Option<bool>,
    start_time: Option<String>,
    end_time: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct LoginAttemptEntry {
    id: i64,
    user_id: Option<i64>,
    username: String,
    auth_provider: String,
    ip_address: Option<String>,
    user_agent: Option<String>,
    success: bool,
    failure_reason: Option<String>,
    created_at: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct LoginAttemptResponse {
    total: u64,
    page: u64,
    per_page: u64,
    attempts: Vec<LoginAttemptEntry>,
}

/// GET /admin/audit/logs
/// List audit logs with optional filters (admin only).
#[utoipa::path(
    get,
    path = "/admin/audit/logs",
    tag = "Audit",
    params(
        ("page" = Option<u64>, Query, description = "Page number (1-based, default 1)"),
        ("per_page" = Option<u64>, Query, description = "Items per page (1-100, default 20)"),
        ("user_id" = Option<i64>, Query, description = "Filter by user ID (the acting account, which may be an agent)"),
        ("on_behalf_of_user_id" = Option<i64>, Query, description = "Filter by the person an agent acted for (the agent's owner)"),
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
) -> Result<Json<AuditLogResponse>, AppError> {
    if require_admin(&state, &headers).await.is_none() {
        return Err(AppError::unauthorized("admin required"));
    }

    // L-4: Standardized to 1-based page numbering (consistent with PaginationParams).
    let page = q.page.unwrap_or(1).max(1);
    let per_page = q.per_page.unwrap_or(20).clamp(1, 100);
    // sea_orm paginator is 0-based internally.
    let page_index = page - 1;

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
        page_index,
        per_page,
        q.user_id,
        q.on_behalf_of_user_id,
        q.action.as_deref(),
        q.resource_type.as_deref(),
        start_time,
        end_time,
    )
    .await
    .map_err(|e| {
        tracing::error!("audit list error: {}", e);
        AppError::internal("database error")
    })?;

    let actors = super::actor_ref::actor_map(&state, logs.iter().filter_map(|m| m.user_id)).await;
    let logs = logs
        .into_iter()
        .map(|m| AuditLogEntry {
            id: m.id,
            user_id: m.user_id,
            username: m.username,
            on_behalf_of_user_id: m.on_behalf_of_user_id,
            user_ref: m.user_id.and_then(|id| actors.get(&id).cloned()),
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
        per_page,
        logs,
    }))
}

/// GET /admin/login-attempts
#[utoipa::path(
    get,
    path = "/admin/login-attempts",
    tag = "Audit",
    params(
        ("page" = Option<u64>, Query, description = "Page number (1-based)"),
        ("per_page" = Option<u64>, Query, description = "Items per page (1-100)"),
        ("username" = Option<String>, Query, description = "Exact username/login filter"),
        ("auth_provider" = Option<String>, Query, description = "Authentication provider filter"),
        ("success" = Option<bool>, Query, description = "Success/failure filter"),
        ("start_time" = Option<String>, Query, description = "ISO 8601 lower bound"),
        ("end_time" = Option<String>, Query, description = "ISO 8601 upper bound"),
    ),
    responses(
        (status = 200, description = "Paginated login attempts", body = LoginAttemptResponse),
        (status = 401, description = "Admin access required"),
    ),
)]
pub async fn list_login_attempts(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<LoginAttemptQuery>,
) -> Result<Json<LoginAttemptResponse>, AppError> {
    if require_admin(&state, &headers).await.is_none() {
        return Err(AppError::unauthorized("admin required"));
    }
    let page = q.page.unwrap_or(1).max(1);
    let per_page = q.per_page.unwrap_or(20).clamp(1, 100);
    let parse_time = |value: Option<&str>, name: &str| {
        value
            .map(|value| {
                chrono::DateTime::<chrono::Utc>::from_str(value)
                    .map_err(|_| AppError::bad_request(format!("{name} must be ISO 8601")))
            })
            .transpose()
    };
    let start_time = parse_time(q.start_time.as_deref(), "start_time")?;
    let end_time = parse_time(q.end_time.as_deref(), "end_time")?;
    if start_time
        .as_ref()
        .zip(end_time.as_ref())
        .is_some_and(|(start, end)| start > end)
    {
        return Err(AppError::bad_request(
            "start_time must be earlier than or equal to end_time",
        ));
    }
    let username = q
        .username
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let auth_provider = q
        .auth_provider
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if username.is_some_and(|value| value.chars().count() > 255) {
        return Err(AppError::bad_request(
            "username filter must not exceed 255 characters",
        ));
    }
    if auth_provider.is_some_and(|value| value.chars().count() > 20) {
        return Err(AppError::bad_request(
            "auth_provider filter must not exceed 20 characters",
        ));
    }
    let (attempts, total) = rg_db::ops::login_log_ops::list_paginated(
        &state.db,
        page - 1,
        per_page,
        username,
        auth_provider,
        q.success,
        start_time,
        end_time,
    )
    .await
    .map_err(|error| {
        tracing::error!(%error, "login attempt list failed");
        AppError::internal("database error")
    })?;
    Ok(Json(LoginAttemptResponse {
        total,
        page,
        per_page,
        attempts: attempts
            .into_iter()
            .map(|attempt| LoginAttemptEntry {
                id: attempt.id,
                user_id: attempt.user_id,
                username: attempt.username,
                auth_provider: attempt.auth_provider,
                ip_address: attempt.ip_address,
                user_agent: attempt.user_agent,
                success: attempt.success,
                failure_reason: attempt.failure_reason,
                created_at: attempt.created_at.to_rfc3339(),
            })
            .collect(),
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
) -> Result<Json<AuditLogEntry>, AppError> {
    if require_admin(&state, &headers).await.is_none() {
        return Err(AppError::unauthorized("admin required"));
    }

    let log = rg_db::ops::audit_log_ops::find_by_id(&state.db, id)
        .await
        .map_err(|e| {
            tracing::error!("audit get error: {}", e);
            AppError::internal("database error")
        })?
        .ok_or_else(|| AppError::not_found("audit log not found"))?;

    let actors = super::actor_ref::actor_map(&state, log.user_id).await;
    Ok(Json(AuditLogEntry {
        id: log.id,
        user_id: log.user_id,
        username: log.username,
        on_behalf_of_user_id: log.on_behalf_of_user_id,
        user_ref: log.user_id.and_then(|id| actors.get(&id).cloned()),
        action: log.action,
        resource_type: log.resource_type,
        resource_id: log.resource_id,
        resource_name: log.resource_name,
        ip_address: log.ip_address,
        details: log.details,
        created_at: log.created_at.to_rfc3339(),
    }))
}

/// Write one audit entry for `user_id` (fire-and-forget: a failure is logged and
/// never fails the request). The IP, user agent and, for a request made with a
/// personal access token, `access_token_id` are taken from `headers`.
///
/// The entries made by the agent-facing write paths (file commits, pull request
/// create / merge / review) use this; older call sites keep their own copies.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn record_write(
    db: &sea_orm::DatabaseConnection,
    user_id: i64,
    action: &str,
    resource_type: &str,
    resource_id: Option<i64>,
    resource_name: &str,
    headers: &HeaderMap,
    details: serde_json::Value,
) {
    // One lookup serves both columns: an agent's entry also names its owner.
    let actor = rg_db::ops::user_ops::find_by_id(db, user_id)
        .await
        .ok()
        .flatten();
    let (username, on_behalf_of_user_id) = match &actor {
        Some(user) if rg_core::agent::is_agent(user) => {
            (Some(user.username.clone()), user.owner_user_id)
        }
        Some(user) => (Some(user.username.clone()), None),
        None => (None, None),
    };
    let (ip_address, user_agent) = extract_ip_and_ua(headers);
    let entry = rg_db::entities::audit_log::ActiveModel {
        id: sea_orm::NotSet,
        user_id: sea_orm::Set(Some(user_id)),
        username: sea_orm::Set(username),
        on_behalf_of_user_id: sea_orm::Set(on_behalf_of_user_id),
        action: sea_orm::Set(action.to_string()),
        resource_type: sea_orm::Set(Some(resource_type.to_string())),
        resource_id: sea_orm::Set(resource_id),
        resource_name: sea_orm::Set(Some(resource_name.to_string())),
        ip_address: sea_orm::Set(ip_address),
        user_agent: sea_orm::Set(user_agent),
        details: sea_orm::Set(with_access_token(headers, Some(details)).map(|v| v.to_string())),
        created_at: sea_orm::Set(chrono::Utc::now()),
    };
    if let Err(e) = rg_db::ops::audit_log_ops::insert(db, entry).await {
        tracing::error!(error = %e, "failed to record audit log");
    }
}

/// Internal request header the PAT middleware sets to the id of the personal
/// access token a request was made with. The middleware removes any value a
/// client sent before it decides whether to set one, so it can be trusted by
/// anything running behind that middleware.
pub(crate) const ACCESS_TOKEN_ID_HEADER: &str = "x-ironforge-access-token-id";

/// Add the id of the personal access token the request was made with, if any,
/// to an audit entry's `details` as `access_token_id`.
///
/// An entry made with an ordinary login is returned unchanged. Details that are
/// not a JSON object are kept under `detail` so the token id never replaces them.
pub(crate) fn with_access_token(
    headers: &HeaderMap,
    details: Option<serde_json::Value>,
) -> Option<serde_json::Value> {
    let Some(token_id) = headers
        .get(ACCESS_TOKEN_ID_HEADER)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<i64>().ok())
    else {
        return details;
    };
    let mut object = match details {
        None => serde_json::Map::new(),
        Some(serde_json::Value::Object(map)) => map,
        Some(other) => {
            let mut map = serde_json::Map::new();
            map.insert("detail".to_string(), other);
            map
        }
    };
    object.insert("access_token_id".to_string(), serde_json::json!(token_id));
    Some(serde_json::Value::Object(object))
}

/// Extract client IP and User-Agent from request headers.
pub(crate) fn extract_ip_and_ua(headers: &HeaderMap) -> (Option<String>, Option<String>) {
    use axum::http::header;

    let ip_address = headers
        .get("X-Forwarded-For")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.split(',').next())
        .and_then(|s| s.trim().parse::<std::net::IpAddr>().ok())
        .map(|address| address.to_string())
        .or_else(|| {
            headers
                .get("X-Real-IP")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.trim().parse::<std::net::IpAddr>().ok())
                .map(|address| address.to_string())
        });

    let user_agent = headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.chars().take(512).collect());

    (ip_address, user_agent)
}

#[cfg(test)]
mod tests {
    use super::{extract_ip_and_ua, with_access_token, ACCESS_TOKEN_ID_HEADER};
    use axum::http::{HeaderMap, HeaderValue};
    use serde_json::json;

    #[test]
    fn the_access_token_id_is_added_to_audit_details_without_replacing_them() {
        let none = HeaderMap::new();
        assert_eq!(with_access_token(&none, None), None);
        assert_eq!(
            with_access_token(&none, Some(json!({"a": 1}))),
            Some(json!({"a": 1}))
        );

        let mut headers = HeaderMap::new();
        headers.insert(ACCESS_TOKEN_ID_HEADER, HeaderValue::from_static("42"));
        assert_eq!(
            with_access_token(&headers, None),
            Some(json!({"access_token_id": 42}))
        );
        assert_eq!(
            with_access_token(&headers, Some(json!({"a": 1}))),
            Some(json!({"a": 1, "access_token_id": 42}))
        );
        assert_eq!(
            with_access_token(&headers, Some(json!("text"))),
            Some(json!({"detail": "text", "access_token_id": 42}))
        );
        // A claimed id the server did not set cannot be anything but a number.
        let mut bad = HeaderMap::new();
        bad.insert(ACCESS_TOKEN_ID_HEADER, HeaderValue::from_static("x"));
        assert_eq!(with_access_token(&bad, None), None);
    }

    #[test]
    fn login_metadata_rejects_fake_ips_and_bounds_user_agents() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-forwarded-for",
            HeaderValue::from_static("not-an-ip, 192.0.2.1"),
        );
        headers.insert("x-real-ip", HeaderValue::from_static("2001:db8::1"));
        headers.insert(
            "user-agent",
            HeaderValue::from_str(&"a".repeat(600)).unwrap(),
        );
        let (ip, user_agent) = extract_ip_and_ua(&headers);
        assert_eq!(ip.as_deref(), Some("2001:db8::1"));
        assert_eq!(user_agent.unwrap().len(), 512);
    }
}
