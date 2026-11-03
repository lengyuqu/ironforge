//! Errors of the collaboration domain.
//!
//! Unlike [`crate::error::CoreError`] these carry the machine-readable codes the
//! REST contract promises (`SESSION_EXPIRED`, `VERSION_CONFLICT`,
//! `IDEMPOTENCY_KEY_REUSED`), so clients never have to parse messages.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum CollabError {
    /// The addressed object does not exist in this repository. Also used for
    /// objects of other repositories, so existence never leaks across repos.
    #[error("{0}")]
    NotFound(String),

    /// Malformed or out-of-range input.
    #[error("{0}")]
    BadRequest(String),

    /// Session id/secret rejected. One variant for "unknown id", "wrong secret",
    /// "other account" and "other repository" so the response is no oracle.
    #[error("invalid session credentials")]
    SessionCredentials,

    /// The session credentials are valid but the session has ended or passed its
    /// hard expiry; it cannot be revived.
    #[error("session has ended or expired")]
    SessionExpired,

    /// Optimistic-lock failure; `current_version` is the winner's version.
    #[error("version conflict")]
    VersionConflict { current_version: i64 },

    /// `client_*_id` was used before with a different request body.
    #[error("idempotency key was already used with a different payload")]
    IdempotencyKeyReused,

    /// Any other state conflict (e.g. handoff no longer pending).
    #[error("{0}")]
    Conflict(String),

    /// Unexpected storage or system failure; never shown to clients verbatim.
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

impl CollabError {
    pub fn not_found(msg: impl Into<String>) -> Self {
        Self::NotFound(msg.into())
    }

    pub fn bad_request(msg: impl Into<String>) -> Self {
        Self::BadRequest(msg.into())
    }
}

impl From<sea_orm::DbErr> for CollabError {
    fn from(err: sea_orm::DbErr) -> Self {
        Self::Internal(anyhow::Error::new(err))
    }
}

pub type CollabResult<T> = Result<T, CollabError>;
