//! Questions and answers: a structured request for a decision.
//!
//! A question has a recipient scope, optional fixed options and the task/commit
//! context it was asked in. At most one answer exists, written in the same
//! transaction that closes the question, so racing answers and cancels leave
//! exactly one terminal state. An answer is a collaboration decision: it is not
//! a review, an approval of a merge, or proof that a human wrote it.

use chrono::{DateTime, Utc};
use rg_db::entities::{collab_answer, collab_question, collab_task};
use rg_db::ops::collab_question_ops::{self, status, AnswerOutcome, CreateOutcome};
use rg_db::ops::collab_task_ops::CasOutcome;
use rg_db::ops::agent_session_ops;
use sea_orm::{DatabaseConnection, Set};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::cursor::{self, KeysetPosition};
use super::error::{CollabError, CollabResult};
use super::message::normalize_commit_sha;
use super::{task, validation, Actor, Page, ORIGIN_USER_CONTENT};

pub const MAX_TITLE_CHARS: usize = 200;
pub const MAX_BODY_BYTES: usize = 16 * 1024;
pub const MAX_OPTIONS: usize = 10;
pub const MAX_OPTION_LABEL_CHARS: usize = 200;

pub const RECIPIENT_REPO_WRITERS: &str = "repo_writers";
pub const RECIPIENT_USER: &str = "user";
pub const RECIPIENT_SESSION: &str = "session";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Recipient {
    pub kind: String,
    pub id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Context {
    /// Task version the question was asked at.
    pub task_version: i64,
    pub base_sha: Option<String>,
    pub head_sha: Option<String>,
}

#[derive(Debug, Clone)]
pub struct NewQuestion {
    pub client_question_id: String,
    pub recipient: Recipient,
    pub title: String,
    pub body: String,
    /// Option labels; ids (`o1`, `o2`, ...) are assigned by the server.
    pub options: Vec<String>,
    /// Defaults to the task's current version when omitted.
    pub task_version: Option<i64>,
    pub base_sha: Option<String>,
    pub head_sha: Option<String>,
}

#[derive(Debug, Clone)]
pub struct NewAnswer {
    pub client_answer_id: String,
    pub expected_version: i64,
    pub body: String,
    pub option_id: Option<String>,
    /// `context_hash` of the question as the answerer saw it.
    pub acknowledged_context_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OptionView {
    pub id: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct AnswerView {
    pub id: String,
    pub actor_kind: String,
    pub actor_id: String,
    pub body: String,
    pub option_id: Option<String>,
    pub context_hash: String,
    pub created_at: DateTime<Utc>,
    pub origin: &'static str,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct QuestionView {
    pub id: String,
    pub task_id: String,
    pub creator_kind: String,
    pub creator_id: String,
    pub recipient: Recipient,
    pub title: String,
    pub body: String,
    pub options: Vec<OptionView>,
    pub context: Context,
    /// Digest of `context`; send it back as `acknowledged_context_hash`.
    pub context_hash: String,
    /// `open` | `answered` | `cancelled`.
    pub status: String,
    /// Optimistic-lock counter; send as `expected_version`.
    pub version: i64,
    pub client_question_id: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// Only on single-question reads.
    pub answer: Option<AnswerView>,
    /// `fresh` | `stale` | `unknown`, only on single-question reads.
    pub context_freshness: Option<&'static str>,
    pub origin: &'static str,
}

pub fn context_hash(context_json: &str) -> String {
    hex::encode(Sha256::digest(context_json.as_bytes()))
}

fn answer_view(row: collab_answer::Model) -> AnswerView {
    AnswerView {
        id: row.id,
        actor_kind: row.actor_kind,
        actor_id: row.actor_id,
        body: row.body,
        option_id: row.option_id,
        context_hash: row.context_hash,
        created_at: row.created_at,
        origin: ORIGIN_USER_CONTENT,
    }
}

pub(crate) fn view(row: collab_question::Model) -> QuestionView {
    QuestionView {
        context_hash: context_hash(&row.context_json),
        options: serde_json::from_str(&row.options_json).unwrap_or_default(),
        context: serde_json::from_str(&row.context_json).unwrap_or_default(),
        id: row.id,
        task_id: row.task_id,
        creator_kind: row.creator_kind,
        creator_id: row.creator_id,
        recipient: Recipient {
            kind: row.recipient_kind,
            id: row.recipient_id,
        },
        title: row.title,
        body: row.body,
        status: row.status,
        version: row.version,
        client_question_id: row.client_question_id,
        created_at: row.created_at,
        updated_at: row.updated_at,
        answer: None,
        context_freshness: None,
        origin: ORIGIN_USER_CONTENT,
    }
}

/// Whether the context a question was asked in still describes the task.
///
/// `branch_head` is the current head of the task's declared branch as observed
/// by the caller: `None` = could not be read. A question asked against a commit
/// can only be judged when the task declares a branch to compare with.
pub fn freshness(
    task: &collab_task::Model,
    context: &Context,
    branch_head: Option<&str>,
) -> &'static str {
    if task.version != context.task_version {
        return "stale";
    }
    let Some(head) = &context.head_sha else {
        return "fresh";
    };
    match (&task.branch, branch_head) {
        (Some(_), Some(current)) if current == head => "fresh",
        (Some(_), Some(_)) => "stale",
        _ => "unknown",
    }
}

fn payload_hash(task_id: &str, q: &NewQuestion, context: &Context) -> String {
    let canonical = serde_json::json!([
        task_id,
        q.recipient.kind,
        q.recipient.id,
        q.title,
        q.body,
        q.options,
        context.task_version,
        context.base_sha,
        context.head_sha
    ]);
    hex::encode(Sha256::digest(canonical.to_string().as_bytes()))
}

async fn validate_recipient(
    db: &DatabaseConnection,
    repo_id: i64,
    recipient: &Recipient,
) -> CollabResult<()> {
    match (recipient.kind.as_str(), recipient.id.as_deref()) {
        (RECIPIENT_REPO_WRITERS, None) => Ok(()),
        (RECIPIENT_REPO_WRITERS, Some(_)) => Err(CollabError::bad_request(
            "recipient.id must be omitted for repo_writers",
        )),
        (RECIPIENT_USER, Some(id)) if id.parse::<i64>().is_ok_and(|n| n > 0) => Ok(()),
        (RECIPIENT_USER, _) => Err(CollabError::bad_request(
            "recipient.id must be a user id for recipient.kind=user",
        )),
        (RECIPIENT_SESSION, Some(id)) if validation::is_uuid(id) => {
            if agent_session_ops::find_in_repo(db, repo_id, id).await?.is_some() {
                Ok(())
            } else {
                Err(CollabError::bad_request(
                    "recipient.id must be a session of this repository",
                ))
            }
        }
        (RECIPIENT_SESSION, _) => Err(CollabError::bad_request(
            "recipient.id must be a session id for recipient.kind=session",
        )),
        _ => Err(CollabError::bad_request(
            "recipient.kind must be repo_writers, user or session",
        )),
    }
}

pub struct Created {
    pub question: QuestionView,
    pub replayed: bool,
}

pub async fn create(
    db: &DatabaseConnection,
    repo_id: i64,
    task_id: &str,
    actor: &Actor,
    input: NewQuestion,
    now: DateTime<Utc>,
) -> CollabResult<Created> {
    validation::client_key("client_question_id", &input.client_question_id)?;
    let title = validation::label("title", &input.title, MAX_TITLE_CHARS)?;
    validation::text("body", &input.body, 1, MAX_BODY_BYTES)?;
    if input.options.len() > MAX_OPTIONS {
        return Err(CollabError::bad_request(format!(
            "at most {MAX_OPTIONS} options are allowed"
        )));
    }
    let labels = input
        .options
        .iter()
        .map(|label| validation::label("option", label, MAX_OPTION_LABEL_CHARS))
        .collect::<CollabResult<Vec<_>>>()?;
    validate_recipient(db, repo_id, &input.recipient).await?;

    let task = task::find(db, repo_id, task_id).await?;
    if let Some(version) = input.task_version {
        if version != task.version {
            return Err(CollabError::VersionConflict {
                current_version: task.version,
            });
        }
    }
    let context = Context {
        task_version: task.version,
        base_sha: input.base_sha.as_deref().map(normalize_commit_sha).transpose()?,
        head_sha: input.head_sha.as_deref().map(normalize_commit_sha).transpose()?,
    };
    let options: Vec<OptionView> = labels
        .into_iter()
        .enumerate()
        .map(|(i, label)| OptionView { id: format!("o{}", i + 1), label })
        .collect();
    let options_json = serde_json::to_string(&options).map_err(|e| CollabError::Internal(e.into()))?;
    let context_json = serde_json::to_string(&context).map_err(|e| CollabError::Internal(e.into()))?;
    let normalized = NewQuestion { title: title.clone(), ..input.clone() };
    let hash = payload_hash(&task.id, &normalized, &context);

    let outcome = collab_question_ops::create_idempotent(
        db,
        collab_question::ActiveModel {
            id: Set(uuid::Uuid::new_v4().to_string()),
            repo_id: Set(repo_id),
            task_id: Set(task.id.clone()),
            creator_kind: Set(actor.kind.to_string()),
            creator_id: Set(actor.id.clone()),
            recipient_kind: Set(input.recipient.kind.clone()),
            recipient_id: Set(input.recipient.id.clone()),
            title: Set(title),
            body: Set(input.body.clone()),
            options_json: Set(options_json),
            context_json: Set(context_json),
            status: Set(status::OPEN.to_string()),
            version: Set(1),
            client_question_id: Set(input.client_question_id.clone()),
            payload_hash: Set(hash.clone()),
            created_at: Set(now),
            updated_at: Set(now),
        },
        actor.kind,
        &actor.id,
        &input.client_question_id,
    )
    .await?;
    match outcome {
        CreateOutcome::Created(row) => Ok(Created { question: view(row), replayed: false }),
        CreateOutcome::Existing(row) if row.payload_hash == hash => {
            Ok(Created { question: view(row), replayed: true })
        }
        CreateOutcome::Existing(_) => Err(CollabError::IdempotencyKeyReused),
    }
}

/// Load a question of this repository; other repositories' ids read as absent.
pub async fn find(
    db: &DatabaseConnection,
    repo_id: i64,
    id: &str,
) -> CollabResult<collab_question::Model> {
    if !validation::is_uuid(id) {
        return Err(CollabError::not_found("question not found"));
    }
    collab_question_ops::find_in_repo(db, repo_id, id)
        .await?
        .ok_or_else(|| CollabError::not_found("question not found"))
}

/// One question with its answer and a freshness verdict.
pub async fn get(
    db: &DatabaseConnection,
    repo_id: i64,
    id: &str,
    branch_head: impl FnOnce(&collab_task::Model) -> Option<String>,
) -> CollabResult<QuestionView> {
    let row = find(db, repo_id, id).await?;
    let task = task::find(db, repo_id, &row.task_id).await?;
    let answer = collab_question_ops::find_answer(db, &row.id).await?;
    let mut out = view(row);
    let head = branch_head(&task);
    out.context_freshness = Some(freshness(&task, &out.context, head.as_deref()));
    out.answer = answer.map(answer_view);
    Ok(out)
}

fn list_scope(repo_id: i64, task_id: &str, status: Option<&str>) -> String {
    format!(
        "questions:v1:repo={repo_id}:task={task_id}:status={}:order=created_asc,id_asc",
        status.unwrap_or("*")
    )
}

pub async fn list(
    db: &DatabaseConnection,
    cursor_key: &[u8],
    repo_id: i64,
    task_id: &str,
    status_filter: Option<&str>,
    limit: u64,
    cursor_token: Option<&str>,
) -> CollabResult<Page<QuestionView>> {
    if let Some(s) = status_filter {
        if ![status::OPEN, status::ANSWERED, status::CANCELLED].contains(&s) {
            return Err(CollabError::bad_request(
                "status must be open, answered or cancelled",
            ));
        }
    }
    let task = task::find(db, repo_id, task_id).await?;
    let scope = list_scope(repo_id, &task.id, status_filter);
    let after = cursor_token
        .map(|token| cursor::decode(cursor_key, &scope, token))
        .transpose()?
        .map(|p| (p.at, p.id));
    let mut rows =
        collab_question_ops::list_page(db, repo_id, &task.id, status_filter, after, limit + 1).await?;
    let has_more = rows.len() as u64 > limit;
    rows.truncate(limit as usize);
    let next_cursor = has_more.then(|| rows.last()).flatten().map(|last| {
        cursor::encode(
            cursor_key,
            &scope,
            &KeysetPosition { at: last.created_at, id: last.id.clone() },
        )
    });
    Ok(Page {
        items: rows.into_iter().map(view).collect(),
        next_cursor,
    })
}

/// Whether `actor` is inside the question's recipient scope. Repository write
/// access of the account was already checked by the caller.
pub fn may_answer(question: &collab_question::Model, actor: &Actor) -> bool {
    match question.recipient_kind.as_str() {
        RECIPIENT_REPO_WRITERS => true,
        kind => question.recipient_id.as_deref() == Some(actor.id.as_str()) && kind == actor.kind,
    }
}

pub struct Answered {
    pub question: QuestionView,
    pub replayed: bool,
}

fn answer_hash(question_id: &str, a: &NewAnswer) -> String {
    let canonical = serde_json::json!([
        question_id,
        a.expected_version,
        a.body,
        a.option_id,
        a.acknowledged_context_hash
    ]);
    hex::encode(Sha256::digest(canonical.to_string().as_bytes()))
}

pub async fn answer(
    db: &DatabaseConnection,
    repo_id: i64,
    question_id: &str,
    actor: &Actor,
    input: NewAnswer,
    branch_head: impl FnOnce(&collab_task::Model) -> Option<String>,
    now: DateTime<Utc>,
) -> CollabResult<Answered> {
    validation::client_key("client_answer_id", &input.client_answer_id)?;
    validation::text("body", &input.body, 0, MAX_BODY_BYTES)?;
    let question = find(db, repo_id, question_id).await?;
    if !may_answer(&question, actor) {
        return Err(CollabError::Forbidden(
            "this question is not addressed to you".to_string(),
        ));
    }
    let hash = answer_hash(&question.id, &input);

    // A retry of an answer that already landed is a replay, not a conflict.
    if let Some(existing) = collab_question_ops::find_answer(db, &question.id).await? {
        if existing.client_answer_id == input.client_answer_id {
            if existing.payload_hash == hash {
                let full = get(db, repo_id, &question.id, |_| None).await?;
                return Ok(Answered { question: full, replayed: true });
            }
            return Err(CollabError::IdempotencyKeyReused);
        }
    }
    if question.status != status::OPEN {
        return Err(not_open(&question.status, question.version));
    }

    let options: Vec<OptionView> = serde_json::from_str(&question.options_json).unwrap_or_default();
    match (&input.option_id, options.is_empty()) {
        (Some(_), true) => {
            return Err(CollabError::bad_request("this question has no options"));
        }
        (Some(id), false) if !options.iter().any(|o| &o.id == id) => {
            return Err(CollabError::bad_request("option_id is not one of the question's options"));
        }
        (None, false) => {
            return Err(CollabError::bad_request("option_id is required for this question"));
        }
        (None, true) if input.body.trim().is_empty() => {
            return Err(CollabError::bad_request("body must not be empty"));
        }
        _ => {}
    }

    // The answerer must have seen the context they are answering in, and that
    // context must still describe the task.
    let expected_hash = context_hash(&question.context_json);
    if input.acknowledged_context_hash != expected_hash {
        return Err(CollabError::bad_request(
            "acknowledged_context_hash does not match the question's context; re-read the question",
        ));
    }
    let task = task::find(db, repo_id, &question.task_id).await?;
    let context: Context = serde_json::from_str(&question.context_json).unwrap_or_default();
    let head = branch_head(&task);
    let verdict = freshness(&task, &context, head.as_deref());
    if verdict == "stale" {
        return Err(CollabError::coded_conflict(
            "CONTEXT_STALE",
            "the task or branch changed since the question was asked; re-read it before answering",
        )
        .with_details(serde_json::json!({
            "asked_task_version": context.task_version,
            "current_task_version": task.version,
        })));
    }

    let outcome = collab_question_ops::answer_cas(
        db,
        repo_id,
        &question.id,
        input.expected_version,
        collab_answer::ActiveModel {
            id: Set(uuid::Uuid::new_v4().to_string()),
            repo_id: Set(repo_id),
            question_id: Set(question.id.clone()),
            actor_kind: Set(actor.kind.to_string()),
            actor_id: Set(actor.id.clone()),
            body: Set(input.body.clone()),
            option_id: Set(input.option_id.clone()),
            context_hash: Set(expected_hash),
            client_answer_id: Set(input.client_answer_id.clone()),
            payload_hash: Set(hash),
            created_at: Set(now),
        },
        now,
    )
    .await?;
    match outcome {
        AnswerOutcome::Answered { question, answer } => {
            // `question` was re-read inside the answering transaction, so it
            // already carries the closed status and the bumped version.
            let mut out = view(question);
            out.answer = Some(answer_view(answer));
            out.context_freshness = Some(verdict);
            Ok(Answered { question: out, replayed: false })
        }
        AnswerOutcome::Conflict { current } if current.status != status::OPEN => {
            Err(not_open(&current.status, current.version))
        }
        AnswerOutcome::Conflict { current } => Err(CollabError::VersionConflict {
            current_version: current.version,
        }),
        AnswerOutcome::NotFound => Err(CollabError::not_found("question not found")),
    }
}

fn not_open(current_status: &str, version: i64) -> CollabError {
    CollabError::coded_conflict("QUESTION_NOT_OPEN", format!("question is already {current_status}"))
        .with_details(serde_json::json!({ "status": current_status, "current_version": version }))
}

/// Cancel an open question; only its creator may.
pub async fn cancel(
    db: &DatabaseConnection,
    repo_id: i64,
    question_id: &str,
    actor: &Actor,
    expected_version: i64,
    now: DateTime<Utc>,
) -> CollabResult<QuestionView> {
    let question = find(db, repo_id, question_id).await?;
    if question.creator_kind != actor.kind || question.creator_id != actor.id {
        return Err(CollabError::Forbidden(
            "only the creator of a question may cancel it".to_string(),
        ));
    }
    match collab_question_ops::cancel_cas(db, repo_id, &question.id, expected_version, now).await? {
        CasOutcome::Updated(row) => Ok(view(row)),
        CasOutcome::Conflict { current } if current.status != status::OPEN => {
            Err(not_open(&current.status, current.version))
        }
        CasOutcome::Conflict { current } => Err(CollabError::VersionConflict {
            current_version: current.version,
        }),
        CasOutcome::NotFound => Err(CollabError::not_found("question not found")),
    }
}
