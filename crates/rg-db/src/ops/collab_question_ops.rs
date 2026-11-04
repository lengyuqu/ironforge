//! Database operations for `collab_questions` and `collab_answers`.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use sea_orm::sea_query::Expr;
use sea_orm::*;

use super::collab_task_ops::CasOutcome;
use super::collab_notice_ops::{self, NewNotice};
use crate::entities::collab_answer::{self, Entity as AnswerEntity, Model as Answer};
use crate::entities::collab_question::{
    self, ActiveModel, Entity as QuestionEntity, Model as Question,
};

pub mod status {
    pub const OPEN: &str = "open";
    pub const ANSWERED: &str = "answered";
    pub const CANCELLED: &str = "cancelled";
}

#[derive(Debug, Clone, PartialEq)]
pub enum CreateOutcome {
    Created(Question),
    /// `(creator_kind, creator_id, client_question_id)` already exists.
    Existing(Question),
}

// Results are consumed immediately; boxing the rows would only add noise.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq)]
pub enum AnswerOutcome {
    Answered { question: Question, answer: Answer },
    /// Not `open` any more or version moved; `current` is the winner's state.
    Conflict { current: Question },
    NotFound,
}

pub async fn find_by_client_id<C: ConnectionTrait>(
    db: &C,
    creator_kind: &str,
    creator_id: &str,
    client_question_id: &str,
) -> Result<Option<Question>> {
    QuestionEntity::find()
        .filter(collab_question::Column::CreatorKind.eq(creator_kind))
        .filter(collab_question::Column::CreatorId.eq(creator_id))
        .filter(collab_question::Column::ClientQuestionId.eq(client_question_id))
        .one(db)
        .await
        .context("db: find question by client id")
}

pub async fn create_idempotent(
    db: &DatabaseConnection,
    model: ActiveModel,
    creator_kind: &str,
    creator_id: &str,
    client_question_id: &str,
) -> Result<CreateOutcome> {
    create_idempotent_with_notices(db, model, creator_kind, creator_id, client_question_id, &[])
        .await
}

/// [`create_idempotent`] that also writes `notices` in the question's
/// transaction (none for a replay).
pub async fn create_idempotent_with_notices(
    db: &DatabaseConnection,
    model: ActiveModel,
    creator_kind: &str,
    creator_id: &str,
    client_question_id: &str,
    notices: &[NewNotice],
) -> Result<CreateOutcome> {
    if let Some(existing) =
        find_by_client_id(db, creator_kind, creator_id, client_question_id).await?
    {
        return Ok(CreateOutcome::Existing(existing));
    }
    let txn = db.begin().await.context("db: begin create question")?;
    match model.insert(&txn).await {
        Ok(created) => {
            collab_notice_ops::insert_all(&txn, notices).await?;
            txn.commit().await.context("db: commit create question")?;
            Ok(CreateOutcome::Created(created))
        }
        Err(err) if matches!(err.sql_err(), Some(SqlErr::UniqueConstraintViolation(_))) => {
            drop(txn);
            let existing = find_by_client_id(db, creator_kind, creator_id, client_question_id)
                .await?
                .context("db: question idempotency key vanished after conflict")?;
            Ok(CreateOutcome::Existing(existing))
        }
        Err(err) => Err(err).context("db: create question"),
    }
}

pub async fn find_in_repo<C: ConnectionTrait>(
    db: &C,
    repo_id: i64,
    id: &str,
) -> Result<Option<Question>> {
    QuestionEntity::find_by_id(id.to_string())
        .filter(collab_question::Column::RepoId.eq(repo_id))
        .one(db)
        .await
        .context("db: find question")
}

pub async fn find_answer<C: ConnectionTrait>(db: &C, question_id: &str) -> Result<Option<Answer>> {
    AnswerEntity::find()
        .filter(collab_answer::Column::QuestionId.eq(question_id))
        .one(db)
        .await
        .context("db: find answer")
}

/// Keyset page of a task's questions ordered by `(created_at, id)` ascending.
pub async fn list_page<C: ConnectionTrait>(
    db: &C,
    repo_id: i64,
    task_id: &str,
    status: Option<&str>,
    after: Option<(DateTime<Utc>, String)>,
    limit: u64,
) -> Result<Vec<Question>> {
    let mut query = QuestionEntity::find()
        .filter(collab_question::Column::RepoId.eq(repo_id))
        .filter(collab_question::Column::TaskId.eq(task_id));
    if let Some(status) = status {
        query = query.filter(collab_question::Column::Status.eq(status));
    }
    if let Some((created, id)) = after {
        query = query.filter(
            Condition::any()
                .add(collab_question::Column::CreatedAt.gt(created))
                .add(
                    Condition::all()
                        .add(collab_question::Column::CreatedAt.eq(created))
                        .add(collab_question::Column::Id.gt(id)),
                ),
        );
    }
    query
        .order_by_asc(collab_question::Column::CreatedAt)
        .order_by_asc(collab_question::Column::Id)
        .limit(limit)
        .all(db)
        .await
        .context("db: list questions")
}

