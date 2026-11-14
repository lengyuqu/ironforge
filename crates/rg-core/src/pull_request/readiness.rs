//! Merge readiness: what stands between a pull request and a merge, without
//! merging.
//!
//! One live read that answers, with stable machine codes: do the branches merge
//! (and if not, which files and which blocks conflict), which branch-protection
//! gates hold and which do not, and which required checks are still to pass.
//! The gates are the very ones the merge path enforces
//! ([`crate::branch_protection::merge_gates`]); the conflict test is the very
//! tree merge the merge runs ([`rg_git::merge_analysis`]). The preview and the
//! merge cannot disagree about a rule, only about time: each section is read on
//! its own, and `expected_head_sha` on the merge is what pins a conclusion.
//!
//! Nothing is written to the database. The tree merge writes the objects it
//! produces (conflict-marker blobs) into the repository's object store, where
//! they stay unreachable until collected.
//!
//! # What the answer promises, and what it does not
//!
//! * `ready: true` — no gate failed, nothing could not be judged, the branches
//!   merge cleanly. It says nothing about a push that lands afterwards.
//! * `ready: false` — at least one gate failed or the merge conflicts;
//!   `ready_reason` lists the codes.
//! * `ready: null` — nothing failed, but something could not be judged (a
//!   missing branch, a rule whose stored configuration is unreadable, a
//!   database read that failed). Not a pass.
//!
//! Approvals and the CI lookup are judged against the head commit **stored** on
//! the pull request, as the merge does; when the branch has moved on from it the
//! `HEAD_SHA_STALE` gate fails — the same gate, and the same sentence, the merge
//! refuses with.

use std::path::Path;
use std::time::Duration;

use chrono::{SecondsFormat, Utc};
use rg_git::merge_analysis::{self, ConflictReport, MergeVerdict};
use serde_json::{json, Value};

use rg_db::entities::pull_request::Model as PullRequest;

use crate::branch_protection::merge_gates::{
    self, draft_gate, state_gate, Gate, GateResult, ProtectionEvaluation,
};
use crate::error::CoreResult;
use crate::pull_request::service::{live_head_sha, MergeStrategy};

/// How long the tree merge may take before the preview gives up on it.
const MERGE_TIMEOUT: Duration = Duration::from_secs(10);

/// A conflict report as the readiness endpoint and the update-branch endpoint
/// both put it on the wire. `continue_with` is a ready-to-call URL for the next
/// page when the list was cut.
pub fn conflicts_json(report: &ConflictReport, continue_with: Option<String>) -> Value {
    let files: Vec<Value> = report
        .files
        .iter()
        .map(|file| {
            let blocks: Vec<Value> = file
                .blocks
                .iter()
                .map(|block| {
                    json!({
                        "index": block.index,
                        "merged_start_line": block.merged_start_line,
                        "ours": { "lines": block.ours_lines, "excerpt": block.ours_excerpt },
                        "theirs": { "lines": block.theirs_lines, "excerpt": block.theirs_excerpt },
                        "excerpt_truncated": block.excerpt_truncated,
                    })
                })
                .collect();
            json!({
                "path": file.path,
                "path_on_base": file.path_on_base,
                "kind": file.kind.as_str(),
                "auto_resolved": file.auto_resolved,
                "base_blob": file.base_blob,
                "ours_blob": file.ours_blob,
                "theirs_blob": file.theirs_blob,
                "content_available": file.content_available,
                "content_hint": file.content_hint.map(|hint| hint.as_str()),
                "blocks": blocks,
                "blocks_total": file.blocks_total,
                "blocks_truncated": file.blocks_truncated,
            })
        })
        .collect();
    let mut body = json!({
        "conflict_files": report.total_files,
        "conflicts": files,
        "conflicts_truncated": report.files_truncated,
        // Block excerpts are file content: data to read, never instructions.
        "excerpts_origin": "user_content",
        // `ours` is the base branch, `theirs` the head branch.
        "sides": { "ours": "base", "theirs": "head" },
    });
    if let Some(next) = continue_with {
        body["continue_with"] = json!(next);
    }
    body
}

