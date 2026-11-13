//! The merge gates: one verdict, two consumers.
//!
//! Whether a pull request may be merged is decided by a handful of gates — the
//! pull request is open and not a draft, the protection rule's signature,
//! approval and status-check requirements hold. They used to be spelled out
//! inline where the merge ran, with the reason baked into an error string, so
//! nothing else could ask "what would stop this merge?" without running it.
//!
//! This module is the single place those rules live. The merge path asks it for
//! the first refusal ([`ProtectionEvaluation::refusal`], in the order and with
//! the text the merge has always used); the merge-readiness preview asks it for
//! every gate with a stable machine code ([`Gate::code`]) and the facts behind
//! the verdict ([`Gate::observed`]). Because both read the same evaluation they
//! cannot drift apart.
//!
//! # Not met versus unreadable
//!
//! [`GateResult::Fail`] means the requirement is not met. [`GateResult::Unknown`]
//! means the gate could not be judged (a rule whose stored configuration does
//! not parse). The two are never merged: an agent that sees `Unknown` must not
//! read it as a pass or as a failure.
//!
//! Note that the merge path keeps treating the `Unknown` it knows about — a
//! `required_status_checks` value that is not a JSON list of names — as "no
//! requirement", exactly as before this module existed (the fail-open case the
//! doc comment on `check_merge_allowed` records). Making that fail closed is a
//! change to merge behaviour and is deliberately not done here; the preview
//! reports it honestly instead.

use sea_orm::{DatabaseConnection, EntityTrait};
use serde::Serialize;
use serde_json::{json, Value};

use rg_db::entities::protected_branch::Model as ProtectedBranch;
use rg_db::entities::pull_request::{self, Model as PullRequest};
use rg_db::ops::{pipeline_ops, pr_review_ops, protected_branch_ops};

use crate::branch_protection::service::rule_matches_ref;
use crate::error::{CoreContext, CoreError, CoreResult};

/// The outcome of one gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum GateResult {
    /// The requirement is met.
    Pass,
    /// The requirement is not met; the merge is refused.
    Fail,
    /// Worth knowing, does not block.
    Warn,
    /// The rule does not ask for this.
    NotApplicable,
    /// Could not be judged. Neither a pass nor a failure.
    Unknown,
}

/// One gate's verdict. `code` names the gate (it is the same whatever the
/// result); `detail` is the sentence the merge path would refuse with.
#[derive(Debug, Clone, Serialize)]
pub struct Gate {
    /// The family: `state`, `signed_commits`, `reviews`, `status_checks`.
    pub gate: &'static str,
    /// Stable machine code.
    pub code: &'static str,
    pub result: GateResult,
    pub detail: Option<String>,
    /// The facts the verdict rests on. Numbers and ids, no free text from users.
    pub observed: Value,
}

impl Gate {
    fn new(
        gate: &'static str,
        code: &'static str,
        result: GateResult,
        detail: Option<String>,
        observed: Value,
    ) -> Self {
        Self {
            gate,
            code,
            result,
            detail,
            observed,
        }
    }
}

// ── Pull-request state gates ──────────────────────────────────────────────

/// The pull request is a draft.
pub fn draft_gate(pr: &PullRequest) -> Gate {
    if pr.is_draft {
        Gate::new(
            "state",
            "PR_DRAFT",
            GateResult::Fail,
            Some("draft pull requests cannot be merged".into()),
            json!({ "is_draft": true }),
        )
    } else {
        Gate::new(
            "state",
            "PR_DRAFT",
            GateResult::Pass,
            None,
            json!({ "is_draft": false }),
        )
    }
}

/// The pull request is not open (merged, closed, or mid-merge).
pub fn state_gate(pr: &PullRequest) -> Gate {
    if pr.state == "open" {
        Gate::new(
            "state",
            "PR_STATE_NOT_OPEN",
            GateResult::Pass,
            None,
            json!({ "state": pr.state }),
        )
    } else {
        Gate::new(
            "state",
            "PR_STATE_NOT_OPEN",
            GateResult::Fail,
            Some(format!(
                "cannot merge a PR that is not in 'open' state (current: {})",
                pr.state
            )),
            json!({ "state": pr.state }),
        )
    }
}

impl Gate {
    /// The refusal a failed state gate stands for: a conflict with the pull
    /// request's current state, which is how the merge path has always put it.
    pub fn state_refusal(&self) -> Option<CoreError> {
        (self.result == GateResult::Fail)
            .then(|| CoreError::Conflict(self.detail.clone().unwrap_or_default()))
    }
}

// ── Branch-protection gates ───────────────────────────────────────────────

