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

// ── Repository allow-list ──────────────────────────────────────────────

/// Most repositories one token may be limited to.
pub const MAX_ALLOWLIST_REPOS: usize = 50;

/// Longest owner or repository name accepted in an allow-list entry.
const MAX_NAME_CHARS: usize = 100;

fn valid_name_part(part: &str) -> bool {
    !part.is_empty()
        && part.len() <= MAX_NAME_CHARS
        && part != "."
        && part != ".."
        && part
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

/// Validate a requested allow-list and return the JSON text to store.
///
/// Each entry is `owner/name`; the list is de-duplicated and sorted so the
/// stored form is canonical. An empty list is refused: "limit to nothing" is
/// almost certainly a mistake, and the way to say "no limit" is to omit it.
///
/// Whether the repositories exist is not checked: the token's owner may create
/// them later, and the limit works on names.
pub fn normalize_repo_allowlist(repos: &[String]) -> CoreResult<String> {
    if repos.is_empty() {
        return Err(CoreError::InvalidInput(
            "repos must list at least one owner/name; omit it for an unrestricted token".into(),
        ));
    }
    let mut out: Vec<String> = Vec::new();
    for entry in repos {
        let entry = entry.trim();
        let Some((owner, name)) = entry.split_once('/') else {
            return Err(CoreError::InvalidInput(format!(
                "repository '{entry}' must be written owner/name"
            )));
        };
        if !valid_name_part(owner) || !valid_name_part(name) {
            return Err(CoreError::InvalidInput(format!(
                "repository '{entry}' must be owner/name using letters, digits, '-', '_' and '.'"
            )));
        }
        out.push(format!("{owner}/{name}"));
    }
    out.sort();
    out.dedup();
    if out.len() > MAX_ALLOWLIST_REPOS {
        return Err(CoreError::InvalidInput(format!(
            "a token can be limited to at most {MAX_ALLOWLIST_REPOS} repositories"
        )));
    }
    serde_json::to_string(&out).map_err(|e| CoreError::internal(e.to_string()))
}

/// Read a stored allow-list. Anything that does not parse becomes the empty
/// list, which allows nothing: a damaged limit must not turn into "no limit".
pub fn parse_repo_allowlist(stored: &str) -> Vec<String> {
    serde_json::from_str::<Vec<String>>(stored).unwrap_or_default()
}

/// Whether `owner/name` is in the allow-list. Exact and case-sensitive on
/// purpose: a lookalike that the database might resolve to another repository
/// is refused rather than guessed at.
pub fn repo_in_allowlist(owner: &str, name: &str, allow: &[String]) -> bool {
    allow.iter().any(|e| {
        e.split_once('/')
            .is_some_and(|(o, n)| o == owner && n == name)
    })
}

/// Whether a REST request path stays inside the allow-list.
///
/// Only paths of the form `/repos/{owner}/{name}[/…]` and
/// `/ai/repos/{owner}/{name}[/…]` for a listed repository are accepted. Anything
/// else in the repository API family is refused, including the global listing
/// and search, organizations, imports and CI routes addressed by id: they cannot
/// be tied to a repository from the path alone.
///
/// Inside a listed repository the routes that reach **other** repositories are
/// also refused: forking, transfer, and cross-repository coordination groups.
///
/// `path` is the request path without the `/api/v1` prefix and without a query.
/// An empty, `.` or `..` segment anywhere refuses the request.
pub fn path_in_allowlist(path: &str, allow: &[String]) -> bool {
    let segments: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    if segments
        .iter()
        .any(|s| s.is_empty() || *s == "." || *s == "..")
    {
        return false;
    }
    let rest = match segments.as_slice() {
        ["repos", owner, name, rest @ ..] if repo_in_allowlist(owner, name, allow) => rest,
        ["ai", "repos", owner, name, rest @ ..] if repo_in_allowlist(owner, name, allow) => rest,
        _ => return false,
    };
    !matches!(
        rest,
        ["fork", ..] | ["forks", ..] | ["transfer", ..] | ["collaboration", "groups", ..]
    )
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

    #[test]
    fn allowlist_entries_are_validated_and_canonical() {
        let ok = normalize_repo_allowlist(&[
            "bob/zeta".into(),
            " alice/demo ".into(),
            "alice/demo".into(),
        ])
        .unwrap();
        assert_eq!(ok, r#"["alice/demo","bob/zeta"]"#);
        for bad in [
            vec![],
            vec!["noslash".to_string()],
            vec!["a/b/c".to_string()],
            vec!["/b".to_string()],
            vec!["a/".to_string()],
            vec!["a/..".to_string()],
            vec!["../b".to_string()],
            vec!["a b/c".to_string()],
            vec!["a/b%2Fc".to_string()],
            vec!["a/b\u{0}".to_string()],
            vec!["a*/b".to_string()],
        ] {
            assert!(normalize_repo_allowlist(&bad).is_err(), "{bad:?}");
        }
        let many: Vec<String> = (0..=MAX_ALLOWLIST_REPOS)
            .map(|i| format!("o/r{i}"))
            .collect();
        assert!(normalize_repo_allowlist(&many).is_err());
        assert!(normalize_repo_allowlist(&many[..MAX_ALLOWLIST_REPOS]).is_ok());
    }

    #[test]
    fn a_damaged_stored_allowlist_allows_nothing() {
        assert_eq!(parse_repo_allowlist(r#"["a/b"]"#), vec!["a/b".to_string()]);
        for broken in ["", "null", "{}", "not json", r#"["a/b""#, "42"] {
            assert!(parse_repo_allowlist(broken).is_empty(), "{broken:?}");
        }
        assert!(!path_in_allowlist(
            "/repos/a/b",
            &parse_repo_allowlist("not json")
        ));
    }

    #[test]
    fn paths_are_checked_against_the_allowlist_exactly() {
        let allow = vec!["alice/demo".to_string()];
        for path in [
            "/repos/alice/demo",
            "/repos/alice/demo/issues",
            "/repos/alice/demo/collaboration/tasks/1/questions",
            "/ai/repos/alice/demo/search/code",
        ] {
            assert!(path_in_allowlist(path, &allow), "{path}");
        }
        for path in [
            // Other repositories, other casing, prefixes, near misses.
            "/repos/alice/other",
            "/repos/bob/demo",
            "/repos/Alice/demo",
            "/repos/alice/Demo",
            "/repos/alice/demo2",
            "/repos/alice/dem",
            "/repos/alice",
            "/repos/alice/demo%2Fx",
            // Families that cannot be tied to a repository.
            "/repos",
            "/orgs/alice",
            "/search",
            "/imports",
            "/ci/pipelines/1",
            "/artifacts/1",
            "/ws/jobs/1",
            "/ai/repos/alice/other/search/code",
            // Empty, dot and dot-dot segments.
            "/repos//alice/demo",
            "/repos/alice//demo",
            "/repos/alice/demo/../other/x",
            "/repos/alice/demo/./issues",
            "/repos/alice/demo/",
            "",
            "/",
        ] {
            assert!(!path_in_allowlist(path, &allow), "{path}");
        }
    }

    #[test]
    fn routes_that_reach_other_repositories_are_refused_even_inside_a_listed_one() {
        let allow = vec!["alice/demo".to_string()];
        for path in [
            "/repos/alice/demo/fork",
            "/repos/alice/demo/forks",
            "/repos/alice/demo/transfer",
            "/repos/alice/demo/collaboration/groups",
            "/repos/alice/demo/collaboration/groups/g1/links",
        ] {
            assert!(!path_in_allowlist(path, &allow), "{path}");
        }
        // The rest of the collaboration area is fine, and so is a path that only
        // looks similar.
        assert!(path_in_allowlist(
            "/repos/alice/demo/collaboration/tasks",
            &allow
        ));
        assert!(path_in_allowlist("/repos/alice/demo/forked", &allow));
        assert!(path_in_allowlist(
            "/repos/alice/demo/collaboration/groupsx",
            &allow
        ));
    }
}
