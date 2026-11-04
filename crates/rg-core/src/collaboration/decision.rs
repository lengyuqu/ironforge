//! Decisions: immutable, traceable records of what was decided on a task.
//!
//! A decision may point at a question and its answer, and records the task
//! version / commit context it was taken in. It is never edited; superseding or
//! revoking appends one terminal change event, and the database's unique key
//! on the decision lets exactly one of several racing writers win.
//!
//! A decision is a collaboration record: it is not an approval, a review or a
//! merge authorization, and it grants no Git, CI or branch-protection right.
//! Title and rationale are claims by the writer (`origin: user_content`), and
//! the writer's account — not necessarily a human — is all the platform knows.
//! A decision whose context changed is reported `stale`; the platform never
//! revokes it on its own.
//!
//! Supersession is ordered: a replacement must be a later decision than the
//! one it replaces, by the per-task sequence number `seq`. Every edge then
//! points strictly forward, so a cycle cannot form however writers interleave,
//! without relying on locks that differ between database backends.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use rg_db::entities::{collab_decision, collab_decision_change, collab_task};
use rg_db::ops::collab_decision_ops::{self, kind, ChangeOutcome, CreateOutcome};
use rg_db::ops::collab_question_ops;
use sea_orm::{DatabaseConnection, Set};
use serde::Serialize;
use sha2::{Digest, Sha256};

use super::cursor::{self, KeysetPosition};
use super::error::{CollabError, CollabResult};
use super::message::normalize_commit_sha;
use super::question::{self, Context};
use super::{task, validation, Actor, Page, ORIGIN_USER_CONTENT};

pub const MAX_TITLE_CHARS: usize = 200;
pub const MAX_RATIONALE_BYTES: usize = 16 * 1024;
pub const MAX_REASON_BYTES: usize = 4 * 1024;
/// Longest supersede chain followed when looking for a cycle; past it the
/// request fails closed instead of being treated as acyclic.
const MAX_CHAIN: usize = 1000;

pub const STATUS_ACTIVE: &str = "active";

#[derive(Debug, Clone)]
pub struct NewDecision {
    pub client_decision_id: String,
    pub title: String,
    pub rationale: String,
    pub question_id: Option<String>,
    pub answer_id: Option<String>,
    /// When given, must equal the task's current version; defaults to it.
    pub task_version: Option<i64>,
    pub base_sha: Option<String>,
    pub head_sha: Option<String>,
}

#[derive(Debug, Clone)]
pub struct NewChange {
    pub client_change_id: String,
    /// `superseded` or `revoked`.
    pub kind: String,
    pub replacement_decision_id: Option<String>,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ChangeView {
    pub id: String,
    pub decision_id: String,
    /// `superseded` | `revoked`.
    pub kind: String,
    pub replacement_decision_id: Option<String>,
    pub reason: String,
    pub actor_kind: String,
    pub actor_id: String,
    pub client_change_id: String,
    pub created_at: DateTime<Utc>,
    pub origin: &'static str,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct DecisionView {
    pub id: String,
    pub task_id: String,
    /// 1-based position among the task's decisions (listing order).
    pub sequence: i64,
    pub question_id: Option<String>,
    pub answer_id: Option<String>,
    pub title: String,
    pub rationale: String,
    /// Task version / commits the decision was taken in.
    pub context: Context,
    pub actor_kind: String,
    pub actor_id: String,
    pub client_decision_id: String,
    pub created_at: DateTime<Utc>,
    /// `active` | `superseded` | `revoked`; derived from `change`.
    pub status: &'static str,
    /// The terminal event, when there is one.
    pub change: Option<ChangeView>,
    /// `fresh` | `stale` | `unknown`; judged like a question's context. A
    /// decision is never revoked because it is stale.
    pub context_freshness: &'static str,
    /// Ids of decisions this one replaces; only on single-decision reads.
    pub supersedes: Option<Vec<String>>,
    pub origin: &'static str,
}

fn change_view(row: collab_decision_change::Model) -> ChangeView {
    ChangeView {
        id: row.id,
        decision_id: row.decision_id,
        kind: row.kind,
        replacement_decision_id: row.replacement_decision_id,
        reason: row.reason,
        actor_kind: row.actor_kind,
        actor_id: row.actor_id,
        client_change_id: row.client_change_id,
        created_at: row.created_at,
        origin: ORIGIN_USER_CONTENT,
    }
}

fn status_of(change: &Option<ChangeView>) -> &'static str {
    match change.as_ref().map(|c| c.kind.as_str()) {
        Some(kind::SUPERSEDED) => kind::SUPERSEDED,
        Some(kind::REVOKED) => kind::REVOKED,
        _ => STATUS_ACTIVE,
    }
}

