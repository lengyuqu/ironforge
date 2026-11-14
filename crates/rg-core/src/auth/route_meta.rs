//! The route metadata registry: every REST route declares, once, what a
//! personal access token (PAT) restriction needs to know about it. The four
//! hand-maintained tables this replaces are now **derived** from the
//! declarations below:
//!
//! | Derived decision | Function | Declared by |
//! |---|---|---|
//! | PAT scope a request needs | [`pat_scope_for`] | [`PatScope`] |
//! | Repository allow-list refuses the route | [`reaches_other_repos`] | `other_repos` |
//! | Branch-limited token may write | [`branch_limited_write_allowed`] | [`BranchLimit`] |
//! | `Idempotency-Key` accepted | [`idempotency_replayable`] | [`Idempotency`] |
//!
//! The collaboration capability registry
//! (`crate::collaboration::capabilities::REGISTRY`) is the declaration for
//! everything under `/repos/{owner}/{name}/collaboration`: each of its routes
//! is a repository-collaboration route (`PatScope::RepoCollab`, cannot move a
//! ref, no idempotency key) unless it is declared with
//! `capabilities::x(..)` because it reaches another repository.
//!
//! # Adding a route
//!
//! Add one row to [`ROUTES`] (or, for a collaboration route, to the capability
//! registry). The constructor you pick is the declaration:
//!
//! * [`repo`], [`user`], [`admin`] — the scope the route needs. `repo` means
//!   `repo:read` for reads and `repo:write` for writes.
//! * [`other_repos`] — a repository route whose request names or reaches
//!   **another** repository. Use it whenever a body field or stored link can
//!   point at one; a token limited to some repositories is then refused.
//! * `.open()` — a write that cannot move a branch or tag, so a token limited
//!   to some branches may make it. `.checked()` — a write that moves a ref and
//!   whose handler holds the target to the token's own limit
//!   (`ensure_token_may_write_branch`); say so **only** if it does. Without
//!   either, such a token is refused.
//! * `.idem()` — the response carries no token or secret, so replaying it from
//!   the idempotency store is safe. Without it the `Idempotency-Key` header is
//!   refused.
//!
//! `rg-http/tests/route_metadata_tests.rs` fails when a route registered in
//! `rg-http/src/lib.rs` is not matched by a row here, when a row matches no
//! registered route, and when a `.checked()` route's handler does not call
//! `ensure_token_may_write_branch`.
//!
//! # What a request that matches no row gets
//!
//! A path that is not a registered route (a 404) falls to the *family
//! defaults* ([`family_scope`] and the cross-repository families): exactly the
//! prefix rules the hand-written tables used, because tests pin that unknown
//! paths in a known family still need that family's scope. Everything else
//! fails closed: branch-limited writes and idempotency keys are refused, and
//! the scope of an unrecognised prefix is `admin`. A **registered** route is
//! never answered from the defaults (the drift test), so a route added without
//! a declaration fails the test instead of silently inheriting a default.
//!
//! What cannot be declared: whether a handler really checks the branch, and
//! whether a body field really names another repository, are facts about the
//! handler. The declarations record them; the tests cross-check what they can.

use std::sync::OnceLock;

use crate::collaboration::capabilities;

/// The PAT scope family a route belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatScope {
    /// No credential needed (docs, health, metrics). Only family defaults use it.
    Public,
    /// `admin`, whatever the method.
    Admin,
    /// `user`, whatever the method.
    User,
    /// `repo:read` for reads, `repo:write` for every other method.
    Repo,
    /// `repo:read` for reads, `repo:collab` for every other method.
    RepoCollab,
}

impl PatScope {
    /// The scope string a request needs. "Read" is decided by the HTTP method
    /// alone, so a `POST` that only reads is treated as a write: fail closed.
    pub fn required(self, is_read: bool) -> Option<&'static str> {
        match self {
            Self::Public => None,
            Self::Admin => Some("admin"),
            Self::User => Some("user"),
            Self::Repo => Some(if is_read { "repo:read" } else { "repo:write" }),
            Self::RepoCollab => Some(if is_read { "repo:read" } else { "repo:collab" }),
        }
    }
}

