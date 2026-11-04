//! The REST operation behind every `collab_*` tool, as data.
//!
//! This table exists so contract tests can prove the MCP tools and the REST API
//! cannot drift apart: `rg-http/tests/collab_contract_tests.rs` checks each row
//! against the routes registered by the server, the OpenAPI document (path,
//! method, query parameters, request-body properties) and the capability
//! registry, and the unit test below checks each row against what
//! `collab::plan` really sends.
//!
//! Adding a tool: add the name to `collab::TOOL_NAMES`, build its plan, add one
//! row here and list the name in the owning capability of
//! `rg_core::collaboration::capabilities::REGISTRY`.

/// One tool → REST operation mapping.
#[derive(Debug, Clone, Copy)]
pub struct ToolRoute {
    pub tool: &'static str,
    pub method: &'static str,
    /// Below `/repos/{owner}/{name}/collaboration`; `{x}` are path parameters.
    pub path: &'static str,
    /// Query parameters the tool may send.
    pub query: &'static [&'static str],
    /// JSON body properties the tool may send (empty = no body).
    pub body: &'static [&'static str],
}

const fn t(
    tool: &'static str,
    method: &'static str,
    path: &'static str,
    query: &'static [&'static str],
    body: &'static [&'static str],
) -> ToolRoute {
    ToolRoute { tool, method, path, query, body }
}

pub const TOOL_ROUTES: &[ToolRoute] = &[
    t("collab_start_session", "POST", "/sessions", &[], &["display_name", "client_kind"]),
    t("collab_list_sessions", "GET", "/sessions", &["limit", "cursor"], &[]),
    t("collab_heartbeat", "POST", "/sessions/{session_id}/heartbeat", &[], &[]),
    t("collab_end_session", "POST", "/sessions/{session_id}/end", &[], &[]),
    t("collab_list_tasks", "GET", "/tasks", &["status", "limit", "cursor"], &[]),
    t("collab_create_task", "POST", "/tasks", &[], &["title", "description", "owner_session_id", "branch", "paths", "issue_number", "pr_number"]),
    t("collab_get_task", "GET", "/tasks/{task_id}", &[], &[]),
    t("collab_update_task", "PATCH", "/tasks/{task_id}", &[], &["expected_version", "title", "description", "status", "owner_session_id", "branch", "paths", "issue_number", "pr_number"]),
    t("collab_list_messages", "GET", "/tasks/{task_id}/messages", &["after_seq", "limit"], &[]),
    t("collab_send_message", "POST", "/tasks/{task_id}/messages", &[], &["client_message_id", "kind", "body", "reply_to_id", "target_session_id", "commit_sha"]),
    t("collab_mark_read", "PUT", "/tasks/{task_id}/read", &[], &["last_read_seq"]),
    t("collab_create_handoff", "POST", "/tasks/{task_id}/handoffs", &[], &["client_handoff_id", "to_session_id", "summary", "completed", "next_steps", "blockers", "commit_sha", "evidence"]),
    t("collab_list_handoffs", "GET", "/tasks/{task_id}/handoffs", &["limit", "cursor"], &[]),
    t("collab_accept_handoff", "POST", "/handoffs/{handoff_id}/accept", &[], &["expected_version"]),
    t("collab_cancel_handoff", "POST", "/handoffs/{handoff_id}/cancel", &[], &["expected_version"]),
    t("collab_get_rules", "GET", "/rules", &["ref", "path"], &[]),
    t("collab_get_inbox", "GET", "/inbox", &["since", "limit"], &[]),
    t("collab_claim_task", "POST", "/tasks/{task_id}/takeovers", &[], &["client_takeover_id", "expected_task_version", "expected_owner_session_id", "reason"]),
    t("collab_list_takeovers", "GET", "/tasks/{task_id}/takeovers", &["limit", "cursor"], &[]),
    t("collab_ask_question", "POST", "/tasks/{task_id}/questions", &[], &["client_question_id", "recipient", "title", "body", "options", "context"]),
    t("collab_list_questions", "GET", "/tasks/{task_id}/questions", &["status", "limit", "cursor"], &[]),
    t("collab_get_question", "GET", "/questions/{question_id}", &[], &[]),
    t("collab_answer_question", "POST", "/questions/{question_id}/answer", &[], &["client_answer_id", "expected_version", "body", "option_id", "acknowledged_context_hash"]),
    t("collab_cancel_question", "POST", "/questions/{question_id}/cancel", &[], &["expected_version"]),
    t("collab_create_work_package", "POST", "/tasks/{task_id}/work-packages", &[], &["client_package_id", "expected_task_version", "commit_sha", "goal", "scope", "artifacts", "completed", "open_items", "next_steps", "evidence_refs"]),
    t("collab_list_work_packages", "GET", "/tasks/{task_id}/work-packages", &["limit", "cursor"], &[]),
    t("collab_get_work_package", "GET", "/work-packages/{package_id}", &[], &[]),
    t("collab_get_completion_status", "GET", "/tasks/{task_id}/completion-status", &[], &[]),
];