/// Open questions of a repository, oldest first, for inbox projections. The
/// caller filters by recipient in `condition`.
pub async fn list_open(
    db: &DatabaseConnection,
    repo_id: i64,
    condition: Condition,
    limit: u64,
) -> Result<Vec<Question>> {
    QuestionEntity::find()
        .filter(collab_question::Column::RepoId.eq(repo_id))
        .filter(collab_question::Column::Status.eq(status::OPEN))
        .filter(condition)
        .order_by_asc(collab_question::Column::CreatedAt)
        .order_by_asc(collab_question::Column::Id)
        .limit(limit)
        .all(db)
        .await
        .context("db: list open questions")
}

/// Answered questions created by one actor, newest answer first.
pub async fn list_answered_by_creator(
    db: &DatabaseConnection,
    repo_id: i64,
    creator_kind: &str,
    creator_id: &str,
    limit: u64,
) -> Result<Vec<Question>> {
    QuestionEntity::find()
        .filter(collab_question::Column::RepoId.eq(repo_id))
        .filter(collab_question::Column::CreatorKind.eq(creator_kind))
        .filter(collab_question::Column::CreatorId.eq(creator_id))
        .filter(collab_question::Column::Status.eq(status::ANSWERED))
        .order_by_desc(collab_question::Column::UpdatedAt)
        .order_by_desc(collab_question::Column::Id)
        .limit(limit)
        .all(db)
        .await
        .context("db: list answered questions")
}

/// `open -> answered` iff the question is still `open` at `expected_version`,
/// inserting the answer in the same transaction. Racing answers and cancels
/// share this one conditional `UPDATE`, so exactly one terminal state wins.
pub async fn answer_cas(
    db: &DatabaseConnection,
    repo_id: i64,
    question_id: &str,
    expected_version: i64,
    answer: collab_answer::ActiveModel,
    now: DateTime<Utc>,
) -> Result<AnswerOutcome> {
    answer_cas_with_notices(db, repo_id, question_id, expected_version, answer, now, &[]).await
}

/// [`answer_cas`] that also writes `notices` in the answering transaction, so
/// only the winner of an answer race emits them.
pub async fn answer_cas_with_notices(
    db: &DatabaseConnection,
    repo_id: i64,
    question_id: &str,
    expected_version: i64,
    answer: collab_answer::ActiveModel,
    now: DateTime<Utc>,
    notices: &[NewNotice],
) -> Result<AnswerOutcome> {
    let txn = db.begin().await.context("db: begin answer")?;
    let res = QuestionEntity::update_many()
        .col_expr(collab_question::Column::Status, Expr::value(status::ANSWERED))
        .col_expr(collab_question::Column::UpdatedAt, Expr::value(now))
        .col_expr(
            collab_question::Column::Version,
            Expr::col(collab_question::Column::Version).add(1),
        )
        .filter(collab_question::Column::Id.eq(question_id))
        .filter(collab_question::Column::RepoId.eq(repo_id))
        .filter(collab_question::Column::Version.eq(expected_version))
        .filter(collab_question::Column::Status.eq(status::OPEN))
        .exec(&txn)
        .await
        .context("db: mark question answered")?;
    let current = find_in_repo(&txn, repo_id, question_id).await?;
    let Some(current) = current else {
        txn.commit().await.context("db: commit answer")?;
        return Ok(AnswerOutcome::NotFound);
    };
    if res.rows_affected == 0 {
        txn.commit().await.context("db: commit answer")?;
        return Ok(AnswerOutcome::Conflict { current });
    }
    let answer = answer.insert(&txn).await.context("db: insert answer")?;
    collab_notice_ops::insert_all(&txn, notices).await?;
    txn.commit().await.context("db: commit answer")?;
    Ok(AnswerOutcome::Answered {
        question: current,
        answer,
    })
}

/// `open -> cancelled` iff still `open` at `expected_version`.
pub async fn cancel_cas(
    db: &DatabaseConnection,
    repo_id: i64,
    question_id: &str,
    expected_version: i64,
    now: DateTime<Utc>,
) -> Result<CasOutcome<Question>> {
    let txn = db.begin().await.context("db: begin cancel")?;
    let res = QuestionEntity::update_many()
        .col_expr(collab_question::Column::Status, Expr::value(status::CANCELLED))
        .col_expr(collab_question::Column::UpdatedAt, Expr::value(now))
        .col_expr(
            collab_question::Column::Version,
            Expr::col(collab_question::Column::Version).add(1),
        )
        .filter(collab_question::Column::Id.eq(question_id))
        .filter(collab_question::Column::RepoId.eq(repo_id))
        .filter(collab_question::Column::Version.eq(expected_version))
        .filter(collab_question::Column::Status.eq(status::OPEN))
        .exec(&txn)
        .await
        .context("db: cancel question")?;
    let current = find_in_repo(&txn, repo_id, question_id).await?;
    txn.commit().await.context("db: commit cancel")?;
    Ok(match (res.rows_affected, current) {
        (_, None) => CasOutcome::NotFound,
        (0, Some(current)) => CasOutcome::Conflict { current },
        (_, Some(updated)) => CasOutcome::Updated(updated),
    })
}