/// What a token limited to some branches may do on a route.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BranchLimit {
    /// Refused (the default): the route can move a ref, open a gate, or change
    /// settings, and its handler does not hold the target to the token.
    Refused,
    /// The route cannot move a branch or tag.
    Open,
    /// The route moves a ref, and its handler calls
    /// `ensure_token_may_write_branch` to hold the target to the token's limit.
    HandlerChecked,
}

/// Whether an `Idempotency-Key` may be used on a route.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Idempotency {
    /// Refused (the default).
    Unsupported,
    /// The response is stored and replayed, so it must carry no token or secret.
    Replayable,
}

/// One route's declaration. `path` is as registered in the router, relative to
/// `/api/v1`: `{name}` is one segment, `{*name}` one or more.
#[derive(Debug, Clone, Copy)]
pub struct RouteMeta {
    pub path: &'static str,
    pub scope: PatScope,
    pub other_repos: bool,
    pub branch_limit: BranchLimit,
    pub idempotency: Idempotency,
}

const fn declare(path: &'static str, scope: PatScope, other_repos: bool) -> RouteMeta {
    RouteMeta {
        path,
        scope,
        other_repos,
        branch_limit: BranchLimit::Refused,
        idempotency: Idempotency::Unsupported,
    }
}

/// A repository, organization or CI route: `repo:read` / `repo:write`.
pub const fn repo(path: &'static str) -> RouteMeta {
    declare(path, PatScope::Repo, false)
}

/// A repository route that can name or reach another repository.
pub const fn other_repos(path: &'static str) -> RouteMeta {
    declare(path, PatScope::Repo, true)
}

/// An account route: the `user` scope.
pub const fn user(path: &'static str) -> RouteMeta {
    declare(path, PatScope::User, false)
}

/// An administrator (or runner) route: the `admin` scope.
pub const fn admin(path: &'static str) -> RouteMeta {
    declare(path, PatScope::Admin, false)
}

impl RouteMeta {
    /// A write that cannot move a branch or tag.
    pub const fn open(self) -> Self {
        Self {
            branch_limit: BranchLimit::Open,
            ..self
        }
    }

    /// A ref-moving write whose handler checks the branch against the token.
    pub const fn checked(self) -> Self {
        Self {
            branch_limit: BranchLimit::HandlerChecked,
            ..self
        }
    }

    /// The response carries no secret: an idempotency key may be used.
    pub const fn idem(self) -> Self {
        Self {
            idempotency: Idempotency::Replayable,
            ..self
        }
    }
}

