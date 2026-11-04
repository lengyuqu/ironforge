//! Fault diagnosis (O10): why does this task or session look stuck?
//!
//! Everything here is derived from stored records at call time and every finding
//! names its source. Findings are either `established` (a fact the records
//! show) or `unknown` (the records cannot tell). A silent or slow session is
//! never blamed: "offline" and "no acknowledgement" are reported as
//! what they are, together with an `unknown` finding about the cause, because the
//! collaboration records cannot see client behaviour, networks or service logs.
//! Read-only: nothing here writes or notifies anyone.

use chrono::{DateTime, Utc};
use rg_db::entities::{
    agent_session, collab_handoff, collab_notice, collab_question, collab_task, repository,
};
use rg_db::ops::collab_notice_ops::{self, kind, source};
use rg_db::ops::{
    agent_session_ops, collab_escalation_ops, collab_handoff_ops, collab_question_ops,
    collab_task_ops,
};
use sea_orm::{
    ColumnTrait, Condition, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder, QuerySelect,
};
use serde::Serialize;
use serde_json::{json, Value};

use super::access;
use super::error::{CollabError, CollabResult};
use super::session::state_of;
use super::{task, validation};

/// Page sizes for the per-object findings.
const OBJECT_LIMIT: u64 = 20;

pub const ESTABLISHED: &str = "established";
pub const UNKNOWN: &str = "unknown";

