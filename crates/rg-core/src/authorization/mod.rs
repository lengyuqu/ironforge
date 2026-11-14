//! Generic approval primitive (#19, step 1).
//!
//! A **request** is one ask to perform a privileged action, bound to
//! `(action, subject, head_sha)`. People **decide** on it; when enough have said
//! yes, executing the action **consumes** it in the same transaction that
//! performs the action. That last clause is the whole point: a ticket that is
//! consumed earlier (or in its own commit) is an approval that stays valid
//! forever, which is exactly the failure mode this primitive exists to prevent.
//!
//! Layering:
//!
//! - `rg_db::ops::authorization_ops` — guards and SQL, no policy.
//! - `action` — the platform's action names, TTLs, retry class.
//! - `payload` — what a request is bound to, and the rule-version digest.
//! - `evaluator` — who may approve, how many votes.
//! - this module — the transitions: request, approve, deny, consume, revoke,
//!   expire.
//!
//! Step 1 has no HTTP endpoint (routes, `openapi.rs` and `route_meta.rs` are
//! shared files owned elsewhere). The tests at the bottom drive every transition
//! end to end, so the state machine is exercised rather than merely declared.

pub mod action;
pub mod evaluator;
pub mod payload;

use chrono::{DateTime, Utc};
use rg_db::entities::authorization_request::{
    Model as Request, KIND_AGENT_SESSION, KIND_HUMAN, REASON_DENIED,
    REASON_EXECUTION_FAILED_NOT_RETRYABLE, STATE_PENDING,
};
use rg_db::entities::repository::Model as Repository;
use rg_db::ops::{authorization_ops, repo_ops};
use sea_orm::{ConnectionTrait, DatabaseConnection};

use crate::collaboration::message::normalize_commit_sha;
use crate::collaboration::validation;
use crate::error::{CoreError, CoreResult};

pub use action::Action;
pub use evaluator::{evaluate, AuthorizationSet};

/// The ticket was already redeemed, expired, revoked or denied.
pub const ALREADY_CONSUMED: &str = "AUTHORIZATION_ALREADY_CONSUMED";
/// The request is still `pending` (or was denied): it authorises nothing.
pub const AUTHORIZATION_NOT_PENDING: &str = "AUTHORIZATION_NOT_PENDING";
/// Not enough approval votes have been cast.
pub const NOT_APPROVED: &str = "AUTHORIZATION_NOT_APPROVED";
/// The request row no longer matches the payload that was approved.
pub const PAYLOAD_CHANGED: &str = "AUTHORIZATION_PAYLOAD_CHANGED";
/// The request's deadline passed.
pub const EXPIRED: &str = "AUTHORIZATION_EXPIRED";
/// The applicant tried to approve their own request.
pub const SELF_APPROVAL: &str = "AUTHORIZATION_SELF_APPROVAL";
/// This person already voted on this request.
pub const ALREADY_DECIDED: &str = "AUTHORIZATION_ALREADY_DECIDED";
/// The same `client_request_id` was reused with a different payload.
pub const IDEMPOTENCY_KEY_REUSED: &str = "IDEMPOTENCY_KEY_REUSED";

/// Who is asking. An agent may file, never approve — regardless of its scopes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequesterKind {
    Human,
    AgentSession,
}

impl RequesterKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Human => KIND_HUMAN,
            Self::AgentSession => KIND_AGENT_SESSION,
        }
    }
}

/// Input of [`request`]. The repository model is required because the evaluator
/// needs it, and the caller has already resolved it (that resolution is also
/// where read access was checked).
pub struct NewRequest<'a> {
    pub repo: &'a Repository,
    pub action: Action,
    pub subject: &'a str,
    pub head_sha: Option<&'a str>,
    pub requested_by_kind: RequesterKind,
    pub requested_by_id: i64,
    pub client_request_id: &'a str,
}

/// Whether the row was created now or is the replay of an earlier identical ask.
#[derive(Debug)]
pub enum Requested {
    Created(Request),
    Replayed(Request),
}

impl Requested {
    pub fn into_request(self) -> Request {
        match self {
            Self::Created(request) | Self::Replayed(request) => request,
        }
    }
}

