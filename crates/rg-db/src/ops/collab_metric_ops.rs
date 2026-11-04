//! Database operations for `collab_metric_events`.
//!
//! The business transactions (task create/update, handoff, question, takeover)
//! call [`record`] on their own transaction handle, so an event exists exactly
//! when the change it describes was committed. Time is always the server's.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use sea_orm::sea_query::Expr;
use sea_orm::*;

use crate::entities::collab_handoff::{self, Entity as HandoffEntity};
use crate::entities::collab_metric_event::{self, ActiveModel, Entity as EventEntity, Model as Event};
use crate::entities::collab_question::{self, Entity as QuestionEntity};
use crate::entities::collab_task::{self, Entity as TaskEntity};


/// Values of `event_type`.
pub mod event_type {
    pub const TASK_CREATED: &str = "task_created";
    pub const TASK_STATUS_CHANGED: &str = "task_status_changed";
    pub const TASK_STATUS_OBSERVED: &str = "task_status_observed";
    pub const TAKEOVER: &str = "takeover";
    pub const HANDOFF_CREATED: &str = "handoff_created";
    pub const HANDOFF_ACCEPTED: &str = "handoff_accepted";
    pub const HANDOFF_CANCELLED: &str = "handoff_cancelled";
    pub const HANDOFF_OBSERVED_PENDING: &str = "handoff_observed_pending";
    pub const QUESTION_OPENED: &str = "question_opened";
    pub const QUESTION_ANSWERED: &str = "question_answered";
    pub const QUESTION_CANCELLED: &str = "question_cancelled";
    pub const QUESTION_OBSERVED_OPEN: &str = "question_observed_open";
}

/// Values of `source_type`.
pub mod source_type {
    pub const TASK: &str = "task";
    pub const HANDOFF: &str = "handoff";
    pub const QUESTION: &str = "question";
    pub const TAKEOVER: &str = "takeover";
    /// One-off observation baseline; never a statement about earlier history.
    pub const BASELINE: &str = "baseline_snapshot";
}

/// Upper bound on events one health computation will load.
pub const MAX_EVENTS_PER_COMPUTATION: u64 = 200_000;

#[derive(Debug, Clone)]
pub struct NewEvent<'a> {
    pub repo_id: i64,
    pub task_id: &'a str,
    pub event_type: &'a str,
    pub from_state: Option<&'a str>,
    pub to_state: Option<&'a str>,
    pub task_version: Option<i64>,
    pub source_type: &'a str,
    pub source_id: String,
    pub occurred_at: DateTime<Utc>,
}

/// Insert an event on `db` (normally the caller's transaction).
///
/// `occurred_at` is raised to the task's newest event time if that is later:
/// handlers read the clock before they commit, so two racing changes could
/// otherwise be stored in an order that disagrees with their commit order and
/// yield negative durations.
pub async fn record<C: ConnectionTrait>(db: &C, event: NewEvent<'_>) -> Result<()> {
    let newest = EventEntity::find()
        .filter(collab_metric_event::Column::TaskId.eq(event.task_id))
        .filter(collab_metric_event::Column::RepoId.eq(event.repo_id))
        .order_by_desc(collab_metric_event::Column::OccurredAt)
        .one(db)
        .await
        .context("db: read newest metric event")?
        .map(|e| e.occurred_at);
    let at = if db.get_database_backend() == DatabaseBackend::MySql {
        // MySQL rounds fractional seconds on writes to TIMESTAMP(0). Creation
        // hooks may receive an already rounded-up object timestamp; it cannot
        // have happened after the server clock at the time we record the event.
        let at = event.occurred_at.min(Utc::now());
        DateTime::from_timestamp(at.timestamp(), 0).unwrap_or(at)
    } else {
        event.occurred_at
    };
    let occurred_at = newest.map_or(at, |n| n.max(at));
    ActiveModel {
        id: Set(uuid::Uuid::new_v4().to_string()),
        repo_id: Set(event.repo_id),
        task_id: Set(event.task_id.to_string()),
        event_type: Set(event.event_type.to_string()),
        from_state: Set(event.from_state.map(str::to_string)),
        to_state: Set(event.to_state.map(str::to_string)),
        task_version: Set(event.task_version),
        source_type: Set(event.source_type.to_string()),
        source_id: Set(event.source_id),
        occurred_at: Set(occurred_at),
        recorded_at: Set(Utc::now()),
    }
    .insert(db)
    .await
    .context("db: record collaboration metric event")?;
    Ok(())
}