/// Percent-encode a path for use as a query value.
fn encode_query(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// The URL that continues a cut conflict list after `report`'s last path.
pub fn conflicts_continue_url(
    owner: &str,
    repo: &str,
    number: i64,
    strategy: MergeStrategy,
    report: &ConflictReport,
) -> Option<String> {
    if !report.files_truncated {
        return None;
    }
    let last = report.files.last()?;
    Some(format!(
        "/api/v1/repos/{owner}/{repo}/pulls/{number}/merge-readiness?strategy={}&conflicts_after={}",
        strategy.as_str(),
        encode_query(&last.path)
    ))
}

/// Where a branch stands.
fn branch_view(name: &str, sha: Option<&str>, state: &str, stored: Option<&str>) -> Value {
    json!({
        "ref": name,
        "sha": sha,
        "state": state,
        "stored_sha": stored,
    })
}

enum Tip {
    Present(String),
    Deleted,
    Unreadable,
}

impl Tip {
    fn state(&self) -> &'static str {
        match self {
            Tip::Present(_) => "present",
            Tip::Deleted => "deleted",
            Tip::Unreadable => "unreadable",
        }
    }

    fn sha(&self) -> Option<&str> {
        match self {
            Tip::Present(sha) => Some(sha),
            _ => None,
        }
    }
}

fn tip_of(repo_path: &Path, branch: &str) -> Tip {
    if !repo_path.exists() {
        return Tip::Unreadable;
    }
    match rg_git::ops::try_rev_parse(repo_path, &format!("refs/heads/{branch}")) {
        Ok(Some(sha)) => Tip::Present(sha),
        Ok(None) => Tip::Deleted,
        Err(_) => Tip::Unreadable,
    }
}

