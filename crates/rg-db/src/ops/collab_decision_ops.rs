//! Database operations for `collab_decisions` and `collab_decision_changes`.
//!
//! Decisions are never updated. A terminal change is a plain insert; the
//! unique key on `decision_id` decides which of several racing writers wins, so
//! no transaction (and no read-before-write inside one) is needed.

use anyhow::{Context, Result};
use sea_orm::*;

use crate::entities::collab_answer::{Entity as AnswerEntity, Model as Answer};
use crate::entities::collab_decision::{
    self, ActiveModel, Entity as DecisionEntity, Model as Decision,
};
use crate::entities::collab_decision_change::{
    self, ActiveModel as ChangeActive, Entity as ChangeEntity, Model as Change,
};

pub mod kind {
    pub const SUPERSEDED: &str = "superseded";
    pub const REVOKED: &str = "revoked";
}

#[derive(Debug, Clone, PartialEq)]
pub enum CreateOutcome {
    Created(Decision),
    /// `(actor_kind, actor_id, client_decision_id)` already exists.
    Existing(Decision),
}

#[derive(Debug, Clone, PartialEq)]
pub enum ChangeOutcome {
    Created(Change),
    /// The change idempotency key already owns a row; compare `payload_hash`.
    Existing(Change),
    /// The decision already has its terminal change; this is the winner's.
    AlreadyChanged(Change),
}

pub async fn find_by_client_id<C: ConnectionTrait>(
    db: &C,
    actor_kind: &str,
    actor_id: &str,
    client_decision_id: &str,
) -> Result<Option<Decision>> {
    DecisionEntity::find()
        .filter(collab_decision::Column::ActorKind.eq(actor_kind))
        .filter(collab_decision::Column::ActorId.eq(actor_id))
        .filter(collab_decision::Column::ClientDecisionId.eq(client_decision_id))
        .one(db)
        .await
        .context("db: find decision by client id")
}

/// Highest `seq` of the task (0 when it has no decision yet).
async fn max_seq<C: ConnectionTrait>(db: &C, task_id: &str) -> Result<i64> {
    Ok(DecisionEntity::find()
        .filter(collab_decision::Column::TaskId.eq(task_id))
        .order_by_desc(collab_decision::Column::Seq)
        .one(db)
        .await
        .context("db: max decision seq")?
        .map_or(0, |d| d.seq))
}

/// Insert `make(seq)` as the task's next decision.
///
/// `seq` is `max + 1`; two concurrent writers may compute the same one and the
/// `(task_id, seq)` unique key rejects the loser, who retries with a fresh
/// maximum. The same client key racing with itself ends in
/// [`CreateOutcome::Existing`].
pub async fn create_next_seq(
    db: &DatabaseConnection,
    task_id: &str,
    actor_kind: &str,
    actor_id: &str,
    client_decision_id: &str,
    make: impl Fn(i64) -> ActiveModel,
) -> Result<CreateOutcome> {
    for _ in 0..8 {
        if let Some(existing) =
            find_by_client_id(db, actor_kind, actor_id, client_decision_id).await?
        {
            return Ok(CreateOutcome::Existing(existing));
        }
        let seq = max_seq(db, task_id).await? + 1;
        match make(seq).insert(db).await {
            Ok(created) => return Ok(CreateOutcome::Created(created)),
            Err(err) if matches!(err.sql_err(), Some(SqlErr::UniqueConstraintViolation(_))) => {
                continue;
            }
            Err(err) => return Err(err).context("db: create decision"),
        }
    }
    anyhow::bail!("db: could not allocate a decision sequence number under contention")
}

pub async fn find_in_repo<C: ConnectionTrait>(
    db: &C,
    repo_id: i64,
    id: &str,
) -> Result<Option<Decision>> {
    DecisionEntity::find_by_id(id.to_string())
        .filter(collab_decision::Column::RepoId.eq(repo_id))
        .one(db)
        .await
        .context("db: find decision")
}

pub async fn find_answer_by_id<C: ConnectionTrait>(db: &C, id: &str) -> Result<Option<Answer>> {
    AnswerEntity::find_by_id(id.to_string())
        .one(db)
        .await
        .context("db: find answer by id")
}