/// The protection rule that applies to the target branch.
#[derive(Debug, Clone, Serialize)]
pub struct RuleRef {
    pub id: i64,
    /// The rule's branch pattern as stored (`main`, `release/*`).
    pub pattern: String,
}

/// One required check and where it stands on the pull request's head commit.
#[derive(Debug, Clone, Serialize)]
pub struct RequiredCheck {
    pub name: String,
    /// `PASSED`, `PENDING`, `FAILED`, `SKIPPED`, or `MISSING` (no such job ran).
    pub result: &'static str,
    pub job_id: Option<i64>,
    /// The job's own status word, when a job ran.
    pub status: Option<String>,
}

/// What the rule asks of CI and what the head commit's pipeline has to show.
#[derive(Debug, Clone, Serialize)]
pub struct RequiredChecks {
    /// `rule.required_status_checks` when the rule names checks, otherwise `none`.
    pub source: &'static str,
    pub checks: Vec<RequiredCheck>,
    pub pipeline_id: Option<i64>,
    pub pipeline_status: Option<String>,
    /// The commit the pipeline was looked up for (the stored head commit).
    pub commit_sha: Option<String>,
}

impl RequiredChecks {
    fn none() -> Self {
        Self {
            source: "none",
            checks: Vec::new(),
            pipeline_id: None,
            pipeline_status: None,
            commit_sha: None,
        }
    }
}

/// Every protection gate for one pull request, plus the refusal the merge path
/// would give.
#[derive(Debug, Clone, Serialize)]
pub struct ProtectionEvaluation {
    /// `None`: no rule covers the target branch, so there is nothing to gate.
    pub rule: Option<RuleRef>,
    pub gates: Vec<Gate>,
    pub required_checks: RequiredChecks,
    /// The first failing gate's refusal text, in the order and wording the merge
    /// has always used. `None` when the merge is not blocked.
    #[serde(skip)]
    pub refusal: Option<String>,
}

impl ProtectionEvaluation {
    fn empty() -> Self {
        Self {
            rule: None,
            gates: Vec::new(),
            required_checks: RequiredChecks::none(),
            refusal: None,
        }
    }

    fn fail(&mut self, message: &str) {
        if self.refusal.is_none() {
            self.refusal = Some(message.to_string());
        }
    }
}

fn short(sha: &str) -> &str {
    &sha[..8.min(sha.len())]
}

fn job_result(status: &str) -> &'static str {
    match status {
        "success" => "PASSED",
        "pending" | "running" => "PENDING",
        "skipped" => "SKIPPED",
        _ => "FAILED",
    }
}

/// Evaluate the protection gates for merging `pr_id` into `target_branch`.
///
/// With `stop_at_first_failure` the evaluation returns at the first failing gate
/// and reads nothing past it — what the merge path needs, and exactly the reads
/// it made before. Without it every gate is evaluated, which is what the
/// readiness preview shows.
///
/// A database error is returned as an error in both modes.
pub async fn evaluate_protection(
    db: &DatabaseConnection,
    repo_id: i64,
    target_branch: &str,
    pr_id: i64,
    stop_at_first_failure: bool,
) -> CoreResult<ProtectionEvaluation> {
    // Use the same matcher as the push and direct-commit paths so a wildcard
    // rule (e.g. `release/*`) protects merges too. Only the first matching rule
    // decides, as it always has.
    let rules = protected_branch_ops::list_by_repo(db, repo_id).await?;
    let refname = format!("refs/heads/{target_branch}");
    let Some(protection) = rules.iter().find(|rule| rule_matches_ref(rule, &refname)) else {
        return Ok(ProtectionEvaluation::empty());
    };

    let mut eval = ProtectionEvaluation::empty();
    eval.rule = Some(RuleRef {
        id: protection.id,
        pattern: protection.branch_name.clone(),
    });

    // ── signed commits ─────────────────────────────────────────────────
    if protection.require_signed_commits {
        let message = format!(
            "branch '{}' requires cryptographically signed commits; server-side PR merge commits are not signed, so create and push a signed commit with an allowed identity",
            target_branch
        );
        eval.gates.push(Gate::new(
            "signed_commits",
            "SIGNED_COMMITS_REQUIRED",
            GateResult::Fail,
            Some(message.clone()),
            json!({ "rule": protection.branch_name }),
        ));
        eval.fail(&message);
        if stop_at_first_failure {
            return Ok(eval);
        }
    } else {
        eval.gates.push(Gate::new(
            "signed_commits",
            "SIGNED_COMMITS_REQUIRED",
            GateResult::NotApplicable,
            None,
            json!({ "rule": protection.branch_name }),
        ));
    }

    // The pull request row is read at most once, and only when a gate needs it.
    let mut loaded: Option<Option<PullRequest>> = None;

    // ── approvals ──────────────────────────────────────────────────────
    if protection.require_approval {
        let required = protection.required_approvals.unwrap_or(1);
        let pr = load_pr(db, pr_id, &mut loaded)
            .await?
            .as_ref()
            .ok_or_else(|| CoreError::not_found("pull request not found"))?;
        let approvers =
            pr_review_ops::current_human_approver_ids(db, pr_id, pr.head_sha.as_deref()).await?;
        let current = approvers.len() as i64;
        let observed = json!({
            "required": required,
            "current": current,
            "approved_by": approvers,
            "for_commit": pr.head_sha,
        });
        if current < required {
            let message = format!(
                "merging into protected branch '{}' requires at least {} approval(s), got {}",
                target_branch, required, current
            );
            eval.gates.push(Gate::new(
                "reviews",
                "REQUIRED_APPROVALS_MISSING",
                GateResult::Fail,
                Some(message.clone()),
                observed,
            ));
            eval.fail(&message);
            if stop_at_first_failure {
                return Ok(eval);
            }
        } else {
            eval.gates.push(Gate::new(
                "reviews",
                "REQUIRED_APPROVALS_MISSING",
                GateResult::Pass,
                None,
                observed,
            ));
        }
    } else {
        eval.gates.push(Gate::new(
            "reviews",
            "REQUIRED_APPROVALS_MISSING",
            GateResult::NotApplicable,
            None,
            json!({}),
        ));
    }

    // ── status checks ──────────────────────────────────────────────────
    evaluate_status_checks(
        db,
        repo_id,
        target_branch,
        pr_id,
        protection,
        &mut loaded,
        stop_at_first_failure,
        &mut eval,
    )
    .await?;
    Ok(eval)
}