fn view(
    row: collab_decision::Model,
    change: Option<collab_decision_change::Model>,
    task: &collab_task::Model,
    branch_head: Option<&str>,
) -> DecisionView {
    let context: Context = serde_json::from_str(&row.context_json).unwrap_or_default();
    let change = change.map(change_view);
    DecisionView {
        context_freshness: question::freshness(task, &context, branch_head),
        status: status_of(&change),
        change,
        context,
        id: row.id,
        task_id: row.task_id,
        sequence: row.seq,
        question_id: row.question_id,
        answer_id: row.answer_id,
        title: row.title,
        rationale: row.rationale,
        actor_kind: row.actor_kind,
        actor_id: row.actor_id,
        client_decision_id: row.client_decision_id,
        created_at: row.created_at,
        supersedes: None,
        origin: ORIGIN_USER_CONTENT,
    }
}

fn payload_hash(task_id: &str, d: &NewDecision, title: &str) -> String {
    let canonical = serde_json::json!([
        task_id,
        title,
        d.rationale,
        d.question_id,
        d.answer_id,
        d.task_version,
        d.base_sha,
        d.head_sha
    ]);
    hex::encode(Sha256::digest(canonical.to_string().as_bytes()))
}

fn change_hash(decision_id: &str, c: &NewChange) -> String {
    let canonical = serde_json::json!([decision_id, c.kind, c.replacement_decision_id, c.reason]);
    hex::encode(Sha256::digest(canonical.to_string().as_bytes()))
}

pub struct Created {
    pub decision: DecisionView,
    pub replayed: bool,
}

/// The question/answer a decision cites must belong to this task: the question
/// is of the task, and the answer is the answer of that question. All
/// mismatches read the same, so ids of other tasks or repositories leak nothing.
async fn validate_links(
    db: &DatabaseConnection,
    repo_id: i64,
    task_id: &str,
    question_id: Option<&str>,
    answer_id: Option<&str>,
) -> CollabResult<()> {
    let mismatch =
        |what: &str| CollabError::bad_request(format!("{what} does not belong to this task"));
    let Some(question_id) = question_id else {
        return if answer_id.is_some() {
            Err(CollabError::bad_request("answer_id requires question_id"))
        } else {
            Ok(())
        };
    };
    if !validation::is_uuid(question_id) {
        return Err(mismatch("question_id"));
    }
    let question = collab_question_ops::find_in_repo(db, repo_id, question_id)
        .await?
        .filter(|q| q.task_id == task_id)
        .ok_or_else(|| mismatch("question_id"))?;
    if let Some(answer_id) = answer_id {
        if !validation::is_uuid(answer_id) {
            return Err(mismatch("answer_id"));
        }
        collab_decision_ops::find_answer_by_id(db, answer_id)
            .await?
            .filter(|a| a.repo_id == repo_id && a.question_id == question.id)
            .ok_or_else(|| mismatch("answer_id"))?;
    }
    Ok(())
}

