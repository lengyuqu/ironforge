//! Who may approve a request, and how many votes it needs.
//!
//! This is the one place that answers the question. Each action's rule is copied
//! from what the corresponding handler does today; the tests below pin the
//! equivalences (and the one deliberate change: PR approvals now need write
//! access or an explicit reviewer seat, see `docs/plans/T06-plan.md` §5).
//!
//! Two shapes of approver live side by side:
//!
//! - `explicit`: a stored list of user ids (a protected environment's
//!   `allowed_approver_ids`, a branch's `allowed_push_user_ids`, a PR's requested
//!   reviewers). These are the `approvers` set.
//! - `admins` / `writers` / `collaborators`: predicates, evaluated at decision
//!   time by calling the *same* function the handler calls today. They are not
//!   expanded into ids on purpose: the membership tables change, an enumerated
//!   roster would be stale by the time anyone votes, and enumerating org writers
//!   means replaying the team-mapping joins. Evaluating the predicate at vote
//!   time is exactly what the current handlers do.
//!
//! `collaborators` is never set by an action in step 1 (see
//! `docs/plans/T06-plan.md` §4.2): folding collaboration members into PR
//! approvals would re-open the very hole this task closes, because a
//! collaboration member may hold only `read`.

use rg_db::entities::repository::Model as Repository;
use rg_db::ops::{
    ci_environment_ops, pipeline_ops, pr_reviewer_request_ops, protected_branch_ops,
    pull_request_ops,
};
use sea_orm::DatabaseConnection;

use super::action::Action;
use super::payload::authorization_set_id;
use crate::collaboration::access::is_collab_member;
use crate::collaboration::validation;
use crate::error::{CoreError, CoreResult};
use crate::repo::service::{can_admin_repo, can_write_repo};

/// The evaluated rule for one `(repo, action, subject)`.
#[derive(Debug, Clone, PartialEq)]
pub struct AuthorizationSet {
    /// Digest of this rule; see `payload::authorization_set_id`.
    pub id: i64,
    /// Named approvers, ascending.
    pub explicit: Vec<i64>,
    /// Any repository administrator may approve.
    pub admins: bool,
    /// Any repository writer may approve.
    pub writers: bool,
    /// Any collaboration member may approve (never set in step 1).
    pub collaborators: bool,
    pub required_approvals: u32,
}

impl AuthorizationSet {
    /// The single admission test. `explicit` wins first, then each predicate in
    /// the order the handlers use (admin before write before collaboration), so
    /// a caller cannot get a different answer from the evaluator than the
    /// handler would give.
    pub async fn may_approve(
        &self,
        db: &DatabaseConnection,
        repo: &Repository,
        user_id: i64,
    ) -> CoreResult<bool> {
        if self.explicit.contains(&user_id) {
            return Ok(true);
        }
        if self.admins && can_admin_repo(db, repo, Some(user_id)).await? {
            return Ok(true);
        }
        if self.writers && can_write_repo(db, repo, Some(user_id)).await? {
            return Ok(true);
        }
        if self.collaborators {
            // `is_collab_member` clears the 30s permission cache on purpose; that
            // behaviour is inherited here rather than re-implemented.
            return is_collab_member(db, repo, user_id)
                .await
                .map_err(CoreError::internal);
        }
        Ok(false)
    }
}

