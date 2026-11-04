//! Repository overview for people (O02): what is waiting, stalled or moving.
//!
//! Each section is its own *live query* against the authoritative records;
//! there is no snapshot and no atomic view across sections, so re-read an
//! object and its version before acting on it. Items only locate records.
//! Nothing here treats an online session as "working", a read as agreement, or
//! an agent's `done` as verified: states are declarations and the flags only
//! say where a declaration looks unreliable (no owner, owner not online).
//! The caller has already been authorised to read the repository.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use rg_db::ops::{agent_session_ops, collab_handoff_ops, collab_task_ops};
use sea_orm::DatabaseConnection;
use serde::Serialize;

use super::error::{CollabError, CollabResult};
use super::inbox;
use super::session::state_of;
use super::ORIGIN_USER_CONTENT;

pub const AWAITING_ME: &str = "awaiting_me";
pub const BLOCKED_WAITING: &str = "blocked_waiting";
pub const PENDING_HANDOFFS: &str = "pending_handoffs";
pub const IN_PROGRESS: &str = "in_progress";
/// Default order of sections.
pub const SECTIONS: [&str; 4] = [AWAITING_ME, BLOCKED_WAITING, PENDING_HANDOFFS, IN_PROGRESS];

pub const NOTES: [&str; 3] = [
    "each section is a separate live query; there is no atomic view across sections",
    "owner_state comes from heartbeats and leases: online does not mean working, and a missing heartbeat does not mean idle",
    "task statuses are declarations by participants, not verification",
];

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct OverviewItem {
    /// `task`, `open_question` or `pending_handoff`.
    pub object_type: &'static str,
    pub object_id: String,
    pub task_id: String,
    /// Client-written text; untrusted.
    pub title: String,
    /// Declared state of the record.
    pub state: String,
    pub occurred_at: DateTime<Utc>,
    /// Task owner, or the handoff target, when there is one.
    pub session_id: Option<String>,
    /// `online`, `offline`, `expired`, `ended`, `missing`, or `null` without a session.
    pub session_state: Option<&'static str>,
    /// Where a declaration looks unreliable: `unowned`, `session_not_online`.
    pub flags: Vec<&'static str>,
    pub origin: &'static str,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Section {
    pub name: &'static str,
    /// `false` when this viewer may not see the section (see `reason`).
    pub available: bool,
    pub reason: Option<&'static str>,
    pub items: Vec<OverviewItem>,
    pub as_of: DateTime<Utc>,
    /// More matched than `limit`; page through `continue_with`.
    pub truncated: bool,
    /// The endpoint that lists the same records with cursor paging.
    pub continue_with: &'static str,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Overview {
    pub sections: Vec<Section>,
    pub as_of: DateTime<Utc>,
    /// Always `per_section_live_query`.
    pub consistency: &'static str,
    pub notes: [&'static str; 3],
}

pub fn parse_sections(raw: Option<&str>) -> CollabResult<Vec<&'static str>> {
    let Some(raw) = raw.filter(|r| !r.trim().is_empty()) else {
        return Ok(SECTIONS.to_vec());
    };
    let mut out = Vec::new();
    for name in raw.split(',').map(str::trim) {
        let known = SECTIONS.iter().find(|s| **s == name).ok_or_else(|| {
            CollabError::bad_request(format!(
                "unknown section `{name}`; use any of {}",
                SECTIONS.join(", ")
            ))
        })?;
        if !out.contains(known) {
            out.push(*known);
        }
    }
    Ok(out)
}

struct States<'a> {
    db: &'a DatabaseConnection,
    repo_id: i64,
    now: DateTime<Utc>,
    seen: HashMap<String, &'static str>,
}

impl States<'_> {
    async fn of(&mut self, session_id: &str) -> CollabResult<&'static str> {
        if let Some(state) = self.seen.get(session_id) {
            return Ok(state);
        }
        let state = match agent_session_ops::find_in_repo(self.db, self.repo_id, session_id)
            .await
            .map_err(CollabError::Internal)?
        {
            Some(session) => state_of(&session, self.now),
            None => "missing",
        };
        self.seen.insert(session_id.to_string(), state);
        Ok(state)
    }

    async fn describe(
        &mut self,
        session_id: Option<&str>,
    ) -> CollabResult<(Option<&'static str>, Vec<&'static str>)> {
        Ok(match session_id {
            None => (None, vec!["unowned"]),
            Some(id) => {
                let state = self.of(id).await?;
                let flags = if state == "online" {
                    Vec::new()
                } else {
                    vec!["session_not_online"]
                };
                (Some(state), flags)
            }
        })
    }
}

