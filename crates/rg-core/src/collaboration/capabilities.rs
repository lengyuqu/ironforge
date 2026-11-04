//! The collaboration capability registry: the one static list of what this
//! build offers over REST and MCP, behind `GET /api/v1/collaboration/capabilities`.
//!
//! Design: `agent-collaboration-p2-design-2026-11.md` §6 (R14), acceptance M10–M11.
//!
//! # Adding a capability (append-only)
//!
//! Add **one** `Capability { .. }` element at the end of [`REGISTRY`] listing
//! its REST routes (exactly as registered in `rg-http/src/lib.rs`, relative to
//! `/api/v1`) and the MCP tools that adapt it. Nothing else is needed for
//! discovery. The contract tests (`rg-http/tests/collab_contract_tests.rs`)
//! fail with a readable message when
//!
//! * a `/collaboration` route registered in `lib.rs` is missing here (or listed
//!   here but not registered, or with different methods),
//! * a listed route is absent from the OpenAPI document,
//! * a name in `rg_mcp::collab::TOOL_NAMES` is not owned by exactly one
//!   capability, or has no row in `rg_mcp::collab_contract::TOOL_ROUTES`.
//!
//! Only list what exists in the build: the registry is what clients are told
//! they may rely on. A new *optional* field or capability bumps
//! [`CONTRACT_MINOR`]; removing or changing meaning needs a new major or a new
//! endpoint (see the version policy in `.ai/README.md`).

use serde::Serialize;

/// Contract version `major.minor`. Optional response fields, new capabilities
/// and new optional enum values bump the minor; removals, semantic changes and
/// new required request fields need a new major (or a new endpoint).
pub const CONTRACT_MAJOR: u32 = 1;
pub const CONTRACT_MINOR: u32 = 1;

pub fn contract_version() -> String {
    format!("{CONTRACT_MAJOR}.{CONTRACT_MINOR}")
}

/// Suggested minimum notice before a deprecated behaviour is removed.
pub const DEPRECATION_WINDOW_DAYS: u32 = 90;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Stability {
    Stable,
    Experimental,
}

/// What a route demands from the caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    /// Any authenticated account.
    Account,
    /// Owner, collaborator, organization member or writer of the repository.
    CollabRead,
    /// Like `CollabRead`, plus the acting session's id and secret headers.
    CollabReadSession,
    /// Repository write access.
    RepoWrite,
    /// Repository write access plus the acting session's id and secret headers.
    RepoWriteSession,
    /// Repository admin.
    RepoAdmin,
    /// Site administrator.
    SiteAdmin,
}

impl Access {
    /// Atomic requirement names, as advertised in `requires`.
    pub fn atoms(self) -> &'static [&'static str] {
        match self {
            Self::Account => &["account"],
            Self::CollabRead => &["repo_collab_read"],
            Self::CollabReadSession => &["repo_collab_read", "session_credentials"],
            Self::RepoWrite => &["repo_write"],
            Self::RepoWriteSession => &["repo_write", "session_credentials"],
            Self::RepoAdmin => &["repo_admin"],
            Self::SiteAdmin => &["site_admin"],
        }
    }
}

/// One REST operation: `method` is upper-case, `path` is as registered in the
/// router (relative to `/api/v1`).
#[derive(Debug, Clone, Copy)]
pub struct Route {
    pub method: &'static str,
    pub path: &'static str,
    pub access: Access,
}

const fn r(method: &'static str, path: &'static str, access: Access) -> Route {
    Route { method, path, access }
}

#[derive(Debug, Clone, Copy)]
pub struct Capability {
    pub name: &'static str,
    /// `major.minor` of this capability.
    pub version: &'static str,
    pub status: Stability,
    pub routes: &'static [Route],
    /// MCP tools adapting this capability (names from `rg_mcp::collab::TOOL_NAMES`).
    pub mcp_tools: &'static [&'static str],
}

