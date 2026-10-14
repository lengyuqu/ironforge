//! Branch protection service — protected branches + required status checks.

use anyhow::{bail, Context, Result};
use chrono::Utc;
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, Set};

use rg_db::entities::protected_branch::{self, Model as ProtectedBranch};
use rg_db::entities::pull_request;
use rg_db::ops::{pipeline_ops, pr_review_ops, protected_branch_ops, repo_ops};

/// Create a branch protection rule.
#[allow(clippy::too_many_arguments)]
pub async fn create_protection(
    db: &DatabaseConnection,
    owner: &str,
    repo_name: &str,
    branch_name: String,
    require_pr: bool,
    require_status_check: bool,
    required_status_checks: Option<Vec<String>>,
    require_approval: bool,
    required_approvals: Option<i64>,
    allow_force_push: bool,
    require_signed_commits: bool,
    allowed_push_user_ids: Option<Vec<i64>>,
) -> Result<ProtectedBranch> {
    let repo = resolve_repo(db, owner, repo_name).await?;

    // Check if protection already exists
    if protected_branch_ops::find_by_repo_and_branch(db, repo.id, &branch_name)
        .await?
        .is_some()
    {
        bail!("branch '{}' is already protected", branch_name);
    }

    let model = protected_branch::ActiveModel {
        id: sea_orm::NotSet,
        repo_id: Set(repo.id),
        branch_name: Set(branch_name),
        require_pr: Set(require_pr),
        require_status_check: Set(require_status_check),
        required_status_checks: Set(
            required_status_checks.map(|v| serde_json::to_string(&v).unwrap_or_default())
        ),
        require_approval: Set(require_approval),
        required_approvals: Set(required_approvals),
        allow_force_push: Set(allow_force_push),
        require_signed_commits: Set(require_signed_commits),
        allowed_push_user_ids: Set(
            allowed_push_user_ids.map(|v| serde_json::to_string(&v).unwrap_or_default())
        ),
        created_at: Set(Utc::now()),
        updated_at: Set(Utc::now()),
    };

    protected_branch_ops::create(db, model).await
}

/// List all branch protection rules for a repo.
pub async fn list_protections(
    db: &DatabaseConnection,
    owner: &str,
    repo_name: &str,
) -> Result<Vec<ProtectedBranch>> {
    let repo = resolve_repo(db, owner, repo_name).await?;
    protected_branch_ops::list_by_repo(db, repo.id).await
}

/// Get a branch protection rule by ID.
pub async fn get_protection(
    db: &DatabaseConnection,
    protection_id: i64,
) -> Result<ProtectedBranch> {
    protected_branch_ops::find_by_id(db, protection_id)
        .await?
        .context("protection rule not found")
}

/// Get a branch protection rule by ID, scoped to a repository route.
pub async fn get_protection_for_repo(
    db: &DatabaseConnection,
    owner: &str,
    repo_name: &str,
    protection_id: i64,
) -> Result<ProtectedBranch> {
    let repo = resolve_repo(db, owner, repo_name).await?;
    let protection = get_protection(db, protection_id).await?;
    if protection.repo_id != repo.id {
        bail!("protection rule not found");
    }
    Ok(protection)
}

/// Update a branch protection rule.
#[allow(clippy::too_many_arguments)]
pub async fn update_protection(
    db: &DatabaseConnection,
    protection_id: i64,
    require_pr: Option<bool>,
    require_status_check: Option<bool>,
    required_status_checks: Option<Vec<String>>,
    require_approval: Option<bool>,
    required_approvals: Option<i64>,
    allow_force_push: Option<bool>,
    require_signed_commits: Option<bool>,
    allowed_push_user_ids: Option<Vec<i64>>,
) -> Result<ProtectedBranch> {
    let mut protection = protected_branch_ops::find_by_id(db, protection_id)
        .await?
        .context("protection rule not found")?;

    if let Some(v) = require_pr {
        protection.require_pr = v;
    }
    if let Some(v) = require_status_check {
        protection.require_status_check = v;
    }
    if let Some(v) = required_status_checks {
        protection.required_status_checks = Some(serde_json::to_string(&v).unwrap_or_default());
    }
    if let Some(v) = require_approval {
        protection.require_approval = v;
    }
    if let Some(v) = required_approvals {
        protection.required_approvals = Some(v);
    }
    if let Some(v) = allow_force_push {
        protection.allow_force_push = v;
    }
    if let Some(v) = require_signed_commits {
        protection.require_signed_commits = v;
    }
    if let Some(v) = allowed_push_user_ids {
        protection.allowed_push_user_ids = Some(serde_json::to_string(&v).unwrap_or_default());
    }
    protection.updated_at = Utc::now();

    let active: protected_branch::ActiveModel = protection.into();
    protected_branch_ops::update(db, active).await
}