/// Earliest event time of the repository (`None` = nothing recorded yet).
pub async fn first_occurred_at<C: ConnectionTrait>(
    db: &C,
    repo_id: i64,
) -> Result<Option<DateTime<Utc>>> {
    Ok(EventEntity::find()
        .filter(collab_metric_event::Column::RepoId.eq(repo_id))
        .order_by_asc(collab_metric_event::Column::OccurredAt)
        .one(db)
        .await
        .context("db: first metric event")?
        .map(|e| e.occurred_at))
}

/// Time of the observation baseline, if one was taken.
pub async fn baseline_taken_at<C: ConnectionTrait>(
    db: &C,
    repo_id: i64,
) -> Result<Option<DateTime<Utc>>> {
    Ok(EventEntity::find()
        .filter(collab_metric_event::Column::RepoId.eq(repo_id))
        .filter(collab_metric_event::Column::SourceType.eq(source_type::BASELINE))
        .order_by_asc(collab_metric_event::Column::OccurredAt)
        .one(db)
        .await
        .context("db: baseline metric event")?
        .map(|e| e.occurred_at))
}

/// Events strictly before `before`, oldest first. `None` = more than
/// [`MAX_EVENTS_PER_COMPUTATION`] exist.
pub async fn load_before<C: ConnectionTrait>(
    db: &C,
    repo_id: i64,
    before: DateTime<Utc>,
) -> Result<Option<Vec<Event>>> {
    let rows = EventEntity::find()
        .filter(collab_metric_event::Column::RepoId.eq(repo_id))
        .filter(collab_metric_event::Column::OccurredAt.lt(before))
        .order_by_asc(collab_metric_event::Column::OccurredAt)
        .order_by_asc(collab_metric_event::Column::Id)
        .limit(MAX_EVENTS_PER_COMPUTATION + 1)
        .all(db)
        .await
        .context("db: load metric events")?;
    Ok((rows.len() as u64 <= MAX_EVENTS_PER_COMPUTATION).then_some(rows))
}

/// `(id, created_at)` of every task created before `before`.
pub async fn task_ids_created_before<C: ConnectionTrait>(
    db: &C,
    repo_id: i64,
    before: DateTime<Utc>,
) -> Result<Vec<(String, DateTime<Utc>)>> {
    TaskEntity::find()
        .select_only()
        .column(collab_task::Column::Id)
        .column(collab_task::Column::CreatedAt)
        .filter(collab_task::Column::RepoId.eq(repo_id))
        .filter(collab_task::Column::CreatedAt.lt(before))
        .into_tuple()
        .all(db)
        .await
        .context("db: list task ids")
}

/// `(id, task_id, created_at, status)` of every handoff created before `before`.
pub async fn handoff_ids_created_before<C: ConnectionTrait>(
    db: &C,
    repo_id: i64,
    before: DateTime<Utc>,
) -> Result<Vec<(String, String, DateTime<Utc>, String)>> {
    HandoffEntity::find()
        .select_only()
        .column(collab_handoff::Column::Id)
        .column(collab_handoff::Column::TaskId)
        .column(collab_handoff::Column::CreatedAt)
        .column(collab_handoff::Column::Status)
        .filter(collab_handoff::Column::RepoId.eq(repo_id))
        .filter(collab_handoff::Column::CreatedAt.lt(before))
        .into_tuple()
        .all(db)
        .await
        .context("db: list handoff ids")
}

