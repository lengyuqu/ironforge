//! `GET /repos/{owner}/{name}/events`: a repository's events, by cursor.
//!
//! For a client that cannot receive a webhook (an agent without a public
//! address). Every event that would be delivered to a webhook is also appended
//! to `repo_events`; this reads them back in order and can hold the request open
//! until something new arrives.
//!
//! The cursor is the event id, which only ever increases, so reading "after the
//! last id I saw" neither misses nor repeats an event. Events are kept 7 days: a
//! cursor older than that may have missed events, and `oldest_retained_at` lets
//! the client tell. The log is not a snapshot of the repository; to catch up
//! after a long gap, read the repository itself.
//!
//! Each event carries both the business payload (`payload`, the same shape it
//! always had) and the envelope it was published in (`envelope`: `schema`, `id`,
//! `kind`, `at`, `repo_id`, `actor`). Rows written before the envelope existed
//! report `envelope: null` and their payload unchanged, so a reader works across
//! both.

use std::time::{Duration, Instant};

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;
use tokio::sync::Semaphore;

use crate::api::repo_access;
use crate::error::AppError;
use crate::AppState;
use rg_core::webhook::service::looks_like_envelope;

/// How long events are kept.
pub const RETENTION: chrono::Duration = chrono::Duration::days(7);
const DEFAULT_LIMIT: u64 = 100;
const MAX_LIMIT: u64 = 200;
const MAX_TIMEOUT_SECS: u64 = 60;
const POLL: Duration = Duration::from_secs(1);
const MAX_TYPES: usize = 20;
/// Most requests held open at once; more get 429 rather than tie up the server.
static WAIT_PERMITS: Semaphore = Semaphore::const_new(64);

#[derive(Deserialize)]
pub struct EventsQuery {
    /// Return events with an id greater than this. Without it nothing is
    /// returned except `next_cursor`, the current end of the log, so a client can
    /// start from "now".
    #[serde(default)]
    pub since: Option<i64>,
    /// Comma-separated event names (`pull_request,push`); default: all.
    #[serde(default)]
    pub types: Option<String>,
    /// Most events to return (default 100, at most 200).
    #[serde(default)]
    pub limit: Option<u64>,
    /// Seconds to hold the request open when there is nothing new (default 0,
    /// at most 60).
    #[serde(default)]
    pub timeout: Option<u64>,
}

/// An event name: 1–64 characters of `[a-z0-9._-]`.
fn valid_event_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name.bytes().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'_' | b'-')
        })
}

fn parse_types(raw: Option<&str>) -> Result<Vec<String>, AppError> {
    let Some(raw) = raw else {
        return Ok(Vec::new());
    };
    let mut types: Vec<String> = Vec::new();
    for part in raw.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if !valid_event_name(part) {
            return Err(AppError::bad_request(
                "types must be comma-separated event names of [a-z0-9._-], at most 64 characters each",
            ));
        }
        if !types.iter().any(|t| t == part) {
            types.push(part.to_string());
        }
    }
    if types.len() > MAX_TYPES {
        return Err(AppError::bad_request(format!(
            "at most {MAX_TYPES} event types may be given"
        )));
    }
    Ok(types)
}

/// Split a stored event payload into `(business payload, envelope)`.
///
/// Rows written after the event envelope was introduced hold
/// `{ schema, id, kind, at, repo_id, actor, data }`; rows written before it hold
/// the business payload at the top level. `payload` is the business payload for
/// both, so an existing consumer keeps reading the fields it knows, and
/// `envelope` is the extra — `null` for a row written before it existed.
fn unpack_payload(stored: Option<&str>) -> (Option<serde_json::Value>, Option<serde_json::Value>) {
    let Some(value) = stored.and_then(|text| serde_json::from_str::<serde_json::Value>(text).ok())
    else {
        return (None, None);
    };
    if !looks_like_envelope(&value) {
        return (Some(value), None);
    }
    let data = value.get("data").cloned().filter(|data| !data.is_null());
    (data, Some(value))
}

