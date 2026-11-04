//! Capability and environment declarations (O07).
//!
//! A session says which tools it has, whether it can run tests and what its
//! network looks like, so that others can decide who to hand work to. This is a
//! **self-report**: the platform never checks it, it can be stale or wrong, and
//! it authorizes nothing — no declaration lets any action happen, and the
//! absence of one means "unknown", not "cannot". Every view carries
//! `source: "user_content"` and `authorization: "none"`.
//!
//! The declaration is replaced as a whole under an optimistic `version`; the
//! server sets `declared_at` itself (the client's clock is not trusted for
//! staleness). A declaration is stale when it is older than a threshold
//! (default 24 h, set per request) or when its session has ended or expired.

use chrono::{DateTime, Utc};
use rg_db::entities::{agent_session, collab_session_declaration};
use rg_db::ops::agent_session_ops;
use rg_db::ops::collab_declaration_ops::{self, PutOutcome};
use sea_orm::{DatabaseConnection, Set};
use serde::Serialize;
use sha2::{Digest, Sha256};

use super::cursor::{self, KeysetPosition};
use super::error::{CollabError, CollabResult};
use super::{session, validation, Page, ORIGIN_USER_CONTENT};

pub const CAN_RUN_TESTS: [&str; 3] = ["yes", "no", "unknown"];
pub const NETWORK: [&str; 4] = ["unrestricted", "restricted", "offline", "unknown"];
pub const MAX_TOOLS: usize = 50;
pub const MAX_TOOL_CHARS: usize = 128;
pub const MAX_NETWORK_NOTES_BYTES: usize = 1024;
pub const MAX_NOTES_BYTES: usize = 4096;

pub const DEFAULT_STALE_AFTER_SECS: i64 = 24 * 60 * 60;
pub const MIN_STALE_AFTER_SECS: i64 = 60;
pub const MAX_STALE_AFTER_SECS: i64 = 30 * 24 * 60 * 60;

/// Stated on every response so no consumer reads more into a declaration.
pub const MEANING: &str = "Self-reported by the session; the platform did not verify it. It grants no permission and authorizes nothing, and an old or missing declaration only means the capabilities are unknown.";

#[derive(Debug, Clone, Default)]
pub struct NewDeclaration {
    /// The version the session last saw; 0 when it has not declared yet.
    pub expected_version: i64,
    pub tools: Vec<String>,
    pub can_run_tests: Option<String>,
    pub network: Option<String>,
    pub network_notes: String,
    pub notes: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct DeclarationView {
    pub session_id: String,
    pub version: i64,
    pub tools: Vec<String>,
    /// `yes` | `no` | `unknown`
    pub can_run_tests: String,
    /// `unrestricted` | `restricted` | `offline` | `unknown`
    pub network: String,
    pub network_notes: String,
    pub notes: String,
    pub declared_at: DateTime<Utc>,
    pub age_seconds: i64,
    pub stale: bool,
    /// Why it is stale: `age`, `session_ended`, `session_expired`.
    pub stale_reasons: Vec<&'static str>,
    pub stale_after_seconds: i64,
    /// State of the declaring session (`online` | `offline` | `expired` | `ended`).
    pub session_state: &'static str,
    /// Always `user_content`.
    pub source: &'static str,
    /// Always `none`: a declaration never authorizes anything.
    pub authorization: &'static str,
    pub meaning: &'static str,
}

/// A session's declaration, or the explicit absence of one.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SessionDeclaration {
    pub session_id: String,
    pub session_state: &'static str,
    /// `declared` | `undeclared`. `undeclared` means unknown, not "cannot".
    pub status: &'static str,
    pub declaration: Option<DeclarationView>,
    pub authorization: &'static str,
    pub meaning: &'static str,
}

pub struct Saved {
    pub declaration: DeclarationView,
    pub replayed: bool,
}

/// Clamp a client-supplied staleness threshold into the supported range.
pub fn stale_threshold(seconds: Option<i64>) -> i64 {
    seconds
        .unwrap_or(DEFAULT_STALE_AFTER_SECS)
        .clamp(MIN_STALE_AFTER_SECS, MAX_STALE_AFTER_SECS)
}

fn choice(field: &str, value: Option<String>, allowed: &[&str]) -> CollabResult<String> {
    let value = value.unwrap_or_else(|| "unknown".to_string());
    if allowed.contains(&value.as_str()) {
        Ok(value)
    } else {
        Err(CollabError::bad_request(format!("{field} must be one of {}", allowed.join(", "))))
    }
}

fn view(
    row: collab_session_declaration::Model,
    session: &agent_session::Model,
    stale_after: i64,
    now: DateTime<Utc>,
) -> DeclarationView {
    let age = (now - row.declared_at).num_seconds().max(0);
    let state = session::state_of(session, now);
    let mut reasons = Vec::new();
    if age > stale_after {
        reasons.push("age");
    }
    match state {
        "ended" => reasons.push("session_ended"),
        "expired" => reasons.push("session_expired"),
        _ => {}
    }
    DeclarationView {
        tools: serde_json::from_str(&row.tools_json).unwrap_or_default(),
        session_id: row.session_id,
        version: row.version,
        can_run_tests: row.can_run_tests,
        network: row.network,
        network_notes: row.network_notes,
        notes: row.notes,
        declared_at: row.declared_at,
        age_seconds: age,
        stale: !reasons.is_empty(),
        stale_reasons: reasons,
        stale_after_seconds: stale_after,
        session_state: state,
        source: ORIGIN_USER_CONTENT,
        authorization: "none",
        meaning: MEANING,
    }
}