/// Page of a task's decisions ordered by `seq` ascending, after `after_seq`.
pub async fn list_page<C: ConnectionTrait>(
    db: &C,
    repo_id: i64,
    task_id: &str,
    after_seq: Option<i64>,
    limit: u64,
) -> Result<Vec<Decision>> {
    let mut query = DecisionEntity::find()
        .filter(collab_decision::Column::RepoId.eq(repo_id))
        .filter(collab_decision::Column::TaskId.eq(task_id));
    if let Some(seq) = after_seq {
        query = query.filter(collab_decision::Column::Seq.gt(seq));
    }
    query
        .order_by_asc(collab_decision::Column::Seq)
        .limit(limit)
        .all(db)
        .await
        .context("db: list decisions")
}

pub async fn find_change_by_client_id<C: ConnectionTrait>(
    db: &C,
    actor_kind: &str,
    actor_id: &str,
    client_change_id: &str,
) -> Result<Option<Change>> {
    ChangeEntity::find()
        .filter(collab_decision_change::Column::ActorKind.eq(actor_kind))
        .filter(collab_decision_change::Column::ActorId.eq(actor_id))
        .filter(collab_decision_change::Column::ClientChangeId.eq(client_change_id))
        .one(db)
        .await
        .context("db: find decision change by client id")
}

/// The terminal change of a decision, if it has one.
pub async fn find_change_for<C: ConnectionTrait>(
    db: &C,
    decision_id: &str,
) -> Result<Option<Change>> {
    ChangeEntity::find()
        .filter(collab_decision_change::Column::DecisionId.eq(decision_id))
        .one(db)
        .await
        .context("db: find decision change")
}

/// Terminal changes of several decisions at once (one page of a listing).
pub async fn list_changes_for<C: ConnectionTrait>(
    db: &C,
    decision_ids: Vec<String>,
) -> Result<Vec<Change>> {
    if decision_ids.is_empty() {
        return Ok(Vec::new());
    }
    ChangeEntity::find()
        .filter(collab_decision_change::Column::DecisionId.is_in(decision_ids))
        .all(db)
        .await
        .context("db: list decision changes")
}

/// Changes that name `decision_id` as their replacement, oldest first.
pub async fn list_superseded_by<C: ConnectionTrait>(
    db: &C,
    decision_id: &str,
) -> Result<Vec<Change>> {
    ChangeEntity::find()
        .filter(collab_decision_change::Column::ReplacementDecisionId.eq(decision_id))
        .order_by_asc(collab_decision_change::Column::CreatedAt)
        .order_by_asc(collab_decision_change::Column::Id)
        .all(db)
        .await
        .context("db: list superseded decisions")
}

/// Append the terminal change of a decision. At most one can exist; racing
/// writers end in exactly one [`ChangeOutcome::Created`].
pub async fn insert_change(
    db: &DatabaseConnection,
    model: ChangeActive,
    decision_id: &str,
    actor_kind: &str,
    actor_id: &str,
    client_change_id: &str,
) -> Result<ChangeOutcome> {
    if let Some(existing) =
        find_change_by_client_id(db, actor_kind, actor_id, client_change_id).await?
    {
        return Ok(ChangeOutcome::Existing(existing));
    }
    if let Some(winner) = find_change_for(db, decision_id).await? {
        return Ok(ChangeOutcome::AlreadyChanged(winner));
    }
    match model.insert(db).await {
        Ok(created) => Ok(ChangeOutcome::Created(created)),
        Err(err) if matches!(err.sql_err(), Some(SqlErr::UniqueConstraintViolation(_))) => {
            // Either this very request raced with its own retry, or another
            // writer took the decision's one terminal slot.
            if let Some(existing) =
                find_change_by_client_id(db, actor_kind, actor_id, client_change_id).await?
            {
                return Ok(ChangeOutcome::Existing(existing));
            }
            let winner = find_change_for(db, decision_id)
                .await?
                .context("db: decision change vanished after conflict")?;
            Ok(ChangeOutcome::AlreadyChanged(winner))
        }
        Err(err) => Err(err).context("db: insert decision change"),
    }
}