impl Capability {
    /// Distinct requirement names across the capability's routes.
    pub fn requires(&self) -> Vec<&'static str> {
        let mut out: Vec<&'static str> = Vec::new();
        for route in self.routes {
            for atom in route.access.atoms() {
                if !out.contains(atom) {
                    out.push(atom);
                }
            }
        }
        out.sort_unstable();
        out
    }
}

use Access::{Account, CollabRead, CollabReadSession, RepoAdmin, RepoWrite, RepoWriteSession, SiteAdmin};
use Stability::Stable;

const SESSIONS: &str = "/repos/{owner}/{name}/collaboration/sessions";

/// Every capability of this build. Append new entries at the end.
pub const REGISTRY: &[Capability] = &[
    Capability { name: "discovery", version: "1.0", status: Stable, mcp_tools: &[], routes: &[
        r("GET", "/collaboration/capabilities", Account),
        r("GET", "/repos/{owner}/{name}/collaboration/capabilities", CollabRead),
    ]},
    Capability { name: "sessions", version: "1.0", status: Stable,
        mcp_tools: &["collab_start_session", "collab_list_sessions", "collab_heartbeat", "collab_end_session"], routes: &[
        r("GET", SESSIONS, RepoWrite),
        r("POST", SESSIONS, RepoWrite),
        r("POST", "/repos/{owner}/{name}/collaboration/sessions/{id}/heartbeat", RepoWriteSession),
        r("POST", "/repos/{owner}/{name}/collaboration/sessions/{id}/end", RepoWriteSession),
    ]},
    Capability { name: "tasks", version: "1.0", status: Stable,
        mcp_tools: &["collab_list_tasks", "collab_create_task", "collab_get_task", "collab_update_task"], routes: &[
        r("GET", "/repos/{owner}/{name}/collaboration/tasks", CollabRead),
        r("POST", "/repos/{owner}/{name}/collaboration/tasks", RepoWriteSession),
        r("GET", "/repos/{owner}/{name}/collaboration/tasks/{id}", CollabRead),
        r("PATCH", "/repos/{owner}/{name}/collaboration/tasks/{id}", RepoWriteSession),
    ]},
    Capability { name: "messages", version: "1.0", status: Stable,
        mcp_tools: &["collab_list_messages", "collab_send_message", "collab_mark_read"], routes: &[
        r("GET", "/repos/{owner}/{name}/collaboration/tasks/{id}/messages", CollabRead),
        r("POST", "/repos/{owner}/{name}/collaboration/tasks/{id}/messages", RepoWriteSession),
        r("PUT", "/repos/{owner}/{name}/collaboration/tasks/{id}/read", RepoWriteSession),
    ]},
    Capability { name: "handoffs", version: "1.0", status: Stable,
        mcp_tools: &["collab_create_handoff", "collab_list_handoffs", "collab_accept_handoff", "collab_cancel_handoff"], routes: &[
        r("GET", "/repos/{owner}/{name}/collaboration/tasks/{id}/handoffs", CollabRead),
        r("POST", "/repos/{owner}/{name}/collaboration/tasks/{id}/handoffs", RepoWriteSession),
        r("POST", "/repos/{owner}/{name}/collaboration/handoffs/{id}/accept", RepoWriteSession),
        r("POST", "/repos/{owner}/{name}/collaboration/handoffs/{id}/cancel", RepoWriteSession),
    ]},
    Capability { name: "rules", version: "1.0", status: Stable,
        mcp_tools: &["collab_get_rules"], routes: &[
        r("GET", "/repos/{owner}/{name}/collaboration/rules", CollabRead),
    ]},
    Capability { name: "takeovers", version: "1.0", status: Stable,
        mcp_tools: &["collab_claim_task", "collab_list_takeovers"], routes: &[
        r("GET", "/repos/{owner}/{name}/collaboration/tasks/{id}/takeovers", CollabRead),
        r("POST", "/repos/{owner}/{name}/collaboration/tasks/{id}/takeovers", RepoWriteSession),
        r("POST", "/repos/{owner}/{name}/collaboration/tasks/{id}/reassign", RepoWrite),
    ]},
    Capability { name: "questions", version: "1.0", status: Stable,
        mcp_tools: &["collab_ask_question", "collab_list_questions", "collab_get_question", "collab_answer_question", "collab_cancel_question"], routes: &[
        r("GET", "/repos/{owner}/{name}/collaboration/tasks/{id}/questions", CollabRead),
        r("POST", "/repos/{owner}/{name}/collaboration/tasks/{id}/questions", RepoWrite),
        r("GET", "/repos/{owner}/{name}/collaboration/questions/{id}", CollabRead),
        r("POST", "/repos/{owner}/{name}/collaboration/questions/{id}/answer", RepoWrite),
        r("POST", "/repos/{owner}/{name}/collaboration/questions/{id}/cancel", RepoWrite),
    ]},
    Capability { name: "work_packages", version: "1.0", status: Stable,
        mcp_tools: &["collab_create_work_package", "collab_list_work_packages", "collab_get_work_package"], routes: &[
        r("GET", "/repos/{owner}/{name}/collaboration/tasks/{id}/work-packages", CollabRead),
        r("POST", "/repos/{owner}/{name}/collaboration/tasks/{id}/work-packages", RepoWrite),
        r("GET", "/repos/{owner}/{name}/collaboration/work-packages/{id}", CollabRead),
    ]},
    Capability { name: "inbox", version: "1.0", status: Stable,
        mcp_tools: &["collab_get_inbox"], routes: &[
        r("GET", "/repos/{owner}/{name}/collaboration/inbox", CollabReadSession),
        r("GET", "/repos/{owner}/{name}/collaboration/inbox/user", RepoWrite),
    ]},
    Capability { name: "completion_status", version: "1.0", status: Stable,
        mcp_tools: &["collab_get_completion_status"], routes: &[
        r("GET", "/repos/{owner}/{name}/collaboration/tasks/{id}/completion-status", CollabRead),
    ]},
    Capability { name: "deletion_impact", version: "1.0", status: Stable, mcp_tools: &[], routes: &[
        r("GET", "/repos/{owner}/{name}/collaboration/deletion-impact", RepoAdmin),
        r("GET", "/admin/users/{id}/collaboration-impact", SiteAdmin),
    ]},
    Capability { name: "dependencies", version: "1.0", status: Stable,
        mcp_tools: &["collab_get_dependencies", "collab_add_dependency", "collab_remove_dependency"], routes: &[
        r("GET", "/repos/{owner}/{name}/collaboration/tasks/{id}/dependencies", CollabRead),
        r("POST", "/repos/{owner}/{name}/collaboration/tasks/{id}/dependencies", RepoWrite),
        r("DELETE", "/repos/{owner}/{name}/collaboration/tasks/{id}/dependencies/{depends_on_id}", RepoWrite),
    ]},
    Capability { name: "decisions", version: "1.0", status: Stable,
        mcp_tools: &["collab_create_decision", "collab_list_decisions", "collab_get_decision", "collab_change_decision"], routes: &[
        r("GET", "/repos/{owner}/{name}/collaboration/tasks/{id}/decisions", CollabRead),
        r("POST", "/repos/{owner}/{name}/collaboration/tasks/{id}/decisions", RepoWrite),
        r("GET", "/repos/{owner}/{name}/collaboration/decisions/{id}", CollabRead),
        r("POST", "/repos/{owner}/{name}/collaboration/decisions/{id}/changes", RepoWrite),
    ]},
    Capability { name: "groups", version: "1.0", status: Stable,
        mcp_tools: &["collab_create_group", "collab_get_group", "collab_add_group_link", "collab_list_group_links", "collab_remove_group_link"], routes: &[
        r("POST", "/repos/{owner}/{name}/collaboration/groups", RepoWrite),
        r("GET", "/repos/{owner}/{name}/collaboration/groups/{group_id}", CollabRead),
        r("GET", "/repos/{owner}/{name}/collaboration/groups/{group_id}/links", CollabRead),
        r("POST", "/repos/{owner}/{name}/collaboration/groups/{group_id}/links", RepoWrite),
        r("DELETE", "/repos/{owner}/{name}/collaboration/groups/{group_id}/links/{link_id}", RepoWrite),
    ]},
    Capability { name: "health", version: "1.0", status: Stable,
        mcp_tools: &[], routes: &[
        r("GET", "/repos/{owner}/{name}/collaboration/health", RepoAdmin),
        r("GET", "/repos/{owner}/{name}/collaboration/health/tasks", RepoAdmin),
        r("POST", "/repos/{owner}/{name}/collaboration/health/baseline", RepoAdmin),
    ]},
    Capability { name: "templates", version: "1.0", status: Stable,
        mcp_tools: &["collab_create_template", "collab_list_templates", "collab_get_template", "collab_task_from_template"], routes: &[
        r("GET", "/repos/{owner}/{name}/collaboration/templates", CollabRead),
        r("POST", "/repos/{owner}/{name}/collaboration/templates", RepoAdmin),
        r("GET", "/repos/{owner}/{name}/collaboration/templates/{id}", CollabRead),
        r("POST", "/repos/{owner}/{name}/collaboration/templates/{id}/versions", RepoAdmin),
        r("POST", "/repos/{owner}/{name}/collaboration/tasks/from-template", RepoWriteSession),
        r("GET", "/repos/{owner}/{name}/collaboration/tasks/{id}/template", CollabRead),
    ]},
    Capability { name: "declarations", version: "1.0", status: Stable,
        mcp_tools: &["collab_declare_capabilities", "collab_get_declaration"], routes: &[
        r("GET", "/repos/{owner}/{name}/collaboration/sessions/{id}/declaration", CollabRead),
        r("PUT", "/repos/{owner}/{name}/collaboration/sessions/{id}/declaration", RepoWriteSession),
        r("GET", "/repos/{owner}/{name}/collaboration/declarations", CollabRead),
    ]},
    Capability { name: "budgets", version: "1.0", status: Stable,
        mcp_tools: &["collab_set_budget", "collab_report_usage", "collab_get_budget"], routes: &[
        r("PUT", "/repos/{owner}/{name}/collaboration/tasks/{id}/budgets", RepoWrite),
        r("GET", "/repos/{owner}/{name}/collaboration/tasks/{id}/budget", CollabRead),
        r("GET", "/repos/{owner}/{name}/collaboration/tasks/{id}/usage", CollabRead),
        r("POST", "/repos/{owner}/{name}/collaboration/tasks/{id}/usage", RepoWrite),
    ]},
    Capability { name: "notices", version: "1.0", status: Stable, mcp_tools: &[], routes: &[
        r("GET", "/repos/{owner}/{name}/collaboration/notices", CollabReadSession),
        r("POST", "/repos/{owner}/{name}/collaboration/notices/{id}/ack", CollabReadSession),
        r("POST", "/repos/{owner}/{name}/collaboration/tasks/{id}/messages/{message_id}/read-receipt", RepoWriteSession),
        r("GET", "/repos/{owner}/{name}/collaboration/tasks/{id}/messages/{message_id}/delivery", CollabReadSession),
    ]},
    Capability { name: "escalation_settings", version: "1.0", status: Stable, mcp_tools: &[], routes: &[
        r("GET", "/repos/{owner}/{name}/collaboration/escalation-settings", CollabRead),
        r("PUT", "/repos/{owner}/{name}/collaboration/escalation-settings", RepoAdmin),
    ]},
    Capability { name: "diagnostics", version: "1.0", status: Stable, mcp_tools: &[], routes: &[
        r("GET", "/repos/{owner}/{name}/collaboration/diagnostics", CollabRead),
    ]},
    Capability { name: "search", version: "1.0", status: Stable, mcp_tools: &[], routes: &[
        r("GET", "/repos/{owner}/{name}/collaboration/search", CollabRead),
    ]},
    // ↑ append new capabilities above this line ↑
];