/// File a request. Idempotent on `(repo, principal, client_request_id)`; the
/// same key with a different payload is a conflict, not a second request.
///
/// Only humans are checked for repository read access. An agent session was
/// admitted to the repository when it was opened, and its numeric id lives in a
/// different table, so the membership check would be meaningless here; step 2
/// will wire the agent's own admission check when the request endpoint appears.
pub async fn request(db: &DatabaseConnection, new: NewRequest<'_>) -> CoreResult<Requested> {
    let subject = validation::label("subject", new.subject, 255)
        .map_err(|error| CoreError::invalid_input(error.to_string()))?;
    validation::client_key("client_request_id", new.client_request_id)
        .map_err(|error| CoreError::invalid_input(error.to_string()))?;
    let head_sha = match new.head_sha {
        Some(sha) => Some(
            normalize_commit_sha(sha)
                .map_err(|error| CoreError::invalid_input(error.to_string()))?,
        ),
        None => None,
    };

    if new.requested_by_kind == RequesterKind::Human
        && !crate::repo::service::can_read_repo(db, new.repo, Some(new.requested_by_id)).await?
    {
        return Err(CoreError::forbidden("access denied"));
    }

    let set = evaluate(db, new.repo, new.action, &subject).await?;
    let hash = payload::payload_hash(new.repo.id, new.action, &subject, head_sha.as_deref());
    let now = Utc::now();
    let outcome = authorization_ops::create(
        db,
        authorization_ops::NewRequest {
            repo_id: new.repo.id,
            action: new.action.as_str().to_string(),
            subject: subject.clone(),
            head_sha,
            requested_by_kind: new.requested_by_kind.as_str().to_string(),
            requested_by_id: new.requested_by_id,
            authorization_set_id: set.id,
            required_approvals: set.required_approvals as i32,
            client_request_id: new.client_request_id.to_string(),
            payload_hash: hash.clone(),
            expires_at: now + new.action.ttl(),
        },
        now,
    )
    .await
    .map_err(CoreError::internal)?;

    match outcome {
        authorization_ops::RequestOutcome::Created(request) => Ok(Requested::Created(request)),
        authorization_ops::RequestOutcome::Existing(request) if request.payload_hash == hash => {
            Ok(Requested::Replayed(request))
        }
        authorization_ops::RequestOutcome::Existing(_) => Err(CoreError::coded(
            409,
            IDEMPOTENCY_KEY_REUSED,
            "client_request_id was used before for a different payload",
        )),
    }
}

/// Progress after a vote, so a caller can say "1 of 2" without a second query.
#[derive(Debug, Clone)]
pub struct Progress {
    pub request: Request,
    pub approvals: u64,
    pub required_approvals: u32,
    /// Enough votes have been cast; the next step is execution, not approval.
    pub ready: bool,
}

/// One person approves a request.
///
/// Refusals, in order: the request must exist and be `pending`; the decider must
/// be a person; the decider must not be the applicant; the decider must be in
/// the authorization set; the row must still hash to what was approved; and a
/// second vote by the same person is a conflict.
pub async fn approve(
    db: &DatabaseConnection,
    request_id: i64,
    decider_id: i64,
) -> CoreResult<Progress> {
    decide(db, request_id, decider_id, "approve").await
}

/// One person refuses a request. A denial is terminal: the applicant files a new
/// request after addressing the objection.
pub async fn deny(
    db: &DatabaseConnection,
    request_id: i64,
    decider_id: i64,
) -> CoreResult<Progress> {
    decide(db, request_id, decider_id, "deny").await
}