pub async fn create(
    db: &DatabaseConnection,
    repo_id: i64,
    task_id: &str,
    actor: &Actor,
    input: NewDecision,
    branch_head: Option<&str>,
    now: DateTime<Utc>,
) -> CollabResult<Created> {
    validation::client_key("client_decision_id", &input.client_decision_id)?;
    let title = validation::label("title", &input.title, MAX_TITLE_CHARS)?;
    validation::text("rationale", &input.rationale, 1, MAX_RATIONALE_BYTES)?;
    let base_sha = input
        .base_sha
        .as_deref()
        .map(normalize_commit_sha)
        .transpose()?;
    let head_sha = input
        .head_sha
        .as_deref()
        .map(normalize_commit_sha)
        .transpose()?;
    let input = NewDecision {
        base_sha,
        head_sha,
        ..input
    };

    let task = task::find(db, repo_id, task_id).await?;
    let hash = payload_hash(&task.id, &input, &title);

    // A retry of a decision that already landed is a replay whatever the task
    // looks like now, so it is checked before any precondition.
    if let Some(existing) =
        collab_decision_ops::find_by_client_id(db, actor.kind, &actor.id, &input.client_decision_id)
            .await?
    {
        return if existing.payload_hash == hash {
            let change = collab_decision_ops::find_change_for(db, &existing.id).await?;
            Ok(Created {
                decision: view(existing, change, &task, branch_head),
                replayed: true,
            })
        } else {
            Err(CollabError::IdempotencyKeyReused)
        };
    }

    if let Some(version) = input.task_version {
        if version != task.version {
            return Err(CollabError::VersionConflict {
                current_version: task.version,
            });
        }
    }
    validate_links(
        db,
        repo_id,
        &task.id,
        input.question_id.as_deref(),
        input.answer_id.as_deref(),
    )
    .await?;

    let context = Context {
        task_version: task.version,
        base_sha: input.base_sha.clone(),
        head_sha: input.head_sha.clone(),
    };
    let context_json =
        serde_json::to_string(&context).map_err(|e| CollabError::Internal(e.into()))?;
    let outcome = collab_decision_ops::create_next_seq(
        db,
        &task.id,
        actor.kind,
        &actor.id,
        &input.client_decision_id,
        |seq| collab_decision::ActiveModel {
            id: Set(uuid::Uuid::new_v4().to_string()),
            repo_id: Set(repo_id),
            task_id: Set(task.id.clone()),
            seq: Set(seq),
            question_id: Set(input.question_id.clone()),
            answer_id: Set(input.answer_id.clone()),
            title: Set(title.clone()),
            rationale: Set(input.rationale.clone()),
            context_json: Set(context_json.clone()),
            actor_kind: Set(actor.kind.to_string()),
            actor_id: Set(actor.id.clone()),
            client_decision_id: Set(input.client_decision_id.clone()),
            payload_hash: Set(hash.clone()),
            created_at: Set(now),
        },
    )
    .await?;
    match outcome {
        CreateOutcome::Created(row) => Ok(Created {
            decision: view(row, None, &task, branch_head),
            replayed: false,
        }),
        CreateOutcome::Existing(row) if row.payload_hash == hash => {
            let change = collab_decision_ops::find_change_for(db, &row.id).await?;
            Ok(Created {
                decision: view(row, change, &task, branch_head),
                replayed: true,
            })
        }
        CreateOutcome::Existing(_) => Err(CollabError::IdempotencyKeyReused),
    }
}

/// Load a decision of this repository; other repositories' ids read as absent.
pub async fn find(
    db: &DatabaseConnection,
    repo_id: i64,
    id: &str,
) -> CollabResult<collab_decision::Model> {
    if !validation::is_uuid(id) {
        return Err(CollabError::not_found("decision not found"));
    }
    collab_decision_ops::find_in_repo(db, repo_id, id)
        .await?
        .ok_or_else(|| CollabError::not_found("decision not found"))
}

/// One decision with its terminal event, freshness and the decisions it replaces.
pub async fn get(
    db: &DatabaseConnection,
    repo_id: i64,
    id: &str,
    branch_head: impl FnOnce(&collab_task::Model) -> Option<String>,
) -> CollabResult<DecisionView> {
    let row = find(db, repo_id, id).await?;
    let task = task::find(db, repo_id, &row.task_id).await?;
    let change = collab_decision_ops::find_change_for(db, &row.id).await?;
    let supersedes = collab_decision_ops::list_superseded_by(db, &row.id)
        .await?
        .into_iter()
        .map(|c| c.decision_id)
        .collect();
    let head = branch_head(&task);
    let mut out = view(row, change, &task, head.as_deref());
    out.supersedes = Some(supersedes);
    Ok(out)
}