/// GET /api/v1/repos/:owner/:name/events
#[utoipa::path(
    get,
    path = "/repos/{owner}/{name}/events",
    tag = "Repositories",
    params(
        ("owner" = String, Path, description = "owner"),
        ("name" = String, Path, description = "name"),
        ("since" = Option<i64>, Query, description = "Return events after this cursor; omit to get only the current cursor"),
        ("types" = Option<String>, Query, description = "Comma-separated event names"),
        ("limit" = Option<u64>, Query, description = "Most events to return (default 100, at most 200)"),
        ("timeout" = Option<u64>, Query, description = "Seconds to wait for a new event (default 0, at most 60)"),
    ),
    responses(
        (status = 200, description = "Events after the cursor, with next_cursor", body = serde_json::Value),
        (status = 400, description = "Bad parameters", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
        (status = 404, description = "Repository not found", body = serde_json::Value),
        (status = 429, description = "Too many concurrent waits", body = serde_json::Value),
    ),
)]
pub async fn list_events(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((owner, name)): Path<(String, String)>,
    Query(params): Query<EventsQuery>,
) -> impl IntoResponse {
    let repo = match repo_access::require_read(&state, &headers, &owner, &name).await {
        Ok(repo) => repo,
        Err(e) => return e.into_response(),
    };
    let types = match parse_types(params.types.as_deref()) {
        Ok(types) => types,
        Err(e) => return e.into_response(),
    };
    if params.since.is_some_and(|since| since < 0) {
        return AppError::bad_request("since must not be negative").into_response();
    }
    let limit = params.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    let timeout = Duration::from_secs(params.timeout.unwrap_or(0).min(MAX_TIMEOUT_SECS));

    let oldest_retained_at =
        match rg_db::ops::repo_event_ops::oldest_created_at(&state.db, repo.id).await {
            Ok(at) => at,
            Err(e) => return AppError::from(e).into_response(),
        };

    // No cursor: report where the log ends now, so the client can start there.
    let Some(since) = params.since else {
        let latest = match rg_db::ops::repo_event_ops::latest_id(&state.db, repo.id).await {
            Ok(id) => id,
            Err(e) => return AppError::from(e).into_response(),
        };
        return (
            StatusCode::OK,
            Json(serde_json::json!({
                "events": [],
                "next_cursor": latest,
                "timed_out": false,
                "retention_days": RETENTION.num_days(),
                "oldest_retained_at": oldest_retained_at,
            })),
        )
            .into_response();
    };

    let deadline = Instant::now() + timeout;
    let mut _permit = None;
    loop {
        let rows =
            match rg_db::ops::repo_event_ops::list_after(&state.db, repo.id, since, &types, limit)
                .await
            {
                Ok(rows) => rows,
                Err(e) => return AppError::from(e).into_response(),
            };
        let remaining = deadline.saturating_duration_since(Instant::now());
        if !rows.is_empty() || remaining.is_zero() {
            let timed_out = rows.is_empty();
            let next_cursor = rows.last().map(|row| row.id).unwrap_or(since);
            let events: Vec<serde_json::Value> = rows
                .into_iter()
                .map(|row| {
                    let (payload, envelope) = unpack_payload(row.payload.as_deref());
                    serde_json::json!({
                        "id": row.id,
                        "type": row.event_type,
                        "created_at": row.created_at,
                        // Parts of a payload (titles, comment text) were written
                        // by users: data, never instructions.
                        "payload": payload,
                        // The kind/id/timestamp/actor wrapper, when the row was
                        // written after the envelope existed.
                        "envelope": envelope,
                        "payload_origin": "user_content",
                    })
                })
                .collect();
            return (
                StatusCode::OK,
                Json(serde_json::json!({
                    "events": events,
                    "next_cursor": next_cursor,
                    "timed_out": timed_out,
                    "retention_days": RETENTION.num_days(),
                    "oldest_retained_at": oldest_retained_at,
                })),
            )
                .into_response();
        }
        if _permit.is_none() {
            match WAIT_PERMITS.try_acquire() {
                Ok(permit) => _permit = Some(permit),
                Err(_) => {
                    return AppError::rate_limited("too many concurrent event waits")
                        .into_response()
                }
            }
        }
        tokio::time::sleep(POLL.min(remaining)).await;
    }
}

/// Delete events older than the retention period, forever.
pub async fn run_cleanup_loop(state: AppState) {
    let mut interval = tokio::time::interval(Duration::from_secs(3600));
    loop {
        interval.tick().await;
        let before = chrono::Utc::now() - RETENTION;
        match rg_db::ops::repo_event_ops::delete_older_than(&state.db, before).await {
            Ok(0) => {}
            Ok(count) => tracing::info!(count, "deleted expired repository events"),
            Err(error) => tracing::warn!(%error, "repository event cleanup failed"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_names_are_short_and_plain() {
        assert!(valid_event_name("push"));
        assert!(valid_event_name("collab.question.asked"));
        assert!(valid_event_name("pull_request"));
        assert!(!valid_event_name(""));
        assert!(!valid_event_name("Push"));
        assert!(!valid_event_name("a b"));
        assert!(!valid_event_name(&"x".repeat(65)));
    }

    #[test]
    fn the_type_filter_is_validated_and_deduplicated() {
        assert!(parse_types(None).unwrap().is_empty());
        assert_eq!(
            parse_types(Some("push, pull_request,push,")).unwrap(),
            vec!["push", "pull_request"]
        );
        assert!(parse_types(Some("push,BAD NAME")).is_err());
        let many: Vec<String> = (0..21).map(|n| format!("e{n}")).collect();
        assert!(parse_types(Some(&many.join(","))).is_err());
    }

    #[test]
    fn a_payload_reads_the_same_with_or_without_an_envelope() {
        // Written before the envelope: the business payload is the whole row.
        let legacy = r#"{"ref":"refs/heads/main","after":"abc"}"#;
        let (payload, envelope) = unpack_payload(Some(legacy));
        assert_eq!(payload.unwrap()["after"], "abc");
        assert!(envelope.is_none());

        // Written after: the business payload is under `data`, unchanged, and
        // the envelope rides beside it.
        let stored = r#"{"schema":1,"id":"018f-1","kind":"push","repo_id":7,
            "at":"2026-11-14T12:00:00Z","actor":null,
            "data":{"ref":"refs/heads/main","after":"abc"}}"#;
        let (payload, envelope) = unpack_payload(Some(stored));
        assert_eq!(payload.unwrap()["after"], "abc");
        let envelope = envelope.unwrap();
        assert_eq!(envelope["kind"], "push");
        assert_eq!(envelope["repo_id"], 7);

        // Unparseable or missing payloads stay absent rather than failing.
        assert_eq!(unpack_payload(None), (None, None));
        assert_eq!(unpack_payload(Some("not json")), (None, None));
    }
}