/// `(id, task_id, created_at, status)` of every question created before `before`.
pub async fn question_ids_created_before<C: ConnectionTrait>(
    db: &C,
    repo_id: i64,
    before: DateTime<Utc>,
) -> Result<Vec<(String, String, DateTime<Utc>, String)>> {
    QuestionEntity::find()
        .select_only()
        .column(collab_question::Column::Id)
        .column(collab_question::Column::TaskId)
        .column(collab_question::Column::CreatedAt)
        .column(collab_question::Column::Status)
        .filter(collab_question::Column::RepoId.eq(repo_id))
        .filter(collab_question::Column::CreatedAt.lt(before))
        .into_tuple()
        .all(db)
        .await
        .context("db: list question ids")
}

/// `(id, title, status)` of the given tasks of a repository.
pub async fn task_titles<C: ConnectionTrait>(
    db: &C,
    repo_id: i64,
    ids: &[String],
) -> Result<Vec<(String, String, String)>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    TaskEntity::find()
        .select_only()
        .column(collab_task::Column::Id)
        .column(collab_task::Column::Title)
        .column(collab_task::Column::Status)
        .filter(collab_task::Column::RepoId.eq(repo_id))
        .filter(collab_task::Column::Id.is_in(ids.iter().cloned()))
        .into_tuple()
        .all(db)
        .await
        .context("db: list task titles")
}

/// What one [`take_baseline`] call added.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BaselineCounts {
    pub tasks: u64,
    pub handoffs: u64,
    pub questions: u64,
}

async fn has_event_for_task<C: ConnectionTrait>(db: &C, repo_id: i64, task_id: &str) -> Result<bool> {
    Ok(EventEntity::find()
        .filter(collab_metric_event::Column::RepoId.eq(repo_id))
        .filter(collab_metric_event::Column::TaskId.eq(task_id))
        .filter(collab_metric_event::Column::SourceType.ne(source_type::BASELINE))
        .filter(collab_metric_event::Column::EventType.is_in([
            event_type::TASK_CREATED,
            event_type::TASK_STATUS_CHANGED,
        ]))
        .one(db)
        .await
        .context("db: task event lookup")?
        .is_some()
        || EventEntity::find()
            .filter(collab_metric_event::Column::RepoId.eq(repo_id))
            .filter(collab_metric_event::Column::SourceType.eq(source_type::BASELINE))
            .filter(collab_metric_event::Column::SourceId.eq(task_id))
            .one(db)
            .await
            .context("db: task baseline lookup")?
            .is_some())
}

async fn has_event_for_object<C: ConnectionTrait>(
    db: &C,
    repo_id: i64,
    kind: &str,
    object_id: &str,
) -> Result<bool> {
    Ok(EventEntity::find()
        .filter(collab_metric_event::Column::RepoId.eq(repo_id))
        .filter(collab_metric_event::Column::SourceType.is_in([kind, source_type::BASELINE]))
        .filter(collab_metric_event::Column::SourceId.eq(object_id))
        .one(db)
        .await
        .context("db: object event lookup")?
        .is_some())
}

