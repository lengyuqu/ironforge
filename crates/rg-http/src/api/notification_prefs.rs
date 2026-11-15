//! REST API for notification preferences (#19 step 5).
//!
//! `GET /user/notification-preferences` answers "what applies to me": one row
//! per catalogue event type and channel with the **derived** value, plus every
//! explicit repository override. `PUT` takes a bare JSON array of overrides.
//!
//! The two endpoints are deliberately asymmetric in shape and symmetric in
//! meaning: GET expands the defaults, PUT stores deviations. Nothing here
//! initialises rows for a fresh account — absence means "on", so the first PUT
//! is the first row (see the migration and
//! `notification_preference_ops::effective`).

use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::error::AppError;
use crate::AppState;

/// Most overrides one request may carry. A settings page sends a handful; the
/// cap keeps one request from becoming an unbounded write loop.
const MAX_BATCH: usize = 200;

#[derive(Debug, Serialize, ToSchema)]
pub struct PreferenceResponse {
    /// `null` = the global default; a value = this repository's override.
    pub repo_id: Option<i64>,
    pub event_type: String,
    /// `in_app` or `email`.
    pub channel: String,
    pub enabled: bool,
    /// `false` = derived from the default (nothing is stored for this scope).
    pub explicit: bool,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PreferencesResponse {
    /// Absence of a stored row means "on"; stated so a client does not have to
    /// hard-code the rule.
    pub default_enabled: bool,
    pub preferences: Vec<PreferenceResponse>,
}

/// One override.
#[derive(Debug, Deserialize, ToSchema)]
pub struct PreferenceUpdate {
    /// `null` = change the global default.
    #[serde(default)]
    pub repo_id: Option<i64>,
    pub event_type: String,
    pub channel: String,
    pub enabled: bool,
}

fn unauthorised() -> axum::response::Response {
    AppError::unauthorized("authentication required").into_response()
}

/// GET /user/notification-preferences
///
/// The effective preference of the caller: catalogue defaults expanded, then
/// the caller's explicit repository overrides.
#[utoipa::path(
    get,
    path = "/user/notification-preferences",
    tag = "Notifications",
    responses(
        (status = 200, description = "Effective preferences", body = PreferencesResponse),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
    ),
)]
pub async fn list_preferences(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let Some(user_id) = super::auth::extract_user_id(&headers, &state.jwt_secret) else {
        return unauthorised();
    };
    match rg_core::notification::dispatch::effective_matrix(&state.db, user_id).await {
        Ok(rows) => Json(PreferencesResponse {
            default_enabled: true,
            preferences: rows
                .into_iter()
                .map(|row| PreferenceResponse {
                    repo_id: row.repo_id,
                    event_type: row.event_type,
                    channel: row.channel,
                    enabled: row.enabled,
                    explicit: row.explicit,
                })
                .collect(),
        })
        .into_response(),
        Err(error) => {
            tracing::warn!(user_id, %error, "failed to read notification preferences");
            AppError::internal("could not read notification preferences").into_response()
        }
    }
}

/// PUT /user/notification-preferences
///
/// Batch upsert. The body is a bare array of overrides; `enabled = true` stores
/// an explicit "on" (useful to re-enable one repository after a global mute),
/// `enabled = false` stores the mute. Responds with the same shape as GET.
#[utoipa::path(
    put,
    path = "/user/notification-preferences",
    tag = "Notifications",
    request_body = Vec<PreferenceUpdate>,
    responses(
        (status = 200, description = "Stored; effective preferences returned", body = PreferencesResponse),
        (status = 400, description = "Invalid channel, event type or repository", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
    ),
)]
pub async fn update_preferences(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(updates): Json<Vec<PreferenceUpdate>>,
) -> impl IntoResponse {
    let Some(user_id) = super::auth::extract_user_id(&headers, &state.jwt_secret) else {
        return unauthorised();
    };
    if updates.len() > MAX_BATCH {
        return AppError::bad_request(format!("at most {MAX_BATCH} preferences per request"))
            .into_response();
    }
    for update in &updates {
        let event_type = update.event_type.trim();
        if event_type.is_empty() || event_type.chars().count() > 64 {
            return AppError::bad_request("event_type must contain 1-64 characters".to_string())
                .into_response();
        }
        if !rg_db::entities::notification_preference::is_known_channel(&update.channel) {
            return AppError::bad_request(format!(
                "unknown channel '{}'; expected in_app or email",
                update.channel
            ))
            .into_response();
        }
        // The foreign key would turn an unknown repository into a 500; answer
        // 400 instead. Whether the caller may *read* the repository is not
        // checked: the row is scoped to the caller and reveals nothing.
        if let Some(repo_id) = update.repo_id {
            match rg_db::ops::repo_ops::find_by_id(&state.db, repo_id).await {
                Ok(Some(_)) => {}
                Ok(None) => {
                    return AppError::bad_request(format!("repository {repo_id} not found"))
                        .into_response();
                }
                Err(error) => {
                    tracing::warn!(user_id, repo_id, %error, "failed to resolve repository");
                    return AppError::internal("could not read repository").into_response();
                }
            }
        }
    }

    for update in &updates {
        if let Err(error) = rg_db::ops::notification_preference_ops::upsert(
            &state.db,
            user_id,
            update.repo_id,
            update.event_type.trim(),
            &update.channel,
            update.enabled,
        )
        .await
        {
            tracing::warn!(user_id, %error, "failed to store notification preference");
            return AppError::internal("could not store notification preference").into_response();
        }
    }

    match rg_core::notification::dispatch::effective_matrix(&state.db, user_id).await {
        Ok(rows) => Json(PreferencesResponse {
            default_enabled: true,
            preferences: rows
                .into_iter()
                .map(|row| PreferenceResponse {
                    repo_id: row.repo_id,
                    event_type: row.event_type,
                    channel: row.channel,
                    enabled: row.enabled,
                    explicit: row.explicit,
                })
                .collect(),
        })
        .into_response(),
        Err(error) => {
            tracing::warn!(user_id, %error, "failed to read notification preferences");
            AppError::internal("could not read notification preferences").into_response()
        }
    }
}