/// Every non-collaboration route of the REST API (`/api/v1`), declared once.
/// The collaboration area comes from the capability registry instead.
pub const ROUTES: &[RouteMeta] = &[
    admin("/runners/{id}/heartbeat"),
    admin("/runners/{id}/deregister"),
    admin("/runners/{id}/jobs/poll"),
    admin("/runners/{id}/jobs/{job_id}/start"),
    admin("/runners/{id}/jobs/{job_id}/log"),
    admin("/runners/{id}/jobs/{job_id}/workspace"),
    admin("/runners/{id}/jobs/{job_id}/cache"),
    admin("/runners/{id}/jobs/{job_id}/finish"),
    admin("/runners/{id}/jobs/{job_id}/artifacts"),
    user("/users/register"),
    user("/users/login"),
    user("/users/forgot-password"),
    user("/users/reset-password"),
    user("/users/logout"),
    user("/users/me"),
    user("/users/{username}"),
    user("/users/tokens"),
    user("/users/tokens/{id}"),
    user("/agents"),
    user("/agents/{username}"),
    user("/agents/{username}/tokens"),
    user("/agents/{username}/tokens/{id}"),
    user("/users/ssh-keys"),
    user("/users/ssh-keys/{id}"),
    repo("/repos/{owner}/{name}/keys"),
    repo("/repos/{owner}/{name}/keys/{id}"),
    user("/users/mfa/setup"),
    user("/users/mfa/enable"),
    user("/users/mfa/verify"),
    user("/users/mfa/backup"),
    user("/users/mfa/disable"),
    user("/auth/sso/providers"),
    user("/auth/sso/{slug}"),
    user("/auth/sso/{slug}/callback"),
    user("/auth/sso/{slug}/refresh"),
    user("/auth/sso/{slug}/unlink"),
    repo("/repos").idem(),
    repo("/repos/templates/gitignores"),
    repo("/repos/templates/licenses"),
    repo("/repos/templates/readmes"),
    repo("/repos/templates/labels"),
    repo("/repos/explore"),
    repo("/repos/{owner}"),
    repo("/repos/{owner}/{name}"),
    repo("/repos/{owner}/{name}/milestones").idem(),
    repo("/repos/{owner}/{name}/milestones/{id}").idem(),
    repo("/repos/{owner}/{name}/labels").idem(),
    repo("/repos/{owner}/{name}/labels/{id}").idem(),
    repo("/repos/{owner}/{name}/issue_templates"),
    repo("/repos/{owner}/{name}/issue_config"),
    repo("/repos/{owner}/{name}/issue_config/validate"),
    repo("/repos/{owner}/{name}/pull_request_template"),
    repo("/repos/{owner}/{name}/issues").open().idem(),
    repo("/repos/{owner}/{name}/issues/{number}").open().idem(),
    repo("/repos/{owner}/{name}/issues/{number}/labels")
        .open()
        .idem(),
    repo("/repos/{owner}/{name}/issues/{number}/assignees")
        .open()
        .idem(),
    repo("/repos/{owner}/{name}/issues/{number}/comments")
        .open()
        .idem(),
    repo("/repos/{owner}/{name}/issues/{number}/reactions")
        .open()
        .idem(),
    repo("/repos/{owner}/{name}/issues/comments/{comment_id}/reactions")
        .open()
        .idem(),
    repo("/repos/{owner}/{name}/issues/{number}/assets")
        .open()
        .idem(),
    repo("/repos/{owner}/{name}/issues/{number}/assets/{attachment_id}")
        .open()
        .idem(),
    repo("/repos/{owner}/{name}/issues/comments/{comment_id}/assets")
        .open()
        .idem(),
    repo("/repos/{owner}/{name}/issues/comments/{comment_id}/assets/{attachment_id}")
        .open()
        .idem(),
    repo("/repos/{owner}/{name}/pulls").open().idem(),
    repo("/repos/{owner}/{name}/pulls/{number}").open().idem(),
    repo("/repos/{owner}/{name}/pulls/{number}/assets").idem(),
    repo("/repos/{owner}/{name}/pulls/{number}/assets/{attachment_id}").idem(),
    repo("/repos/{owner}/{name}/pulls/comments/{comment_id}/assets").idem(),
    repo("/repos/{owner}/{name}/pulls/comments/{comment_id}/assets/{attachment_id}").idem(),
    repo("/repos/{owner}/{name}/pulls/{number}/export").idem(),
    repo("/repos/{owner}/{name}/pulls/{number}/diff").idem(),
    repo("/repos/{owner}/{name}/pulls/{number}/merge").idem(),
    repo("/repos/{owner}/{name}/pulls/{number}/merge-readiness").idem(),
    // Writes the head branch. Not `.open()` and not `.checked()`: a token
    // limited to some branches is refused by default (the handler also calls
    // `ensure_token_may_write_branch`, so relaxing this row later stays safe).
    repo("/repos/{owner}/{name}/pulls/{number}/update-branch").idem(),
    repo("/repos/{owner}/{name}/pulls/{number}/auto-merge").idem(),
    repo("/repos/{owner}/{name}/merge-queue"),
    repo("/repos/{owner}/{name}/pulls/{number}/merge-queue").idem(),
    repo("/repos/{owner}/{name}/pulls/{number}/reviews")
        .open()
        .idem(),
    repo("/repos/{owner}/{name}/pulls/{number}/reviews/{id}").idem(),
    repo("/repos/{owner}/{name}/pulls/{number}/reviews/{id}/dismiss").idem(),
    repo("/repos/{owner}/{name}/pulls/{number}/comments")
        .open()
        .idem(),
    repo("/repos/{owner}/{name}/pulls/{number}/timeline").idem(),
    repo("/repos/{owner}/{name}/pulls/{number}/comments/{id}/resolution")
        .open()
        .idem(),
    repo("/repos/{owner}/{name}/pulls/{number}/comments/{id}/suggestion/apply").idem(),
    repo("/repos/{owner}/{name}/pulls/{number}/suggestions/apply").idem(),
    repo("/repos/{owner}/{name}/pulls/{number}/reviewers")
        .open()
        .idem(),
    repo("/repos/{owner}/{name}/pulls/{number}/reviewers/{username}")
        .open()
        .idem(),
    repo("/repos/{owner}/{name}/wiki").idem(),
    repo("/repos/{owner}/{name}/wiki/{title}").idem(),
    repo("/repos/{owner}/{name}/wiki/{title}/history").idem(),
    repo("/repos/{owner}/{name}/wiki/{title}/revisions/{rev_id}").idem(),
    repo("/repos/{owner}/{name}/lfs/objects/batch"),
    repo("/repos/{owner}/{name}/lfs/objects/{oid}"),
    repo("/repos/{owner}/{name}/hooks"),
    repo("/repos/{owner}/{name}/hooks/{id}"),
    repo("/repos/{owner}/{name}/hooks/{id}/deliveries"),
    repo("/repos/{owner}/{name}/hooks/{id}/deliveries/{delivery_id}/redeliver"),
    repo("/repos/{owner}/{name}/pipelines"),
    repo("/repos/{owner}/{name}/pipelines/{id}"),
    repo("/repos/{owner}/{name}/pipelines/{id}/retry"),
    repo("/repos/{owner}/{name}/pipelines/{id}/cancel"),
    repo("/repos/{owner}/{name}/pipelines/{id}/jobs/{job_id}"),
    repo("/repos/{owner}/{name}/pipelines/{id}/jobs/{job_id}/log"),
    repo("/repos/{owner}/{name}/pipelines/{id}/wait"),
    repo("/repos/{owner}/{name}/events"),
    admin("/collaboration/capabilities"),
    repo("/repos/{owner}/{name}/pipelines/{id}/jobs/{job_id}/play"),
    repo("/repos/{owner}/{name}/pipelines/{pipeline_id}/jobs/{job_id}/approve"),
    repo("/repos/{owner}/{name}/pipelines/{pipeline_id}/jobs/{job_id}/rerun"),
    repo("/repos/{owner}/{name}/actions/environments"),
    repo("/repos/{owner}/{name}/actions/environments/{id}"),
    repo("/ci/oidc/.well-known/openid-configuration"),
    repo("/ci/oidc/jwks"),
    repo("/ci/oidc/token"),
    repo("/repos/{owner}/{name}/actions/retention"),
    repo("/repos/{owner}/{name}/actions/retention/expired"),
    repo("/repos/{owner}/{name}/actions/secrets"),
    repo("/repos/{owner}/{name}/actions/secrets/{secret_name}"),
    repo("/repos/{owner}/{name}/archive/{archive}"),
    repo("/repos/{owner}/{name}/branches/protection").idem(),
    repo("/repos/{owner}/{name}/branches/protection/{id}").idem(),
    repo("/repos/{owner}/{name}/tags/protection"),
    repo("/repos/{owner}/{name}/tags/protection/{id}"),
    repo("/repos/{owner}/{name}/collaborators"),
    repo("/repos/{owner}/{name}/collaborators/{id}"),
    repo("/repos/{owner}/{name}/tree"),
    repo("/repos/{owner}/{name}/blob/{*path}"),
    repo("/repos/{owner}/{name}/blame/{*path}"),
    repo("/repos/{owner}/{name}/log"),
    repo("/repos/{owner}/{name}/branches").idem(),
    repo("/repos/{owner}/{name}/branches/{*branch}")
        .checked()
        .idem(),
    repo("/repos/{owner}/{name}/tags"),
    repo("/repos/{owner}/{name}/commits/{sha}/signature").idem(),
    repo("/repos/{owner}/{name}/commits").checked().idem(),
    repo("/repos/{owner}/{name}/commits/{sha}/cherry-pick")
        .checked()
        .idem(),
    repo("/repos/{owner}/{name}/commits/{sha}/revert")
        .checked()
        .idem(),
    repo("/repos/{owner}/{name}/contents/{*path}")
        .checked()
        .idem(),
    repo("/repos/{owner}/{name}/mirror"),
    repo("/repos/{owner}/{name}/mirror/sync"),
    repo("/imports"),
    repo("/imports/{id}"),
    repo("/repos/{owner}/{name}/boards"),
    repo("/repos/{owner}/{name}/boards/{id}"),
    repo("/repos/{owner}/{name}/boards/{id}/columns"),
    repo("/repos/{owner}/{name}/boards/{id}/columns/{col_id}"),
    repo("/repos/{owner}/{name}/boards/{id}/columns/{col_id}/cards"),
    repo("/repos/{owner}/{name}/boards/{id}/cards/{card_id}"),
    repo("/repos/{owner}/{name}/boards/{id}/cards/{card_id}/move"),
    repo("/repos/{owner}/{name}/boards/{id}/cards/reorder"),
    repo("/repos/{owner}/{name}/issues/{number}/time")
        .open()
        .idem(),
    repo("/repos/{owner}/{name}/issues/{number}/time/total")
        .open()
        .idem(),
    repo("/repos/{owner}/{name}/issues/{number}/time/{id}")
        .open()
        .idem(),
    repo("/repos/{owner}/{name}/statuses/{sha}"),
    repo("/repos/{owner}/{name}/commits/{sha}/statuses").idem(),
    repo("/repos/{owner}/{name}/commits/{sha}/status").idem(),
    repo("/orgs"),
    repo("/orgs/{name}"),
    repo("/orgs/{name}/members"),
    repo("/orgs/{name}/members/{user_id}"),
    repo("/orgs/{name}/teams"),
    repo("/orgs/{name}/teams/{team_id}"),
    repo("/orgs/{name}/teams/{team_id}/members"),
    repo("/orgs/{name}/teams/{team_id}/members/{user_id}"),
    repo("/orgs/{name}/teams/{team_id}/repositories"),
    user("/notifications"),
    user("/notifications/unread-count"),
    user("/notifications/mark-all-read"),
    user("/notifications/{id}/read"),
    user("/notifications/{id}"),
    repo("/repos/{owner}/{name}/star"),
    repo("/repos/{owner}/{name}/starred"),
    repo("/repos/{owner}/{name}/stargazers"),
    repo("/repos/{owner}/{name}/watch"),
    repo("/repos/{owner}/{name}/releases").idem(),
    repo("/repos/{owner}/{name}/releases/tags/{tag}").idem(),
    repo("/repos/{owner}/{name}/releases/{id}").idem(),
    repo("/repos/{owner}/{name}/releases/{release_id}/assets").idem(),
    repo("/repos/{owner}/{name}/releases/assets/{asset_id}").idem(),
    repo("/repos/{owner}/{name}/releases/assets/{asset_id}/download").idem(),
    other_repos("/repos/{owner}/{name}/fork"),
    other_repos("/repos/{owner}/{name}/forks"),
    other_repos("/repos/{owner}/{name}/transfer"),
    repo("/repos/{owner}/{name}/packages"),
    repo("/repos/{owner}/{name}/packages/{pkg_type}/publish"),
    repo("/repos/{owner}/{name}/packages/{pkg_type}/list"),
    repo("/repos/{owner}/{name}/packages/cargo/index/{pkg}"),
    repo("/repos/{owner}/{name}/packages/npm/publish"),
    repo("/repos/{owner}/{name}/packages/npm/list"),
    repo("/repos/{owner}/{name}/packages/npm/{pkg_name}"),
    repo("/repos/{owner}/{name}/packages/pypi/simple/{pkg_name}"),
    repo("/repos/{owner}/{name}/packages/maven/{group_id}/{artifact_id}/maven-metadata.xml"),
    repo("/repos/{owner}/{name}/packages/nuget/index.json"),
    repo("/repos/{owner}/{name}/packages/nuget/registration/{id}/index.json"),
    repo("/repos/{owner}/{name}/packages/nuget/query"),
    repo("/repos/{owner}/{name}/packages/rubygems/api/v1/dependencies"),
    repo("/repos/{owner}/{name}/packages/rubygems/api/v1/gems/{gem_name}"),
    repo("/repos/{owner}/{name}/packages/helm/index.yaml"),
    repo("/repos/{owner}/{name}/packages/composer/packages.json"),
    repo("/repos/{owner}/{name}/packages/{pkg_type}/{pkg_name}"),
    repo("/repos/{owner}/{name}/packages/{pkg_type}/{pkg_name}/versions"),
    repo("/repos/{owner}/{name}/packages/{pkg_type}/{pkg_name}/{version}"),
    repo("/repos/{owner}/{name}/packages/{pkg_type}/{pkg_name}/{version}/yank"),
    repo("/repos/{owner}/{name}/packages/{pkg_type}/{pkg_name}/{version}/{*file}"),
    admin("/runners/register"),
    repo("/repos/{owner}/{name}/pipelines/{id}/artifacts"),
    repo("/artifacts/{id}"),
    repo("/artifacts/{id}/download"),
    admin("/admin/runners"),
    admin("/admin/runners/{id}"),
    admin("/admin/users"),
    admin("/admin/users/{id}"),
    admin("/admin/users/{id}/unlock"),
    admin("/admin/users/{id}/collaboration-impact"),
    admin("/admin/orgs"),
    admin("/admin/orgs/{name}"),
    admin("/admin/sso/providers"),
    admin("/admin/sso/providers/{id}"),
    admin("/admin/sso/providers/{id}/test"),
    admin("/admin/audit/logs"),
    admin("/admin/audit/logs/{id}"),
    admin("/admin/login-attempts"),
    admin("/admin/settings"),
    repo("/search"),
    repo("/repos/{owner}/{name}/webhooks/external/ci"),
    repo("/ai/repos/{owner}/{name}/summary"),
    repo("/ai/repos/{owner}/{name}/issues"),
    repo("/ai/repos/{owner}/{name}/prs"),
    repo("/ai/repos/{owner}/{name}/tree"),
    repo("/ai/repos/{owner}/{name}/search/code"),
    repo("/ai/repos/{owner}/{name}/index"),
    repo("/ai/repos/{owner}/{name}/pulls/{number}/context"),
    user("/ws/notifications"),
    repo("/ws/job/{job_id}"),
];