/// Record the current state of every object that has no events yet as
/// "observed at `now`".
///
/// This is an observation start, not history: a task observed `blocked` is
/// `blocked since at least now`, and a pending handoff or open question waits
/// since `now` at the earliest. Objects that already have events keep their
/// exact timeline and are skipped, so repeating the call adds nothing for them.
/// Each object is handled in its own transaction whose first statement locks
/// the object's row, so a concurrent change either happened before (its event
/// exists, the object is skipped) or happens after (its event follows the
/// baseline).
pub async fn take_baseline(
    db: &DatabaseConnection,
    repo_id: i64,
    now: DateTime<Utc>,
) -> Result<BaselineCounts> {
    let mut counts = BaselineCounts::default();

    let task_ids: Vec<String> = TaskEntity::find()
        .select_only()
        .column(collab_task::Column::Id)
        .filter(collab_task::Column::RepoId.eq(repo_id))
        .into_tuple()
        .all(db)
        .await
        .context("db: baseline list tasks")?;
    for id in task_ids {
        let txn = db.begin().await.context("db: begin task baseline")?;
        TaskEntity::update_many()
            .col_expr(collab_task::Column::UpdatedAt, Expr::col(collab_task::Column::UpdatedAt).into())
            .filter(collab_task::Column::Id.eq(id.as_str()))
            .filter(collab_task::Column::RepoId.eq(repo_id))
            .exec(&txn)
            .await
            .context("db: lock task for baseline")?;
        let task = TaskEntity::find_by_id(id.clone()).one(&txn).await.context("db: baseline task")?;
        if let Some(task) = task {
            if !has_event_for_task(&txn, repo_id, &task.id).await? {
                record(
                    &txn,
                    NewEvent {
                        repo_id,
                        task_id: &task.id,
                        event_type: event_type::TASK_STATUS_OBSERVED,
                        from_state: None,
                        to_state: Some(&task.status),
                        task_version: Some(task.version),
                        source_type: source_type::BASELINE,
                        source_id: task.id.clone(),
                        occurred_at: now,
                    },
                )
                .await?;
                counts.tasks += 1;
            }
        }
        txn.commit().await.context("db: commit task baseline")?;
    }

    let handoff_ids: Vec<String> = HandoffEntity::find()
        .select_only()
        .column(collab_handoff::Column::Id)
        .filter(collab_handoff::Column::RepoId.eq(repo_id))
        .filter(collab_handoff::Column::Status.eq("pending"))
        .into_tuple()
        .all(db)
        .await
        .context("db: baseline list handoffs")?;
    for id in handoff_ids {
        let txn = db.begin().await.context("db: begin handoff baseline")?;
        HandoffEntity::update_many()
            .col_expr(collab_handoff::Column::Version, Expr::col(collab_handoff::Column::Version).into())
            .filter(collab_handoff::Column::Id.eq(id.as_str()))
            .filter(collab_handoff::Column::RepoId.eq(repo_id))
            .exec(&txn)
            .await
            .context("db: lock handoff for baseline")?;
        let handoff = HandoffEntity::find_by_id(id.clone())
            .one(&txn)
            .await
            .context("db: baseline handoff")?;
        if let Some(h) = handoff.filter(|h| h.status == "pending") {
            if !has_event_for_object(&txn, repo_id, source_type::HANDOFF, &h.id).await? {
                record(
                    &txn,
                    NewEvent {
                        repo_id,
                        task_id: &h.task_id,
                        event_type: event_type::HANDOFF_OBSERVED_PENDING,
                        from_state: None,
                        to_state: Some("pending"),
                        task_version: None,
                        source_type: source_type::BASELINE,
                        source_id: h.id.clone(),
                        occurred_at: now,
                    },
                )
                .await?;
                counts.handoffs += 1;
            }
        }
        txn.commit().await.context("db: commit handoff baseline")?;
    }

    let question_ids: Vec<String> = QuestionEntity::find()
        .select_only()
        .column(collab_question::Column::Id)
        .filter(collab_question::Column::RepoId.eq(repo_id))
        .filter(collab_question::Column::Status.eq("open"))
        .into_tuple()
        .all(db)
        .await
        .context("db: baseline list questions")?;
    for id in question_ids {
        let txn = db.begin().await.context("db: begin question baseline")?;
        QuestionEntity::update_many()
            .col_expr(collab_question::Column::Version, Expr::col(collab_question::Column::Version).into())
            .filter(collab_question::Column::Id.eq(id.as_str()))
            .filter(collab_question::Column::RepoId.eq(repo_id))
            .exec(&txn)
            .await
            .context("db: lock question for baseline")?;
        let question = QuestionEntity::find_by_id(id.clone())
            .one(&txn)
            .await
            .context("db: baseline question")?;
        if let Some(q) = question.filter(|q| q.status == "open") {
            if !has_event_for_object(&txn, repo_id, source_type::QUESTION, &q.id).await? {
                record(
                    &txn,
                    NewEvent {
                        repo_id,
                        task_id: &q.task_id,
                        event_type: event_type::QUESTION_OBSERVED_OPEN,
                        from_state: None,
                        to_state: Some("open"),
                        task_version: None,
                        source_type: source_type::BASELINE,
                        source_id: q.id.clone(),
                        occurred_at: now,
                    },
                )
                .await?;
                counts.questions += 1;
            }
        }
        txn.commit().await.context("db: commit question baseline")?;
    }
    Ok(counts)
}