/// Information outside this diagnosis, stated on every response.
pub const NOT_COVERED: [&str; 5] = [
    "what the client program is doing (busy, idle, crashed, disconnected)",
    "network conditions between a client and the server",
    "server process health and logs",
    "whether a client fetched or displayed a notice (fetches are not recorded)",
    "message delivery and read receipts (available only to the message author)",
];

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Source {
    /// Kind of record the finding was derived from.
    pub record: &'static str,
    pub id: Option<String>,
    /// Fields of that record that were read.
    pub fields: Vec<&'static str>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Finding {
    pub code: &'static str,
    /// `established` | `unknown`.
    pub confidence: &'static str,
    /// `info` | `attention`: whether it could explain something being stuck.
    pub severity: &'static str,
    pub statement: String,
    pub source: Source,
    pub details: Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct Diagnosis {
    pub subject: Value,
    pub as_of: DateTime<Utc>,
    pub findings: Vec<Finding>,
    pub not_covered: [&'static str; 5],
    /// Always `derived_from_records`; never a statement about fault.
    pub authority: &'static str,
}

#[allow(clippy::too_many_arguments)]
fn finding(
    code: &'static str,
    confidence: &'static str,
    severity: &'static str,
    statement: impl Into<String>,
    record: &'static str,
    id: Option<&str>,
    fields: &[&'static str],
    details: Value,
) -> Finding {
    Finding {
        code,
        confidence,
        severity,
        statement: statement.into(),
        source: Source {
            record,
            id: id.map(str::to_string),
            fields: fields.to_vec(),
        },
        details,
    }
}

fn secs_since(at: DateTime<Utc>, now: DateTime<Utc>) -> i64 {
    (now - at).num_seconds().max(0)
}

/// Findings that describe one session: lifecycle, and the cause that cannot be
/// established when it is merely offline.
fn session_state_findings(s: &agent_session::Model, now: DateTime<Utc>) -> Vec<Finding> {
    let state = state_of(s, now);
    let fields = &["last_seen_at", "expires_at", "ended_at"];
    let details = json!({
        "state": state, "last_seen_at": s.last_seen_at, "expires_at": s.expires_at, "ended_at": s.ended_at,
    });
    let mut out = Vec::new();
    match state {
        "ended" => out.push(finding(
            "SESSION_ENDED",
            ESTABLISHED,
            "attention",
            "The session was ended; it cannot read notices or act again. Work addressed to it needs a new session.",
            "agent_session",
            Some(&s.id),
            fields,
            details,
        )),
        "expired" => out.push(finding(
            "SESSION_EXPIRED",
            ESTABLISHED,
            "attention",
            "The session passed its hard expiry; it cannot be revived. Work addressed to it needs a new session.",
            "agent_session",
            Some(&s.id),
            fields,
            details,
        )),
        "offline" => {
            out.push(finding(
                "SESSION_OFFLINE",
                ESTABLISHED,
                "attention",
                format!(
                    "No heartbeat for {} s (online window is {} s). The session has not ended or expired, so it may still be working.",
                    secs_since(s.last_seen_at, now),
                    super::session::ONLINE_WINDOW.num_seconds()
                ),
                "agent_session",
                Some(&s.id),
                fields,
                details,
            ));
            out.push(finding(
                "SESSION_OFFLINE_CAUSE",
                UNKNOWN,
                "info",
                "The records cannot show why heartbeats stopped (idle, busy, disconnected or stopped); offline is not evidence of failure.",
                "agent_session",
                Some(&s.id),
                &["last_seen_at"],
                json!({}),
            ));
        }
        _ => out.push(finding(
            "SESSION_ONLINE",
            ESTABLISHED,
            "info",
            format!("Last heartbeat {} s ago.", secs_since(s.last_seen_at, now)),
            "agent_session",
            Some(&s.id),
            fields,
            details,
        )),
    }
    out
}

/// Whether the session's account still belongs to the repository's working group.
async fn access_finding(
    db: &DatabaseConnection,
    repo: &repository::Model,
    s: &agent_session::Model,
) -> CollabResult<Finding> {
    let member = access::is_collab_member(db, repo, s.user_id).await?;
    Ok(if member {
        finding(
            "ACCOUNT_ACCESS",
            ESTABLISHED,
            "info",
            "The session's account still has collaboration access to this repository.",
            "repository_permissions",
            Some(&s.id),
            &["owner", "collaborators", "organization", "write access"],
            json!({ "has_access": true }),
        )
    } else {
        finding(
            "ACCOUNT_ACCESS_REVOKED",
            ESTABLISHED,
            "attention",
            "The session's account no longer has collaboration access to this repository; requests the session makes are rejected and it receives no reminders.",
            "repository_permissions",
            Some(&s.id),
            &["owner", "collaborators", "organization", "write access"],
            json!({ "has_access": false }),
        )
    })
}

/// State of the notice that announced `source_id` to `recipient`, as a finding
/// detail: `pending`, `acknowledged`, `resolved` or `none_recorded`.
async fn notice_state(
    db: &DatabaseConnection,
    recipient: &str,
    notice_kind: &str,
    source_type: &str,
    source_id: &str,
) -> CollabResult<Option<collab_notice::Model>> {
    Ok(
        collab_notice_ops::find_for_source(db, recipient, notice_kind, source_type, source_id)
            .await?,
    )
}

fn notice_finding(
    code: &'static str,
    notice: Option<&collab_notice::Model>,
    notice_kind: &'static str,
    source_id: &str,
) -> Finding {
    let state = match notice {
        None => "none_recorded",
        Some(n) if n.ack_at.is_some() => "acknowledged",
        Some(n) if n.resolved_at.is_some() => "resolved",
        Some(_) => "pending",
    };
    let (confidence, severity, statement) = match state {
        "pending" => (ESTABLISHED, "attention", "A notice for this was stored and has not been acknowledged. Whether the recipient's client fetched it is not recorded.".to_string()),
        "none_recorded" => (UNKNOWN, "info", "No notice is recorded for this; it may predate notices or the recipient was not live when it was created.".to_string()),
        _ => (ESTABLISHED, "info", format!("The notice for this is {state}.")),
    };
    finding(
        code,
        confidence,
        severity,
        statement,
        "collab_notice",
        notice.map(|n| n.id.as_str()),
        &["kind", "source_id", "ack_at", "resolved_at"],
        json!({ "notice_kind": notice_kind, "notice_state": state, "source_id": source_id }),
    )
}

/// Explain a session.
pub async fn for_session(
    db: &DatabaseConnection,
    repo: &repository::Model,
    session_id: &str,
    now: DateTime<Utc>,
) -> CollabResult<Diagnosis> {
    let s = if validation::is_uuid(session_id) {
        agent_session_ops::find_in_repo(db, repo.id, session_id).await?
    } else {
        None
    }
    .ok_or_else(|| CollabError::not_found("session not found"))?;

    let mut findings = session_state_findings(&s, now);
    findings.push(access_finding(db, repo, &s).await?);

    let (pending, oldest) = collab_notice_ops::pending_summary(db, repo.id, &s.id).await?;
    if pending > 0 {
        findings.push(finding(
            "NOTICES_NOT_ACKNOWLEDGED",
            ESTABLISHED,
            "attention",
            format!(
                "{pending} notice(s) addressed to the session are stored and not acknowledged. Whether the client fetched them is not recorded."
            ),
            "collab_notice",
            None,
            &["recipient_session_id", "ack_at", "resolved_at", "created_at"],
            json!({ "recipient_session_id": s.id, "pending": pending, "oldest_created_at": oldest }),
        ));
        findings.push(finding(
            "NOTICE_FETCH_UNKNOWN",
            UNKNOWN,
            "info",
            "Whether the session's client has read these notices cannot be told from the records.",
            "collab_notice",
            None,
            &["ack_at"],
            json!({ "recipient_session_id": s.id }),
        ));
    }

    let handoffs =
        collab_handoff_ops::list_pending_to_session(db, repo.id, &s.id, OBJECT_LIMIT).await?;
    if !handoffs.is_empty() {
        findings.push(finding(
            "HANDOFFS_WAITING_FOR_SESSION",
            ESTABLISHED,
            "attention",
            format!(
                "{} pending handoff(s) are addressed to this session.",
                handoffs.len()
            ),
            "collab_handoff",
            None,
            &["to_session_id", "status"],
            json!({ "handoff_ids": handoffs.iter().map(|h| &h.id).collect::<Vec<_>>() }),
        ));
    }
    let questions = collab_question_ops::list_open(
        db,
        repo.id,
        Condition::all()
            .add(collab_question::Column::RecipientKind.eq("session"))
            .add(collab_question::Column::RecipientId.eq(s.id.as_str())),
        OBJECT_LIMIT,
    )
    .await?;
    if !questions.is_empty() {
        findings.push(finding(
            "QUESTIONS_WAITING_FOR_SESSION",
            ESTABLISHED,
            "attention",
            format!(
                "{} open question(s) are addressed to this session.",
                questions.len()
            ),
            "collab_question",
            None,
            &["recipient_kind", "recipient_id", "status"],
            json!({ "question_ids": questions.iter().map(|q| &q.id).collect::<Vec<_>>() }),
        ));
    }
    let owned = collab_task_ops::list_owned_open(db, repo.id, &s.id, OBJECT_LIMIT).await?;
    if !owned.is_empty() {
        findings.push(finding(
            "TASKS_OWNED",
            ESTABLISHED,
            "info",
            format!("The session is the declared owner of {} unfinished task(s).", owned.len()),
            "collab_task",
            None,
            &["owner_session_id", "status"],
            json!({ "tasks": owned.iter().map(|t| json!({"id": t.id, "status": t.status})).collect::<Vec<_>>() }),
        ));
    }
    add_fallback(&mut findings);
    Ok(Diagnosis {
        subject: json!({ "type": "session", "id": s.id }),
        as_of: now,
        findings,
        not_covered: NOT_COVERED,
        authority: "derived_from_records",
    })
}

/// Explain a task.
pub async fn for_task(
    db: &DatabaseConnection,
    repo: &repository::Model,
    task_id: &str,
    now: DateTime<Utc>,
) -> CollabResult<Diagnosis> {
    let t: collab_task::Model = task::find(db, repo.id, task_id).await?;
    let mut findings = vec![finding(
        "TASK_DECLARED_STATE",
        ESTABLISHED,
        if matches!(t.status.as_str(), "blocked" | "waiting") { "attention" } else { "info" },
        format!(
            "The task declares status {} (a claim by its owner or a user, not verified); last changed {} s ago.",
            t.status,
            secs_since(t.updated_at, now)
        ),
        "collab_task",
        Some(&t.id),
        &["status", "version", "updated_at"],
        json!({ "status": t.status, "version": t.version, "updated_at": t.updated_at }),
    )];

    if t.status == "done" {
        findings.push(finding(
            "TASK_DONE",
            ESTABLISHED,
            "info",
            "The task is done and cannot be claimed until reopened.",
            "collab_task",
            Some(&t.id),
            &["status"],
            json!({}),
        ));
    }
    match &t.owner_session_id {
        None => findings.push(finding(
            "TASK_UNOWNED",
            ESTABLISHED,
            "attention",
            if t.status == "done" {
                "The task has no owner session; it must be reopened before it can be claimed."
            } else {
                "The task has no owner session; an eligible session of the repository may claim it."
            },
            "collab_task",
            Some(&t.id),
            &["owner_session_id"],
            json!({}),
        )),
        Some(owner_id) => {
            match agent_session_ops::find_in_repo(db, repo.id, owner_id).await? {
                None => findings.push(finding(
                    "OWNER_SESSION_MISSING",
                    ESTABLISHED,
                    "attention",
                    "The owner session no longer exists in this repository.",
                    "collab_task",
                    Some(&t.id),
                    &["owner_session_id"],
                    json!({ "owner_session_id": owner_id }),
                )),
                Some(owner) => {
                    let state = state_of(&owner, now);
                    findings.extend(session_state_findings(&owner, now).into_iter().map(
                        |mut f| {
                            f.details["role"] = json!("owner");
                            f
                        },
                    ));
                    if t.status != "done" {
                        findings.push(match state {
                    "ended" | "expired" => finding(
                        "OWNER_CAN_BE_REPLACED",
                        ESTABLISHED,
                        "attention",
                        "The owner session has ended or expired, so another session may claim the task.",
                        "agent_session",
                        Some(&owner.id),
                        &["ended_at", "expires_at"],
                        json!({ "owner_state": state }),
                    ),
                    _ => finding(
                        "OWNER_MAY_BE_ACTIVE",
                        ESTABLISHED,
                        "info",
                        "The owner session has neither ended nor expired, so a claim by another session is refused; coordinate with it or ask a user to reassign.",
                        "agent_session",
                        Some(&owner.id),
                        &["ended_at", "expires_at"],
                        json!({ "owner_state": state }),
                    ),
                });
                    }
                    findings.push(access_finding(db, repo, &owner).await?);
                }
            }
        }
    }

    // Open questions of the task.
    for q in
        collab_question_ops::list_page(db, repo.id, &t.id, Some("open"), None, OBJECT_LIMIT).await?
    {
        let mut details = json!({
            "question_id": q.id, "age_secs": secs_since(q.created_at, now),
            "recipient_kind": q.recipient_kind,
        });
        findings.push(finding(
            "QUESTION_OPEN",
            ESTABLISHED,
            "attention",
            format!(
                "A question has been open for {} s without an answer.",
                secs_since(q.created_at, now)
            ),
            "collab_question",
            Some(&q.id),
            &["status", "created_at", "recipient_kind"],
            details.clone(),
        ));
        if let (RECIPIENT, Some(target)) = (q.recipient_kind.as_str(), q.recipient_id.as_deref()) {
            let target_session = agent_session_ops::find_in_repo(db, repo.id, target).await?;
            let target_state = target_session.as_ref().map(|s| state_of(s, now));
            details["recipient_session_state"] = json!(target_state);
            if matches!(target_state, Some("ended" | "expired")) || target_session.is_none() {
                findings.push(finding(
                    "QUESTION_TARGET_GONE",
                    ESTABLISHED,
                    "attention",
                    "The session this question is addressed to has ended or expired and cannot answer it.",
                    "agent_session",
                    Some(target),
                    &["ended_at", "expires_at"],
                    details.clone(),
                ));
            }
            let state =
                notice_state(db, target, kind::QUESTION_ASKED, source::QUESTION, &q.id).await?;
            findings.push(notice_finding(
                "QUESTION_NOTICE",
                state.as_ref(),
                kind::QUESTION_ASKED,
                &q.id,
            ));
        }
    }

    // Pending handoffs of the task.
    let handoffs = collab_handoff::Entity::find()
        .filter(collab_handoff::Column::RepoId.eq(repo.id))
        .filter(collab_handoff::Column::TaskId.eq(&t.id))
        .filter(collab_handoff::Column::Status.eq(collab_handoff_ops::status::PENDING))
        .order_by_asc(collab_handoff::Column::CreatedAt)
        .order_by_asc(collab_handoff::Column::Id)
        .limit(OBJECT_LIMIT + 1)
        .all(db)
        .await?;
    if handoffs.len() > OBJECT_LIMIT as usize {
        findings.push(finding(
            "HANDOFFS_TRUNCATED",
            UNKNOWN,
            "info",
            "Additional pending handoffs are not included in this diagnosis.",
            "collab_handoff",
            None,
            &["status"],
            json!({"limit": OBJECT_LIMIT}),
        ));
    }
    for h in handoffs.iter().take(OBJECT_LIMIT as usize) {
        let target = agent_session_ops::find_in_repo(db, repo.id, &h.to_session_id).await?;
        let target_state = target.as_ref().map(|s| state_of(s, now));
        findings.push(finding(
            "HANDOFF_PENDING",
            ESTABLISHED,
            "attention",
            format!("A handoff has been pending for {} s.", secs_since(h.created_at, now)),
            "collab_handoff",
            Some(&h.id),
            &["status", "created_at", "to_session_id"],
            json!({ "handoff_id": h.id, "age_secs": secs_since(h.created_at, now), "target_state": target_state }),
        ));
        if matches!(target_state, Some("ended" | "expired")) || target.is_none() {
            findings.push(finding(
                "HANDOFF_TARGET_GONE",
                ESTABLISHED,
                "attention",
                "The session the handoff is addressed to has ended or expired and cannot accept it. The creator can cancel it and create a new handoff; that is two steps, not one atomic move.",
                "agent_session",
                Some(&h.to_session_id),
                &["ended_at", "expires_at"],
                json!({ "handoff_id": h.id, "target_state": target_state }),
            ));
        }
        let state = notice_state(
            db,
            &h.to_session_id,
            kind::HANDOFF_RECEIVED,
            source::HANDOFF,
            &h.id,
        )
        .await?;
        findings.push(notice_finding(
            "HANDOFF_NOTICE",
            state.as_ref(),
            kind::HANDOFF_RECEIVED,
            &h.id,
        ));
    }

    let escalation = collab_escalation_ops::get(db, repo.id).await?;
    findings.push(finding(
        "ESCALATION_SETTINGS",
        ESTABLISHED,
        "info",
        if escalation.as_ref().is_some_and(|e| e.enabled) {
            "Escalation reminders are enabled for this repository."
        } else {
            "Escalation reminders are not enabled for this repository, so waiting items produce no reminder."
        },
        "collab_escalation_settings",
        None,
        &["enabled"],
        json!({ "enabled": escalation.as_ref().is_some_and(|e| e.enabled) }),
    ));

    add_fallback(&mut findings);
    Ok(Diagnosis {
        subject: json!({ "type": "task", "id": t.id }),
        as_of: now,
        findings,
        not_covered: NOT_COVERED,
        authority: "derived_from_records",
    })
}

const RECIPIENT: &str = "session";

/// When nothing in the records explains a stall, say so instead of guessing.
fn add_fallback(findings: &mut Vec<Finding>) {
    if findings.iter().all(|f| f.severity != "attention") {
        findings.push(finding(
            "NO_CAUSE_IN_RECORDS",
            UNKNOWN,
            "info",
            "The collaboration records show nothing that explains a stall. Causes outside the records cannot be determined here and are not attributed to any agent.",
            "collaboration_records",
            None,
            &[],
            json!({}),
        ));
    }
}