async fn decide(
    db: &DatabaseConnection,
    request_id: i64,
    decider_id: i64,
    decision: &str,
) -> CoreResult<Progress> {
    let request = authorization_ops::find_by_id(db, request_id)
        .await
        .map_err(CoreError::internal)?
        .ok_or_else(|| CoreError::not_found("authorization request not found"))?;
    if request.state != STATE_PENDING {
        return Err(not_pending(&request));
    }
    if authorization_ops::expire_if_overdue(db, request_id, Utc::now())
        .await
        .map_err(CoreError::internal)?
    {
        return Err(CoreError::coded(
            409,
            EXPIRED,
            "this authorization request has expired",
        ));
    }

    evaluator::ensure_human_decider(db, decider_id).await?;
    ensure_separate_people(&request, decider_id)?;

    let repo = repo_ops::find_by_id(db, request.repo_id)
        .await
        .map_err(CoreError::internal)?
        .ok_or_else(|| CoreError::not_found("repository not found"))?;
    let action = Action::parse(&request.action)?;
    // Membership is judged by the rule as it is *now*: write access taken away
    // after the request was filed must not still approve it. The threshold, on
    // the other hand, is the one frozen on the row (`required_approvals`) — a
    // request filed under "one approval" does not silently need two because the
    // branch rule changed in the meantime.
    let set = evaluate(db, &repo, action, &request.subject).await?;
    if !set.may_approve(db, &repo, decider_id).await? {
        return Err(CoreError::forbidden(
            "this account may not decide this request",
        ));
    }
    require_payload_unchanged(&request, action)?;

    let recorded = authorization_ops::add_decision(
        db,
        request.id,
        decider_id,
        decision,
        &request.payload_hash,
        Utc::now(),
    )
    .await
    .map_err(CoreError::internal)?;
    if !recorded {
        return Err(CoreError::coded(
            409,
            ALREADY_DECIDED,
            "this account has already decided this request",
        ));
    }

    if decision == "deny" {
        authorization_ops::deny(db, request.id, REASON_DENIED, Utc::now())
            .await
            .map_err(CoreError::internal)?;
    }

    let request = authorization_ops::find_by_id(db, request_id)
        .await
        .map_err(CoreError::internal)?
        .ok_or_else(|| CoreError::not_found("authorization request not found"))?;
    let approvals = authorization_ops::count_approvals(db, request_id)
        .await
        .map_err(CoreError::internal)?;
    let required = required_of(&request);
    Ok(Progress {
        ready: request.state == STATE_PENDING && approvals >= u64::from(required),
        request,
        approvals,
        required_approvals: required,
    })
}

/// The single self-approval rule. The applicant is a person only when the
/// request came from one; an agent's session id lives in another table and can
/// coincide numerically with a user id, so it is not compared.
pub fn ensure_separate_people(request: &Request, decider_id: i64) -> CoreResult<()> {
    if request.requested_by_kind == KIND_HUMAN && request.requested_by_id == decider_id {
        return Err(CoreError::coded(
            403,
            SELF_APPROVAL,
            "the applicant cannot decide their own request",
        ));
    }
    Ok(())
}

/// Guarded consumption: `pending` -> `consumed`, in the caller's transaction.
///
/// Call it inside the transaction that performs the action:
///
/// ```rust,ignore
/// let txn = db.begin().await?;
/// authorization::consume(&txn, request_id, actor_id, &effect_hash).await?;
/// perform_the_action(&txn).await?;
/// txn.commit().await?;   // on failure: drop/rollback, and the ticket is pending again
/// ```
///
/// Committing the consumption on its own would authorise the action once and
/// leave the ticket spent even if the action never ran — or, worse, leave a
/// valid ticket behind after a partially applied action.
pub async fn consume<C: ConnectionTrait>(
    conn: &C,
    request_id: i64,
    actor_id: i64,
    effect_hash: &str,
) -> CoreResult<()> {
    let request = authorization_ops::find_by_id(conn, request_id)
        .await
        .map_err(CoreError::internal)?
        .ok_or_else(|| CoreError::not_found("authorization request not found"))?;
    if request.state != STATE_PENDING {
        return Err(not_pending(&request));
    }
    let now = Utc::now();
    if authorization_ops::expire_if_overdue(conn, request_id, now)
        .await
        .map_err(CoreError::internal)?
    {
        return Err(CoreError::coded(
            409,
            EXPIRED,
            "this authorization request has expired",
        ));
    }
    let approvals = authorization_ops::count_approvals(conn, request_id)
        .await
        .map_err(CoreError::internal)?;
    if approvals < u64::from(required_of(&request)) {
        return Err(CoreError::coded(
            409,
            NOT_APPROVED,
            "this authorization request does not have enough approvals",
        ));
    }
    let consumed = authorization_ops::consume_guarded(conn, request_id, actor_id, effect_hash, now)
        .await
        .map_err(CoreError::internal)?;
    if !consumed {
        // Lost a race with a concurrent consumer between the read and the update.
        return Err(CoreError::coded(
            409,
            ALREADY_CONSUMED,
            "this authorization request was already consumed",
        ));
    }
    Ok(())
}