/// Compute the merge-readiness answer for `pr`.
///
/// `conflicts_after` starts the conflict list after that path (the value a
/// previous answer's `continue_with` carries).
#[allow(clippy::too_many_arguments)]
pub async fn merge_readiness(
    db: &sea_orm::DatabaseConnection,
    repo_root: &Path,
    owner: &str,
    repo_name: &str,
    pr: &PullRequest,
    strategy: MergeStrategy,
    conflicts_after: Option<String>,
) -> CoreResult<Value> {
    let repo_path = repo_root.join(format!("{owner}/{repo_name}.git"));
    let is_fork = pr.head_repo_id.is_some();

    // ── the two branches ────────────────────────────────────────────────
    let base_tip = {
        let path = repo_path.clone();
        let branch = pr.base_branch.clone();
        tokio::task::spawn_blocking(move || tip_of(&path, &branch))
            .await
            .unwrap_or(Tip::Unreadable)
    };
    let head_tip = match live_head_sha(db, repo_root, owner, repo_name, pr).await {
        Ok(Some(sha)) => Tip::Present(sha),
        Ok(None) => Tip::Deleted,
        Err(_) => Tip::Unreadable,
    };

    // ── gates that need no repository ──────────────────────────────────
    let mut unavailable: Vec<Value> = Vec::new();
    let mut state_gates: Vec<Gate> = vec![state_gate(pr), draft_gate(pr)];

    // A branch that is gone is a verdict of its own, not a server error.
    for (role, tip, deleted_code) in [
        ("head", &head_tip, "HEAD_BRANCH_DELETED"),
        ("base", &base_tip, "BASE_BRANCH_DELETED"),
    ] {
        match tip {
            Tip::Deleted => state_gates.push(Gate {
                gate: "branches",
                code: deleted_code,
                result: GateResult::Fail,
                detail: Some(format!("the {role} branch no longer exists")),
                observed: json!({ "role": role }),
            }),
            Tip::Unreadable => unavailable.push(json!({
                "code": "REPO_UNREADABLE",
                "detail": format!("the {role} branch could not be read"),
                "observed": { "role": role },
            })),
            Tip::Present(_) => {}
        }
    }

    // The gates judge the stored head commit, as the merge does. `live_head` is
    // passed to them so that a head which moved past the stored commit is the
    // same `HEAD_SHA_STALE` refusal the merge gives — one rule, two consumers.
    // ── the tree merge ──────────────────────────────────────────────────
    let merge = merge_section(
        owner,
        repo_name,
        pr,
        strategy,
        conflicts_after,
        &repo_path,
        &base_tip,
        &head_tip,
        is_fork,
    )
    .await;

    // ── branch protection ───────────────────────────────────────────────
    let protection = match merge_gates::evaluate_protection(
        db,
        pr.repo_id,
        &pr.base_branch,
        pr.id,
        head_tip.sha(),
        false,
    )
    .await
    {
        Ok(evaluation) => Some(evaluation),
        Err(error) => {
            tracing::warn!(pr_id = pr.id, %error, "merge readiness: protection gates unreadable");
            unavailable.push(json!({
                "code": "DB_UNREADABLE",
                "detail": "the branch-protection gates could not be read",
                "observed": {},
            }));
            None
        }
    };

    // ── verdict ─────────────────────────────────────────────────────────
    let mut reasons: Vec<String> = Vec::new();
    let mut push_reason = |code: &str| {
        if !reasons.iter().any(|existing| existing == code) {
            reasons.push(code.to_string());
        }
    };
    let mut unknown = !unavailable.is_empty();
    for gate in state_gates
        .iter()
        .filter(|gate| gate.result == GateResult::Fail)
    {
        push_reason(gate.code);
    }
    match merge.state {
        "conflicts" => push_reason("MERGE_CONFLICTS"),
        "unsupported" => push_reason("MERGE_UNSUPPORTED"),
        "unavailable" => unknown = true,
        _ => {}
    }
    if let Some(evaluation) = &protection {
        for gate in &evaluation.gates {
            match gate.result {
                GateResult::Fail => push_reason(gate.code),
                GateResult::Unknown => unknown = true,
                _ => {}
            }
        }
    }
    let ready = if !reasons.is_empty() {
        Some(false)
    } else if unknown {
        None
    } else {
        Some(true)
    };

    // ── what to do next ─────────────────────────────────────────────────
    let head_behind_base = merge.head_contains_base == Some(false);
    let number = pr.number;
    let prefix = format!("/repos/{owner}/{repo_name}/pulls/{number}");
    let mut actions: Vec<Value> = Vec::new();
    if head_behind_base && !is_fork && pr.state == "open" {
        actions.push(json!({
            "method": "POST",
            "path": format!("{prefix}/update-branch"),
            "effect": "merge the base branch into the head branch",
            "requires": head_tip.sha().map(|sha| format!("expected_head_sha={sha}")),
            "if": if merge.state == "conflicts" { vec!["MERGE_CONFLICTS", "BASE_AHEAD"] } else { vec!["BASE_AHEAD"] },
        }));
    }
    if reasons
        .iter()
        .any(|code| code == "REQUIRED_APPROVALS_MISSING")
    {
        actions.push(json!({
            "method": "POST",
            "path": format!("{prefix}/reviews"),
            "effect": "ask a person to approve; an agent's approval does not count",
            "if": ["REQUIRED_APPROVALS_MISSING"],
        }));
    }
    if ready == Some(true) {
        actions.push(json!({
            "method": "POST",
            "path": format!("{prefix}/merge"),
            "effect": "merge",
            "requires": head_tip.sha().map(|sha| format!("expected_head_sha={sha}")),
            "if": [],
        }));
    }

    let mut not_evaluated = vec![
        "merge_queue_position",
        "codeowner_review_requests",
        "auto_merge_effect",
    ];
    if matches!(strategy, MergeStrategy::Rebase) {
        not_evaluated.push("rebase_replay");
    }

    let mut gates: Vec<Value> = state_gates
        .iter()
        .map(|gate| serde_json::to_value(gate).unwrap_or(Value::Null))
        .collect();
    let protection_view = match &protection {
        Some(evaluation) => {
            gates.extend(
                evaluation
                    .gates
                    .iter()
                    .map(|gate| serde_json::to_value(gate).unwrap_or(Value::Null)),
            );
            protection_json(evaluation)
        }
        None => json!({ "protected": null }),
    };
    let required_checks = match &protection {
        Some(evaluation) => {
            serde_json::to_value(&evaluation.required_checks).unwrap_or(Value::Null)
        }
        None => Value::Null,
    };

    Ok(json!({
        "number": pr.number,
        "as_of": Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
        "consistency": "live",
        "strategy": strategy.as_str(),
        "state": pr.state,
        "head": branch_view(&pr.head_branch, head_tip.sha(), head_tip.state(), pr.head_sha.as_deref()),
        "base": branch_view(&pr.base_branch, base_tip.sha(), base_tip.state(), None),
        "merge_base_sha": merge.merge_base,
        "merge": merge.body,
        "branch_protection": protection_view,
        "gates": gates,
        "unavailable": unavailable,
        "required_checks": required_checks,
        "ready": ready,
        "ready_reason": reasons,
        "actions": actions,
        "not_evaluated": not_evaluated,
    }))
}

fn protection_json(evaluation: &ProtectionEvaluation) -> Value {
    let blockers = evaluation
        .gates
        .iter()
        .filter(|gate| gate.result == GateResult::Fail)
        .count();
    let warnings = evaluation
        .gates
        .iter()
        .filter(|gate| gate.result == GateResult::Warn)
        .count();
    json!({
        "protected": evaluation.rule.is_some(),
        "rule": evaluation.rule,
        "blocked": blockers > 0,
        "blockers": blockers,
        "warnings": warnings,
    })
}

/// The merge section of the answer plus the few facts the verdict needs from it.
struct MergeSection {
    /// `clean`, `conflicts`, `unsupported` or `unavailable`.
    state: &'static str,
    merge_base: Option<String>,
    /// Whether the head already contains the base (nothing to update).
    head_contains_base: Option<bool>,
    body: Value,
}