// ------------------------------------------------------------------ hooks
// One-line entry points for the business transactions.

pub async fn record_task_created<C: ConnectionTrait>(
    db: &C,
    task: &collab_task::Model,
) -> Result<()> {
    record(
        db,
        NewEvent {
            repo_id: task.repo_id,
            task_id: &task.id,
            event_type: event_type::TASK_CREATED,
            from_state: None,
            to_state: Some(&task.status),
            task_version: Some(task.version),
            source_type: source_type::TASK,
            source_id: task.id.clone(),
            occurred_at: task.created_at,
        },
    )
    .await
}

/// A committed status change. Unique per `(task, new version)`.
pub async fn record_task_status_change<C: ConnectionTrait>(
    db: &C,
    task: &collab_task::Model,
    from_status: &str,
    at: DateTime<Utc>,
) -> Result<()> {
    record(
        db,
        NewEvent {
            repo_id: task.repo_id,
            task_id: &task.id,
            event_type: event_type::TASK_STATUS_CHANGED,
            from_state: Some(from_status),
            to_state: Some(&task.status),
            task_version: Some(task.version),
            source_type: source_type::TASK,
            source_id: format!("{}:{}", task.id, task.version),
            occurred_at: at,
        },
    )
    .await
}

pub async fn record_takeover<C: ConnectionTrait>(
    db: &C,
    repo_id: i64,
    task_id: &str,
    takeover_id: &str,
    outcome: &str,
    at: DateTime<Utc>,
) -> Result<()> {
    record(
        db,
        NewEvent {
            repo_id,
            task_id,
            event_type: event_type::TAKEOVER,
            from_state: None,
            to_state: Some(outcome),
            task_version: None,
            source_type: source_type::TAKEOVER,
            source_id: takeover_id.to_string(),
            occurred_at: at,
        },
    )
    .await
}

/// Handoff lifecycle event: `event` is one of the `HANDOFF_*` types.
pub async fn record_handoff<C: ConnectionTrait>(
    db: &C,
    handoff: &collab_handoff::Model,
    event: &str,
    from_status: Option<&str>,
    at: DateTime<Utc>,
) -> Result<()> {
    record(
        db,
        NewEvent {
            repo_id: handoff.repo_id,
            task_id: &handoff.task_id,
            event_type: event,
            from_state: from_status,
            to_state: Some(&handoff.status),
            task_version: None,
            source_type: source_type::HANDOFF,
            source_id: handoff.id.clone(),
            occurred_at: at,
        },
    )
    .await
}

/// Question lifecycle event: `event` is one of the `QUESTION_*` types.
pub async fn record_question<C: ConnectionTrait>(
    db: &C,
    question: &collab_question::Model,
    event: &str,
    from_status: Option<&str>,
    at: DateTime<Utc>,
) -> Result<()> {
    record(
        db,
        NewEvent {
            repo_id: question.repo_id,
            task_id: &question.task_id,
            event_type: event,
            from_state: from_status,
            to_state: Some(&question.status),
            task_version: None,
            source_type: source_type::QUESTION,
            source_id: question.id.clone(),
            occurred_at: at,
        },
    )
    .await
}