/// Invalidate a request after an action that cannot be retried failed, so it can
/// never be redeemed later. Retryable actions instead rely on the rollback of
/// the executing transaction, which leaves the ticket `pending`.
pub async fn revoke_after_failure(
    db: &DatabaseConnection,
    request_id: i64,
    action: Action,
) -> CoreResult<()> {
    if action.is_retryable() {
        return Err(CoreError::invalid_input(
            "a retryable action keeps its ticket; do not revoke it",
        ));
    }
    authorization_ops::revoke(
        db,
        request_id,
        REASON_EXECUTION_FAILED_NOT_RETRYABLE,
        Utc::now(),
    )
    .await
    .map_err(CoreError::internal)?;
    Ok(())
}

/// When the head of a merge target moves, every pending request bound to the old
/// head is marked `expired` (reason `head_sha_changed`) and kept. This is the
/// event form of `pr_review_ops`' `commit_id == head_sha` filter.
///
/// The sha goes through `normalize_commit_sha`, so a short or upper-case id can
/// never silently fail to match.
pub async fn expire_on_head_sha_change(
    db: &DatabaseConnection,
    repo_id: i64,
    action: Action,
    subject: &str,
    head_sha: &str,
) -> CoreResult<u64> {
    let head_sha = normalize_commit_sha(head_sha)
        .map_err(|error| CoreError::invalid_input(error.to_string()))?;
    authorization_ops::expire_on_head_sha_change(
        db,
        repo_id,
        action.as_str(),
        subject,
        &head_sha,
        Utc::now(),
    )
    .await
    .map_err(CoreError::internal)
}

fn required_of(request: &Request) -> u32 {
    request.required_approvals.max(1) as u32
}

/// Refuse anything that is not still `pending`. The message never echoes stored
/// state back in a way a caller could use as an oracle across repositories.
fn not_pending(request: &Request) -> CoreError {
    let message = match request.state.as_str() {
        "consumed" => "this authorization request was already consumed",
        "expired" => "this authorization request has expired",
        "denied" => "this authorization request was denied",
        "revoked" => "this authorization request was revoked",
        _ => "this authorization request is not pending",
    };
    CoreError::coded(409, ALREADY_CONSUMED, message)
}

/// Recompute the payload digest from the row and compare. A mismatch means the
/// bindings moved after the vote (TOCTOU), so the vote cannot be honoured.
fn require_payload_unchanged(request: &Request, action: Action) -> CoreResult<()> {
    let recomputed = payload::payload_hash(
        request.repo_id,
        action,
        &request.subject,
        request.head_sha.as_deref(),
    );
    if recomputed != request.payload_hash {
        return Err(CoreError::coded(
            409,
            PAYLOAD_CHANGED,
            "the request no longer matches the payload that was approved",
        ));
    }
    Ok(())
}

