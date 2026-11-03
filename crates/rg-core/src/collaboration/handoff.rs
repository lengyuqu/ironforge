//! Handoffs: a session declares that another session should take over a task.
//!
//! Every text section and evidence entry is an unverified claim by the creator.
//! Accepting a handoff changes only the handoff; it does not reassign the task,
//! post a message, or approve anything. The state machine is
//! `pending -> accepted | cancelled`, both terminal.

use chrono::{DateTime, Utc};
use rg_db::entities::{agent_session, collab_handoff};
use rg_db::ops::agent_session_ops;
use rg_db::ops::collab_handoff_ops::{self, status, CreateOutcome};
use rg_db::ops::collab_task_ops::CasOutcome;
use sea_orm::{DatabaseConnection, Set};
use serde::Serialize;
use sha2::{Digest, Sha256};

use super::cursor::{self, KeysetPosition};
use super::error::{CollabError, CollabResult};
use super::{task, validation, Page, ORIGIN_USER_CONTENT};

pub const MAX_SECTION_BYTES: usize = 16 * 1024;
pub const MAX_EVIDENCE: usize = 20;
pub const MAX_EVIDENCE_BYTES: usize = 2048;

#[derive(Debug, Clone)]
pub struct NewHandoff {
    pub client_handoff_id: String,
    pub to_session_id: String,
    pub summary: String,
    pub completed: String,
    pub next_steps: String,
    pub blockers: String,
    pub commit_sha: Option<String>,
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct HandoffView {
    pub id: String,
    pub task_id: String,
    pub from_session_id: String,
    pub to_session_id: String,
    pub summary: String,
    pub completed: String,
    pub next_steps: String,
    pub blockers: String,
    pub commit_sha: Option<String>,
    pub evidence: Vec<String>,
    pub client_handoff_id: String,
    /// `pending` | `accepted` | `cancelled`.
    pub status: String,
    /// Optimistic-lock counter; send as `expected_version` to accept/cancel.
    pub version: i64,
    pub created_at: DateTime<Utc>,
    pub accepted_at: Option<DateTime<Utc>>,
    /// Claims by the creating session, not verified facts.
    pub origin: &'static str,
}

fn view(row: collab_handoff::Model) -> HandoffView {
    HandoffView {
        evidence: serde_json::from_str(&row.evidence_json).unwrap_or_default(),
        id: row.id,
        task_id: row.task_id,
        from_session_id: row.from_session_id,
        to_session_id: row.to_session_id,
        summary: row.summary,
        completed: row.completed,
        next_steps: row.next_steps,
        blockers: row.blockers,
        commit_sha: row.commit_sha,
        client_handoff_id: row.client_handoff_id,
        status: row.status,
        version: row.version,
        created_at: row.created_at,
        accepted_at: row.accepted_at,
        origin: ORIGIN_USER_CONTENT,
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Created {
    New(HandoffView),
    Replayed(HandoffView),
}

fn payload_hash(task_id: &str, h: &NewHandoff) -> String {
    let canonical = serde_json::json!([
        task_id,
        h.to_session_id,
        h.summary,
        h.completed,
        h.next_steps,
        h.blockers,
        h.commit_sha,
        h.evidence
    ]);
    hex::encode(Sha256::digest(canonical.to_string().as_bytes()))
}

fn validate_evidence(evidence: &[String]) -> CollabResult<String> {
    if evidence.len() > MAX_EVIDENCE {
        return Err(CollabError::bad_request(format!(
            "at most {MAX_EVIDENCE} evidence entries are allowed"
        )));
    }
    for entry in evidence {
        if entry.is_empty()
            || entry.len() > MAX_EVIDENCE_BYTES
            || entry.chars().any(char::is_control)
        {
            return Err(CollabError::bad_request(
                "evidence entries must be non-empty single-line text of at most 2048 bytes",
            ));
        }
    }
    serde_json::to_string(evidence).map_err(|e| CollabError::Internal(e.into()))
}

/// Create a handoff from `from` to another session of the same repository.
/// The target may be offline or expired; the handoff then simply stays pending.
pub async fn create(
    db: &DatabaseConnection,
    repo_id: i64,
    task_id: &str,
    from: &agent_session::Model,
    mut input: NewHandoff,
    now: DateTime<Utc>,
) -> CollabResult<Created> {
    validation::client_key("client_handoff_id", &input.client_handoff_id)?;
    validation::text("summary", &input.summary, 1, MAX_SECTION_BYTES)?;
    validation::text("completed", &input.completed, 0, MAX_SECTION_BYTES)?;
    validation::text("next_steps", &input.next_steps, 0, MAX_SECTION_BYTES)?;
    validation::text("blockers", &input.blockers, 0, MAX_SECTION_BYTES)?;
    if let Some(sha) = &input.commit_sha {
        input.commit_sha = Some(super::message::normalize_commit_sha(sha)?);
    }
    let evidence_json = validate_evidence(&input.evidence)?;

    let task = task::find(db, repo_id, task_id).await?;
    let target_ok = validation::is_uuid(&input.to_session_id)
        && input.to_session_id != from.id
        && agent_session_ops::find_in_repo(db, repo_id, &input.to_session_id)
            .await?
            .is_some();
    if !target_ok {
        return Err(CollabError::bad_request(
            "to_session_id must be another session of this repository",
        ));
    }

    let hash = payload_hash(&task.id, &input);
    let outcome = collab_handoff_ops::create_idempotent(
        db,
        collab_handoff::ActiveModel {
            id: Set(uuid::Uuid::new_v4().to_string()),
            repo_id: Set(repo_id),
            task_id: Set(task.id.clone()),
            from_session_id: Set(from.id.clone()),
            to_session_id: Set(input.to_session_id.clone()),
            summary: Set(input.summary.clone()),
            completed: Set(input.completed.clone()),
            next_steps: Set(input.next_steps.clone()),
            blockers: Set(input.blockers.clone()),
            commit_sha: Set(input.commit_sha.clone()),
            evidence_json: Set(evidence_json),
            client_handoff_id: Set(input.client_handoff_id.clone()),
            payload_hash: Set(hash.clone()),
            status: Set(status::PENDING.to_string()),
            version: Set(1),
            created_at: Set(now),
            accepted_at: Set(None),
        },
        &from.id,
        &input.client_handoff_id,
    )
    .await?;
    match outcome {
        CreateOutcome::Created(row) => Ok(Created::New(view(row))),
        CreateOutcome::Existing(row) if row.payload_hash == hash => Ok(Created::Replayed(view(row))),
        CreateOutcome::Existing(_) => Err(CollabError::IdempotencyKeyReused),
    }
}

fn list_scope(repo_id: i64, task_id: &str) -> String {
    format!("handoffs:v1:repo={repo_id}:task={task_id}:order=created_asc,id_asc")
}

pub async fn list(
    db: &DatabaseConnection,
    cursor_key: &[u8],
    repo_id: i64,
    task_id: &str,
    limit: u64,
    cursor_token: Option<&str>,
) -> CollabResult<Page<HandoffView>> {
    let task = task::find(db, repo_id, task_id).await?;
    let scope = list_scope(repo_id, &task.id);
    let after = cursor_token
        .map(|token| cursor::decode(cursor_key, &scope, token))
        .transpose()?
        .map(|p| (p.at, p.id));
    let mut rows = collab_handoff_ops::list_page(db, repo_id, &task.id, after, limit + 1).await?;
    let has_more = rows.len() as u64 > limit;
    rows.truncate(limit as usize);
    let next_cursor = has_more
        .then(|| rows.last())
        .flatten()
        .map(|last| {
            cursor::encode(
                cursor_key,
                &scope,
                &KeysetPosition {
                    at: last.created_at,
                    id: last.id.clone(),
                },
            )
        });
    Ok(Page {
        items: rows.into_iter().map(view).collect(),
        next_cursor,
    })
}

async fn transition(
    db: &DatabaseConnection,
    repo_id: i64,
    handoff_id: &str,
    actor: &agent_session::Model,
    expected_version: i64,
    to_status: &str,
    now: DateTime<Utc>,
) -> CollabResult<HandoffView> {
    if !validation::is_uuid(handoff_id) {
        return Err(CollabError::not_found("handoff not found"));
    }
    let handoff = collab_handoff_ops::find_in_repo(db, repo_id, handoff_id)
        .await?
        .ok_or_else(|| CollabError::not_found("handoff not found"))?;

    // Only the addressed session may accept, only the creator may cancel.
    let (allowed, who) = if to_status == status::ACCEPTED {
        (handoff.to_session_id == actor.id, "the session it is addressed to")
    } else {
        (handoff.from_session_id == actor.id, "the session that created it")
    };
    if !allowed {
        return Err(CollabError::Forbidden(format!(
            "only {who} may {} this handoff",
            if to_status == status::ACCEPTED { "accept" } else { "cancel" }
        )));
    }

    match collab_handoff_ops::transition_pending(db, repo_id, handoff_id, expected_version, to_status, now)
        .await?
    {
        CasOutcome::Updated(row) => Ok(view(row)),
        CasOutcome::Conflict { current } if current.status != status::PENDING => Err(
            CollabError::Conflict(format!("handoff is already {}", current.status)),
        ),
        CasOutcome::Conflict { current } => Err(CollabError::VersionConflict {
            current_version: current.version,
        }),
        CasOutcome::NotFound => Err(CollabError::not_found("handoff not found")),
    }
}

/// Accept a pending handoff addressed to `actor`.
pub async fn accept(
    db: &DatabaseConnection,
    repo_id: i64,
    handoff_id: &str,
    actor: &agent_session::Model,
    expected_version: i64,
    now: DateTime<Utc>,
) -> CollabResult<HandoffView> {
    transition(db, repo_id, handoff_id, actor, expected_version, status::ACCEPTED, now).await
}

/// Cancel a pending handoff created by `actor`.
pub async fn cancel(
    db: &DatabaseConnection,
    repo_id: i64,
    handoff_id: &str,
    actor: &agent_session::Model,
    expected_version: i64,
    now: DateTime<Utc>,
) -> CollabResult<HandoffView> {
    transition(db, repo_id, handoff_id, actor, expected_version, status::CANCELLED, now).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> NewHandoff {
        NewHandoff {
            client_handoff_id: "h".into(),
            to_session_id: "to".into(),
            summary: "s".into(),
            completed: "c".into(),
            next_steps: "n".into(),
            blockers: "b".into(),
            commit_sha: None,
            evidence: vec!["ci:1".into()],
        }
    }

    #[test]
    fn payload_hash_covers_task_and_every_section() {
        let h = payload_hash("t1", &base());
        assert_eq!(h, payload_hash("t1", &base()));
        assert_ne!(h, payload_hash("t2", &base()));
        for mutate in [
            |x: &mut NewHandoff| x.summary.push('!'),
            |x: &mut NewHandoff| x.to_session_id = "other".into(),
            |x: &mut NewHandoff| x.evidence.push("more".into()),
            |x: &mut NewHandoff| x.blockers.clear(),
        ] {
            let mut x = base();
            mutate(&mut x);
            assert_ne!(h, payload_hash("t1", &x));
        }
    }

    #[test]
    fn evidence_is_bounded_single_line() {
        assert!(validate_evidence(&vec!["x".into(); 20]).is_ok());
        assert!(validate_evidence(&vec!["x".into(); 21]).is_err());
        assert!(validate_evidence(&["".into()]).is_err());
        assert!(validate_evidence(&["a\nb".into()]).is_err());
        assert!(validate_evidence(&["x".repeat(2049)]).is_err());
    }
}