/// Update a branch protection rule, scoped to a repository route.
#[allow(clippy::too_many_arguments)]
pub async fn update_protection_for_repo(
    db: &DatabaseConnection,
    owner: &str,
    repo_name: &str,
    protection_id: i64,
    require_pr: Option<bool>,
    require_status_check: Option<bool>,
    required_status_checks: Option<Vec<String>>,
    require_approval: Option<bool>,
    required_approvals: Option<i64>,
    allow_force_push: Option<bool>,
    require_signed_commits: Option<bool>,
    allowed_push_user_ids: Option<Vec<i64>>,
) -> Result<ProtectedBranch> {
    get_protection_for_repo(db, owner, repo_name, protection_id).await?;
    update_protection(
        db,
        protection_id,
        require_pr,
        require_status_check,
        required_status_checks,
        require_approval,
        required_approvals,
        allow_force_push,
        require_signed_commits,
        allowed_push_user_ids,
    )
    .await
}

/// Delete a branch protection rule.
pub async fn delete_protection(db: &DatabaseConnection, protection_id: i64) -> Result<()> {
    protected_branch_ops::delete_by_id(db, protection_id).await
}

/// Delete a branch protection rule, scoped to a repository route.
pub async fn delete_protection_for_repo(
    db: &DatabaseConnection,
    owner: &str,
    repo_name: &str,
    protection_id: i64,
) -> Result<()> {
    get_protection_for_repo(db, owner, repo_name, protection_id).await?;
    delete_protection(db, protection_id).await
}

// ── Shared protection-rule semantics ─────────────────────────────────
//
// There is exactly one place that decides what a protection rule means. Both
// write paths consume it:
//
// - the git push path (receive-pack pre-receive hook, HTTP and SSH), which
//   turns rules into (ref-pattern, reason) rejections and a signature-required
//   pattern list before the pack is even indexed;
// - the direct-commit path (REST `contents` API, review suggestions), which
//   calls [`check_direct_commit_allowed`] before writing a commit.
//
// Keeping the decision here — rather than re-implementing it in each caller —
// is what keeps wildcard matching and allowlist semantics identical.

/// Match a full ref name against a protection pattern.
///
/// `*` matches any sequence of characters; patterns without a wildcard retain
/// exact-match behavior. This is the same matcher the receive-pack hook uses
/// for tag patterns, lifted here so branch rules share it.
pub fn ref_matches_pattern(refname: &str, pattern: &str) -> bool {
    if !pattern.contains('*') {
        return refname == pattern;
    }
    let value = refname.as_bytes();
    let pattern = pattern.as_bytes();
    let mut dp = vec![vec![false; value.len() + 1]; pattern.len() + 1];
    dp[0][0] = true;
    for i in 1..=pattern.len() {
        if pattern[i - 1] == b'*' {
            dp[i][0] = dp[i - 1][0];
        }
        for j in 1..=value.len() {
            dp[i][j] = if pattern[i - 1] == b'*' {
                dp[i - 1][j] || dp[i][j - 1]
            } else {
                dp[i - 1][j - 1] && pattern[i - 1] == value[j - 1]
            };
        }
    }
    dp[pattern.len()][value.len()]
}

/// The ref pattern a rule protects: `refs/heads/<branch_name>`.
///
/// `branch_name` may itself contain `*` (e.g. `release/*`), which the matcher
/// expands against the pushed ref.
pub fn protection_ref_pattern(rule: &ProtectedBranch) -> String {
    format!("refs/heads/{}", rule.branch_name)
}

/// Whether `refname` (a full ref) is covered by `rule`.
pub fn rule_matches_ref(rule: &ProtectedBranch, refname: &str) -> bool {
    ref_matches_pattern(refname, &protection_ref_pattern(rule))
}