/// Capabilities, in registry order.
pub fn all() -> &'static [Capability] {
    REGISTRY
}

pub fn find(name: &str) -> Option<&'static Capability> {
    REGISTRY.iter().find(|c| c.name == name)
}

/// All MCP tool names the registry knows, in registry order.
pub fn mcp_tools() -> Vec<&'static str> {
    REGISTRY.iter().flat_map(|c| c.mcp_tools.iter().copied()).collect()
}

/// Whether `client` (a `major.minor` it was built against) can talk to this
/// server: same major, and the client is not newer than the server.
pub fn client_is_compatible(client: &str) -> bool {
    let mut parts = client.split('.');
    let (Some(major), Some(minor), None) = (parts.next(), parts.next(), parts.next()) else {
        return false;
    };
    match (major.parse::<u32>(), minor.parse::<u32>()) {
        (Ok(major), Ok(minor)) => major == CONTRACT_MAJOR && minor <= CONTRACT_MINOR,
        _ => false,
    }
}

/// A scheduled removal of behaviour, advertised in `deprecations`.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Deprecation {
    pub capability: &'static str,
    /// What is deprecated (a field, value, route or behaviour).
    pub what: &'static str,
    pub replacement: &'static str,
    /// `YYYY-MM-DD` the deprecation was announced.
    pub announced: &'static str,
    /// `YYYY-MM-DD`; at least [`DEPRECATION_WINDOW_DAYS`] after `announced`.
    pub earliest_removal: &'static str,
}