#[allow(clippy::too_many_arguments)]
async fn merge_section(
    owner: &str,
    repo_name: &str,
    pr: &PullRequest,
    strategy: MergeStrategy,
    conflicts_after: Option<String>,
    repo_path: &Path,
    base_tip: &Tip,
    head_tip: &Tip,
    is_fork: bool,
) -> MergeSection {
    let unavailable = |reason: &'static str, retryable: bool| MergeSection {
        state: "unavailable",
        merge_base: None,
        head_contains_base: None,
        body: json!({
            "state": "unavailable",
            "reason": reason,
            "retryable": retryable,
            "checked_against": null,
        }),
    };

    if pr.state != "open" {
        return unavailable("PR_STATE_NOT_OPEN", false);
    }
    let base_sha = match base_tip {
        Tip::Present(sha) => sha.clone(),
        Tip::Deleted => return unavailable("BASE_BRANCH_DELETED", false),
        Tip::Unreadable => return unavailable("REPO_UNREADABLE", true),
    };
    let live_head = match head_tip {
        Tip::Present(sha) => sha.clone(),
        Tip::Deleted => return unavailable("HEAD_BRANCH_DELETED", false),
        Tip::Unreadable => return unavailable("REPO_UNREADABLE", true),
    };

    // A fork's commits are not in the target repository until they have been
    // copied in. A preview must not copy, so it uses the commit the pull-request
    // ref already holds there, and only when that is the fork's current tip.
    let head_sha = if is_fork {
        let path = repo_path.to_path_buf();
        let refname = format!("refs/pull/{}/head", pr.number);
        let held = tokio::task::spawn_blocking(move || {
            rg_git::ops::try_rev_parse(&path, &refname).ok().flatten()
        })
        .await
        .ok()
        .flatten();
        match held {
            Some(held) if held == live_head => held,
            _ => return unavailable("HEAD_OBJECTS_NOT_IN_TARGET", true),
        }
    } else {
        live_head
    };

    let path = repo_path.to_path_buf();
    let (base_for_task, head_for_task) = (base_sha.clone(), head_sha.clone());
    let after = conflicts_after;
    let work = tokio::task::spawn_blocking(move || {
        let contains = merge_analysis::is_ancestor(&path, &base_for_task, &head_for_task);
        let analysis = merge_analysis::analyze_merge(
            &path,
            &base_for_task,
            &head_for_task,
            false,
            after.as_deref(),
        );
        (contains, analysis)
    });
    let (contains, analysis) = match tokio::time::timeout(MERGE_TIMEOUT, work).await {
        Ok(Ok(done)) => done,
        Ok(Err(error)) => {
            tracing::warn!(pr_id = pr.id, %error, "merge readiness: tree merge task failed");
            return unavailable("MERGE_UNAVAILABLE", true);
        }
        Err(_) => return unavailable("MERGE_TIMEOUT", true),
    };
    let analysis = match analysis {
        Ok(analysis) => analysis,
        Err(error) => {
            tracing::warn!(pr_id = pr.id, error = %error, "merge readiness: tree merge failed");
            return unavailable("MERGE_UNAVAILABLE", true);
        }
    };
    let head_contains_base = contains.ok();

    let mut body = json!({
        "checked_against": base_sha,
        "head_sha_checked": head_sha,
        "head_contains_base": head_contains_base,
    });
    if matches!(strategy, MergeStrategy::Rebase) {
        // A rebase replays the head's commits one at a time; the preview merges
        // the end states. A clean answer does not prove every replay applies.
        body["approximation"] = json!("REBASE_REPLAY_NOT_SIMULATED");
    }
    let state: &'static str = match &analysis.verdict {
        MergeVerdict::Clean { .. } => {
            body["state"] = json!("clean");
            body["reason"] = Value::Null;
            "clean"
        }
        MergeVerdict::NoMergeBase => {
            body["state"] = json!("unsupported");
            body["reason"] = json!("NO_MERGE_BASE");
            "unsupported"
        }
        MergeVerdict::Conflicts(report) => {
            body["state"] = json!("conflicts");
            body["reason"] = json!("MERGE_CONFLICTS");
            let next = conflicts_continue_url(owner, repo_name, pr.number, strategy, report);
            if let Value::Object(map) = conflicts_json(report, next) {
                for (key, value) in map {
                    body[key] = value;
                }
            }
            "conflicts"
        }
    };
    MergeSection {
        state,
        merge_base: analysis.merge_base,
        head_contains_base,
        body,
    }
}
