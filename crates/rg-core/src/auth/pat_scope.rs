//! Personal Access Token scope parsing and enforcement.

use crate::error::{CoreError, CoreResult};

/// Supported PAT scopes. `admin` permits every scope but never bypasses the
/// authenticated user's own administrator/repository permissions.
///
/// The repository scopes form a ladder, each level including the ones below it:
///
/// | scope | may |
/// |---|---|
/// | `repo:read` | read (`GET`/`HEAD`), `git clone`/`fetch` |
/// | `repo:collab` | the above, plus write the collaboration area (sessions, tasks, questions, ...) |
/// | `repo:write` | the above, plus every other repository write, `git push` |
/// | `repo` | the historical full repository scope; includes all of the above |
///
/// A scope only limits which API family and which kind of request a token may
/// make. The user's own permissions on each repository still apply on top, so a
/// token can never do more than its owner can.
///
/// The order here is the canonical order tokens are stored in.
pub const ALLOWED_SCOPES: &[&str] = &[
    "repo",
    "repo:write",
    "repo:collab",
    "repo:read",
    "user",
    "admin",
];

/// Parse, validate and canonicalize a comma/whitespace separated scope list.
pub fn normalize_scopes(input: &str) -> CoreResult<String> {
    let requested: std::collections::HashSet<&str> = input
        .split(|c: char| c == ',' || c.is_whitespace())
        .map(str::trim)
        .filter(|scope| !scope.is_empty())
        .collect();

    if requested.is_empty() {
        return Err(CoreError::InvalidInput(
            "at least one token scope is required".into(),
        ));
    }
    if let Some(scope) = requested
        .iter()
        .find(|scope| !ALLOWED_SCOPES.contains(scope))
    {
        return Err(CoreError::InvalidInput(format!(
            "unsupported token scope '{}'; allowed scopes: {}",
            scope,
            ALLOWED_SCOPES.join(", ")
        )));
    }

    Ok(ALLOWED_SCOPES
        .iter()
        .filter(|scope| requested.contains(**scope))
        .copied()
        .collect::<Vec<_>>()
        .join(","))
}

/// Scopes that satisfy `required`, from the narrowest to the widest.
fn satisfying(required: &str) -> &'static [&'static str] {
    match required {
        "repo:read" => &["repo:read", "repo:collab", "repo:write", "repo"],
        "repo:collab" => &["repo:collab", "repo:write", "repo"],
        "repo:write" => &["repo:write", "repo"],
        _ => &[],
    }
}

/// Return whether the stored scopes grant the required scope.
///
/// `admin` grants everything. The repository scopes are hierarchical (see
/// [`ALLOWED_SCOPES`]); `user` and `repo` match only themselves, apart from
/// `repo` also covering the narrower repository scopes.
pub fn has_scope(granted: &str, required: &str) -> bool {
    let granted: Vec<&str> = granted
        .split(|c: char| c == ',' || c.is_whitespace())
        .map(str::trim)
        .collect();
    granted.iter().any(|scope| {
        *scope == "admin" || *scope == required || satisfying(required).contains(scope)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_and_orders_scopes() {
        assert_eq!(normalize_scopes("user repo,repo").unwrap(), "repo,user");
    }

    #[test]
    fn rejects_unknown_or_empty_scopes() {
        assert!(normalize_scopes(" ").is_err());
        assert!(normalize_scopes("repo,delete_everything").is_err());
    }

    #[test]
    fn narrower_repository_scopes_are_accepted_and_canonically_ordered() {
        assert_eq!(
            normalize_scopes("repo:read user repo:collab").unwrap(),
            "repo:collab,repo:read,user"
        );
        assert_eq!(normalize_scopes("repo:write").unwrap(), "repo:write");
    }

    #[test]
    fn repository_scopes_form_a_ladder() {
        // (granted, required, expected)
        let cases = [
            ("repo", "repo:read", true),
            ("repo", "repo:collab", true),
            ("repo", "repo:write", true),
            ("repo:write", "repo:collab", true),
            ("repo:write", "repo:read", true),
            ("repo:write", "repo", false),
            ("repo:collab", "repo:read", true),
            ("repo:collab", "repo:write", false),
            ("repo:read", "repo:collab", false),
            ("repo:read", "repo:write", false),
            ("repo:read", "repo", false),
            ("user", "repo:read", false),
            ("repo:read,user", "user", true),
        ];
        for (granted, required, expected) in cases {
            assert_eq!(
                has_scope(granted, required),
                expected,
                "granted {granted:?}, required {required:?}"
            );
        }
        assert!(has_scope("admin", "repo:write"));
    }

    #[test]
    fn admin_implies_all_scopes() {
        assert!(has_scope("admin", "repo"));
        assert!(has_scope("admin", "user"));
        assert!(has_scope("repo,user", "user"));
        assert!(!has_scope("user", "repo"));
    }
}
