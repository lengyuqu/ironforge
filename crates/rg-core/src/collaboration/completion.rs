//! Completion status: what a task *claims* versus what the platform can show.
//!
//! This is a read-only evaluation. It never changes the task, never blocks
//! `done`, and is no approval: an agent's `done` stays a declaration, and each
//! condition below says whether platform records support it, contradict it,
//! are outdated for the current branch head, or are missing.
//!
//! Conditions (derived from existing records, none user-defined yet):
//! * `questions_resolved` — no question of the task is still open;
//! * `work_package_present` — at least one work package was written (it proves
//!   a package exists, not that its claims are true);
//! * `ci_passed_for_commit` — the *latest* CI pipeline recorded for exactly the
//!   commit of the newest work package succeeded. A pipeline for any other
//!   commit says nothing about this one, and Commit statuses (postable by any
//!   writer, including the agent being judged) are never evidence.

use chrono::{DateTime, Utc};
use rg_db::ops::{collab_work_package_ops, pipeline_ops};
use sea_orm::DatabaseConnection;
use serde::Serialize;

use super::ci_evidence::PipelineEvidence;
use super::error::CollabResult;
use super::task;
use rg_db::ops::collab_question_ops::status as qstatus;

pub const SATISFIED: &str = "satisfied";
pub const UNSATISFIED: &str = "unsatisfied";
pub const STALE: &str = "stale";
pub const UNKNOWN: &str = "unknown";

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ConditionStatus {
    pub id: &'static str,
    /// `satisfied` | `unsatisfied` | `stale` | `unknown`.
    pub status: &'static str,
    pub detail: String,
    pub evidence: Vec<PipelineEvidence>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CompletionStatus {
    pub task_id: String,
    pub task_version: i64,
    /// The task's own status field: a claim by whoever set it.
    pub declared_status: String,
    pub conditions: Vec<ConditionStatus>,
    /// `all_satisfied` | `unsatisfied` | `incomplete` (some condition is stale
    /// or unknown, none unsatisfied).
    pub overall: &'static str,
    /// The task says `done` while the evidence does not support it.
    pub declared_done_without_evidence: bool,
    /// Head of the task's declared branch as observed now (`None` = no branch
    /// declared or unreadable).
    pub observed_branch_head: Option<String>,
    pub evaluated_at: DateTime<Utc>,
}

fn condition(id: &'static str, status: &'static str, detail: impl Into<String>) -> ConditionStatus {
    ConditionStatus {
        id,
        status,
        detail: detail.into(),
        evidence: Vec::new(),
    }
}

fn ci_condition(
    commit: Option<&str>,
    pipelines: &[rg_db::entities::pipeline::Model],
    branch_head: Option<&str>,
    now: DateTime<Utc>,
) -> ConditionStatus {
    let Some(commit) = commit else {
        return condition(
            "ci_passed_for_commit",
            UNKNOWN,
            "no work package names a commit, so there is nothing to look a pipeline up for",
        );
    };
    let evidence: Vec<PipelineEvidence> = pipelines
        .iter()
        .map(|p| PipelineEvidence {
            verification: "verified_reference",
            pipeline_id: p.id,
            status: Some(p.status.clone()),
            commit_sha: Some(p.commit_sha.clone()),
            ref_name: Some(p.ref_name.clone()),
            finished_at: p.finished_at.map(|t| t.and_utc()),
            resolved_at: now,
        })
        .collect();
    let (status, detail) = match pipelines.first() {
        None => (
            UNKNOWN,
            format!("no CI pipeline is recorded for commit {commit}"),
        ),
        Some(latest) => match latest.status.as_str() {
            "success" => (
                SATISFIED,
                format!("the latest pipeline ({}) for commit {commit} succeeded", latest.id),
            ),
            "failed" => (
                UNSATISFIED,
                format!("the latest pipeline ({}) for commit {commit} failed", latest.id),
            ),
            "canceled" => (
                UNSATISFIED,
                format!("the latest pipeline ({}) for commit {commit} was canceled", latest.id),
            ),
            other => (
                UNKNOWN,
                format!("the latest pipeline ({}) for commit {commit} is {other}", latest.id),
            ),
        },
    };
    // Evidence for an older commit cannot vouch for what the branch holds now.
    let (status, detail) = match (status, branch_head) {
        (SATISFIED, Some(head)) if head != commit => (
            STALE,
            format!("{detail}, but the task's branch has since moved to {head}"),
        ),
        _ => (status, detail),
    };
    ConditionStatus {
        id: "ci_passed_for_commit",
        status,
        detail,
        evidence,
    }
}

fn overall_of(conditions: &[ConditionStatus]) -> &'static str {
    if conditions.iter().any(|c| c.status == UNSATISFIED) {
        "unsatisfied"
    } else if conditions.iter().all(|c| c.status == SATISFIED) {
        "all_satisfied"
    } else {
        "incomplete"
    }
}

