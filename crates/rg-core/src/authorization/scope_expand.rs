//! Execution surface of `scope_expand` (#19, step 6).
//!
//! The question step 1 left open was "what does the agent actually get when an
//! expansion is approved". The answer implemented here:
//!
//! **The token's `scopes` are widened in place. No new token is minted, no
//! session is restarted, and the change is confined to the `repo` scope family.**
//!
//! - `access_tokens.scopes` is updated inside the same transaction that consumes
//!   the ticket, so a failed consume rolls the widening back (the ticket stays
//!   `pending` and can be retried, exactly like the other retryable actions).
//! - The PAT middleware resolves a token from the database on every request
//!   (`rg-http/src/lib.rs::resolve_pat`) and never caches scopes, so the next
//!   request made with the same token already sees the wider scope.
//! - `agent::validate_agent_token_scopes` is applied to the target, so expansion
//!   can never hand an agent `user` or `admin`; the ladder inside the repo family
//!   is enforced with `pat_scope::has_scope` (see [`widening_of`]).
//!
//! The widening is permanent on the token: there is no "narrow it back" API
//! today, so removing the power means revoking or replacing the token. That is a
//! property of this definition, recorded in `docs/design/authorization.md`, not an
//! oversight.
//!
//! ## What the approval is bound to
//!
//! A request names `subject = "<token id>:<target scopes>"`, for example
//! `10:repo:write`, and is filed against a repository. The target is part of the
//! subject on purpose: `payload::payload_hash` covers the subject, so the hash
//! the approver saw is the hash of *this token and this target*. Without it the
//! approval would only bind a token id, and an executor could pass any target
//! the semantic ladder accepts — `repo` instead of the approved `repo:write`
//! widens more than anyone approved. There is no `parameters` column to bind it
//! to (and adding one means a migration), but the subject is hashed, which makes
//! this encoding equivalent (see `docs/design/authorization.md`).
//!
//! The format is canonical: a positive decimal token id without leading zeros,
//! then `:`, then scopes in [`pat_scope::normalize_scopes`] order. Both the
//! evaluator (when filing and when a vote is cast) and [`consume_scope_expand`]
//! validate it through [`parse_subject`].
//!
//! ## Which token may be expanded
//!
//! A request is filed against a repository. The approval is given by *that
//! repository's* administrators, so the token must be provably confined to that
//! same repository, otherwise one repository's admin could widen a token that
//! also reaches other repositories. [`load_expandable_token`] therefore requires
//! the token to be an active **agent** token whose `repo_allowlist` is exactly
//! `["<owner>/<name>"]` of the request's repository. Tokens without an
//! allow-list are refused: "widen it everywhere" cannot be approved by a
//! repository admin.

use chrono::Utc;
use rg_db::entities::access_token::{self, Model as AccessToken};
use rg_db::entities::repository::{self, Column as RepositoryColumn, Model as Repository};
use rg_db::ops::{authorization_ops, user_ops};
use sea_orm::sea_query::Expr;
use sea_orm::{
    ColumnTrait, ConnectionTrait, DatabaseConnection, DatabaseTransaction, EntityTrait,
    QueryFilter, TransactionTrait,
};
use sha2::{Digest, Sha256};

use super::action::Action;
use super::{consume, evaluator, SELF_APPROVAL};
use crate::auth::pat_scope;
use crate::error::{CoreError, CoreResult};

/// Stable code for "the token changed between approval and execution".
pub const SCOPES_CHANGED: &str = "SCOPE_EXPAND_TOKEN_CHANGED";

/// What the caller and the audit trail need to know after a successful
/// expansion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenScopeExpandResult {
    pub token_id: i64,
    pub previous_scopes: String,
    pub scopes: String,
}