fn list_scope(repo_id: i64, task_id: &str) -> String {
    format!("decisions:v1:repo={repo_id}:task={task_id}:order=seq_asc")
}

/// Decisions of a task, oldest first, each with its terminal event and a
/// freshness verdict computed against the one `branch_head` observation.
pub async fn list(
    db: &DatabaseConnection,
    cursor_key: &[u8],
    repo_id: i64,
    task_id: &str,
    branch_head: impl FnOnce(&collab_task::Model) -> Option<String>,
    limit: u64,
    cursor_token: Option<&str>,
) -> CollabResult<Page<DecisionView>> {
    let task = task::find(db, repo_id, task_id).await?;
    let scope = list_scope(repo_id, &task.id);
    let after = cursor_token
        .map(|token| cursor::decode(cursor_key, &scope, token))
        .transpose()?
        .map(|p| p.at.timestamp());
    let mut rows = collab_decision_ops::list_page(db, repo_id, &task.id, after, limit + 1).await?;
    let has_more = rows.len() as u64 > limit;
    rows.truncate(limit as usize);
    // The shared cursor carries a timestamp and an id; the position here is the
    // sequence number, stored as seconds since the epoch.
    let next_cursor = has_more.then(|| rows.last()).flatten().map(|last| {
        cursor::encode(
            cursor_key,
            &scope,
            &KeysetPosition {
                at: DateTime::<Utc>::from_timestamp(last.seq, 0).unwrap_or_default(),
                id: last.id.clone(),
            },
        )
    });
    let mut changes: HashMap<String, collab_decision_change::Model> =
        collab_decision_ops::list_changes_for(db, rows.iter().map(|r| r.id.clone()).collect())
            .await?
            .into_iter()
            .map(|c| (c.decision_id.clone(), c))
            .collect();
    let head = branch_head(&task);
    Ok(Page {
        items: rows
            .into_iter()
            .map(|row| {
                let change = changes.remove(&row.id);
                view(row, change, &task, head.as_deref())
            })
            .collect(),
        next_cursor,
    })
}

fn already_changed(winner: &collab_decision_change::Model) -> CollabError {
    CollabError::coded_conflict(
        "DECISION_ALREADY_CHANGED",
        format!("decision was already {}", winner.kind),
    )
    .with_details(serde_json::json!({
        "change_id": winner.id,
        "kind": winner.kind,
        "replacement_decision_id": winner.replacement_decision_id,
    }))
}

/// Whether following "superseded by" links from `start` reaches `target`.
async fn chain_reaches(db: &DatabaseConnection, start: &str, target: &str) -> CollabResult<bool> {
    let mut current = start.to_string();
    for _ in 0..MAX_CHAIN {
        if current == target {
            return Ok(true);
        }
        let Some(change) = collab_decision_ops::find_change_for(db, &current).await? else {
            return Ok(false);
        };
        match (change.kind.as_str(), change.replacement_decision_id) {
            (kind::SUPERSEDED, Some(next)) => current = next,
            _ => return Ok(false),
        }
    }
    Err(CollabError::coded_conflict(
        "DECISION_CHAIN_TOO_LONG",
        "the supersede chain is too long to check for cycles",
    ))
}

pub struct Changed {
    pub decision: DecisionView,
    pub replayed: bool,
}