/// Whether the actor is explicitly allowlisted to push to the rule's branch.
///
/// The allowlist only bypasses the *push* restrictions (`require_pr`,
/// force-push). It deliberately does **not** bypass `require_signed_commits`:
/// signature requirements apply to everyone, exactly as the receive-pack hook
/// has always enforced them.
pub fn is_push_allowlisted(rule: &ProtectedBranch, actor_id: Option<i64>) -> bool {
    actor_id.is_some_and(|uid| {
        rule.allowed_push_user_ids
            .as_deref()
            .and_then(|json| serde_json::from_str::<Vec<i64>>(json).ok())
            .is_some_and(|ids| ids.contains(&uid))
    })
}

/// Rejections for the receive-pack pre-receive hook.
///
/// Returns one `(ref_pattern, message)` entry per rule that rejects a direct
/// push by `actor_id`. `ref_pattern` may contain a wildcard, which the
/// protocol layer matches against each pushed ref.
pub fn direct_push_rejections(
    rules: &[ProtectedBranch],
    actor_id: Option<i64>,
) -> Vec<(String, String)> {
    rules
        .iter()
        .filter_map(|rule| {
            if is_push_allowlisted(rule, actor_id) {
                return None;
            }

            let message = if rule.require_pr {
                format!(
                    "push to protected branch '{}' is not allowed; open a pull request instead",
                    rule.branch_name
                )
            } else if !rule.allow_force_push {
                format!(
                    "force push to protected branch '{}' is not allowed",
                    rule.branch_name
                )
            } else {
                return None;
            };

            Some((protection_ref_pattern(rule), message))
        })
        .collect()
}

/// Ref patterns whose pushed commits must carry a valid cryptographic
/// signature. Applies to every actor, allowlisted or not.
pub fn signed_commit_required_patterns(rules: &[ProtectedBranch]) -> Vec<String> {
    rules
        .iter()
        .filter(|rule| rule.require_signed_commits)
        .map(protection_ref_pattern)
        .collect()
}

/// Decide whether a direct server-side commit may land on `branch_name`.
///
/// This is the shared verdict for writes that create a commit themselves
/// rather than receiving a signed pack: the REST `contents` API and applying
/// review suggestions. A direct commit is always fast-forward, so
/// `allow_force_push` does not apply; only `require_pr` and
/// `require_signed_commits` can block.
///
/// Returns `Some(reason)` when the commit must be refused.
pub fn direct_commit_rejection(
    rules: &[ProtectedBranch],
    branch_name: &str,
    actor_id: Option<i64>,
) -> Option<String> {
    let refname = format!("refs/heads/{branch_name}");
    let mut require_pr = false;
    let mut require_signed = false;

    for rule in rules {
        if !rule_matches_ref(rule, &refname) {
            continue;
        }
        if rule.require_pr && !is_push_allowlisted(rule, actor_id) {
            require_pr = true;
        }
        if rule.require_signed_commits {
            require_signed = true;
        }
    }

    if require_signed {
        return Some(format!(
            "branch '{branch_name}' requires cryptographically signed commits; server-side content edits are not signed"
        ));
    }
    if require_pr {
        return Some(format!(
            "direct commits to protected branch '{branch_name}' are not allowed; open a pull request instead"
        ));
    }
    None
}

/// Check if a push to a branch is allowed.
/// Returns Ok(()) if allowed, or Err with the reason if blocked.
pub async fn check_push_allowed(
    db: &DatabaseConnection,
    repo_id: i64,
    branch_name: &str,
    user_id: Option<i64>,
) -> Result<()> {
    let rules = protected_branch_ops::list_by_repo(db, repo_id).await?;
    let refname = format!("refs/heads/{branch_name}");

    let Some(rule) = rules.iter().find(|rule| rule_matches_ref(rule, &refname)) else {
        // Not protected, push is allowed
        return Ok(());
    };

    if is_push_allowlisted(rule, user_id) {
        return Ok(());
    }

    if rule.require_pr {
        bail!(
            "push to protected branch '{}' is not allowed; open a pull request instead",
            branch_name
        );
    }

    if !rule.allow_force_push {
        bail!(
            "force push to protected branch '{}' is not allowed",
            branch_name
        );
    }

    Ok(())
}

