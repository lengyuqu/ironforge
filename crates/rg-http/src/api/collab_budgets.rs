//! Task budget and usage endpoints (O08).
//!
//! Informational bookkeeping: budgets are set by writers, usage is reported by
//! sessions (or a user door), and every figure carries its unit and source.
//! Different units or sources are never summed or converted, missing usage is
//! reported as unknown rather than zero, and no threshold pauses or blocks
//! anything.

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use chrono::{DateTime, Utc};
use rg_core::collaboration::{self, budget as svc};
use serde::Deserialize;
use utoipa::ToSchema;

use super::collab::{cursor_key, require_collab_read, require_collab_write, write_actor};
use crate::error::AppError;
use crate::AppState;

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SetBudgetRequest {
    /// The budget's `version` you last saw for this unit and source; 0 when
    /// none exists. A stale value is a `VERSION_CONFLICT`.
    pub expected_version: i64,
    /// Required, 1-32 of `[A-Za-z0-9._:/@+-]`. Compared exactly and never
    /// converted (`USD` and `usd` are different units).
    pub unit: String,
    /// Required, 1-64 of the same characters: where the figures come from.
    pub source: String,
    /// Non-negative decimal, greater than zero, at most 18 integer and 6
    /// fractional digits. A JSON string or number.
    #[schema(value_type = String, example = "25.00")]
    pub amount: serde_json::Value,
    /// Optional lower threshold in the same unit (not above `amount`);
    /// `over_threshold` is judged against it when present.
    #[schema(value_type = Option<String>)]
    pub alert_at: Option<serde_json::Value>,
    /// Required free text (up to 64 characters): the period the source's
    /// measurements cover or are sampled over, e.g. `task_total`, `per_day`.
    pub sampling_period: String,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ReportUsageRequest {
    /// Idempotency key, unique per reporting session or user.
    pub client_usage_id: String,
    pub unit: String,
    pub source: String,
    /// Non-negative decimal consumed during the period (a delta, not a running
    /// total). A JSON string or number.
    #[schema(value_type = String, example = "0.42")]
    pub amount: serde_json::Value,
    pub period_start: DateTime<Utc>,
    pub period_end: DateTime<Utc>,
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct UsageListQuery {
    /// Exact unit filter.
    pub unit: Option<String>,
    /// Exact source filter.
    pub source: Option<String>,
    pub limit: Option<u64>,
    pub cursor: Option<String>,
}

/// An amount arrives as a JSON string or number; numbers are read as their
/// decimal text so no float rounding is introduced here.
fn amount_text(field: &str, value: serde_json::Value) -> Result<String, AppError> {
    match value {
        serde_json::Value::String(s) => Ok(s),
        serde_json::Value::Number(n) => Ok(n.to_string()),
        _ => Err(AppError::bad_request(format!(
            "{field} must be a decimal string or number"
        ))),
    }
}

#[utoipa::path(
    put,
    path = "/repos/{owner}/{name}/collaboration/tasks/{id}/budgets",
    tag = "Collaboration",
    params(("owner" = String, Path), ("name" = String, Path), ("id" = String, Path)),
    request_body = SetBudgetRequest,
    responses(
        (status = 200, description = "Budget replaced (or an identical earlier replace replayed), with its current usage evaluation", body = serde_json::Value),
        (status = 201, description = "Budget created", body = serde_json::Value),
        (status = 400, description = "Invalid field"),
        (status = 403, description = "Write access denied or invalid session credentials"),
        (status = 404, description = "No such task in this repository"),
        (status = 409, description = "VERSION_CONFLICT, BUDGET_LIMIT_REACHED or SESSION_EXPIRED")
    )
)]
pub async fn set_budget(
    State(state): State<AppState>,
    Path((owner, name, id)): Path<(String, String, String)>,
    headers: HeaderMap,
    super::collab_json::CollabJson(req): super::collab_json::CollabJson<SetBudgetRequest>,
) -> Result<(StatusCode, Json<svc::BudgetEntry>), AppError> {
    let (repo, user_id) = require_collab_write(&state, &headers, &owner, &name).await?;
    let actor = write_actor(&state, &headers, repo.id, user_id).await?;
    let created = req.expected_version == 0;
    let saved = svc::set_budget(
        &state.db,
        repo.id,
        &actor,
        &id,
        svc::SetBudget {
            expected_version: req.expected_version,
            unit: req.unit,
            source: req.source,
            amount: amount_text("amount", req.amount)?,
            alert_at: req
                .alert_at
                .map(|v| amount_text("alert_at", v))
                .transpose()?,
            sampling_period: req.sampling_period,
        },
        Utc::now(),
    )
    .await?;
    let status = if created && !saved.replayed {
        StatusCode::CREATED
    } else {
        StatusCode::OK
    };
    Ok((status, Json(saved.budget)))
}

