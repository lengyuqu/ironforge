//! Drift guard for the route metadata registry
//! (`rg_core::auth::route_meta`).
//!
//! The registry declares, once per route, what the PAT scope check, the
//! repository allow-list, the branch allow-list and the idempotency key need to
//! know. These tests keep it honest:
//!
//! * every route registered in `rg-http/src/lib.rs` is declared, and every
//!   declaration is a registered route (so a route added without metadata, or
//!   left behind after a removal, fails here);
//! * for every registered route the derived decisions equal the hand-written
//!   tables the registry replaced (kept below as `legacy`, the oracle), and on
//!   near-miss and non-route paths the registry is never *looser* than they
//!   were;
//! * a route declared `.checked()` has a handler that really calls
//!   `ensure_token_may_write_branch`.
//!
//! The route list is read from the source of `lib.rs` (the router offers no way
//! to enumerate itself), the same approach as `collab_contract_tests.rs`.

use std::collections::BTreeSet;
use std::fs;

use rg_core::auth::route_meta::{self, BranchLimit, Idempotency};
use rg_core::collaboration::capabilities;

// ── Oracle: the tables as they were before the registry ────────────────

mod legacy {
    pub const REPO_SCOPED: &[&str] = &[
        "/repos",
        "/orgs",
        "/ci",
        "/artifacts",
        "/imports",
        "/search",
        "/ai",
        "/ws",
    ];

    /// `required_pat_scope` + `repo_scope_for`, with the method reduced to
    /// "is it a read".
    pub fn scope(is_read: bool, path: &str) -> Option<&'static str> {
        if path.starts_with("/api-docs") || path == "/health" || path == "/metrics" {
            return None;
        }
        if path.starts_with("/admin") || path == "/runners/register" {
            return Some("admin");
        }
        if path.starts_with("/users")
            // The singular `/user/` family (notification preferences, #19
            // step 5). The trailing slash keeps `/users` in charge of the rest.
            || path.starts_with("/user/")
            || path == "/agents"
            || path.starts_with("/agents/")
            || path.starts_with("/notifications")
            || path.starts_with("/auth")
            || path == "/ws/notifications"
        {
            return Some("user");
        }
        if REPO_SCOPED.iter().any(|prefix| path.starts_with(prefix)) {
            if is_read {
                return Some("repo:read");
            }
            let mut parts = path.trim_start_matches('/').splitn(4, '/');
            let is_collab = parts.next() == Some("repos")
                && parts.next().is_some_and(|owner| !owner.is_empty())
                && parts.next().is_some_and(|name| !name.is_empty())
                && parts.next().is_some_and(|rest| {
                    (rest == "collaboration" || rest.starts_with("collaboration/"))
                        && !rest.split('/').any(|seg| seg == ".." || seg == ".")
                });
            return Some(if is_collab {
                "repo:collab"
            } else {
                "repo:write"
            });
        }
        if path.starts_with("/runners") {
            return Some("admin");
        }
        Some("admin")
    }

    /// The refusal in `path_in_allowlist` for routes reaching other repositories
    /// (the repository itself is assumed to be on the list).
    pub fn reaches_other_repos(path: &str) -> bool {
        let segments: Vec<&str> = path.trim_start_matches('/').split('/').collect();
        if segments
            .iter()
            .any(|s| s.is_empty() || *s == "." || *s == "..")
        {
            return false;
        }
        let rest = match segments.as_slice() {
            ["repos", _, _, rest @ ..] => rest,
            ["ai", "repos", _, _, rest @ ..] => rest,
            _ => return false,
        };
        matches!(
            rest,
            ["fork", ..] | ["forks", ..] | ["transfer", ..] | ["collaboration", "groups", ..]
        )
    }

    pub fn branch_limited_write(path: &str) -> bool {
        let segments: Vec<&str> = path.trim_start_matches('/').split('/').collect();
        if segments
            .iter()
            .any(|s| s.is_empty() || *s == "." || *s == "..")
        {
            return false;
        }
        let ["repos", _owner, _name, rest @ ..] = segments.as_slice() else {
            return false;
        };
        matches!(
            rest,
            ["collaboration", ..]
                | ["issues", ..]
                | ["pulls"]
                | ["pulls", _]
                | ["pulls", _, "comments"]
                | ["pulls", _, "comments", _, "resolution"]
                | ["pulls", _, "reviews"]
                | ["pulls", _, "reviewers"]
                | ["pulls", _, "reviewers", _]
                | ["contents", _, ..]
                | ["commits"]
                // Added with the cherry-pick / revert routes: their handlers
                // check the branch themselves (`.checked()`).
                | ["commits", _, "cherry-pick"]
                | ["commits", _, "revert"]
                // Added with the apply-patch route, which holds the target
                // branch to the token's limit the same way.
                | ["commits", "apply-patch"]
                // Added with the agent draft area (#18): registering a
                // namespace moves no ref, and the handler holds the prefix to
                // the token's own limit (`ensure_token_may_write_branch`).
                | ["agent-drafts"]
                | ["agent-drafts", _]
        ) || matches!(
            rest,
            ["branches", first, ..] if *first != "protection"
        )
    }

    pub fn idempotent(path: &str) -> bool {
        let segments: Vec<&str> = path.trim_start_matches('/').split('/').collect();
        if segments
            .iter()
            .any(|s| s.is_empty() || *s == "." || *s == "..")
        {
            return false;
        }
        matches!(
            segments.as_slice(),
            ["repos"]
                | [
                    "repos",
                    _,
                    _,
                    "issues"
                        | "pulls"
                        | "contents"
                        | "commits"
                        | "releases"
                        | "labels"
                        | "milestones"
                        | "branches"
                        | "wiki",
                    ..
                ]
        )
    }
}