/// The full REST path (relative to `/api/v1`) of a row, in router notation.
pub fn rest_path(route: &ToolRoute) -> String {
    format!("/repos/{{owner}}/{{name}}/collaboration{}", route.path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collab::{plan_for_contract_test, TOOL_NAMES};
    use serde_json::{json, Map, Value};
    use std::collections::BTreeSet;

    const UUID: &str = "11111111-1111-4111-8111-111111111111";

    fn sample_value(key: &str) -> Value {
        match key {
            k if k.ends_with("version") || k.ends_with("seq") || k.ends_with("number") => json!(1),
            "recipient" => json!({"kind": "repo_writers"}),
            _ => json!("x"),
        }
    }

    fn fill_template(path: &str) -> String {
        let mut out = String::new();
        let mut rest = path;
        while let Some(start) = rest.find('{') {
            out.push_str(&rest[..start]);
            let end = rest[start..].find('}').unwrap() + start;
            out.push_str(UUID);
            rest = &rest[end + 1..];
        }
        out.push_str(rest);
        out
    }

    #[test]
    fn table_covers_exactly_the_advertised_tools() {
        let table: Vec<_> = TOOL_ROUTES.iter().map(|r| r.tool).collect();
        let names: Vec<_> = TOOL_NAMES.to_vec();
        assert_eq!(table, names, "TOOL_ROUTES must list every TOOL_NAMES entry, in the same order");
    }

    #[test]
    fn table_matches_what_each_plan_really_sends() {
        for row in TOOL_ROUTES {
            // Every id-like path parameter and every query/body key gets a value.
            let mut args = Map::new();
            args.insert("owner".into(), json!("o"));
            args.insert("repo".into(), json!("r"));
            for key in ["task_id", "handoff_id", "question_id", "package_id"] {
                args.insert(key.into(), json!(UUID));
            }
            for key in row.query.iter().chain(row.body) {
                args.insert((*key).into(), sample_value(key));
            }
            let plan = plan_for_contract_test(row.tool, &Value::Object(args), Some(UUID))
                .unwrap_or_else(|_| panic!("{}: plan rejected the sample arguments", row.tool));

            assert_eq!(plan.method, row.method, "{}: HTTP method", row.tool);
            let (path, query) = plan.path.split_once('?').unwrap_or((plan.path.as_str(), ""));
            assert_eq!(path, fill_template(row.path), "{}: path", row.tool);

            let sent_query: BTreeSet<_> = query
                .split('&')
                .filter(|p| !p.is_empty())
                .map(|p| p.split('=').next().unwrap().to_string())
                .collect();
            let want_query: BTreeSet<_> = row.query.iter().map(|k| k.to_string()).collect();
            assert_eq!(sent_query, want_query, "{}: query parameters", row.tool);

            let sent_body: BTreeSet<_> = plan
                .body
                .as_ref()
                .and_then(Value::as_object)
                .map(|o| o.keys().cloned().collect())
                .unwrap_or_default();
            let want_body: BTreeSet<_> = row.body.iter().map(|k| k.to_string()).collect();
            assert_eq!(sent_body, want_body, "{}: body properties", row.tool);
        }
    }
}