/// Fail-closed patterns that are **not** routes: they win over a broader
/// catch-all route so that a path the catch-all would otherwise take stays
/// refused. Each needs a reason.
pub const GUARDS: &[RouteMeta] = &[
    // `branches/protection/**` is the protection-rule settings. The router's
    // static `branches/protection/{id}` wins for the paths it matches, but a
    // longer path such as `branches/protection/a/b` falls to the
    // `branches/{*branch}` catch-all, which deletes a branch. A branch-limited
    // token is never opened that way.
    repo("/repos/{owner}/{name}/branches/protection/{*rest}"),
];

// ── Lookup ─────────────────────────────────────────────────────────────

/// What the registry says about one route.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Resolved {
    pub scope: PatScope,
    pub other_repos: bool,
    pub branch_limit: BranchLimit,
    pub idempotency: Idempotency,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Seg {
    Literal(&'static str),
    Param,
    /// `{*rest}`: one or more segments, and always the last.
    Rest,
}

impl Seg {
    /// A literal beats a parameter, which beats a catch-all: the order axum's
    /// router prefers.
    fn rank(self) -> u8 {
        match self {
            Self::Literal(_) => 2,
            Self::Param => 1,
            Self::Rest => 0,
        }
    }
}

struct Entry {
    segs: Vec<Seg>,
    meta: Resolved,
}

fn parse_pattern(path: &'static str) -> Vec<Seg> {
    path.trim_start_matches('/')
        .split('/')
        .map(|s| {
            if s.starts_with("{*") {
                Seg::Rest
            } else if s.starts_with('{') {
                Seg::Param
            } else {
                Seg::Literal(s)
            }
        })
        .collect()
}

impl Entry {
    fn matches(&self, path: &[&str]) -> bool {
        for (i, seg) in self.segs.iter().enumerate() {
            match seg {
                Seg::Rest => return path.len() > i,
                Seg::Param => {
                    if i >= path.len() {
                        return false;
                    }
                }
                Seg::Literal(lit) => {
                    if path.get(i) != Some(lit) {
                        return false;
                    }
                }
            }
        }
        path.len() == self.segs.len()
    }

    /// Whether `self` is a closer match than `other`, both of which match the
    /// same path.
    fn more_specific_than(&self, other: &Entry) -> bool {
        for (a, b) in self.segs.iter().zip(&other.segs) {
            if a.rank() != b.rank() {
                return a.rank() > b.rank();
            }
        }
        false
    }
}

/// The collaboration routes, derived from the capability registry.
const COLLAB_PREFIX: &str = "/repos/{owner}/{name}/collaboration";

fn table() -> &'static [Entry] {
    static TABLE: OnceLock<Vec<Entry>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut entries: Vec<Entry> = ROUTES
            .iter()
            .chain(GUARDS)
            .map(|m| Entry {
                segs: parse_pattern(m.path),
                meta: Resolved {
                    scope: m.scope,
                    other_repos: m.other_repos,
                    branch_limit: m.branch_limit,
                    idempotency: m.idempotency,
                },
            })
            .collect();
        for route in capabilities::REGISTRY.iter().flat_map(|c| c.routes) {
            let segs = parse_pattern(route.path);
            if !collab_path(route.path) {
                continue;
            }
            // A path with several methods is one declaration; reaching another
            // repository on any of them counts for all.
            if let Some(existing) = entries.iter_mut().find(|e| e.segs == segs) {
                existing.meta.other_repos |= route.other_repos;
                continue;
            }
            entries.push(Entry {
                segs,
                meta: Resolved {
                    scope: PatScope::RepoCollab,
                    other_repos: route.other_repos,
                    branch_limit: BranchLimit::Open,
                    idempotency: Idempotency::Unsupported,
                },
            });
        }
        entries
    })
}

