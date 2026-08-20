//! Centralized input validation helpers for REST API handlers.
//!
//! Provides reusable length, format, and content validators that handlers
//! call before passing user input to the core layer.  This mirrors the
//! "one place to enforce limits" pattern used by [`crate::pagination`].

use crate::error::AppError;

// ── Length constants ───────────────────────────────────────────────────

pub const MAX_TITLE_LEN: usize = 255;
pub const MAX_NAME_LEN: usize = 100;
pub const MAX_BODY_LEN: usize = 65_536;
pub const MAX_CONTENT_LEN: usize = 1_048_576; // 1 MiB
pub const MAX_LABEL_NAME_LEN: usize = 50;
pub const MAX_LABEL_DESC_LEN: usize = 255;
pub const MAX_WEBHOOK_URL_LEN: usize = 2048;
pub const MAX_BRANCH_NAME_LEN: usize = 255;
pub const MAX_COMMIT_MSG_LEN: usize = 8192;

// ── Helpers ────────────────────────────────────────────────────────────

/// Trim, then reject empty strings.  Returns the trimmed value.
pub fn require_non_empty<'a>(value: &'a str, field: &str) -> Result<&'a str, AppError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(AppError::bad_request(format!("{field} cannot be empty")));
    }
    Ok(trimmed)
}

/// Validate that a string's character count is within bounds (after trim).
pub fn validate_length(value: &str, max: usize, field: &str) -> Result<(), AppError> {
    if value.chars().count() > max {
        return Err(AppError::bad_request(format!(
            "{field} exceeds maximum length of {max} characters"
        )));
    }
    Ok(())
}

/// Convenience: trim + non-empty + length check.  Returns the trimmed string.
pub fn require_valid_text(
    value: &str,
    max: usize,
    field: &str,
) -> Result<String, AppError> {
    let trimmed = require_non_empty(value, field)?;
    validate_length(trimmed, max, field)?;
    Ok(trimmed.to_string())
}

/// Validate an optional text field (allows empty/None, but enforces length if present).
pub fn validate_optional_text(
    value: &Option<String>,
    max: usize,
    field: &str,
) -> Result<(), AppError> {
    if let Some(s) = value {
        let trimmed = s.trim();
        if !trimmed.is_empty() {
            validate_length(trimmed, max, field)?;
        }
    }
    Ok(())
}

/// Validate a branch / ref name: non-empty, no whitespace, reasonable length.
pub fn validate_branch_name(name: &str, field: &str) -> Result<String, AppError> {
    let trimmed = require_non_empty(name, field)?;
    validate_length(trimmed, MAX_BRANCH_NAME_LEN, field)?;
    if trimmed.chars().any(char::is_whitespace) {
        return Err(AppError::bad_request(format!(
            "{field} must not contain whitespace"
        )));
    }
    Ok(trimmed.to_string())
}

/// Validate a hex color string (e.g. `#ff5500`).  Accepts 3 or 6 hex digits
/// with an optional leading `#`.
pub fn validate_color(color: &str) -> Result<String, AppError> {
    let trimmed = color.trim();
    let hex = trimmed.strip_prefix('#').unwrap_or(trimmed);
    if hex.len() != 3 && hex.len() != 6 {
        return Err(AppError::bad_request(
            "color must be a 3 or 6 digit hex code (e.g. #ff5500)".to_string(),
        ));
    }
    if !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(AppError::bad_request(
            "color contains invalid hex characters".to_string(),
        ));
    }
    Ok(format!("#{hex}"))
}

/// Validate a webhook URL: non-empty, http(s) scheme, reasonable length.
pub fn validate_webhook_url(url: &str) -> Result<String, AppError> {
    let trimmed = require_non_empty(url, "webhook URL")?;
    validate_length(trimmed, MAX_WEBHOOK_URL_LEN, "webhook URL")?;
    if !trimmed.starts_with("http://") && !trimmed.starts_with("https://") {
        return Err(AppError::bad_request(
            "webhook URL must use http or https scheme".to_string(),
        ));
    }
    Ok(trimmed.to_string())
}

/// Reject strings containing ASCII control characters (except newline/tab).
pub fn reject_control_chars(value: &str, field: &str) -> Result<(), AppError> {
    if value
        .chars()
        .any(|c| c.is_control() && c != '\n' && c != '\t' && c != '\r')
    {
        return Err(AppError::bad_request(format!(
            "{field} contains invalid control characters"
        )));
    }
    Ok(())
}
