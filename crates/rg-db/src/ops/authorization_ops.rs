//! Storage primitives for the generic approval primitive (#19, step 1).
//!
//! This module owns the *mechanics* of the state machine and nothing else: it
//! does not know who is allowed to approve (that is
//! `rg_core::authorization::evaluate`) and it does not know what executing an
//! action means. Every state change is a guarded `UPDATE ... WHERE state = ...`
//! whose affected-row count is the answer, because a read-then-write would turn
//! a one-shot ticket into a reusable one under concurrency.
//!
//! The rule that must not be broken: `consume_guarded` runs **in the caller's
//! transaction**, together with the action it authorises. If the action fails
//! and the transaction rolls back, the ticket is `pending` again — not consumed.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use sea_orm::sea_query::Expr;
use sea_orm::*;

use crate::entities::authorization_decision::{
    self, ActiveModel as DecisionActiveModel, Model as Decision,
};
use crate::entities::authorization_request::{
    self, ActiveModel as RequestActiveModel, Model as Request, REASON_HEAD_SHA_CHANGED,
    STATE_CONSUMED, STATE_DENIED, STATE_EXPIRED, STATE_PENDING, STATE_REVOKED,
};

/// Everything a caller must supply to file a request. `payload_hash` and
/// `authorization_set_id` are computed by `rg_core::authorization`, so the
/// storage layer never decides what a request is bound to.
pub struct NewRequest {
    pub repo_id: i64,
    pub action: String,
    pub subject: String,
    pub head_sha: Option<String>,
    pub requested_by_kind: String,
    pub requested_by_id: i64,
    pub authorization_set_id: i64,
    pub required_approvals: i32,
    pub client_request_id: String,
    pub payload_hash: String,
    pub expires_at: DateTime<Utc>,
}

/// Outcome of [`create`]. `Existing` means the same principal sent the same
/// `client_request_id` before; whether that is a replay or a key misuse is the
/// caller's call (it owns the payload comparison).
pub enum RequestOutcome {
    Created(Request),
    Existing(Request),
}

fn is_duplicate(error: &DbErr) -> bool {
    matches!(error.sql_err(), Some(SqlErr::UniqueConstraintViolation(_)))
        || error.to_string().to_ascii_lowercase().contains("unique")
}

/// File a request, or return the row that already holds the idempotency key.
///
/// `INSERT ... ON CONFLICT DO NOTHING` is not portable to MySQL (CLAUDE.md
/// pitfall 19), so a plain insert whose unique violation means "this key already
/// exists" is used instead.
pub async fn create(
    db: &DatabaseConnection,
    new: NewRequest,
    now: DateTime<Utc>,
) -> Result<RequestOutcome> {
    let model = RequestActiveModel {
        id: NotSet,
        repo_id: Set(new.repo_id),
        action: Set(new.action),
        subject: Set(new.subject),
        head_sha: Set(new.head_sha),
        requested_by_kind: Set(new.requested_by_kind.clone()),
        requested_by_id: Set(new.requested_by_id),
        authorization_set_id: Set(new.authorization_set_id),
        required_approvals: Set(new.required_approvals),
        state: Set(STATE_PENDING.to_string()),
        state_reason: Set(None),
        expires_at: Set(new.expires_at),
        consumed_at: Set(None),
        consumed_by: Set(None),
        consumed_effect_hash: Set(None),
        client_request_id: Set(new.client_request_id.clone()),
        payload_hash: Set(new.payload_hash),
        created_at: Set(now),
        updated_at: Set(now),
    };
    match model.insert(db).await {
        Ok(row) => Ok(RequestOutcome::Created(row)),
        Err(error) if is_duplicate(&error) => {
            let existing = find_by_client_key(
                db,
                new.repo_id,
                &new.requested_by_kind,
                new.requested_by_id,
                &new.client_request_id,
            )
            .await?
            .ok_or_else(|| anyhow::anyhow!("db: idempotent request row vanished"))?;
            Ok(RequestOutcome::Existing(existing))
        }
        Err(error) => Err(error).context("db: create authorization request"),
    }
}

