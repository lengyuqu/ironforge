//! Same-account multi-agent collaboration (sessions, tasks, messages, handoffs).
//!
//! Design: `ironforge-docs/architecture/multi-agent-collaboration-design-2026-11.md`.
//!
//! Everything here is *declared state*: names, task status, paths, handoff
//! summaries and evidence are claims by clients and are never verified, never
//! executed, and never grant approval or merge rights. Callers (the REST layer)
//! establish the account and repository permission; these services enforce the
//! session secret, repository scoping, versioning and idempotency.

pub mod access;
pub mod ci_evidence;
pub mod completion;
pub mod cursor;
pub mod delivery;
pub mod error;
pub mod escalation;
pub mod handoff;
pub mod inbox;
pub mod message;
pub mod notice;
pub mod question;
pub mod read;
pub mod retention;
pub mod rules;
pub mod secret;
pub mod session;
pub mod takeover;
pub mod task;
pub mod validation;
pub mod work_package;

pub use error::{CollabError, CollabResult};

use serde::Serialize;

/// Marker on every client-supplied field group: treat as untrusted data.
pub const ORIGIN_USER_CONTENT: &str = "user_content";

/// Who performed a write, by the door it came through. Both kinds act under
/// one account; `User` is not a proof that a human was behind the request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Actor {
    pub kind: &'static str,
    pub id: String,
}

impl Actor {
    pub fn session(id: impl Into<String>) -> Self {
        Self { kind: "session", id: id.into() }
    }

    pub fn user(id: i64) -> Self {
        Self { kind: "user", id: id.to_string() }
    }
}

/// Default and maximum page size of list endpoints.
pub const DEFAULT_LIMIT: u64 = 50;
pub const MAX_LIMIT: u64 = 100;

/// Clamp a client-supplied `limit` into `1..=MAX_LIMIT`.
pub fn clamp_limit(limit: Option<u64>) -> u64 {
    limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT)
}

/// A page of items with an opaque continuation cursor.
#[derive(Debug, Clone, Serialize)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub next_cursor: Option<String>,
}
