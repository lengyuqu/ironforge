//! Domain error types for rg-core.
//!
//! Replaces `anyhow::anyhow!()` with typed errors that carry semantic meaning
//! and can be automatically mapped to HTTP status codes via `From` impls.
//!
//! # Usage
//!
//! ```rust,ignore
//! use rg_core::error::{CoreError, CoreResult};
//!
//! fn get_repo(name: &str) -> CoreResult<Repo> {
//!     if name.is_empty() {
//!         return Err(CoreError::InvalidInput("repo name cannot be empty".into()));
//!     }
//!     // ...
//!     Ok(repo)
//! }
//! ```

use thiserror::Error;

/// Unified error type for rg-core business logic.
#[derive(Debug, Error)]
pub enum CoreError {
    /// Resource not found (repo, user, issue, PR, etc.)
    #[error("{0}")]
    NotFound(String),

    /// Permission denied — actor lacks required access.
    #[error("forbidden: {0}")]
    Forbidden(String),

    /// Resource already exists or state conflict (e.g., PR already merged).
    #[error("conflict: {0}")]
    Conflict(String),

    /// Invalid input / validation failure.
    #[error("invalid input: {0}")]
    InvalidInput(String),

    /// Generic internal error (wraps anyhow for gradual migration).
    #[error(transparent)]
    Internal(anyhow::Error),
}

/// Convenience type alias for results returning [`CoreError`].
pub type CoreResult<T> = Result<T, CoreError>;

impl CoreError {
    /// Create a not-found error with a formatted message.
    pub fn not_found(msg: impl Into<String>) -> Self {
        CoreError::NotFound(msg.into())
    }

    /// Create a forbidden error with a formatted message.
    pub fn forbidden(msg: impl Into<String>) -> Self {
        CoreError::Forbidden(msg.into())
    }

    /// Create a conflict error with a formatted message.
    pub fn conflict(msg: impl Into<String>) -> Self {
        CoreError::Conflict(msg.into())
    }

    /// Create an invalid-input error.
    pub fn invalid_input(msg: impl Into<String>) -> Self {
        CoreError::InvalidInput(msg.into())
    }

    /// Create an internal error from anything that implements `Display`.
    pub fn internal(msg: impl std::fmt::Display) -> Self {
        CoreError::Internal(anyhow::anyhow!("{}", msg))
    }
}

// Convenience: convert anyhow::Error directly into CoreError::Internal.
//
// A `CoreError` that was widened into `anyhow::Error` (`?` inside a function
// that still returns `anyhow::Result`, with or without `.context(..)`) keeps its
// category when it comes back: it is recovered by downcast instead of being
// wrapped as `Internal`. Without this, a "not found" or "forbidden" that
// crosses one anyhow-returning function turned into a 500, which made it unsafe
// to migrate services to `CoreResult` one at a time.
impl From<anyhow::Error> for CoreError {
    fn from(e: anyhow::Error) -> Self {
        match e.downcast::<CoreError>() {
            Ok(core) => core,
            Err(other) => CoreError::Internal(other),
        }
    }
}

// Database failures are internal errors. This lets `?` work in service
// functions that return `CoreResult` without an explicit `map_err`.
impl From<sea_orm::DbErr> for CoreError {
    fn from(e: sea_orm::DbErr) -> Self {
        CoreError::Internal(anyhow::Error::new(e))
    }
}

// ── Context trait (replaces anyhow::Context) ─────────────────────────────

/// Provides `.context()` and `.with_context()` for any `Result<T, E>` where
/// `E: std::error::Error + Send + Sync + 'static`, returning `CoreResult<T>`.
///
/// This allows service functions that return `CoreResult<T>` to use the same
/// `.context("msg")?` pattern as anyhow, without importing anyhow's `Context`
/// trait.
pub trait CoreContext<T> {
    /// Wrap the error with a static context message.
    fn context<C: Into<String>>(self, context: C) -> CoreResult<T>;

    /// Wrap the error with a lazily-evaluated context message.
    fn with_context<C: Into<String>, F: FnOnce() -> C>(self, f: F) -> CoreResult<T>;
}

// Implement for any Result<T, E> where E is a standard error type.
// This covers sea_orm::DbErr, std::io::Error, reqwest::Error, etc.
// NOTE: anyhow::Error does NOT implement std::error::Error, so it is not
// covered here.  Use the `?` operator (which invokes From<anyhow::Error>)
// to propagate anyhow errors in CoreResult-returning functions.
impl<T, E> CoreContext<T> for Result<T, E>
where
    E: std::error::Error + Send + Sync + 'static,
{
    fn context<C: Into<String>>(self, context: C) -> CoreResult<T> {
        self.map_err(|e| CoreError::Internal(anyhow::Error::new(e).context(context.into())))
    }

    fn with_context<C: Into<String>, F: FnOnce() -> C>(self, f: F) -> CoreResult<T> {
        self.map_err(|e| CoreError::Internal(anyhow::Error::new(e).context(f().into())))
    }
}

// Also implement for Option<T> (matching anyhow's behavior).
impl<T> CoreContext<T> for Option<T> {
    fn context<C: Into<String>>(self, context: C) -> CoreResult<T> {
        self.ok_or_else(|| CoreError::Internal(anyhow::anyhow!("{}", context.into())))
    }

    fn with_context<C: Into<String>, F: FnOnce() -> C>(self, f: F) -> CoreResult<T> {
        self.ok_or_else(|| CoreError::Internal(anyhow::anyhow!("{}", f().into())))
    }
}

#[cfg(test)]
mod anyhow_round_trip_tests {
    use super::*;
    use anyhow::Context as _;

    fn through_anyhow(e: CoreError) -> anyhow::Result<()> {
        Err(anyhow::Error::from(e))
    }

    #[test]
    fn category_survives_a_trip_through_anyhow() {
        for (original, check) in [
            (CoreError::not_found("nope"), "NotFound"),
            (CoreError::forbidden("no"), "Forbidden"),
            (CoreError::conflict("dup"), "Conflict"),
            (CoreError::invalid_input("bad"), "InvalidInput"),
        ] {
            let back: CoreError = through_anyhow(original).unwrap_err().into();
            let kind = match back {
                CoreError::NotFound(_) => "NotFound",
                CoreError::Forbidden(_) => "Forbidden",
                CoreError::Conflict(_) => "Conflict",
                CoreError::InvalidInput(_) => "InvalidInput",
                CoreError::Internal(_) => "Internal",
            };
            assert_eq!(kind, check);
        }
    }

    #[test]
    fn category_survives_added_context() {
        let widened = through_anyhow(CoreError::not_found("repo")).context("while loading");
        let back: CoreError = widened.unwrap_err().into();
        assert!(matches!(back, CoreError::NotFound(_)), "{back:?}");
    }

    #[test]
    fn foreign_errors_stay_internal() {
        let back: CoreError = anyhow::anyhow!("disk exploded").into();
        assert!(matches!(back, CoreError::Internal(_)));
    }
}