pub async fn find_by_id<C: ConnectionTrait>(conn: &C, id: i64) -> Result<Option<Request>> {
    authorization_request::Entity::find_by_id(id)
        .one(conn)
        .await
        .context("db: find authorization request")
}

pub async fn find_by_client_key(
    db: &DatabaseConnection,
    repo_id: i64,
    requested_by_kind: &str,
    requested_by_id: i64,
    client_request_id: &str,
) -> Result<Option<Request>> {
    authorization_request::Entity::find()
        .filter(authorization_request::Column::RepoId.eq(repo_id))
        .filter(authorization_request::Column::RequestedByKind.eq(requested_by_kind))
        .filter(authorization_request::Column::RequestedById.eq(requested_by_id))
        .filter(authorization_request::Column::ClientRequestId.eq(client_request_id))
        .one(db)
        .await
        .context("db: find authorization request by client key")
}

pub async fn list_decisions(db: &DatabaseConnection, request_id: i64) -> Result<Vec<Decision>> {
    authorization_decision::Entity::find()
        .filter(authorization_decision::Column::RequestId.eq(request_id))
        .order_by_asc(authorization_decision::Column::Id)
        .all(db)
        .await
        .context("db: list authorization decisions")
}

/// How many approval votes the request has. Denials are not votes against in
/// this primitive: a single `deny` marks the request `denied` (see
/// `rg_core::authorization::decide`), which is a different migration.
pub async fn count_approvals<C: ConnectionTrait>(conn: &C, request_id: i64) -> Result<u64> {
    authorization_decision::Entity::find()
        .filter(authorization_decision::Column::RequestId.eq(request_id))
        .filter(
            authorization_decision::Column::Decision.eq(authorization_decision::DECISION_APPROVE),
        )
        .count(conn)
        .await
        .context("db: count authorization approvals")
}

/// Record one person's vote. `Ok(false)` is the "already voted" answer, the same
/// semantics `ci_environment_ops::add_approval` gives a duplicate.
///
/// Takes `&impl ConnectionTrait` so a caller can vote inside the same
/// transaction as the state check it just did.
pub async fn add_decision<C: ConnectionTrait>(
    conn: &C,
    request_id: i64,
    decider_user_id: i64,
    decision: &str,
    payload_hash: &str,
    now: DateTime<Utc>,
) -> Result<bool> {
    let model = DecisionActiveModel {
        id: NotSet,
        request_id: Set(request_id),
        decider_user_id: Set(decider_user_id),
        decision: Set(decision.to_string()),
        payload_hash: Set(payload_hash.to_string()),
        created_at: Set(now),
    };
    match model.insert(conn).await {
        Ok(_) => Ok(true),
        Err(error) if is_duplicate(&error) => Ok(false),
        Err(error) => Err(error).context("db: add authorization decision"),
    }
}

/// Atomically move `pending` -> `consumed` and stamp the effect.
///
/// Returns `false` when no row matched, i.e. the ticket was already consumed,
/// revoked or expired. Callers must treat that as
/// `409 AUTHORIZATION_ALREADY_CONSUMED` and **not** perform the action.
///
/// This is the one place that turns a ticket into an execution right. It has to
/// run inside the transaction that performs the action; committing it on its own
/// is what produces "approved once, unlocked forever".
pub async fn consume_guarded<C: ConnectionTrait>(
    conn: &C,
    request_id: i64,
    actor_id: i64,
    effect_hash: &str,
    now: DateTime<Utc>,
) -> Result<bool> {
    let result = authorization_request::Entity::update_many()
        .col_expr(
            authorization_request::Column::State,
            Expr::value(STATE_CONSUMED),
        )
        .col_expr(authorization_request::Column::ConsumedAt, Expr::value(now))
        .col_expr(
            authorization_request::Column::ConsumedBy,
            Expr::value(actor_id),
        )
        .col_expr(
            authorization_request::Column::ConsumedEffectHash,
            Expr::value(effect_hash),
        )
        .col_expr(authorization_request::Column::UpdatedAt, Expr::value(now))
        .filter(authorization_request::Column::Id.eq(request_id))
        .filter(authorization_request::Column::State.eq(STATE_PENDING))
        .exec(conn)
        .await
        .context("db: consume authorization request")?;
    Ok(result.rows_affected == 1)
}