/// Build the requested sections. `can_write` gates `awaiting_me`: answering a
/// question needs write access, so a reader has nothing waiting on them.
pub async fn build(
    db: &DatabaseConnection,
    repo_id: i64,
    user_id: i64,
    can_write: bool,
    sections: &[&'static str],
    limit: u64,
    now: DateTime<Utc>,
) -> CollabResult<Overview> {
    let mut states = States {
        db,
        repo_id,
        now,
        seen: HashMap::new(),
    };
    let mut out = Vec::new();
    for name in sections {
        out.push(match *name {
            AWAITING_ME => awaiting_me(db, repo_id, user_id, can_write, limit, now).await?,
            BLOCKED_WAITING => {
                tasks(
                    db,
                    repo_id,
                    &mut states,
                    BLOCKED_WAITING,
                    &["blocked", "waiting"],
                    limit,
                    now,
                )
                .await?
            }
            IN_PROGRESS => {
                tasks(
                    db,
                    repo_id,
                    &mut states,
                    IN_PROGRESS,
                    &["in_progress"],
                    limit,
                    now,
                )
                .await?
            }
            PENDING_HANDOFFS => pending_handoffs(db, repo_id, &mut states, limit, now).await?,
            other => unreachable!("parse_sections admitted unknown section {other}"),
        });
    }
    Ok(Overview {
        sections: out,
        as_of: now,
        consistency: "per_section_live_query",
        notes: NOTES,
    })
}

async fn awaiting_me(
    db: &DatabaseConnection,
    repo_id: i64,
    user_id: i64,
    can_write: bool,
    limit: u64,
    now: DateTime<Utc>,
) -> CollabResult<Section> {
    let continue_with = "GET …/collaboration/inbox/user";
    if !can_write {
        return Ok(Section {
            name: AWAITING_ME,
            available: false,
            reason: Some("write access is needed to answer questions"),
            items: Vec::new(),
            as_of: now,
            truncated: false,
            continue_with,
        });
    }
    // The inbox also lists answers to the viewer's own questions; only open
    // questions are "waiting for me". Read one past `limit` of those: ask for
    // enough inbox items that filtering cannot hide a truncation.
    let inbox = inbox::for_user(db, repo_id, user_id, None, super::MAX_LIMIT, now).await?;
    let mut open: Vec<_> = inbox
        .items
        .into_iter()
        .filter(|i| i.r#type == inbox::OPEN_QUESTION)
        .collect();
    let truncated = open.len() as u64 > limit || inbox.truncated;
    open.truncate(limit as usize);
    Ok(Section {
        name: AWAITING_ME,
        available: true,
        reason: None,
        items: open
            .into_iter()
            .map(|i| OverviewItem {
                object_type: "open_question",
                object_id: i.object_id,
                task_id: i.task_id,
                title: i.title,
                state: i.state,
                occurred_at: i.occurred_at,
                session_id: None,
                session_state: None,
                flags: Vec::new(),
                origin: ORIGIN_USER_CONTENT,
            })
            .collect(),
        as_of: now,
        truncated,
        continue_with,
    })
}

async fn tasks(
    db: &DatabaseConnection,
    repo_id: i64,
    states: &mut States<'_>,
    name: &'static str,
    statuses: &[&str],
    limit: u64,
    now: DateTime<Utc>,
) -> CollabResult<Section> {
    let mut rows = collab_task_ops::list_with_statuses(db, repo_id, statuses, limit + 1).await?;
    let truncated = rows.len() as u64 > limit;
    rows.truncate(limit as usize);
    let mut items = Vec::with_capacity(rows.len());
    for t in rows {
        let (session_state, flags) = states.describe(t.owner_session_id.as_deref()).await?;
        items.push(OverviewItem {
            object_type: "task",
            object_id: t.id.clone(),
            task_id: t.id,
            title: t.title,
            state: t.status,
            occurred_at: t.updated_at,
            session_id: t.owner_session_id,
            session_state,
            flags,
            origin: ORIGIN_USER_CONTENT,
        });
    }
    Ok(Section {
        name,
        available: true,
        reason: None,
        items,
        as_of: now,
        truncated,
        continue_with: "GET …/collaboration/tasks?status=",
    })
}

async fn pending_handoffs(
    db: &DatabaseConnection,
    repo_id: i64,
    states: &mut States<'_>,
    limit: u64,
    now: DateTime<Utc>,
) -> CollabResult<Section> {
    let mut rows = collab_handoff_ops::list_pending_in_repo(db, repo_id, limit + 1).await?;
    let truncated = rows.len() as u64 > limit;
    rows.truncate(limit as usize);
    let mut items = Vec::with_capacity(rows.len());
    for h in rows {
        let (session_state, flags) = states.describe(Some(h.to_session_id.as_str())).await?;
        items.push(OverviewItem {
            object_type: "pending_handoff",
            object_id: h.id,
            task_id: h.task_id,
            title: h.summary,
            state: h.status,
            occurred_at: h.created_at,
            session_id: Some(h.to_session_id),
            session_state,
            flags,
            origin: ORIGIN_USER_CONTENT,
        });
    }
    Ok(Section {
        name: PENDING_HANDOFFS,
        available: true,
        reason: None,
        items,
        as_of: now,
        truncated,
        continue_with: "GET …/collaboration/tasks/{id}/handoffs",
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sections_default_to_all_in_order_and_dedupe() {
        assert_eq!(parse_sections(None).unwrap(), SECTIONS.to_vec());
        assert_eq!(parse_sections(Some("")).unwrap(), SECTIONS.to_vec());
        assert_eq!(
            parse_sections(Some("in_progress, awaiting_me,in_progress")).unwrap(),
            vec![IN_PROGRESS, AWAITING_ME]
        );
    }

    #[test]
    fn unknown_sections_are_rejected_not_ignored() {
        assert!(parse_sections(Some("in_progress,everything")).is_err());
    }
}