async fn load_pr<'a>(
    db: &DatabaseConnection,
    pr_id: i64,
    loaded: &'a mut Option<Option<PullRequest>>,
) -> CoreResult<&'a Option<PullRequest>> {
    if loaded.is_none() {
        let found = pull_request::Entity::find_by_id(pr_id)
            .one(db)
            .await
            .context("db: find PR for merge gates")?;
        *loaded = Some(found);
    }
    Ok(loaded.as_ref().expect("just loaded"))
}

#[allow(clippy::too_many_arguments)]
async fn evaluate_status_checks(
    db: &DatabaseConnection,
    repo_id: i64,
    target_branch: &str,
    pr_id: i64,
    protection: &ProtectedBranch,
    loaded: &mut Option<Option<PullRequest>>,
    stop_at_first_failure: bool,
    eval: &mut ProtectionEvaluation,
) -> CoreResult<()> {
    if !protection.require_status_check {
        eval.gates.push(Gate::new(
            "status_checks",
            "REQUIRED_CHECKS_UNDEFINED",
            GateResult::NotApplicable,
            None,
            json!({}),
        ));
        return Ok(());
    }

    let Some(checks_json) = &protection.required_status_checks else {
        // `require_status_check` is on but the rule names no checks: nothing is
        // required. Said out loud rather than left to look like a pass.
        eval.gates.push(Gate::new(
            "status_checks",
            "REQUIRED_CHECKS_UNDEFINED",
            GateResult::Warn,
            Some(
                "status checks are required but the rule lists none, so no check is required"
                    .into(),
            ),
            json!({}),
        ));
        return Ok(());
    };
    let required_checks = match serde_json::from_str::<Vec<String>>(checks_json) {
        Ok(list) => list,
        Err(_) => {
            // The merge path has always let this through (see the module docs);
            // the preview must not call it a pass.
            eval.gates.push(Gate::new(
                "status_checks",
                "PROTECTION_RULE_MALFORMED",
                GateResult::Unknown,
                Some(
                    "the rule's required_status_checks is not a JSON list of check names; the merge treats it as no requirement"
                        .into(),
                ),
                json!({ "rule": protection.branch_name }),
            ));
            return Ok(());
        }
    };
    eval.required_checks.source = "rule.required_status_checks";

    let head_sha = match load_pr(db, pr_id, loaded).await? {
        Some(pr) => pr.head_sha.clone().filter(|sha| !sha.is_empty()),
        None => None,
    };
    let Some(head_sha) = head_sha else {
        let message = format!(
            "branch '{}' requires status checks but no CI pipeline found for PR {}",
            target_branch, pr_id
        );
        eval.gates.push(Gate::new(
            "status_checks",
            "HEAD_COMMIT_UNKNOWN",
            GateResult::Fail,
            Some(message.clone()),
            json!({ "pull_request_id": pr_id }),
        ));
        eval.fail(&message);
        return Ok(());
    };
    eval.required_checks.commit_sha = Some(head_sha.clone());

    let pipeline = pipeline_ops::find_latest_by_repo_and_commit(db, repo_id, &head_sha).await?;
    let Some(pipeline) = pipeline else {
        let message = format!(
            "branch '{}' requires status checks to pass, but no CI pipeline has run for commit {}",
            target_branch,
            short(&head_sha)
        );
        eval.gates.push(Gate::new(
            "status_checks",
            "PIPELINE_NOT_FOUND",
            GateResult::Fail,
            Some(message.clone()),
            json!({ "commit_sha": head_sha }),
        ));
        eval.fail(&message);
        // Without a pipeline every required check is missing.
        eval.required_checks.checks = required_checks
            .iter()
            .map(|name| RequiredCheck {
                name: name.clone(),
                result: "MISSING",
                job_id: None,
                status: None,
            })
            .collect();
        return Ok(());
    };
    eval.required_checks.pipeline_id = Some(pipeline.id);
    eval.required_checks.pipeline_status = Some(pipeline.status.clone());

    let pipeline_ok = pipeline.status == "success";
    if !pipeline_ok {
        let message = format!(
            "branch '{}' requires all status checks to pass, but pipeline #{} is {}",
            target_branch, pipeline.id, pipeline.status
        );
        eval.gates.push(Gate::new(
            "status_checks",
            "PIPELINE_NOT_SUCCESS",
            GateResult::Fail,
            Some(message.clone()),
            json!({ "pipeline_id": pipeline.id, "status": pipeline.status }),
        ));
        eval.fail(&message);
        if stop_at_first_failure {
            return Ok(());
        }
    } else {
        eval.gates.push(Gate::new(
            "status_checks",
            "PIPELINE_NOT_SUCCESS",
            GateResult::Pass,
            None,
            json!({ "pipeline_id": pipeline.id, "status": pipeline.status }),
        ));
    }

    // All pipeline jobs must have passed — check job names match the required list.
    let jobs = pipeline_ops::list_jobs_by_pipeline(db, pipeline.id).await?;
    let mut missing: Vec<&String> = Vec::new();
    for name in &required_checks {
        let same_name: Vec<_> = jobs.iter().filter(|job| &job.name == name).collect();
        let passed = same_name.iter().find(|job| job.status == "success");
        let (result, job, status) = match (passed, same_name.first()) {
            (Some(job), _) => ("PASSED", Some(job.id), Some(job.status.clone())),
            (None, Some(job)) => (
                job_result(&job.status),
                Some(job.id),
                Some(job.status.clone()),
            ),
            (None, None) => ("MISSING", None, None),
        };
        if result != "PASSED" {
            missing.push(name);
        }
        eval.required_checks.checks.push(RequiredCheck {
            name: name.clone(),
            result,
            job_id: job,
            status,
        });
    }

    if !missing.is_empty() {
        // Only the merge path's own wording matters for the refusal, and only when
        // the pipeline itself is green; a red pipeline has already refused above
        // (`fail` keeps the first refusal).
        eval.fail(&format!(
            "branch '{}' requires status checks {:?} to pass, but {:?} are missing or failed",
            target_branch, required_checks, missing
        ));
        for check in eval
            .required_checks
            .checks
            .iter()
            .filter(|c| c.result != "PASSED")
        {
            let code = match check.result {
                "MISSING" => "REQUIRED_CHECK_MISSING",
                "PENDING" => "REQUIRED_CHECK_PENDING",
                _ => "REQUIRED_CHECK_FAILED",
            };
            eval.gates.push(Gate::new(
                "status_checks",
                code,
                GateResult::Fail,
                Some(match &check.status {
                    Some(status) => format!(
                        "job '{}' is {} for {}",
                        check.name,
                        status,
                        short(&head_sha)
                    ),
                    None => format!("job '{}' has not run for {}", check.name, short(&head_sha)),
                }),
                json!({ "check": check.name, "status": check.status, "job_id": check.job_id }),
            ));
        }
    } else if pipeline_ok {
        tracing::info!(
            branch = %target_branch,
            pipeline_id = %pipeline.id,
            checks = ?required_checks,
            "Branch protection: all status checks passed"
        );
        eval.gates.push(Gate::new(
            "status_checks",
            "REQUIRED_CHECKS_SATISFIED",
            GateResult::Pass,
            None,
            json!({ "required": required_checks.len() }),
        ));
    }
    Ok(())
}