/// Parse the `scope_expand` subject: `"{token_id}:{target_scopes}"`.
///
/// The token id must be a positive decimal integer written without leading
/// zeros (`0`, `-3`, `007`, `1.5`, `+10` and the empty string are all refused),
/// and the scopes must already be in canonical `normalize_scopes` form and pass
/// `agent::validate_agent_token_scopes`. Requiring the canonical form is what
/// makes the exact-match check in [`consume_scope_expand`] meaningful: two
/// different spellings of the same target never share an approved subject.
pub(crate) fn parse_subject(subject: &str) -> CoreResult<(i64, String)> {
    let Some((token_part, scopes_part)) = subject.split_once(':') else {
        return Err(CoreError::invalid_input(
            "a scope_expand subject must be \"<token id>:<target scopes>\", e.g. \"10:repo:write\"",
        ));
    };
    let token_id = token_part
        .parse::<u64>()
        .ok()
        .filter(|id| *id > 0 && id.to_string() == token_part)
        .and_then(|id| i64::try_from(id).ok())
        .ok_or_else(|| {
            CoreError::invalid_input(
                "the token id in a scope_expand subject must be a positive decimal integer without leading zeros",
            )
        })?;

    let scopes = pat_scope::normalize_scopes(scopes_part)?;
    if scopes != scopes_part {
        return Err(CoreError::invalid_input(
            "the target scopes in a scope_expand subject must be canonical (comma separated, in normalize_scopes order)",
        ));
    }
    crate::agent::validate_agent_token_scopes(&scopes)?;
    Ok((token_id, scopes))
}

/// Consume a `scope_expand` ticket and widen the token in the same transaction.
///
/// `actor_id` is the person executing the approval. It must be a person: an
/// agent can file the request and never redeem it.
///
/// `token_id` and `target_scopes` must reproduce the approved subject exactly:
/// `"{token_id}:{target_scopes}"`. Both checks run: the exact match proves the
/// execution target is the one that was approved, and [`widening_of`] proves
/// the target still widens the token's current scopes without buying `user` or
/// `admin`. The semantic ladder alone is *not* enough — it accepts `repo` for an
/// approved `repo:write`, which is more than the approver agreed to.
///
/// On any failure the whole transaction is rolled back, so neither the token nor
/// the ticket changes.
pub async fn consume_scope_expand(
    db: &DatabaseConnection,
    request_id: i64,
    actor_id: i64,
    token_id: i64,
    target_scopes: &str,
) -> CoreResult<TokenScopeExpandResult> {
    // Defence in depth: `approve` already refuses agents, but the executor must
    // not be an agent either, however it reached this function.
    evaluator::ensure_human_decider(db, actor_id).await?;

    let txn = db.begin().await.map_err(CoreError::internal)?;
    match apply(&txn, request_id, actor_id, token_id, target_scopes).await {
        Ok(result) => {
            txn.commit().await.map_err(CoreError::internal)?;
            Ok(result)
        }
        Err(error) => {
            // Explicit rollback: the token update must never survive a failed
            // consume (test 9 in the task is exactly this).
            let _ = txn.rollback().await;
            Err(error)
        }
    }
}

async fn apply(
    txn: &DatabaseTransaction,
    request_id: i64,
    actor_id: i64,
    token_id: i64,
    target_scopes: &str,
) -> CoreResult<TokenScopeExpandResult> {
    let request = authorization_ops::find_by_id(txn, request_id)
        .await
        .map_err(CoreError::internal)?
        .ok_or_else(|| CoreError::not_found("authorization request not found"))?;
    if request.action != Action::ScopeExpand.as_str() {
        return Err(CoreError::invalid_input(
            "this authorization request is not a scope_expand request",
        ));
    }
    let (approved_token_id, approved_scopes) = parse_subject(&request.subject)?;
    if approved_token_id != token_id {
        return Err(CoreError::invalid_input(
            "the token id does not match the approved subject",
        ));
    }
    // The gate this patch adds: the approved subject is the only target that may
    // be executed. `"10:repo"` does not match an approved `"10:repo:write"`,
    // even though `repo` would satisfy the semantic ladder below.
    let target = pat_scope::normalize_scopes(target_scopes)?;
    if request.subject != format!("{token_id}:{target}") {
        return Err(CoreError::invalid_input(format!(
            "target scopes do not match the approved target \"{approved_scopes}\""
        )));
    }

    let repo = repository::Entity::find_by_id(request.repo_id)
        .filter(RepositoryColumn::DeletedAt.is_null())
        .one(txn)
        .await
        .map_err(CoreError::internal)?
        .ok_or_else(|| CoreError::not_found("repository not found"))?;

    // Re-check the binding at execution time: the token must still be an active
    // agent token confined to this repository.
    let token = load_expandable_token(txn, &repo, token_id).await?;
    let (previous_scopes, scopes) = widening_of(&token.scopes, target_scopes)?;
    let effect_hash = effect_hash(request_id, token_id, &scopes);

    // The business write comes first, the consume second, both in the same
    // transaction. The CAS on the scopes value stops two concurrent expansions
    // from overwriting each other's starting point.
    let updated = access_token::Entity::update_many()
        .col_expr(access_token::Column::Scopes, Expr::value(scopes.clone()))
        .filter(access_token::Column::Id.eq(token_id))
        .filter(access_token::Column::Scopes.eq(previous_scopes.clone()))
        .exec(txn)
        .await
        .map_err(CoreError::internal)?;
    if updated.rows_affected != 1 {
        return Err(CoreError::coded(
            409,
            SCOPES_CHANGED,
            "the token's scopes changed after this expansion was approved",
        ));
    }

    consume(txn, request_id, actor_id, &effect_hash).await?;

    Ok(TokenScopeExpandResult {
        token_id,
        previous_scopes,
        scopes,
    })
}