fn collab_path(path: &str) -> bool {
    path == COLLAB_PREFIX || path.starts_with("/repos/{owner}/{name}/collaboration/")
}

/// The declaration of the route `path` (without the `/api/v1` prefix and
/// without a query) is, or `None` when it is not a registered route. A path
/// without exactly one leading slash, or with an empty, `.` or `..` segment,
/// never matches.
pub fn lookup(path: &str) -> Option<Resolved> {
    // Exactly one leading slash: `//users/me` and `users/me` are not the route
    // (and the scope defaults, which match on the raw prefix, must agree).
    let segs: Vec<&str> = path.strip_prefix('/')?.split('/').collect();
    if segs.iter().any(|s| s.is_empty() || *s == "." || *s == "..") {
        return None;
    }
    let mut best: Option<&Entry> = None;
    for entry in table() {
        if !entry.matches(&segs) {
            continue;
        }
        best = match best {
            Some(current) if !entry.more_specific_than(current) => Some(current),
            _ => Some(entry),
        };
    }
    best.map(|e| e.meta)
}

/// All route declarations (static rows and derived collaboration routes, not
/// [`GUARDS`]) as patterns, for the drift tests.
pub fn declared_patterns() -> Vec<String> {
    let guards: Vec<Vec<Seg>> = GUARDS.iter().map(|g| parse_pattern(g.path)).collect();
    table()
        .iter()
        .filter(|e| !guards.contains(&e.segs))
        .map(|e| {
            let mut out = String::new();
            for seg in &e.segs {
                out.push('/');
                match seg {
                    Seg::Literal(s) => out.push_str(s),
                    Seg::Param => out.push_str("{}"),
                    Seg::Rest => out.push_str("{*}"),
                }
            }
            out
        })
        .collect()
}