/// Invalidate a request that must not be reusable: `pending` or `consumed` ->
/// `revoked`. Used after an action that is not idempotent failed, and for an
/// explicit withdrawal.
pub async fn revoke(
    db: &DatabaseConnection,
    request_id: i64,
    reason: &str,
    now: DateTime<Utc>,
) -> Result<bool> {
    let result = authorization_request::Entity::update_many()
        .col_expr(
            authorization_request::Column::State,
            Expr::value(STATE_REVOKED),
        )
        .col_expr(
            authorization_request::Column::StateReason,
            Expr::value(reason.to_string()),
        )
        .col_expr(authorization_request::Column::UpdatedAt, Expr::value(now))
        .filter(authorization_request::Column::Id.eq(request_id))
        .filter(authorization_request::Column::State.is_in([STATE_PENDING, STATE_CONSUMED]))
        .exec(db)
        .await
        .context("db: revoke authorization request")?;
    Ok(result.rows_affected == 1)
}

/// `pending` -> `denied`. A denial is terminal for the request; the applicant
/// has to file a new one.
pub async fn deny(
    db: &DatabaseConnection,
    request_id: i64,
    reason: &str,
    now: DateTime<Utc>,
) -> Result<bool> {
    let result = authorization_request::Entity::update_many()
        .col_expr(
            authorization_request::Column::State,
            Expr::value(STATE_DENIED),
        )
        .col_expr(
            authorization_request::Column::StateReason,
            Expr::value(reason.to_string()),
        )
        .col_expr(authorization_request::Column::UpdatedAt, Expr::value(now))
        .filter(authorization_request::Column::Id.eq(request_id))
        .filter(authorization_request::Column::State.eq(STATE_PENDING))
        .exec(db)
        .await
        .context("db: deny authorization request")?;
    Ok(result.rows_affected == 1)
}

/// `pending` -> `expired` once the deadline has passed. Returns whether the row
/// moved, so the caller can answer "expired" instead of "not approved yet".
pub async fn expire_if_overdue<C: ConnectionTrait>(
    conn: &C,
    request_id: i64,
    now: DateTime<Utc>,
) -> Result<bool> {
    let result = authorization_request::Entity::update_many()
        .col_expr(
            authorization_request::Column::State,
            Expr::value(STATE_EXPIRED),
        )
        .col_expr(
            authorization_request::Column::StateReason,
            Expr::value(crate::entities::authorization_request::REASON_EXPIRED),
        )
        .col_expr(authorization_request::Column::UpdatedAt, Expr::value(now))
        .filter(authorization_request::Column::Id.eq(request_id))
        .filter(authorization_request::Column::State.eq(STATE_PENDING))
        .filter(authorization_request::Column::ExpiresAt.lt(now))
        .exec(conn)
        .await
        .context("db: expire overdue authorization request")?;
    Ok(result.rows_affected == 1)
}

