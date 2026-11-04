//! Input validation shared by the collaboration services.

use super::error::{CollabError, CollabResult};

/// Single-line label (names, titles): trimmed, `1..=max_chars` characters, no
/// control characters.
pub fn label(field: &str, value: &str, max_chars: usize) -> CollabResult<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(CollabError::bad_request(format!(
            "{field} must not be empty"
        )));
    }
    if trimmed.chars().count() > max_chars {
        return Err(CollabError::bad_request(format!(
            "{field} must be at most {max_chars} characters"
        )));
    }
    if trimmed.chars().any(char::is_control) {
        return Err(CollabError::bad_request(format!(
            "{field} must not contain control characters"
        )));
    }
    Ok(trimmed.to_string())
}

/// Multi-line text (bodies, descriptions): `min_bytes..=max_bytes` bytes, no NUL
/// or control characters other than tab, newline and carriage return.
pub fn text(field: &str, value: &str, min_bytes: usize, max_bytes: usize) -> CollabResult<()> {
    if value.len() < min_bytes {
        return Err(CollabError::bad_request(format!(
            "{field} must not be empty"
        )));
    }
    if value.len() > max_bytes {
        return Err(CollabError::bad_request(format!(
            "{field} must be at most {max_bytes} bytes"
        )));
    }
    if value
        .chars()
        .any(|c| c.is_control() && !matches!(c, '\t' | '\n' | '\r'))
    {
        return Err(CollabError::bad_request(format!(
            "{field} must not contain control characters"
        )));
    }
    Ok(())
}

/// Client-chosen idempotency key: 1–128 characters of `[A-Za-z0-9._:-]`.
pub fn client_key(field: &str, value: &str) -> CollabResult<()> {
    let ok = !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b':' | b'-'));
    if ok {
        Ok(())
    } else {
        Err(CollabError::bad_request(format!(
            "{field} must be 1-128 characters of [A-Za-z0-9._:-]"
        )))
    }
}

/// Server-generated object id (UUID text). Anything else cannot exist, so it is
/// reported as absent rather than as a validation error.
pub fn is_uuid(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_are_trimmed_and_bounded() {
        assert_eq!(label("n", "  agent A ", 80).unwrap(), "agent A");
        assert!(label("n", "   ", 80).is_err());
        assert!(label("n", &"x".repeat(81), 80).is_err());
        assert!(
            label("n", &"中".repeat(80), 80).is_ok(),
            "limit counts characters"
        );
        assert!(label("n", "bad\u{7}name", 80).is_err());
        assert!(label("n", "two\nlines", 80).is_err());
    }

    #[test]
    fn text_allows_whitespace_controls_only() {
        assert!(text("b", "line1\nline2\t\r\n", 1, 100).is_ok());
        assert!(text("b", "nul\0byte", 1, 100).is_err());
        assert!(text("b", "esc\u{1b}[0m", 1, 100).is_err());
        assert!(text("b", "", 1, 100).is_err());
        assert!(text("b", &"x".repeat(101), 1, 100).is_err());
    }

    #[test]
    fn client_keys_are_restricted() {
        assert!(client_key("k", "a-42").is_ok());
        assert!(client_key("k", "").is_err());
        assert!(client_key("k", "has space").is_err());
        assert!(client_key("k", &"a".repeat(129)).is_err());
    }
}