// ── Derived decisions ──────────────────────────────────────────────────

/// The PAT scope a request needs, or `None` for a route that needs no
/// credential. `path` is without the `/api/v1` prefix and without a query.
///
/// A registered route answers from its declaration; any other path from the
/// [`family_scope`] defaults, and an unrecognised prefix needs `admin`.
pub fn pat_scope_for(is_read: bool, path: &str) -> Option<&'static str> {
    match lookup(path) {
        Some(meta) => meta.scope.required(is_read),
        None => family_scope(is_read, path),
    }
}

/// Whether the route reaches a repository other than the one in its path.
/// Beyond the declarations, the whole `fork`, `forks`, `transfer` and
/// `collaboration/groups` families are refused for any path (a route added
/// there later is covered even before it is declared).
///
/// `path` is without the `/api/v1` prefix and without a query.
pub fn reaches_other_repos(path: &str) -> bool {
    if lookup(path).is_some_and(|m| m.other_repos) {
        return true;
    }
    let segments: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    if segments
        .iter()
        .any(|s| s.is_empty() || *s == "." || *s == "..")
    {
        // Not a path the allow-list accepts in the first place.
        return false;
    }
    let rest = match segments.as_slice() {
        ["repos", _, _, rest @ ..] | ["ai", "repos", _, _, rest @ ..] => rest,
        _ => return false,
    };
    matches!(
        rest,
        ["fork", ..] | ["forks", ..] | ["transfer", ..] | ["collaboration", "groups", ..]
    )
}