/// Load `token_id` and prove it is a token this repository's administrators may
/// widen.
///
/// Requirements, each of which answers "could this approval widen something
/// outside the approving repository?":
///
/// 1. the token exists and has not expired (an expired token cannot be revived
///    by an approval);
/// 2. its owner is an active **agent** with an active human owner (only agent
///    tokens are in scope for this action);
/// 3. its `repo_allowlist` is exactly this repository, so the widening cannot
///    leak into another repository through the same token.
pub(crate) async fn load_expandable_token<C: ConnectionTrait>(
    conn: &C,
    repo: &Repository,
    token_id: i64,
) -> CoreResult<AccessToken> {
    let token = access_token::Entity::find_by_id(token_id)
        .one(conn)
        .await
        .map_err(CoreError::internal)?
        .ok_or_else(|| CoreError::not_found("token not found"))?;
    if token
        .expires_at
        .is_some_and(|expires_at| expires_at <= Utc::now())
    {
        return Err(CoreError::invalid_input(
            "this token has expired; an approval cannot revive it",
        ));
    }

    let agent = user_ops::find_by_id(conn, token.user_id)
        .await
        .map_err(CoreError::internal)?
        .ok_or_else(|| CoreError::not_found("token owner not found"))?;
    if !crate::agent::is_agent(&agent) {
        return Err(CoreError::invalid_input(
            "scope_expand only applies to an agent token",
        ));
    }
    if !agent.is_active {
        return Err(CoreError::invalid_input(
            "the agent that owns this token is not active",
        ));
    }
    let Some(owner_id) = agent.owner_user_id else {
        return Err(CoreError::invalid_input(
            "the agent that owns this token has no owner",
        ));
    };
    let Some(owner) = user_ops::find_by_id(conn, owner_id)
        .await
        .map_err(CoreError::internal)?
    else {
        return Err(CoreError::invalid_input(
            "the agent's owner no longer exists",
        ));
    };
    if !owner.is_active || crate::agent::is_agent(&owner) {
        return Err(CoreError::invalid_input(
            "the agent's owner is not an active person",
        ));
    }

    let Some(stored) = token.repo_allowlist.as_deref() else {
        return Err(CoreError::invalid_input(
            "scope_expand needs a token limited to this repository; create it with repos: [\"owner/name\"]",
        ));
    };
    let allow = pat_scope::parse_repo_allowlist(stored);
    let expected = repository_allowlist_entry(conn, repo).await?;
    if allow.len() != 1 || allow[0] != expected {
        return Err(CoreError::invalid_input(
            "scope_expand needs the token to be limited to exactly this repository",
        ));
    }

    Ok(token)
}

async fn repository_allowlist_entry<C: ConnectionTrait>(
    conn: &C,
    repo: &Repository,
) -> CoreResult<String> {
    let owner = user_ops::find_by_id(conn, repo.owner_id)
        .await
        .map_err(CoreError::internal)?
        .ok_or_else(|| CoreError::not_found("repository owner not found"))?;
    Ok(format!("{}/{}", owner.username, repo.name))
}