/// Check whether a direct server-side commit (the REST `contents` API or
/// applying a review suggestion) may land on `branch_name`.
///
/// The git push path enforces protection in the receive-pack pre-receive hook.
/// A server-side commit has no pack and no signature, so `require_pr` and
/// `require_signed_commits` must be enforced here instead. Both write paths
/// call this so the rules cannot drift apart.
pub async fn check_direct_commit_allowed(
    db: &DatabaseConnection,
    repo_id: i64,
    branch_name: &str,
    user_id: Option<i64>,
) -> Result<()> {
    let rules = protected_branch_ops::list_by_repo(db, repo_id).await?;

    if let Some(reason) = direct_commit_rejection(&rules, branch_name, user_id) {
        bail!("{reason}");
    }

    Ok(())
}

/// Check if a PR merge is allowed under branch protection rules.
pub async fn check_merge_allowed(
    db: &DatabaseConnection,
    repo_id: i64,
    target_branch: &str,
    pr_id: i64,
) -> Result<()> {
    // Use the same matcher as the push and direct-commit paths so a wildcard
    // rule (e.g. `release/*`) protects merges too.
    let rules = protected_branch_ops::list_by_repo(db, repo_id).await?;
    let refname = format!("refs/heads/{target_branch}");

    let Some(protection) = rules.iter().find(|rule| rule_matches_ref(rule, &refname)) else {
        return Ok(());
    };

    if protection.require_signed_commits {
        bail!(
            "branch '{}' requires cryptographically signed commits; server-side PR merge commits are not signed, so create and push a signed commit with an allowed identity",
            target_branch
        );
    }

    // Check required approvals
    if protection.require_approval {
        let required = protection.required_approvals.unwrap_or(1);
        let pr = pull_request::Entity::find_by_id(pr_id)
            .one(db)
            .await?
            .context("pull request not found")?;
        let approval_count =
            pr_review_ops::count_current_approvals(db, pr_id, pr.head_sha.as_deref()).await?;
        if approval_count < required {
            bail!(
                "merging into protected branch '{}' requires at least {} approval(s), got {}",
                target_branch,
                required,
                approval_count
            );
        }
    }

    // Check required status checks
    if protection.require_status_check {
        if let Some(checks_json) = &protection.required_status_checks {
            if let Ok(required_checks) = serde_json::from_str::<Vec<String>>(checks_json) {
                // Find the PR to get its head commit SHA
                let pr = pull_request::Entity::find()
                    .filter(pull_request::Column::Id.eq(pr_id))
                    .one(db)
                    .await
                    .context("db: find PR for status check")?;

                let head_sha = match pr.and_then(|p| p.head_sha) {
                    Some(s) if !s.is_empty() => s,
                    _ => {
                        bail!(
                            "branch '{}' requires status checks but no CI pipeline found for PR {}",
                            target_branch,
                            pr_id
                        );
                    }
                };

                // Find the latest pipeline for this commit
                let pipeline = pipeline_ops::find_latest_by_repo_and_commit(db, repo_id, &head_sha)
                    .await
                    .context("db: find pipeline for status check")?;

                match pipeline {
                    None => {
                        bail!(
                            "branch '{}' requires status checks to pass, but no CI pipeline has run for commit {}",
                            target_branch,
                            &head_sha[..8.min(head_sha.len())]
                        );
                    }
                    Some(p) if p.status != "success" => {
                        bail!(
                            "branch '{}' requires all status checks to pass, but pipeline #{} is {}",
                            target_branch,
                            p.id,
                            p.status
                        );
                    }
                    Some(p) => {
                        // All pipeline jobs must have passed — check job names match required list
                        let jobs = pipeline_ops::list_jobs_by_pipeline(db, p.id).await?;
                        let passed_jobs: std::collections::HashSet<_> = jobs
                            .iter()
                            .filter(|j| j.status == "success")
                            .map(|j| j.name.clone())
                            .collect();

                        let missing: Vec<_> = required_checks
                            .iter()
                            .filter(|name| !passed_jobs.contains(*name))
                            .collect();

                        if !missing.is_empty() {
                            bail!(
                                "branch '{}' requires status checks {:?} to pass, but {:?} are missing or failed",
                                target_branch,
                                required_checks,
                                missing
                            );
                        }

                        tracing::info!(
                            branch = %target_branch,
                            pipeline_id = %p.id,
                            checks = ?required_checks,
                            "Branch protection: all status checks passed"
                        );
                    }
                }
            }
        }
    }

    Ok(())
}