/// Build the rule for one request.
///
/// `subject` is the domain of the decision: a PR id, a job id, a full ref, a
/// release version. It is validated here (length, control characters) and
/// resolved per action, so a request can never name an object of another
/// repository.
pub async fn evaluate(
    db: &DatabaseConnection,
    repo: &Repository,
    action: Action,
    subject: &str,
) -> CoreResult<AuthorizationSet> {
    let subject = validation::label("subject", subject, 255)
        .map_err(|error| CoreError::invalid_input(error.to_string()))?;

    match action {
        Action::CiEnvironmentRelease => {
            let job_id = parse_id(&subject, "job id")?;
            let job = pipeline_ops::get_job(db, job_id)
                .await
                .map_err(CoreError::internal)?
                .ok_or_else(|| CoreError::not_found("pipeline job not found"))?;
            let environment_id = job
                .environment_id
                .ok_or_else(|| CoreError::invalid_input("job has no protected environment"))?;
            let environment = ci_environment_ops::find_by_id(db, environment_id)
                .await
                .map_err(CoreError::internal)?
                .filter(|environment| environment.repo_id == repo.id && environment.protected)
                .ok_or_else(|| CoreError::not_found("protected environment not found"))?;
            Ok(build(
                repo,
                action,
                &subject,
                RuleParts {
                    explicit: environment_approvers(environment.allowed_approver_ids.as_deref()),
                    admins: true,
                    writers: false,
                    collaborators: false,
                    required_approvals: required_or_one(environment.required_approvals as i64),
                },
            ))
        }

        Action::ProtectedBranchMerge => {
            let pr_id = parse_id(&subject, "pull request id")?;
            let pr = pull_request_ops::find_by_id(db, pr_id)
                .await
                .map_err(CoreError::internal)?
                .filter(|pr| pr.repo_id == repo.id)
                .ok_or_else(|| CoreError::not_found("pull request not found"))?;
            let required = merge_required_approvals(db, repo.id, &pr.base_branch).await?;
            // The reviewers the repository asked for are the explicit approvers.
            // Requesting one requires PR-manager rights, and CODEOWNERS
            // assignments land in the same table, so this is not a self-service
            // route to a vote.
            let requested: Vec<i64> = pr_reviewer_request_ops::list_by_pr(db, pr.id)
                .await
                .map_err(CoreError::internal)?
                .into_iter()
                .map(|request| request.reviewer_id)
                .collect();
            Ok(build(
                repo,
                action,
                &subject,
                RuleParts {
                    explicit: requested,
                    admins: false,
                    writers: true,
                    collaborators: false,
                    required_approvals: required,
                },
            ))
        }

        Action::ProtectedBranchPush => {
            if !subject.starts_with("refs/") {
                return Err(CoreError::invalid_input(
                    "subject of a push authorization must be a full ref",
                ));
            }
            let rules = protected_branch_ops::list_by_repo(db, repo.id)
                .await
                .map_err(CoreError::internal)?;
            let rule = rules
                .iter()
                .find(|rule| crate::branch_protection::service::rule_matches_ref(rule, &subject))
                .ok_or_else(|| {
                    CoreError::invalid_input("no protected branch rule covers this ref")
                })?;
            // `allowed_push_user_ids` is the only bypass the push path has; it
            // deliberately does not grant merge approvals (see
            // `entities/protected_branch.rs`), and this rule keeps it that way.
            Ok(build(
                repo,
                action,
                &subject,
                RuleParts {
                    explicit: environment_approvers(rule.allowed_push_user_ids.as_deref()),
                    admins: false,
                    writers: false,
                    collaborators: false,
                    required_approvals: 1,
                },
            ))
        }

        // Publishing has no approver list today: repository administrators are
        // the closest existing rule, and there is no call point in step 1.
        Action::ReleasePublish => Ok(build(
            repo,
            action,
            &subject,
            RuleParts {
                explicit: Vec::new(),
                admins: true,
                writers: false,
                collaborators: false,
                required_approvals: 1,
            },
        )),

        // Widening an agent token is expanding write power over a whole
        // repository, so only repository administrators may approve it.
        // `writers: false` / `collaborators: false` keep a writer or a
        // collaboration member from approving an expansion on its own.
        //
        // The subject is `"<token id>:<target scopes>"` (e.g. `10:repo:write`),
        // so the target the administrator approves is inside the hash that
        // binds the row (`payload::payload_hash`). Parsing here also proves the
        // token id and the scopes before anything is stored.
        //
        // The token id alone is not enough: `load_expandable_token` also proves
        // the token is an active agent token confined to *this* repository, so
        // an administrator of one repository cannot widen a token that reaches
        // another.
        Action::ScopeExpand => {
            let (token_id, _) = super::scope_expand::parse_subject(&subject)?;
            super::scope_expand::load_expandable_token(db, repo, token_id).await?;
            Ok(build(
                repo,
                action,
                &subject,
                RuleParts {
                    explicit: Vec::new(),
                    admins: true,
                    writers: false,
                    collaborators: false,
                    required_approvals: 1,
                },
            ))
        }
    }
}

/// The variable half of a rule, so `build` stays a plain constructor.
struct RuleParts {
    explicit: Vec<i64>,
    admins: bool,
    writers: bool,
    collaborators: bool,
    required_approvals: u32,
}

fn build(repo: &Repository, action: Action, subject: &str, parts: RuleParts) -> AuthorizationSet {
    let RuleParts {
        mut explicit,
        admins,
        writers,
        collaborators,
        required_approvals,
    } = parts;
    explicit.sort_unstable();
    explicit.dedup();
    let id = authorization_set_id(
        repo.id,
        action,
        subject,
        &explicit,
        admins,
        writers,
        collaborators,
        required_approvals,
    );
    AuthorizationSet {
        id,
        explicit,
        admins,
        writers,
        collaborators,
        required_approvals,
    }
}

/// Threshold of the protected branch rule that covers `branch`. The merge gate
/// uses `required_approvals.unwrap_or(1)`; so does this, including for the
/// "no rule matches" case (a merge that reached a request has a rule, but
/// defaulting keeps the two paths from disagreeing on the number).
async fn merge_required_approvals(
    db: &DatabaseConnection,
    repo_id: i64,
    branch: &str,
) -> CoreResult<u32> {
    let refname = format!("refs/heads/{branch}");
    let rules = protected_branch_ops::list_by_repo(db, repo_id)
        .await
        .map_err(CoreError::internal)?;
    let required = rules
        .iter()
        .find(|rule| crate::branch_protection::service::rule_matches_ref(rule, &refname))
        .and_then(|rule| rule.required_approvals)
        .unwrap_or(1);
    Ok(required_or_one(required))
}

/// A threshold of zero would mean "a ticket is already approved without a
/// vote"; the primitive always requires at least one person to say yes.
fn required_or_one(required: i64) -> u32 {
    required.max(1) as u32
}

/// Parse a JSON array of user ids the way the CI handlers do: an unparsable or
/// missing list means "nobody is named", never "everybody".
fn environment_approvers(json: Option<&str>) -> Vec<i64> {
    json.and_then(|value| serde_json::from_str::<Vec<i64>>(value).ok())
        .unwrap_or_default()
}

fn parse_id(subject: &str, what: &str) -> CoreResult<i64> {
    subject
        .parse::<i64>()
        .ok()
        .filter(|id| *id > 0)
        .ok_or_else(|| CoreError::invalid_input(format!("subject must be a positive {what}")))
}

/// Whether `user_id` is a person. An agent can file a request and never vote on
/// one; the check lives here so every caller shares it.
///
/// The "who is the actor" half comes from [`crate::agent::is_human_account`],
/// the same predicate the HTTP approval gates use; the refusal below stays this
/// module's own so its code and wording do not move.
pub async fn ensure_human_decider(db: &DatabaseConnection, user_id: i64) -> CoreResult<()> {
    if crate::agent::is_human_account(db, user_id).await? {
        return Ok(());
    }
    Err(CoreError::coded(
        403,
        "AGENT_MAY_NOT_APPROVE",
        "an agent may request an authorization but never approve one",
    ))
}