// ── Reading the registered routes from lib.rs ──────────────────────────

struct Registered {
    /// As written in the router, relative to `/api/v1`.
    path: String,
    /// `api::module::handler` pairs named in the route expression.
    handlers: Vec<(String, String)>,
}

fn lib_source() -> String {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/src/lib.rs");
    fs::read_to_string(path).expect("read rg-http/src/lib.rs")
}

/// The routes of the `/api/v1` router: everything `build_routes` registers after
/// the Git router (which authenticates on its own, not through the PAT scope
/// table).
fn registered_routes() -> Vec<Registered> {
    let src = lib_source();
    let start = src
        .find("let runners_auth = Router::new()")
        .expect("runners_auth router in build_routes");
    let end = src[start..]
        .find("(api_v1, git_routes)\n}")
        .map(|i| start + i)
        .expect("end of build_routes");
    let region = &src[start..end];

    let mut out = Vec::new();
    let mut search = 0;
    let mut calls = 0;
    while let Some(i) = region[search..].find(".route(") {
        calls += 1;
        let open = search + i + ".route(".len();
        // Find the matching close paren of the `.route(` call.
        let mut depth = 1usize;
        let mut j = open;
        let bytes = region.as_bytes();
        let mut in_str = false;
        while depth > 0 {
            let c = bytes[j];
            if in_str {
                if c == b'"' {
                    in_str = false;
                }
            } else {
                match c {
                    b'"' => in_str = true,
                    b'(' => depth += 1,
                    b')' => depth -= 1,
                    _ => {}
                }
            }
            j += 1;
        }
        let body = &region[open..j - 1];
        search = j;
        let body_trim = body.trim_start();
        if !body_trim.starts_with('"') {
            continue; // a non-literal path would fail the call count below
        }
        let rest = &body_trim[1..];
        let close = rest.find('"').expect("closing quote");
        let path = rest[..close].to_string();
        let mut handlers = Vec::new();
        let mut h = &rest[close..];
        while let Some(k) = h.find("api::") {
            h = &h[k + "api::".len()..];
            let Some((module, tail)) = h.split_once("::") else {
                break;
            };
            let name: String = tail
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            handlers.push((module.to_string(), name));
        }
        out.push(Registered { path, handlers });
    }
    assert_eq!(
        calls,
        out.len(),
        "a `.route(` call in build_routes has a path that is not a string literal; \
         declare it by hand or teach this test about it"
    );
    out
}