#[utoipa::path(
    get,
    path = "/repos/{owner}/{name}/collaboration/tasks/{id}/budget",
    tag = "Collaboration",
    params(("owner" = String, Path), ("name" = String, Path), ("id" = String, Path)),
    responses(
        (status = 200, description = "Budgets with matched usage (per exact unit and source; `unknown` + reason, never zero, when nothing was reported), derived `over_threshold` with its numbers, and unmatched usage. Informational: nothing is enforced", body = serde_json::Value),
        (status = 403, description = "Read access denied"),
        (status = 404, description = "No such task in this repository")
    )
)]
pub async fn get_budget(
    State(state): State<AppState>,
    Path((owner, name, id)): Path<(String, String, String)>,
    headers: HeaderMap,
) -> Result<Json<svc::BudgetSummary>, AppError> {
    let (repo, _user) = require_collab_read(&state, &headers, &owner, &name).await?;
    Ok(Json(
        svc::summary(&state.db, repo.id, &id, Utc::now()).await?,
    ))
}

#[utoipa::path(
    post,
    path = "/repos/{owner}/{name}/collaboration/tasks/{id}/usage",
    tag = "Collaboration",
    params(("owner" = String, Path), ("name" = String, Path), ("id" = String, Path)),
    request_body = ReportUsageRequest,
    responses(
        (status = 201, description = "Usage record appended", body = serde_json::Value),
        (status = 200, description = "Identical earlier request replayed", body = serde_json::Value),
        (status = 400, description = "Invalid field"),
        (status = 403, description = "Write access denied or invalid session credentials"),
        (status = 404, description = "No such task in this repository"),
        (status = 409, description = "IDEMPOTENCY_KEY_REUSED, USAGE_LIMIT_REACHED or SESSION_EXPIRED")
    )
)]
pub async fn report_usage(
    State(state): State<AppState>,
    Path((owner, name, id)): Path<(String, String, String)>,
    headers: HeaderMap,
    super::collab_json::CollabJson(req): super::collab_json::CollabJson<ReportUsageRequest>,
) -> Result<(StatusCode, Json<svc::UsageView>), AppError> {
    let (repo, user_id) = require_collab_write(&state, &headers, &owner, &name).await?;
    let actor = write_actor(&state, &headers, repo.id, user_id).await?;
    let created = svc::report_usage(
        &state.db,
        repo.id,
        &actor,
        &id,
        svc::NewUsage {
            client_usage_id: req.client_usage_id,
            unit: req.unit,
            source: req.source,
            amount: amount_text("amount", req.amount)?,
            period_start: req.period_start,
            period_end: req.period_end,
        },
        Utc::now(),
    )
    .await?;
    let status = if created.replayed {
        StatusCode::OK
    } else {
        StatusCode::CREATED
    };
    Ok((status, Json(created.usage)))
}

#[utoipa::path(
    get,
    path = "/repos/{owner}/{name}/collaboration/tasks/{id}/usage",
    tag = "Collaboration",
    params(("owner" = String, Path), ("name" = String, Path), ("id" = String, Path), UsageListQuery),
    responses(
        (status = 200, description = "Usage records, newest reported first: {items, next_cursor}. Each is the reporter's claim", body = serde_json::Value),
        (status = 403, description = "Read access denied"),
        (status = 404, description = "No such task in this repository")
    )
)]
pub async fn list_usage(
    State(state): State<AppState>,
    Path((owner, name, id)): Path<(String, String, String)>,
    Query(query): Query<UsageListQuery>,
    headers: HeaderMap,
) -> Result<Json<collaboration::Page<svc::UsageView>>, AppError> {
    let (repo, _user) = require_collab_read(&state, &headers, &owner, &name).await?;
    Ok(Json(
        svc::list_usage(
            &state.db,
            cursor_key(&state),
            repo.id,
            &id,
            query.unit.as_deref(),
            query.source.as_deref(),
            collaboration::clamp_limit(query.limit),
            query.cursor.as_deref(),
        )
        .await?,
    ))
}