/// Nothing is deprecated yet. To deprecate: append an entry, keep the old
/// behaviour under a regression test until `earliest_removal`, bump the minor.
pub const DEPRECATIONS: &[Deprecation] = &[];

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn names_are_unique_and_versions_are_major_minor() {
        let mut seen = HashSet::new();
        for c in REGISTRY {
            assert!(seen.insert(c.name), "duplicate capability {}", c.name);
            let (major, minor) = c.version.split_once('.').expect("major.minor");
            assert!(major.parse::<u32>().is_ok() && minor.parse::<u32>().is_ok(), "{}", c.name);
        }
    }

    #[test]
    fn a_route_belongs_to_exactly_one_capability() {
        let mut seen = HashSet::new();
        for c in REGISTRY {
            for route in c.routes {
                assert!(
                    seen.insert((route.method, route.path)),
                    "{} {} is listed twice",
                    route.method,
                    route.path
                );
                assert!(route.path.starts_with('/'), "{}", route.path);
                assert_eq!(route.method, route.method.to_uppercase());
            }
        }
    }

    #[test]
    fn a_tool_belongs_to_exactly_one_capability() {
        let tools = mcp_tools();
        let unique: HashSet<_> = tools.iter().collect();
        assert_eq!(tools.len(), unique.len(), "an MCP tool is owned by two capabilities");
    }

    #[test]
    fn deprecations_respect_the_window() {
        for d in DEPRECATIONS {
            let announced = chrono::NaiveDate::parse_from_str(d.announced, "%Y-%m-%d").unwrap();
            let removal = chrono::NaiveDate::parse_from_str(d.earliest_removal, "%Y-%m-%d").unwrap();
            assert!((removal - announced).num_days() >= i64::from(DEPRECATION_WINDOW_DAYS), "{}", d.what);
            assert!(find(d.capability).is_some(), "{}", d.capability);
        }
    }

    #[test]
    fn compatibility_rule_is_same_major_and_not_newer() {
        assert!(client_is_compatible("1.0"));
        assert!(client_is_compatible("1.1"));
        assert!(!client_is_compatible("1.2"));
        assert!(!client_is_compatible("2.0"));
        assert!(!client_is_compatible("1"));
        assert!(!client_is_compatible("1.0.0"));
        assert!(!client_is_compatible("x.y"));
    }
}