/// A concrete request path for a registered route pattern.
fn sample(pattern: &str) -> String {
    pattern
        .split('/')
        .map(|seg| {
            if seg == "{owner}" {
                "o".to_string()
            } else if seg == "{name}" {
                "n".to_string()
            } else if seg.starts_with("{*") {
                "a/b".to_string()
            } else if seg.starts_with('{') {
                "p1".to_string()
            } else {
                seg.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// `{x}` → `{}` and `{*x}` → `{*}`, the form `declared_patterns` prints.
fn shape(pattern: &str) -> String {
    pattern
        .split('/')
        .map(|seg| {
            if seg.starts_with("{*") {
                "{*}"
            } else if seg.starts_with('{') {
                "{}"
            } else {
                seg
            }
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn all_registered_shapes() -> BTreeSet<String> {
    registered_routes().iter().map(|r| shape(&r.path)).collect()
}

// ── Tests ──────────────────────────────────────────────────────────────

#[test]
fn the_route_list_is_read_sensibly() {
    let routes = registered_routes();
    assert!(
        routes.len() > 250,
        "only {} routes found; the extraction is broken",
        routes.len()
    );
    for known in [
        "/repos",
        "/repos/{owner}/{name}/contents/{*path}",
        "/repos/{owner}/{name}/collaboration/tasks",
        "/users/tokens",
        "/admin/users",
        "/ws/job/{job_id}",
    ] {
        assert!(
            routes.iter().any(|r| r.path == known),
            "route {known} not found in lib.rs"
        );
    }
}

#[test]
fn every_registered_route_is_declared_and_every_declaration_is_registered() {
    let registered = all_registered_shapes();
    let declared: Vec<String> = route_meta::declared_patterns();
    let declared_set: BTreeSet<String> = declared.iter().cloned().collect();

    assert_eq!(
        declared.len(),
        declared_set.len(),
        "two declarations have the same shape: {:?}",
        declared
            .iter()
            .filter(|d| declared.iter().filter(|x| x == d).count() > 1)
            .collect::<BTreeSet<_>>()
    );

    let undeclared: Vec<_> = registered.difference(&declared_set).collect();
    assert!(
        undeclared.is_empty(),
        "routes registered in rg-http/src/lib.rs but not declared in \
         rg_core::auth::route_meta::ROUTES (or, for collaboration routes, in \
         capabilities::REGISTRY): {undeclared:#?}\n\
         Add a row; the constructor you choose states the PAT scope, whether the route \
         can reach another repository, whether a branch-limited token may use it and \
         whether an idempotency key is safe."
    );
    let stale: Vec<_> = declared_set.difference(&registered).collect();
    assert!(
        stale.is_empty(),
        "declared in the route registry but not registered in rg-http/src/lib.rs: {stale:#?}"
    );
}

#[test]
fn a_registered_route_is_never_answered_from_the_family_defaults() {
    for route in registered_routes() {
        let path = sample(&route.path);
        assert!(
            route_meta::lookup(&path).is_some(),
            "{} ({path}) resolves to no declaration",
            route.path
        );
    }
}

#[test]
fn the_derived_decisions_equal_the_hand_written_tables_on_every_registered_route() {
    for route in registered_routes() {
        let path = sample(&route.path);
        for is_read in [true, false] {
            assert_eq!(
                route_meta::pat_scope_for(is_read, &path),
                legacy::scope(is_read, &path),
                "scope of {} (read: {is_read})",
                route.path
            );
        }
        assert_eq!(
            route_meta::reaches_other_repos(&path),
            legacy::reaches_other_repos(&path),
            "cross-repository refusal of {}",
            route.path
        );
        assert_eq!(
            route_meta::branch_limited_write_allowed(&path),
            legacy::branch_limited_write(&path),
            "branch-limited write of {}",
            route.path
        );
        assert_eq!(
            route_meta::idempotency_replayable(&path),
            legacy::idempotent(&path),
            "idempotency of {}",
            route.path
        );
    }
}

/// Paths that are not routes: near misses and extensions of every registered
/// route, plus the lookalikes the existing unit tests pin.
fn probe_paths() -> Vec<String> {
    let mut seeds: Vec<String> = registered_routes()
        .iter()
        .map(|r| sample(&r.path))
        .collect();
    seeds.extend(
        capabilities::REGISTRY
            .iter()
            .flat_map(|c| c.routes)
            .map(|r| sample(r.path)),
    );
    let mut out = Vec::new();
    for seed in seeds {
        let segs: Vec<&str> = seed.trim_start_matches('/').split('/').collect();
        out.push(format!("{seed}/zz"));
        out.push(format!("{seed}/zz/yy"));
        out.push(format!("{seed}x"));
        out.push(format!("{seed}/"));
        out.push(format!("/{seed}"));
        if segs.len() > 1 {
            out.push(format!("/{}", segs[..segs.len() - 1].join("/")));
            let mut dotted = segs.clone();
            dotted.insert(segs.len() - 1, "..");
            out.push(format!("/{}", dotted.join("/")));
            let mut dotted = segs.clone();
            dotted.insert(segs.len() - 1, ".");
            out.push(format!("/{}", dotted.join("/")));
        }
        if segs.first() == Some(&"repos") && segs.len() >= 3 {
            let tail = segs[3..].join("/");
            out.push(format!("/repos/o/n/collaboration/{tail}"));
            out.push(format!("/repos/o/n/collaborationx/{tail}"));
            out.push(format!("/repos/o/collaboration/{tail}"));
            out.push(format!("/repos/o//{tail}"));
            out.push(format!("/orgs/o/n/{tail}"));
            out.push(format!("/ai/repos/o/n/{tail}"));
        }
    }
    out.extend(
        [
            "/brand-new",
            "/repos/o/n/brand-new-route",
            "/repos/o/n/collaboration",
            "/repos/o/n/collaboration/groupsx",
            "/repos/o/n/forked",
            "/ci/pipelines",
            "/orgs/x/teams",
            "/reposfoo",
            "/api-docsx",
            "/runners/zz",
            "",
            "/",
        ]
        .map(String::from),
    );
    out
}

#[test]
fn off_route_paths_keep_their_scope_and_are_never_looser_elsewhere() {
    for path in probe_paths() {
        for is_read in [true, false] {
            // A path that matches no declaration gets exactly the old family
            // answer. One that does match a (different) route is checked by the
            // route test above; the near misses here may land on another route,
            // for which the declared scope must still be what the tables said.
            assert_eq!(
                route_meta::pat_scope_for(is_read, &path),
                legacy::scope(is_read, &path),
                "scope of probe {path:?} (read: {is_read})"
            );
        }
        // A refusal must never become a permission.
        assert!(
            !route_meta::branch_limited_write_allowed(&path) || legacy::branch_limited_write(&path),
            "probe {path:?}: the registry lets a branch-limited token write where the old table refused"
        );
        assert!(
            !route_meta::idempotency_replayable(&path) || legacy::idempotent(&path),
            "probe {path:?}: the registry accepts an idempotency key where the old table refused"
        );
        assert_eq!(
            route_meta::reaches_other_repos(&path),
            legacy::reaches_other_repos(&path),
            "cross-repository refusal of probe {path:?}"
        );
    }
}

#[test]
fn a_path_that_is_not_a_route_fails_closed_where_it_can() {
    let path = "/repos/o/n/brand-new-route";
    assert!(route_meta::lookup(path).is_none());
    assert!(!route_meta::branch_limited_write_allowed(path));
    assert!(!route_meta::idempotency_replayable(path));
    // Writes in a known family keep that family's scope (pinned by the
    // `pat_scope_tests` in lib.rs); an unknown family needs admin.
    assert_eq!(route_meta::pat_scope_for(false, path), Some("repo:write"));
    assert_eq!(
        route_meta::pat_scope_for(false, "/brand-new"),
        Some("admin")
    );
}

#[test]
fn a_more_specific_declaration_wins_like_the_router() {
    // `branches/protection` is its own route; the catch-all deletes a branch.
    let protection = route_meta::lookup("/repos/o/n/branches/protection").unwrap();
    assert_eq!(protection.branch_limit, BranchLimit::Refused);
    let delete = route_meta::lookup("/repos/o/n/branches/agent/x").unwrap();
    assert_eq!(delete.branch_limit, BranchLimit::HandlerChecked);
    // `/repos/templates/labels` is static; `/repos/{owner}/{name}` is not it.
    assert!(route_meta::lookup("/repos/templates/labels").is_some());
    let repo = route_meta::lookup("/repos/o/n").unwrap();
    assert_eq!(repo.idempotency, Idempotency::Unsupported);
}

/// The body of `fn name` in `src/api/<module>.rs`: from its signature to the
/// next top-level item.
fn handler_body(module: &str, name: &str) -> String {
    let path = format!("{}/src/api/{module}.rs", env!("CARGO_MANIFEST_DIR"));
    let src = fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path}: {e}"));
    let needle = format!("fn {name}(");
    let start = src
        .find(&needle)
        .unwrap_or_else(|| panic!("no `fn {name}(` in {path}"));
    let rest = &src[start..];
    let end = rest
        .match_indices("\n}\n")
        .next()
        .map(|(i, _)| i)
        .unwrap_or(rest.len());
    rest[..end].to_string()
}

#[test]
fn a_route_declared_handler_checked_has_a_handler_that_checks_the_branch() {
    let mut checked = 0;
    for route in registered_routes() {
        let path = sample(&route.path);
        let Some(meta) = route_meta::lookup(&path) else {
            continue;
        };
        if meta.branch_limit != BranchLimit::HandlerChecked {
            continue;
        }
        assert!(
            !route.handlers.is_empty(),
            "{}: no handler named in the route expression",
            route.path
        );
        for (module, name) in &route.handlers {
            checked += 1;
            let body = handler_body(module, name);
            assert!(
                body.contains("ensure_token_may_write_branch"),
                "{} is declared `.checked()` but api::{module}::{name} does not call \
                 ensure_token_may_write_branch; a branch-limited token could move any branch \
                 through it",
                route.path
            );
        }
    }
    assert!(
        checked >= 4,
        "expected the contents, commits and branch-delete handlers, found {checked}"
    );
}

// ── The live entry points the middleware calls ─────────────────────────

/// The legacy `path_in_allowlist`, for a token limited to `o/n`.
fn legacy_in_allowlist(path: &str) -> bool {
    let segments: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    if segments
        .iter()
        .any(|s| s.is_empty() || *s == "." || *s == "..")
    {
        return false;
    }
    let listed = |owner: &str, name: &str| owner == "o" && name == "n";
    let rest = match segments.as_slice() {
        ["repos", owner, name, rest @ ..] if listed(owner, name) => rest,
        ["ai", "repos", owner, name, rest @ ..] if listed(owner, name) => rest,
        _ => return false,
    };
    !matches!(
        rest,
        ["fork", ..] | ["forks", ..] | ["transfer", ..] | ["collaboration", "groups", ..]
    )
}

#[test]
fn the_live_entry_points_agree_with_the_hand_written_tables() {
    use rg_core::auth::pat_scope::{path_in_allowlist, ref_limited_write_allowed};
    use rg_http::idempotency::idempotency_supported;

    let allow = vec!["o/n".to_string()];
    let routes: Vec<String> = registered_routes()
        .iter()
        .map(|r| sample(&r.path))
        .collect();
    for path in routes.iter().cloned().chain(probe_paths()) {
        assert_eq!(
            path_in_allowlist(&path, &allow),
            legacy_in_allowlist(&path),
            "path_in_allowlist({path:?})"
        );
        // Never looser; and equal on every registered route.
        let live_ref = ref_limited_write_allowed(&path);
        let live_idem = idempotency_supported(&path);
        assert!(
            !live_ref || legacy::branch_limited_write(&path),
            "ref_limited_write_allowed({path:?}) is looser than before"
        );
        assert!(
            !live_idem || legacy::idempotent(&path),
            "idempotency_supported({path:?}) is looser than before"
        );
        if routes.contains(&path) {
            assert_eq!(
                live_ref,
                legacy::branch_limited_write(&path),
                "ref_limited_write_allowed({path:?})"
            );
            assert_eq!(
                live_idem,
                legacy::idempotent(&path),
                "idempotency_supported({path:?})"
            );
        }
    }
}