pub async fn evaluate(
    db: &DatabaseConnection,
    repo_id: i64,
    task_id: &str,
    branch_head: Option<String>,
    now: DateTime<Utc>,
) -> CollabResult<CompletionStatus> {
    let task = task::find(db, repo_id, task_id).await?;

    let questions = {
        let mut open = 0u64;
        let mut total = 0u64;
        let mut after = None;
        loop {
            let rows = rg_db::ops::collab_question_ops::list_page(db, repo_id, &task.id, None, after, 200).await?;
            let Some(last) = rows.last() else { break };
            after = Some((last.created_at, last.id.clone()));
            total += rows.len() as u64;
            open += rows.iter().filter(|q| q.status == qstatus::OPEN).count() as u64;
            if rows.len() < 200 {
                break;
            }
        }
        if open > 0 {
            condition("questions_resolved", UNSATISFIED, format!("{open} of {total} questions are still open"))
        } else {
            condition("questions_resolved", SATISFIED, format!("no open questions ({total} asked)"))
        }
    };

    let latest = collab_work_package_ops::list_page(db, repo_id, &task.id, None, 1)
        .await?
        .into_iter()
        .next();
    let package = match &latest {
        None => condition("work_package_present", UNSATISFIED, "no work package has been written"),
        Some(p) => condition(
            "work_package_present",
            SATISFIED,
            format!("revision {} exists (its contents are the author's claims)", p.revision),
        ),
    };

    let commit = latest.as_ref().and_then(|p| p.commit_sha.clone());
    let pipelines = match &commit {
        Some(sha) => pipeline_ops::list_by_repo_and_commit(db, repo_id, sha, 20).await?,
        None => Vec::new(),
    };
    let ci = ci_condition(commit.as_deref(), &pipelines, branch_head.as_deref(), now);

    let conditions = vec![questions, package, ci];
    let overall = overall_of(&conditions);
    Ok(CompletionStatus {
        task_id: task.id,
        task_version: task.version,
        declared_done_without_evidence: task.status == "done" && overall != "all_satisfied",
        declared_status: task.status,
        conditions,
        overall,
        observed_branch_head: branch_head,
        evaluated_at: now,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rg_db::entities::pipeline;

    fn pipe(id: i64, sha: &str, status: &str) -> pipeline::Model {
        pipeline::Model {
            id,
            repo_id: 1,
            commit_sha: sha.into(),
            ref_name: "main".into(),
            status: status.into(),
            trigger_type: "push".into(),
            triggered_by: None,
            started_at: None,
            finished_at: None,
            created_at: Utc::now().naive_utc(),
        }
    }

    #[test]
    fn only_the_latest_pipeline_of_the_exact_commit_decides() {
        let now = Utc::now();
        let sha = "a".repeat(40);
        // Newest first: a rerun succeeded after a failure.
        let rerun = ci_condition(Some(&sha), &[pipe(9, &sha, "success"), pipe(8, &sha, "failed")], None, now);
        assert_eq!(rerun.status, SATISFIED);
        // A success followed by a failure is unsatisfied.
        let regressed = ci_condition(Some(&sha), &[pipe(9, &sha, "failed"), pipe(8, &sha, "success")], None, now);
        assert_eq!(regressed.status, UNSATISFIED);
        assert_eq!(ci_condition(Some(&sha), &[pipe(9, &sha, "canceled")], None, now).status, UNSATISFIED);
        for running in ["pending", "running"] {
            assert_eq!(ci_condition(Some(&sha), &[pipe(9, &sha, running)], None, now).status, UNKNOWN);
        }
        assert_eq!(ci_condition(Some(&sha), &[], None, now).status, UNKNOWN);
        assert_eq!(ci_condition(None, &[], None, now).status, UNKNOWN);
    }

    #[test]
    fn success_for_an_older_commit_goes_stale_when_the_branch_moved() {
        let now = Utc::now();
        let sha = "a".repeat(40);
        let head = "b".repeat(40);
        let cond = ci_condition(Some(&sha), &[pipe(1, &sha, "success")], Some(&head), now);
        assert_eq!(cond.status, STALE);
        assert!(cond.detail.contains(&head));
        let same = ci_condition(Some(&sha), &[pipe(1, &sha, "success")], Some(&sha), now);
        assert_eq!(same.status, SATISFIED);
        // A failure stays a failure even when the branch moved.
        assert_eq!(ci_condition(Some(&sha), &[pipe(1, &sha, "failed")], Some(&head), now).status, UNSATISFIED);
    }

    #[test]
    fn overall_prefers_unsatisfied_then_incomplete() {
        let c = |s| condition("x", s, "");
        assert_eq!(overall_of(&[c(SATISFIED), c(SATISFIED)]), "all_satisfied");
        assert_eq!(overall_of(&[c(SATISFIED), c(UNKNOWN)]), "incomplete");
        assert_eq!(overall_of(&[c(STALE), c(SATISFIED)]), "incomplete");
        assert_eq!(overall_of(&[c(UNKNOWN), c(UNSATISFIED)]), "unsatisfied");
    }
}