// ── Helpers ───────────────────────────────────────────────────────────

async fn resolve_repo(
    db: &DatabaseConnection,
    owner: &str,
    repo_name: &str,
) -> Result<rg_db::entities::repository::Model> {
    let user = rg_db::ops::user_ops::find_by_username(db, owner)
        .await?
        .context("owner not found")?;
    repo_ops::find_by_owner_and_name(db, user.id, repo_name)
        .await?
        .context("repository not found")
}

#[cfg(test)]
mod shared_rule_tests {
    use super::*;
    use chrono::Utc;

    fn rule(branch: &str) -> ProtectedBranch {
        ProtectedBranch {
            id: 1,
            repo_id: 7,
            branch_name: branch.to_string(),
            require_pr: false,
            require_status_check: false,
            required_status_checks: None,
            require_approval: false,
            required_approvals: None,
            allow_force_push: false,
            require_signed_commits: false,
            allowed_push_user_ids: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn wildcard_rule_covers_matching_branch() {
        let release = rule("release/*");
        assert!(rule_matches_ref(&release, "refs/heads/release/1.0"));
        assert!(rule_matches_ref(&release, "refs/heads/release/2026/07"));
        assert!(!rule_matches_ref(&release, "refs/heads/main"));
        assert!(!rule_matches_ref(&release, "refs/heads/release"));
    }

    #[test]
    fn exact_rule_still_requires_exact_match() {
        let main = rule("main");
        assert!(rule_matches_ref(&main, "refs/heads/main"));
        assert!(!rule_matches_ref(&main, "refs/heads/main-2"));
        assert!(!rule_matches_ref(&main, "refs/tags/main"));
    }

    #[test]
    fn allowlist_does_not_waive_signed_commit_requirement() {
        let mut signed = rule("main");
        signed.require_pr = true;
        signed.require_signed_commits = true;
        signed.allowed_push_user_ids = Some("[42]".to_string());
        let rules = vec![signed];

        // The allowlisted user still cannot make an unsigned server-side
        // commit — the receive-pack hook never waived signatures either.
        assert!(direct_commit_rejection(&rules, "main", Some(42)).is_some());
        assert!(direct_commit_rejection(&rules, "main", None).is_some());
    }

    #[test]
    fn allowlist_waives_require_pr_for_direct_commit() {
        let mut pr_only = rule("main");
        pr_only.require_pr = true;
        pr_only.allowed_push_user_ids = Some("[42]".to_string());
        let rules = vec![pr_only];

        assert!(direct_commit_rejection(&rules, "main", Some(42)).is_none());
        assert!(direct_commit_rejection(&rules, "main", Some(7)).is_some());
        // Anonymous callers are never allowlisted.
        assert!(direct_commit_rejection(&rules, "main", None).is_some());
    }

    #[test]
    fn wildcard_rule_blocks_direct_commit() {
        let mut release = rule("release/*");
        release.require_pr = true;
        let rules = vec![release];

        assert!(direct_commit_rejection(&rules, "release/1.0", None).is_some());
        assert!(direct_commit_rejection(&rules, "main", None).is_none());
    }

    #[test]
    fn direct_push_rejections_carry_wildcard_patterns() {
        let mut release = rule("release/*");
        release.require_pr = true;
        let rules = vec![release];

        let rejections = direct_push_rejections(&rules, None);
        assert_eq!(rejections.len(), 1);
        assert_eq!(rejections[0].0, "refs/heads/release/*");
    }

    #[test]
    fn signed_commit_patterns_ignore_allowlist() {
        let mut signed = rule("main");
        signed.require_signed_commits = true;
        signed.allowed_push_user_ids = Some("[42]".to_string());
        let rules = vec![signed];

        assert_eq!(
            signed_commit_required_patterns(&rules),
            vec!["refs/heads/main".to_string()]
        );
    }
}