/// Replace the authenticated session's declaration. `session` is the session
/// the caller authenticated with its secret; nobody can declare for another
/// session.
pub async fn put(
    db: &DatabaseConnection,
    repo_id: i64,
    session: &agent_session::Model,
    input: NewDeclaration,
    stale_after: i64,
    now: DateTime<Utc>,
) -> CollabResult<Saved> {
    if input.expected_version < 0 {
        return Err(CollabError::bad_request("expected_version must not be negative"));
    }
    if input.tools.len() > MAX_TOOLS {
        return Err(CollabError::bad_request(format!("tools may have at most {MAX_TOOLS} entries")));
    }
    let tools = input
        .tools
        .iter()
        .map(|tool| validation::label("tools entry", tool, MAX_TOOL_CHARS))
        .collect::<CollabResult<Vec<_>>>()?;
    let can_run_tests = choice("can_run_tests", input.can_run_tests, &CAN_RUN_TESTS)?;
    let network = choice("network", input.network, &NETWORK)?;
    validation::text("network_notes", &input.network_notes, 0, MAX_NETWORK_NOTES_BYTES)?;
    validation::text("notes", &input.notes, 0, MAX_NOTES_BYTES)?;
    let tools_json = serde_json::to_string(&tools).map_err(|e| CollabError::Internal(e.into()))?;
    let hash = hex::encode(Sha256::digest(
        serde_json::json!([tools, can_run_tests, network, input.network_notes, input.notes])
            .to_string()
            .as_bytes(),
    ));

    let outcome = collab_declaration_ops::put(
        db,
        repo_id,
        input.expected_version,
        collab_session_declaration::ActiveModel {
            session_id: Set(session.id.clone()),
            repo_id: Set(repo_id),
            version: Set(input.expected_version + 1),
            tools_json: Set(tools_json),
            can_run_tests: Set(can_run_tests),
            network: Set(network),
            network_notes: Set(input.network_notes),
            notes: Set(input.notes),
            payload_hash: Set(hash),
            declared_at: Set(now),
        },
    )
    .await?;
    match outcome {
        PutOutcome::Saved(row) => Ok(Saved { declaration: view(row, session, stale_after, now), replayed: false }),
        PutOutcome::Replayed(row) => Ok(Saved { declaration: view(row, session, stale_after, now), replayed: true }),
        PutOutcome::Conflict { current_version } => Err(CollabError::VersionConflict { current_version }),
    }
}

/// One session's declaration, or `undeclared`. The session must belong to the
/// repository.
pub async fn get(
    db: &DatabaseConnection,
    repo_id: i64,
    session_id: &str,
    stale_after: i64,
    now: DateTime<Utc>,
) -> CollabResult<SessionDeclaration> {
    if !validation::is_uuid(session_id) {
        return Err(CollabError::not_found("session not found"));
    }
    let session = agent_session_ops::find_in_repo(db, repo_id, session_id)
        .await?
        .ok_or_else(|| CollabError::not_found("session not found"))?;
    let row = collab_declaration_ops::find(db, repo_id, session_id).await?;
    Ok(SessionDeclaration {
        session_id: session.id.clone(),
        session_state: session::state_of(&session, now),
        status: if row.is_some() { "declared" } else { "undeclared" },
        declaration: row.map(|row| view(row, &session, stale_after, now)),
        authorization: "none",
        meaning: MEANING,
    })
}

fn list_scope(repo_id: i64) -> String {
    format!("declarations:v1:repo={repo_id}:order=declared_desc,session_desc")
}

/// Declarations of the repository, most recently declared first. Sessions that
/// never declared are absent; join with the session listing by `session_id`.
pub async fn list(
    db: &DatabaseConnection,
    cursor_key: &[u8],
    repo_id: i64,
    stale_after: i64,
    limit: u64,
    cursor_token: Option<&str>,
    now: DateTime<Utc>,
) -> CollabResult<Page<DeclarationView>> {
    let scope = list_scope(repo_id);
    let after = cursor_token
        .map(|token| cursor::decode(cursor_key, &scope, token))
        .transpose()?
        .map(|p| (p.at, p.id));
    let mut rows = collab_declaration_ops::list_page(db, repo_id, after, limit + 1).await?;
    let has_more = rows.len() as u64 > limit;
    rows.truncate(limit as usize);
    let next_cursor = has_more.then(|| rows.last()).flatten().map(|last| {
        cursor::encode(
            cursor_key,
            &scope,
            &KeysetPosition { at: last.declared_at, id: last.session_id.clone() },
        )
    });
    let mut items = Vec::with_capacity(rows.len());
    for row in rows {
        if let Some(session) = agent_session_ops::find_in_repo(db, repo_id, &row.session_id).await? {
            items.push(view(row, &session, stale_after, now));
        }
    }
    Ok(Page { items, next_cursor })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn threshold_is_clamped() {
        assert_eq!(stale_threshold(None), 86_400);
        assert_eq!(stale_threshold(Some(1)), MIN_STALE_AFTER_SECS);
        assert_eq!(stale_threshold(Some(i64::MAX)), MAX_STALE_AFTER_SECS);
        assert_eq!(stale_threshold(Some(3600)), 3600);
    }

    #[test]
    fn choices_are_closed_and_default_to_unknown() {
        assert_eq!(choice("n", None, &NETWORK).unwrap(), "unknown");
        assert!(choice("n", Some("fast".into()), &NETWORK).is_err());
        assert!(choice("t", Some("yes".into()), &CAN_RUN_TESTS).is_ok());
    }
}