/// Supersede or revoke a decision. Each decision accepts one such event; a
/// second one — from the same writer or another — is `DECISION_ALREADY_CHANGED`.
pub async fn change(
    db: &DatabaseConnection,
    repo_id: i64,
    decision_id: &str,
    actor: &Actor,
    input: NewChange,
    branch_head: impl FnOnce(&collab_task::Model) -> Option<String>,
    now: DateTime<Utc>,
) -> CollabResult<Changed> {
    validation::client_key("client_change_id", &input.client_change_id)?;
    validation::text("reason", &input.reason, 1, MAX_REASON_BYTES)?;
    match (input.kind.as_str(), &input.replacement_decision_id) {
        (kind::SUPERSEDED, Some(_)) | (kind::REVOKED, None) => {}
        (kind::SUPERSEDED, None) => {
            return Err(CollabError::bad_request(
                "replacement_decision_id is required to supersede a decision",
            ));
        }
        (kind::REVOKED, Some(_)) => {
            return Err(CollabError::bad_request(
                "replacement_decision_id must be omitted when revoking a decision",
            ));
        }
        _ => {
            return Err(CollabError::bad_request(
                "kind must be superseded or revoked",
            ));
        }
    }
    let decision = find(db, repo_id, decision_id).await?;
    let hash = change_hash(&decision.id, &input);

    // Replay first: a retry of a change that landed stays a replay even though
    // the decision is now terminal.
    let replay =
        |existing: collab_decision_change::Model| -> CollabResult<collab_decision_change::Model> {
            if existing.payload_hash == hash && existing.decision_id == decision.id {
                Ok(existing)
            } else {
                Err(CollabError::IdempotencyKeyReused)
            }
        };

    let outcome = if let Some(existing) = collab_decision_ops::find_change_by_client_id(
        db,
        actor.kind,
        &actor.id,
        &input.client_change_id,
    )
    .await?
    {
        ChangeOutcome::Existing(existing)
    } else {
        if let Some(winner) = collab_decision_ops::find_change_for(db, &decision.id).await? {
            return Err(already_changed(&winner));
        }
        if let Some(replacement_id) = &input.replacement_decision_id {
            check_replacement(db, repo_id, &decision, replacement_id).await?;
        }
        collab_decision_ops::insert_change(
            db,
            collab_decision_change::ActiveModel {
                id: Set(uuid::Uuid::new_v4().to_string()),
                repo_id: Set(repo_id),
                task_id: Set(decision.task_id.clone()),
                decision_id: Set(decision.id.clone()),
                kind: Set(input.kind.clone()),
                replacement_decision_id: Set(input.replacement_decision_id.clone()),
                reason: Set(input.reason.clone()),
                actor_kind: Set(actor.kind.to_string()),
                actor_id: Set(actor.id.clone()),
                client_change_id: Set(input.client_change_id.clone()),
                payload_hash: Set(hash.clone()),
                created_at: Set(now),
            },
            &decision.id,
            actor.kind,
            &actor.id,
            &input.client_change_id,
        )
        .await?
    };
    let (change, replayed) = match outcome {
        ChangeOutcome::Created(row) => (row, false),
        ChangeOutcome::Existing(row) => (replay(row)?, true),
        ChangeOutcome::AlreadyChanged(winner) => return Err(already_changed(&winner)),
    };
    let task = task::find(db, repo_id, &decision.task_id).await?;
    let head = branch_head(&task);
    Ok(Changed {
        decision: view(decision, Some(change), &task, head.as_deref()),
        replayed,
    })
}

/// A replacement must be another decision of the same task with a higher `seq`. Anything earlier is either a cycle (reported as such when
/// the chain of the replacement leads back here) or simply not a replacement.
async fn check_replacement(
    db: &DatabaseConnection,
    repo_id: i64,
    decision: &collab_decision::Model,
    replacement_id: &str,
) -> CollabResult<()> {
    if replacement_id == decision.id {
        return Err(CollabError::bad_request(
            "a decision cannot supersede itself",
        ));
    }
    let replacement = if validation::is_uuid(replacement_id) {
        collab_decision_ops::find_in_repo(db, repo_id, replacement_id)
            .await?
            .filter(|r| r.task_id == decision.task_id)
    } else {
        None
    }
    .ok_or_else(|| {
        CollabError::bad_request("replacement_decision_id does not belong to this task")
    })?;
    if replacement.seq > decision.seq {
        return Ok(());
    }
    if chain_reaches(db, &replacement.id, &decision.id).await? {
        return Err(CollabError::coded_conflict(
            "DECISION_CYCLE",
            "the replacement was itself superseded by this decision, directly or through other decisions",
        )
        .with_details(serde_json::json!({ "replacement_decision_id": replacement.id })));
    }
    Err(CollabError::coded_conflict(
        "REPLACEMENT_NOT_NEWER",
        "the replacement must be a decision recorded after the one it replaces",
    )
    .with_details(serde_json::json!({ "replacement_decision_id": replacement.id })))
}