/// A UTC clock, exposed for tests that need to reason about expiry.
pub fn clock() -> DateTime<Utc> {
    Utc::now()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rg_db::entities::repository::Model as RepositoryModel;
    use sea_orm::{
        ConnectOptions, Database, DatabaseBackend, DatabaseConnection, Statement, TransactionTrait,
    };

    const SHA_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const SHA_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    async fn setup() -> DatabaseConnection {
        let mut opt = ConnectOptions::new("sqlite::memory:");
        opt.max_connections(1);
        let db = Database::connect(opt).await.expect("connect");
        rg_db::run_migrations(&db).await.expect("migrate");
        exec(&db, "INSERT INTO users(id, username, email, password_hash, is_admin, is_active, created_at, updated_at) VALUES(1, 'owner', 'owner@test.com', 'x', 0, 1, '2024-01-01 00:00:00', '2024-01-01 00:00:00')").await;
        exec(&db, "INSERT INTO users(id, username, email, password_hash, is_admin, is_active, created_at, updated_at) VALUES(2, 'applicant', 'applicant@test.com', 'x', 0, 1, '2024-01-01 00:00:00', '2024-01-01 00:00:00')").await;
        exec(&db, "INSERT INTO users(id, username, email, password_hash, is_admin, is_active, created_at, updated_at) VALUES(3, 'reviewer', 'reviewer@test.com', 'x', 0, 1, '2024-01-01 00:00:00', '2024-01-01 00:00:00')").await;
        exec(&db, "INSERT INTO users(id, username, email, password_hash, is_admin, is_active, created_at, updated_at) VALUES(4, 'stranger', 'stranger@test.com', 'x', 0, 1, '2024-01-01 00:00:00', '2024-01-01 00:00:00')").await;
        exec(&db, "INSERT INTO users(id, username, email, password_hash, is_admin, is_active, created_at, updated_at, kind) VALUES(5, 'agent', 'agent@test.com', 'x', 0, 1, '2024-01-01 00:00:00', '2024-01-01 00:00:00', 'agent')").await;
        exec(&db, "INSERT INTO repositories(id, owner_id, name, is_private, default_branch, stars_count, forks_count, created_at, updated_at) VALUES(1, 1, 'repo', 0, 'main', 0, 0, '2024-01-01 00:00:00', '2024-01-01 00:00:00')").await;
        exec(&db, "INSERT INTO pull_requests(id, repo_id, number, title, state, is_draft, auto_merge_enabled, author_id, head_branch, base_branch, head_sha, created_at, updated_at) VALUES(1, 1, 1, 'change', 'open', 0, 0, 2, 'feature', 'main', 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', '2024-01-01 00:00:00', '2024-01-01 00:00:00')").await;
        exec(&db, "INSERT INTO protected_branches(id, repo_id, branch_name, require_pr, require_status_check, require_approval, required_approvals, allow_force_push, require_signed_commits, created_at, updated_at) VALUES(1, 1, 'main', 0, 0, 1, 2, 0, 0, '2024-01-01 00:00:00', '2024-01-01 00:00:00')").await;
        db
    }

    async fn exec(db: &DatabaseConnection, sql: &str) {
        db.execute(Statement::from_string(
            DatabaseBackend::Sqlite,
            sql.to_string(),
        ))
        .await
        .unwrap_or_else(|error| panic!("{sql}: {error}"));
    }

    async fn repo(db: &DatabaseConnection) -> RepositoryModel {
        repo_ops::find_by_id(db, 1).await.unwrap().unwrap()
    }

    fn merge_request<'a>(
        repo: &'a RepositoryModel,
        applicant: i64,
        key: &'a str,
        head_sha: Option<&'a str>,
    ) -> NewRequest<'a> {
        NewRequest {
            repo,
            action: Action::ProtectedBranchMerge,
            subject: "1",
            head_sha,
            requested_by_kind: RequesterKind::Human,
            requested_by_id: applicant,
            client_request_id: key,
        }
    }

    fn code(error: &CoreError) -> &'static str {
        match error {
            CoreError::Coded { code, .. } => code,
            other => panic!("expected a coded error, got {other:?}"),
        }
    }

    fn status(error: &CoreError) -> u16 {
        match error {
            CoreError::Coded { status, .. } => *status,
            other => panic!("expected a coded error, got {other:?}"),
        }
    }

    async fn file_merge(db: &DatabaseConnection, key: &str) -> Request {
        let repo = repo(db).await;
        request(db, merge_request(&repo, 2, key, Some(SHA_A)))
            .await
            .unwrap()
            .into_request()
    }

    #[tokio::test]
    async fn the_rule_matches_the_merge_gate() {
        let db = setup().await;
        let repo = repo(&db).await;
        let set = evaluate(&db, &repo, Action::ProtectedBranchMerge, "1")
            .await
            .unwrap();
        assert_eq!(set.required_approvals, 2, "rule says two approvals");
        assert!(set.writers, "writers may approve");
        assert!(!set.admins && !set.collaborators);
        assert!(set.explicit.is_empty());

        // The owner is a writer, so may approve; a stranger with read access is
        // not one, and is not a requested reviewer either.
        assert!(set.may_approve(&db, &repo, 1).await.unwrap());
        assert!(!set.may_approve(&db, &repo, 4).await.unwrap());
    }

    #[tokio::test]
    async fn a_requested_reviewer_is_an_approver() {
        let db = setup().await;
        exec(&db, "INSERT INTO pr_reviewer_requests(id, pr_id, reviewer_id, requested_by_id, created_at) VALUES(1, 1, 3, 1, '2024-01-01 00:00:00')").await;
        let repo = repo(&db).await;
        let set = evaluate(&db, &repo, Action::ProtectedBranchMerge, "1")
            .await
            .unwrap();
        assert_eq!(set.explicit, vec![3]);
        assert!(set.may_approve(&db, &repo, 3).await.unwrap());
    }

    #[tokio::test]
    async fn pushing_is_gated_by_the_allowlist_only() {
        let db = setup().await;
        let repo = repo(&db).await;
        exec(
            &db,
            "UPDATE protected_branches SET allowed_push_user_ids = '[3]' WHERE id = 1",
        )
        .await;
        let set = evaluate(&db, &repo, Action::ProtectedBranchPush, "refs/heads/main")
            .await
            .unwrap();
        assert_eq!(set.explicit, vec![3]);
        assert!(
            !set.writers,
            "the push allowlist does not inherit write access"
        );
        assert!(set.may_approve(&db, &repo, 3).await.unwrap());
        assert!(
            !set.may_approve(&db, &repo, 1).await.unwrap(),
            "the owner is a writer but not on the push allowlist"
        );
        // A branch that no rule covers cannot be the subject of a request.
        assert!(
            evaluate(&db, &repo, Action::ProtectedBranchPush, "refs/heads/other")
                .await
                .is_err()
        );
        assert!(evaluate(&db, &repo, Action::ProtectedBranchPush, "main")
            .await
            .is_err());
    }

    #[tokio::test]
    async fn scope_expand_is_not_evaluated_yet() {
        let db = setup().await;
        let repo = repo(&db).await;
        let error = evaluate(&db, &repo, Action::ScopeExpand, "session")
            .await
            .unwrap_err();
        assert_eq!(status(&error), 501);
    }

    #[tokio::test]
    async fn a_subject_of_another_repository_is_invisible() {
        let db = setup().await;
        let repo = repo(&db).await;
        // No PR 99: the same message as "not yours", so ids cannot be probed.
        assert!(evaluate(&db, &repo, Action::ProtectedBranchMerge, "99")
            .await
            .is_err());
        assert!(
            evaluate(&db, &repo, Action::ProtectedBranchMerge, "not-a-number")
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn filing_is_idempotent_and_key_reuse_is_a_conflict() {
        let db = setup().await;
        let first = match request(&db, merge_request(&repo(&db).await, 2, "k1", Some(SHA_A)))
            .await
            .unwrap()
        {
            Requested::Created(request) => request,
            Requested::Replayed(_) => panic!("first send must create"),
        };
        let replay = request(&db, merge_request(&repo(&db).await, 2, "k1", Some(SHA_A)))
            .await
            .unwrap();
        assert_eq!(replay.into_request().id, first.id);

        let error = request(&db, merge_request(&repo(&db).await, 2, "k1", Some(SHA_B)))
            .await
            .unwrap_err();
        assert_eq!(code(&error), IDEMPOTENCY_KEY_REUSED);
        assert_eq!(status(&error), 409);
    }

    #[tokio::test]
    async fn a_short_or_upper_case_head_is_normalised_or_rejected() {
        let db = setup().await;
        let repo = repo(&db).await;
        // Short shas are refused rather than silently stored: they would never
        // match the gate's full sha.
        assert!(request(&db, merge_request(&repo, 2, "k1", Some("abc123")))
            .await
            .is_err());
        let upper = SHA_A.to_ascii_uppercase();
        let request = request(&db, merge_request(&repo, 2, "k2", Some(&upper)))
            .await
            .unwrap()
            .into_request();
        assert_eq!(request.head_sha.as_deref(), Some(SHA_A));
    }

    #[tokio::test]
    async fn the_applicant_cannot_approve_their_own_request() {
        let db = setup().await;
        let request = file_merge(&db, "k1").await;
        let error = approve(&db, request.id, 2).await.unwrap_err();
        assert_eq!(code(&error), SELF_APPROVAL);
        assert_eq!(status(&error), 403);
    }

    #[tokio::test]
    async fn an_agent_cannot_approve_whatever_its_scope() {
        let db = setup().await;
        let request = file_merge(&db, "k1").await;
        let error = approve(&db, request.id, 5).await.unwrap_err();
        assert_eq!(code(&error), "AGENT_MAY_NOT_APPROVE");
        assert_eq!(status(&error), 403);
    }

    #[tokio::test]
    async fn a_reader_without_a_seat_cannot_approve() {
        let db = setup().await;
        let request = file_merge(&db, "k1").await;
        let error = approve(&db, request.id, 4).await.unwrap_err();
        assert!(matches!(error, CoreError::Forbidden(_)));
    }

    #[tokio::test]
    async fn approvals_count_until_the_rule_is_satisfied() {
        let db = setup().await;
        exec(&db, "INSERT INTO repo_collaborators(id, repo_id, user_id, permission, created_at) VALUES(1, 1, 3, 'write', '2024-01-01 00:00:00')").await;
        let request = file_merge(&db, "k1").await;
        assert_eq!(request.required_approvals, 2);

        let first = approve(&db, request.id, 1).await.unwrap();
        assert_eq!((first.approvals, first.ready), (1, false));
        let second = approve(&db, request.id, 3).await.unwrap();
        assert_eq!((second.approvals, second.ready), (2, true));

        let duplicate = approve(&db, request.id, 3).await.unwrap_err();
        assert_eq!(code(&duplicate), ALREADY_DECIDED);
    }

    #[tokio::test]
    async fn a_vote_without_enough_votes_cannot_be_consumed() {
        let db = setup().await;
        let request = file_merge(&db, "k1").await;
        approve(&db, request.id, 1).await.unwrap();
        let error = consume(&db, request.id, 1, "merge-sha").await.unwrap_err();
        assert_eq!(code(&error), NOT_APPROVED);
    }

    #[tokio::test]
    async fn consumption_is_one_shot_and_needs_the_votes() {
        let db = setup().await;
        exec(&db, "INSERT INTO repo_collaborators(id, repo_id, user_id, permission, created_at) VALUES(1, 1, 3, 'write', '2024-01-01 00:00:00')").await;
        let request = file_merge(&db, "k1").await;
        approve(&db, request.id, 1).await.unwrap();
        approve(&db, request.id, 3).await.unwrap();

        consume(&db, request.id, 1, "merge-sha").await.unwrap();
        let error = consume(&db, request.id, 1, "merge-sha").await.unwrap_err();
        assert_eq!(code(&error), ALREADY_CONSUMED);
        assert_eq!(status(&error), 409);

        let row = authorization_ops::find_by_id(&db, request.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(row.state, "consumed");
        assert_eq!(row.consumed_effect_hash.as_deref(), Some("merge-sha"));
        assert_eq!(row.consumed_by, Some(1));
    }

    #[tokio::test]
    async fn a_rolled_back_execution_leaves_the_ticket_usable() {
        let db = setup().await;
        exec(&db, "INSERT INTO repo_collaborators(id, repo_id, user_id, permission, created_at) VALUES(1, 1, 3, 'write', '2024-01-01 00:00:00')").await;
        let request = file_merge(&db, "k1").await;
        approve(&db, request.id, 1).await.unwrap();
        approve(&db, request.id, 3).await.unwrap();

        let txn = db.begin().await.unwrap();
        consume(&txn, request.id, 1, "merge-sha").await.unwrap();
        txn.rollback().await.unwrap();

        let row = authorization_ops::find_by_id(&db, request.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(row.state, "pending");
        // The retry still works: the failure did not burn the ticket.
        consume(&db, request.id, 1, "merge-sha").await.unwrap();
    }

    #[tokio::test]
    async fn a_denial_is_terminal() {
        let db = setup().await;
        let request = file_merge(&db, "k1").await;
        let progress = deny(&db, request.id, 1).await.unwrap();
        assert_eq!(progress.request.state, "denied");
        let error = approve(&db, request.id, 3).await.unwrap_err();
        assert_eq!(code(&error), ALREADY_CONSUMED);
    }

    #[tokio::test]
    async fn a_moved_head_expires_the_ticket() {
        let db = setup().await;
        let request = file_merge(&db, "k1").await;
        let repo = repo(&db).await;
        let expired =
            expire_on_head_sha_change(&db, repo.id, Action::ProtectedBranchMerge, "1", SHA_B)
                .await
                .unwrap();
        assert_eq!(expired, 1);
        let row = authorization_ops::find_by_id(&db, request.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(row.state, "expired");
        assert_eq!(row.state_reason.as_deref(), Some("head_sha_changed"));
        assert!(approve(&db, request.id, 1).await.is_err());
    }

    #[tokio::test]
    async fn a_short_head_cannot_silently_miss_the_sweep() {
        let db = setup().await;
        let repo = repo(&db).await;
        assert!(
            expire_on_head_sha_change(&db, repo.id, Action::ProtectedBranchMerge, "1", "abc123")
                .await
                .is_err(),
            "a short sha must be rejected, not compared"
        );
    }

    #[tokio::test]
    async fn an_expired_request_cannot_be_approved() {
        let db = setup().await;
        let request = file_merge(&db, "k1").await;
        // Move the deadline into the past: the wall clock decides, not a flag.
        exec(
            &db,
            "UPDATE authorization_requests SET expires_at = '2000-01-01 00:00:00' WHERE id = 1",
        )
        .await;
        let error = approve(&db, request.id, 1).await.unwrap_err();
        assert_eq!(code(&error), EXPIRED);
        let row = authorization_ops::find_by_id(&db, request.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(row.state, "expired");
    }

    #[tokio::test]
    async fn a_tampered_binding_is_refused_at_vote_time() {
        let db = setup().await;
        let request = file_merge(&db, "k1").await;
        // Someone rewrites the binding after the request was filed.
        exec(
            &db,
            &format!("UPDATE authorization_requests SET head_sha = '{SHA_B}' WHERE id = 1"),
        )
        .await;
        let error = approve(&db, request.id, 1).await.unwrap_err();
        assert_eq!(code(&error), PAYLOAD_CHANGED);
    }

    #[tokio::test]
    async fn only_non_retryable_actions_revoke_on_failure() {
        let db = setup().await;
        let request = file_merge(&db, "k1").await;
        assert!(
            revoke_after_failure(&db, request.id, Action::ProtectedBranchMerge)
                .await
                .is_err(),
            "a merge can be retried, so its ticket stays"
        );
        revoke_after_failure(&db, request.id, Action::ReleasePublish)
            .await
            .unwrap();
        let row = authorization_ops::find_by_id(&db, request.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(row.state, "revoked");
        assert_eq!(
            row.state_reason.as_deref(),
            Some(REASON_EXECUTION_FAILED_NOT_RETRYABLE)
        );
    }

    #[tokio::test]
    async fn a_stranger_cannot_file_a_request_for_a_repository_they_cannot_read() {
        let db = setup().await;
        exec(&db, "UPDATE repositories SET is_private = 1 WHERE id = 1").await;
        let repo = repo(&db).await;
        let error = request(&db, merge_request(&repo, 4, "k1", Some(SHA_A)))
            .await
            .unwrap_err();
        assert!(matches!(error, CoreError::Forbidden(_)));
        // The applicant (2) is not a member either, in this private repository.
        let error = request(&db, merge_request(&repo, 2, "k2", Some(SHA_A)))
            .await
            .unwrap_err();
        assert!(matches!(error, CoreError::Forbidden(_)));
    }
}