/// Return `(current, target)` in canonical order, after proving that `target`
/// contains every scope `current` has and actually adds something.
///
/// "Contains" is `pat_scope::has_scope`, the same ladder the middleware uses:
/// `repo:read` < `repo:collab` < `repo:write` < `repo`. In particular
/// `repo:write` satisfies `repo:collab` (verified against the current
/// implementation and its `repository_scopes_form_a_ladder` test), while
/// `repo:collab` does not satisfy `repo:write`.
pub(crate) fn widening_of(current: &str, target: &str) -> CoreResult<(String, String)> {
    let previous = pat_scope::normalize_scopes(current).map_err(|_| {
        CoreError::internal("the token has an unsupported stored scope".to_string())
    })?;
    let next = pat_scope::normalize_scopes(target)?;
    // An agent token may only ever hold repo-family scopes; this is what stops
    // an expansion from buying `user` or `admin`.
    crate::agent::validate_agent_token_scopes(&next)?;

    if !previous
        .split(',')
        .all(|scope| pat_scope::has_scope(&next, scope))
    {
        return Err(CoreError::invalid_input(
            "target scopes must include every scope the token already has; an expansion cannot narrow or move a token sideways",
        ));
    }
    if next == previous {
        return Err(CoreError::invalid_input(
            "target scopes are not wider than the token's current scopes",
        ));
    }
    Ok((previous, next))
}

/// Digest of the effect: the same `(request, token, target)` always produces the
/// same value, and a different widening produces a different one, so the
/// audit trail can compare executions without storing the target separately
/// (there is no `parameters` column in step 1).
fn effect_hash(request_id: i64, token_id: i64, target_scopes: &str) -> String {
    let canonical = format!("scope_expand:{request_id}:{token_id}:{target_scopes}");
    hex::encode(Sha256::digest(canonical.as_bytes()))
}

/// The applicant of a `scope_expand` is an agent, so the generic
/// `ensure_separate_people` (which compares a human applicant's id) never fires.
/// The person who may not approve is the one standing behind the agent: its
/// owner. Letting the owner approve their own agent's widening would be
/// self-approval by another name.
pub async fn ensure_decider_is_not_the_agent_owner(
    db: &DatabaseConnection,
    request: &rg_db::entities::authorization_request::Model,
    decider_id: i64,
) -> CoreResult<()> {
    // `evaluate` has already validated the subject as
    // `"<token id>:<scopes>"` by the time this runs. Parsing is still fallible
    // and fails closed: a subject this cannot read must not skip the owner check.
    let (token_id, _) = parse_subject(&request.subject)?;
    let Some(token) = access_token::Entity::find_by_id(token_id)
        .one(db)
        .await
        .map_err(CoreError::internal)?
    else {
        return Ok(());
    };
    let Some(agent) = user_ops::find_by_id(db, token.user_id)
        .await
        .map_err(CoreError::internal)?
    else {
        return Ok(());
    };
    if agent.owner_user_id == Some(decider_id) {
        return Err(CoreError::coded(
            403,
            SELF_APPROVAL,
            "the person behind the agent cannot approve its scope expansion",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_subject_is_a_token_id_then_canonical_scopes() {
        for (subject, token_id, scopes) in [
            ("10:repo:write", 10, "repo:write"),
            ("1:repo", 1, "repo"),
            ("42:repo:write,repo:read", 42, "repo:write,repo:read"),
        ] {
            assert_eq!(
                parse_subject(subject).unwrap(),
                (token_id, scopes.to_string())
            );
        }
    }

    #[test]
    fn a_malformed_subject_is_invalid_input() {
        for subject in [
            "",
            "10",
            "session",
            "10:",
            ":repo:write",
            "0:repo:write",
            "-3:repo:write",
            "007:repo:write",
            "1.5:repo:write",
            "+10:repo:write",
            "10:repo:write:extra",
            "10:not-a-scope",
            "10:user",
            "10:admin",
            "10:repo:read,repo:write",
        ] {
            let error = parse_subject(subject).unwrap_err();
            assert!(
                matches!(error, CoreError::InvalidInput(_)),
                "{subject:?} must be invalid input, got {error:?}"
            );
        }
    }
}
