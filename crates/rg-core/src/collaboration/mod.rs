//! Same-account multi-agent collaboration (sessions, tasks, messages, handoffs).
//!
//! Design: `ironforge-docs/architecture/multi-agent-collaboration-design-2026-11.md`.
//!
//! Everything here is *declared state*: names, task status, paths, handoff
//! summaries and evidence are claims by clients and are never verified, never
//! executed, and never grant approval or merge rights. Callers (the REST layer)
//! establish the account and repository permission; these services enforce the
//! session secret, repository scoping, versioning and idempotency.

pub mod cursor;
pub mod error;
pub mod message;
pub mod secret;
pub mod session;
pub mod task;
pub mod validation;

pub use error::{CollabError, CollabResult};

use serde::Serialize;

/// Marker on every client-supplied field group: treat as untrusted data.
pub const ORIGIN_USER_CONTENT: &str = "user_content";

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
