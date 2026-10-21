//! Keep server filesystem paths out of error responses.
//!
//! Handlers build error text from internal errors, and internal errors embed
//! absolute paths (`failed to open source repository: "/srv/forge/repos/a/b.git"
//! does not appear to be a git repository`). Fixing that one handler at a time
//! does not hold — there are ~100 places that echo an error, plus OCI/git
//! responses that bypass `AppError` — so the error-response layer scrubs every
//! 4xx/5xx body instead (see `middleware::request_id_middleware`).
//!
//! Only paths under *registered server roots* are touched. A generic
//! "anything that looks like /a/b" rule would mangle legitimate text such as a
//! user's own `/src/main.rs`; roots are exact, so ordinary messages are left
//! alone.

use std::borrow::Cow;
use std::path::Path;
use std::sync::{Mutex, OnceLock};

/// What a scrubbed path is replaced with.
pub const REDACTED: &str = "<server-path>";

fn roots() -> &'static Mutex<Vec<String>> {
    static ROOTS: OnceLock<Mutex<Vec<String>>> = OnceLock::new();
    ROOTS.get_or_init(|| Mutex::new(Vec::new()))
}

/// Register a directory whose absolute path must never reach a client.
///
/// Both the path as given and its canonical form are registered: they differ
/// when a symlink is involved (macOS `/var` → `/private/var`), and error
/// messages may carry either. Roots that are too short to be meaningful (`/`,
/// `/tmp`, ...) are ignored so a misconfiguration cannot blank out ordinary
/// text.
pub fn register_server_path(path: &Path) {
    let mut candidates = vec![path.to_path_buf()];
    if let Ok(canonical) = path.canonicalize() {
        candidates.push(canonical);
    }
    let mut roots = roots().lock().unwrap_or_else(|e| e.into_inner());
    for candidate in candidates {
        let text = candidate.display().to_string();
        let text = text.trim_end_matches(['/', '\\']).to_string();
        if text.len() < 5 || !text.starts_with('/') && !text.contains(":\\") {
            continue;
        }
        if !roots.contains(&text) {
            roots.push(text);
            // Longest first so `/data/repos/x` is matched before `/data`.
            roots.sort_by_key(|root| std::cmp::Reverse(root.len()));
        }
    }
}

fn is_path_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '/' | '\\' | '.' | '_' | '-' | '~' | '+' | '@' | '%' | ':')
}

/// Replace every path under a registered server root with [`REDACTED`].
///
/// The whole path token is replaced (root plus whatever follows it), not just
/// the root, because the remainder (`/alice/secret-project.git`) is also
/// internal layout. A match must start at a token boundary, so `refs/repos/x`
/// is not mistaken for the root `/repos`.
pub fn redact_server_paths(text: &str) -> Cow<'_, str> {
    let roots = roots().lock().unwrap_or_else(|e| e.into_inner()).clone();
    if roots.is_empty() || !roots.iter().any(|root| text.contains(root.as_str())) {
        return Cow::Borrowed(text);
    }

    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    'scan: while !rest.is_empty() {
        for root in &roots {
            if let Some(index) = rest.find(root.as_str()) {
                let before = &rest[..index];
                let boundary_ok = before
                    .chars()
                    .next_back()
                    .map(|c| !is_path_char(c))
                    .unwrap_or(true);
                let after = &rest[index + root.len()..];
                let end_ok = after
                    .chars()
                    .next()
                    .map(|c| !c.is_alphanumeric() && c != '_' && c != '-' && c != '.')
                    .unwrap_or(true);
                if boundary_ok && end_ok {
                    // Consume the rest of the path token.
                    let tail = after
                        .char_indices()
                        .find(|(_, c)| !is_path_char(*c) || *c == ':' )
                        .map(|(i, _)| i)
                        .unwrap_or(after.len());
                    out.push_str(before);
                    out.push_str(REDACTED);
                    rest = &after[tail..];
                    continue 'scan;
                }
            }
        }
        // No (valid) match left: the rest is clean, except that a rejected
        // candidate may hide a later valid one, so advance one char and retry
        // only if some root still occurs.
        let still = roots.iter().filter_map(|root| rest.find(root.as_str())).min();
        match still {
            Some(index) => {
                let step = index + rest[index..].chars().next().map_or(1, char::len_utf8);
                out.push_str(&rest[..step]);
                rest = &rest[step..];
            }
            None => {
                out.push_str(rest);
                break;
            }
        }
    }
    Cow::Owned(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_root(root: &str) {
        let mut roots = roots().lock().unwrap_or_else(|e| e.into_inner());
        let root = root.to_string();
        if !roots.contains(&root) {
            roots.push(root);
            roots.sort_by_key(|r| std::cmp::Reverse(r.len()));
        }
    }

    #[test]
    fn redacts_whole_path_tokens_under_a_registered_root() {
        with_root("/srv/forge-redact-test/repos");
        let input = r#"failed to open source repository: "/srv/forge-redact-test/repos/alice/x.git" does not appear to be a git repository"#;
        let out = redact_server_paths(input);
        assert!(!out.contains("/srv/forge-redact-test"), "{out}");
        assert!(!out.contains("alice/x.git"), "{out}");
        assert!(out.contains(REDACTED), "{out}");
        assert!(out.contains("does not appear to be a git repository"), "{out}");
    }

    #[test]
    fn handles_debug_quoting_and_json_escaping() {
        with_root("/srv/forge-redact-test/repos");
        let out = redact_server_paths(
            r#"{"message":"fork failed: path \"/srv/forge-redact-test/repos/a/b.git\" exists"}"#,
        );
        assert!(!out.contains("forge-redact-test"), "{out}");
        assert!(out.ends_with(r#"exists"}"#), "{out}");
    }

    #[test]
    fn leaves_ordinary_text_and_lookalikes_alone() {
        with_root("/srv/forge-redact-test/repos");
        for text in [
            "repository not found",
            "refs/srv/forge-redact-test/repos is not a ref",
            "/srv/forge-redact-test/reposX is somebody else's path",
            "user path /src/main.rs not found",
        ] {
            assert_eq!(redact_server_paths(text), text, "must not change: {text}");
        }
    }

    #[test]
    fn redacts_every_occurrence() {
        with_root("/srv/forge-redact-test/repos");
        let out = redact_server_paths(
            "from /srv/forge-redact-test/repos/a.git to /srv/forge-redact-test/repos/b.git failed",
        );
        assert_eq!(out, format!("from {REDACTED} to {REDACTED} failed"));
    }

    #[test]
    fn ignores_roots_that_are_too_short_to_be_safe() {
        register_server_path(Path::new("/"));
        register_server_path(Path::new("/tmp"));
        assert_eq!(redact_server_paths("see /tmp/x and / root"), "see /tmp/x and / root");
    }
}