/// Whether a **write** by a token limited to some branches is allowed on the
/// route. The caller must already know it is a write: this never looks at the
/// method. A path that is not a declared route is refused.
pub fn branch_limited_write_allowed(path: &str) -> bool {
    lookup(path).is_some_and(|m| m.branch_limit != BranchLimit::Refused)
}

/// Whether an `Idempotency-Key` may be used on the route. A path that is not a
/// declared route is refused.
pub fn idempotency_replayable(path: &str) -> bool {
    lookup(path).is_some_and(|m| m.idempotency == Idempotency::Replayable)
}

// ── Family defaults (paths that are not registered routes) ─────────────

/// The scope of a path that is not a registered route. Reads of a known
/// repository family need `repo:read`, writes `repo:write` (or `repo:collab`
/// inside `/repos/{owner}/{name}/collaboration`); an unrecognised prefix needs
/// `admin`, so a new family cannot be reached by a repository token by
/// accident.
pub fn family_scope(is_read: bool, path: &str) -> Option<&'static str> {
    // Public docs and health checks need no credential at all.
    if path.starts_with("/api-docs") || path == "/health" || path == "/metrics" {
        return None;
    }

    // Administrator surfaces. Registering a runner also mints a credential, so
    // it stays behind `admin` even though the route lives under /runners.
    if path.starts_with("/admin") || path == "/runners/register" {
        return Some("admin");
    }

    // The authenticated user's own account, notifications and session.
    if path.starts_with("/users")
        || path == "/agents"
        || path.starts_with("/agents/")
        || path.starts_with("/notifications")
        || path.starts_with("/auth")
        || path == "/ws/notifications"
    {
        return Some("user");
    }

    // Repository-scoped surfaces, including organization and CI routes that
    // resolve a repository before acting.
    const REPO_SCOPED: &[&str] = &[
        "/repos",
        "/orgs",
        "/ci",
        "/artifacts",
        "/imports",
        "/search",
        "/ai",
        "/ws",
    ];
    if REPO_SCOPED.iter().any(|prefix| path.starts_with(prefix)) {
        return Some(family_repo_scope(is_read, path));
    }

    // Runner job traffic authenticates with a runner token, not a PAT, and is
    // gated by `authenticate_runner`; anything else unknown needs `admin`.
    if path.starts_with("/runners") {
        return Some("admin");
    }

    // Fail closed: a PAT must not reach an unrecognised route by default.
    Some("admin")
}

fn family_repo_scope(is_read: bool, path: &str) -> &'static str {
    if is_read {
        return "repo:read";
    }
    // `/repos/{owner}/{name}/collaboration` and anything below it.
    let mut parts = path.trim_start_matches('/').splitn(4, '/');
    let is_collab = parts.next() == Some("repos")
        && parts.next().is_some_and(|owner| !owner.is_empty())
        && parts.next().is_some_and(|name| !name.is_empty())
        && parts.next().is_some_and(|rest| {
            (rest == "collaboration" || rest.starts_with("collaboration/"))
                && !rest.split('/').any(|seg| seg == ".." || seg == ".")
        });
    if is_collab {
        "repo:collab"
    } else {
        "repo:write"
    }
}