/// The event form of what `pr_review_ops::current_human_approver_ids` does by
/// filtering: when the head moves, every pending request bound to the old head
/// becomes `expired` with reason `head_sha_changed`. Rows are kept, so "why did
/// my ticket die" is answerable later.
///
/// `head_sha` must already be normalised (`normalize_commit_sha`); this function
/// does no short-SHA or case handling of its own.
pub async fn expire_on_head_sha_change(
    db: &DatabaseConnection,
    repo_id: i64,
    action: &str,
    subject: &str,
    head_sha: &str,
    now: DateTime<Utc>,
) -> Result<u64> {
    let result = authorization_request::Entity::update_many()
        .col_expr(
            authorization_request::Column::State,
            Expr::value(STATE_EXPIRED),
        )
        .col_expr(
            authorization_request::Column::StateReason,
            Expr::value(REASON_HEAD_SHA_CHANGED),
        )
        .col_expr(authorization_request::Column::UpdatedAt, Expr::value(now))
        .filter(authorization_request::Column::RepoId.eq(repo_id))
        .filter(authorization_request::Column::Action.eq(action))
        .filter(authorization_request::Column::Subject.eq(subject))
        .filter(authorization_request::Column::State.eq(STATE_PENDING))
        .filter(authorization_request::Column::HeadSha.is_not_null())
        .filter(authorization_request::Column::HeadSha.ne(head_sha))
        .exec(db)
        .await
        .context("db: expire requests on head sha change")?;
    Ok(result.rows_affected)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::authorization_request::REASON_EXECUTION_FAILED_NOT_RETRYABLE;
    use sea_orm::{ConnectOptions, Database, Statement};

    const SHA_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const SHA_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    async fn setup() -> DatabaseConnection {
        let mut opt = ConnectOptions::new("sqlite::memory:");
        opt.max_connections(1);
        let db = Database::connect(opt)
            .await
            .expect("connect to in-memory db");
        crate::run_migrations(&db).await.expect("run migrations");
        db.execute(Statement::from_string(
            DatabaseBackend::Sqlite,
            "INSERT INTO users(id, username, email, password_hash, is_admin, is_active, created_at, updated_at) VALUES(1, 'applicant', 'a@test.com', 'x', 0, 1, '2024-01-01', '2024-01-01')",
        ))
        .await
        .expect("insert applicant");
        db.execute(Statement::from_string(
            DatabaseBackend::Sqlite,
            "INSERT INTO users(id, username, email, password_hash, is_admin, is_active, created_at, updated_at) VALUES(2, 'approver', 'b@test.com', 'x', 0, 1, '2024-01-01', '2024-01-01')",
        ))
        .await
        .expect("insert approver");
        db.execute(Statement::from_string(
            DatabaseBackend::Sqlite,
            "INSERT INTO repositories(id, owner_id, name, is_private, default_branch, stars_count, forks_count, created_at, updated_at) VALUES(1, 1, 'repo', 0, 'main', 0, 0, '2024-01-01', '2024-01-01')",
        ))
        .await
        .expect("insert repo");
        db
    }

    fn new_request(client_key: &str) -> NewRequest {
        NewRequest {
            repo_id: 1,
            action: "protected_branch_merge".to_string(),
            subject: "1".to_string(),
            head_sha: Some(SHA_A.to_string()),
            requested_by_kind: "human".to_string(),
            requested_by_id: 1,
            authorization_set_id: 7,
            required_approvals: 1,
            client_request_id: client_key.to_string(),
            payload_hash: "deadbeef".to_string(),
            expires_at: Utc::now() + chrono::Duration::hours(1),
        }
    }

    async fn file(db: &DatabaseConnection, client_key: &str) -> Request {
        match create(db, new_request(client_key), Utc::now())
            .await
            .unwrap()
        {
            RequestOutcome::Created(row) | RequestOutcome::Existing(row) => row,
        }
    }

    #[tokio::test]
    async fn request_is_idempotent_on_client_key() {
        let db = setup().await;
        let first = file(&db, "key-1").await;
        let second = file(&db, "key-1").await;
        assert_eq!(first.id, second.id, "same key must reuse the row");
        assert_eq!(first.state, STATE_PENDING);
    }

    #[tokio::test]
    async fn a_second_vote_by_the_same_person_is_not_a_second_ticket() {
        let db = setup().await;
        let request = file(&db, "key-1").await;
        assert!(add_decision(&db, request.id, 2, "approve", "h", Utc::now())
            .await
            .unwrap());
        assert!(
            !add_decision(&db, request.id, 2, "approve", "h", Utc::now())
                .await
                .unwrap(),
            "duplicate vote must answer false"
        );
        assert_eq!(count_approvals(&db, request.id).await.unwrap(), 1);
    }

    #[tokio::test]
    async fn consuming_twice_cannot_happen() {
        let db = setup().await;
        let request = file(&db, "key-1").await;
        assert!(consume_guarded(&db, request.id, 1, "merge-sha", Utc::now())
            .await
            .unwrap());
        assert!(
            !consume_guarded(&db, request.id, 1, "merge-sha", Utc::now())
                .await
                .unwrap(),
            "the guard must make the second consume a no-op"
        );
        let row = find_by_id(&db, request.id).await.unwrap().unwrap();
        assert_eq!(row.state, STATE_CONSUMED);
        assert_eq!(row.consumed_effect_hash.as_deref(), Some("merge-sha"));
    }

    /// The property that keeps a failed execution from eating the ticket: the
    /// consume and the action share a transaction, so a rollback leaves the row
    /// `pending`.
    #[tokio::test]
    async fn rolling_back_the_execution_leaves_the_ticket_pending() {
        let db = setup().await;
        let request = file(&db, "key-1").await;

        let txn = db.begin().await.unwrap();
        assert!(consume_guarded(&txn, request.id, 1, "effect", Utc::now())
            .await
            .unwrap());
        // The action failed: the caller drops the transaction instead of
        // committing it.
        txn.rollback().await.unwrap();

        let row = find_by_id(&db, request.id).await.unwrap().unwrap();
        assert_eq!(row.state, STATE_PENDING, "a rollback must not consume");
    }

    #[tokio::test]
    async fn a_non_retryable_failure_revokes_instead_of_restoring() {
        let db = setup().await;
        let request = file(&db, "key-1").await;
        assert!(
            consume_guarded(&db, request.id, 1, "effect", Utc::now())
                .await
                .unwrap(),
            "the first consume takes the ticket"
        );
        assert!(revoke(
            &db,
            request.id,
            REASON_EXECUTION_FAILED_NOT_RETRYABLE,
            Utc::now()
        )
        .await
        .unwrap());
        let row = find_by_id(&db, request.id).await.unwrap().unwrap();
        assert_eq!(row.state, STATE_REVOKED);
        assert_eq!(
            row.state_reason.as_deref(),
            Some(REASON_EXECUTION_FAILED_NOT_RETRYABLE)
        );
        assert!(
            !consume_guarded(&db, request.id, 1, "effect", Utc::now())
                .await
                .unwrap(),
            "a revoked ticket must not be consumable"
        );
    }

    #[tokio::test]
    async fn a_moved_head_expires_the_old_request_and_keeps_it() {
        let db = setup().await;
        let request = file(&db, "key-1").await;

        let expired =
            expire_on_head_sha_change(&db, 1, "protected_branch_merge", "1", SHA_B, Utc::now())
                .await
                .unwrap();
        assert_eq!(expired, 1);

        let row = find_by_id(&db, request.id)
            .await
            .unwrap()
            .expect("expiry keeps the row");
        assert_eq!(row.state, STATE_EXPIRED);
        assert_eq!(row.state_reason.as_deref(), Some(REASON_HEAD_SHA_CHANGED));
        assert!(
            !consume_guarded(&db, request.id, 1, "effect", Utc::now())
                .await
                .unwrap(),
            "an expired ticket cannot authorise anything"
        );
    }

    #[tokio::test]
    async fn a_request_for_the_current_head_is_not_expired() {
        let db = setup().await;
        let request = file(&db, "key-1").await;
        let expired =
            expire_on_head_sha_change(&db, 1, "protected_branch_merge", "1", SHA_A, Utc::now())
                .await
                .unwrap();
        assert_eq!(expired, 0);
        assert_eq!(
            find_by_id(&db, request.id).await.unwrap().unwrap().state,
            STATE_PENDING
        );
    }

    #[tokio::test]
    async fn overdue_requests_expire_before_they_can_be_approved() {
        let db = setup().await;
        let mut new = new_request("key-1");
        new.expires_at = Utc::now() - chrono::Duration::seconds(1);
        let request = match create(&db, new, Utc::now()).await.unwrap() {
            RequestOutcome::Created(row) | RequestOutcome::Existing(row) => row,
        };
        assert!(expire_if_overdue(&db, request.id, Utc::now())
            .await
            .unwrap());
        assert!(!expire_if_overdue(&db, request.id, Utc::now())
            .await
            .unwrap());
        assert_eq!(
            find_by_id(&db, request.id).await.unwrap().unwrap().state,
            STATE_EXPIRED
        );
    }

    #[tokio::test]
    async fn denying_is_terminal() {
        let db = setup().await;
        let request = file(&db, "key-1").await;
        assert!(deny(&db, request.id, "denied", Utc::now()).await.unwrap());
        assert!(!deny(&db, request.id, "denied", Utc::now()).await.unwrap());
        assert!(!consume_guarded(&db, request.id, 1, "e", Utc::now())
            .await
            .unwrap());
    }
}
