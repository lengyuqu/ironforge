//! Notification escalation (O05): remind when something has waited too long.
//!
//! Per-repository settings (managed by repository admins) say how long a
//! blocked/waiting task, an unanswered question, or a pending handoff whose
//! target expired may sit before the responsible *session* gets an
//! `escalation` notice. There is no background scheduler: [`evaluate_for_session`]
//! runs lazily when a session reads its notices or inbox, and [`evaluate_repo`]
//! is the entry point a periodic task can call later.
//!
//! Rules this module keeps:
//! * Conditions are re-derived from current state every time. An escalation
//!   whose condition ended (answered, cancelled, unblocked, handoff settled,
//!   feature switched off) is marked `resolved` and stops asking for attention.
//! * One unacknowledged reminder per recipient and source at a time; a repeat
//!   within an episode needs `repeat_after_secs > 0`, an acknowledgement of the
//!   previous one and the interval to have passed since it. A new episode (the
//!   condition was seen to end and came back) starts a new reminder. The database unique key on
//!   `(recipient, kind, source, generation)` decides races between evaluators.
//! * Nothing is emitted inside the quiet window; the condition is re-evaluated
//!   next time, so a reminder is deferred, not lost.
//! * Recipients must be live sessions of the repository whose account is still
//!   in the working group ([`super::access::is_collab_member`]).
//! * "Since when" is a lower bound: a task's time in `blocked` is measured from
//!   its `updated_at`, which any later edit also moves, so a reminder can come
//!   late but never early.

use std::collections::HashMap;

use chrono::{DateTime, Duration, Timelike, Utc};
use rg_db::entities::{agent_session, collab_escalation_setting, collab_notice, repository};
use rg_db::ops::collab_escalation_ops::{self, SaveOutcome};
use rg_db::ops::collab_notice_ops::{self, kind, source, NewNotice};
use rg_db::ops::{
    agent_session_ops, collab_handoff_ops, collab_question_ops, collab_task_ops,
};
use sea_orm::{Condition, DatabaseConnection};
use serde::{Deserialize, Serialize};

use super::access;
use super::error::{CollabError, CollabResult};
use super::session::state_of;

pub const MIN_THRESHOLD_SECS: i64 = 60;
pub const MIN_REPEAT_SECS: i64 = 300;
pub const MAX_SECS: i64 = 30 * 24 * 3600;
/// Candidates examined per category in one evaluation.
pub const CANDIDATE_LIMIT: u64 = 200;

pub const CONDITION_TASK_BLOCKED: &str = "task_blocked";
pub const CONDITION_QUESTION_UNANSWERED: &str = "question_unanswered";
pub const CONDITION_HANDOFF_TARGET_EXPIRED: &str = "handoff_target_expired";

/// Quiet window in minutes after 00:00 UTC (the repository has no time zone).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuietHours {
    pub start_minute: i32,
    pub end_minute: i32,
}

impl QuietHours {
    fn contains(&self, minute_of_day: i32) -> bool {
        if self.start_minute < self.end_minute {
            (self.start_minute..self.end_minute).contains(&minute_of_day)
        } else {
            minute_of_day >= self.start_minute || minute_of_day < self.end_minute
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SettingsView {
    pub enabled: bool,
    pub blocked_after_secs: i64,
    pub question_after_secs: i64,
    pub handoff_after_secs: i64,
    /// 0 = remind once per episode.
    pub repeat_after_secs: i64,
    pub quiet_hours: Option<QuietHours>,
    pub timezone: &'static str,
    /// Send as `expected_version`; 0 = nothing stored yet.
    pub version: i64,
    /// `default` (nothing stored, escalation off) or `stored`.
    pub source: &'static str,
    pub updated_at: Option<DateTime<Utc>>,
    pub updated_by_user_id: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SettingsInput {
    pub expected_version: i64,
    pub enabled: bool,
    pub blocked_after_secs: i64,
    pub question_after_secs: i64,
    pub handoff_after_secs: i64,
    pub repeat_after_secs: i64,
    pub quiet_hours: Option<QuietHours>,
}

fn defaults() -> SettingsView {
    SettingsView {
        enabled: false,
        blocked_after_secs: 4 * 3600,
        question_after_secs: 3600,
        handoff_after_secs: 900,
        repeat_after_secs: 0,
        quiet_hours: None,
        timezone: "UTC",
        version: 0,
        source: "default",
        updated_at: None,
        updated_by_user_id: None,
    }
}

fn view(row: &collab_escalation_setting::Model) -> SettingsView {
    SettingsView {
        enabled: row.enabled,
        blocked_after_secs: row.blocked_after_secs,
        question_after_secs: row.question_after_secs,
        handoff_after_secs: row.handoff_after_secs,
        repeat_after_secs: row.repeat_after_secs,
        quiet_hours: match (row.quiet_start_minute, row.quiet_end_minute) {
            (Some(start_minute), Some(end_minute)) => Some(QuietHours { start_minute, end_minute }),
            _ => None,
        },
        timezone: "UTC",
        version: row.version,
        source: "stored",
        updated_at: Some(row.updated_at),
        updated_by_user_id: Some(row.updated_by_user_id),
    }
}

pub async fn get_settings(db: &DatabaseConnection, repo_id: i64) -> CollabResult<SettingsView> {
    Ok(collab_escalation_ops::get(db, repo_id)
        .await?
        .map(|row| view(&row))
        .unwrap_or_else(defaults))
}

fn validate(input: &SettingsInput) -> CollabResult<()> {
    for (name, value) in [
        ("blocked_after_secs", input.blocked_after_secs),
        ("question_after_secs", input.question_after_secs),
        ("handoff_after_secs", input.handoff_after_secs),
    ] {
        if !(MIN_THRESHOLD_SECS..=MAX_SECS).contains(&value) {
            return Err(CollabError::bad_request(format!(
                "{name} must be between {MIN_THRESHOLD_SECS} and {MAX_SECS} seconds"
            )));
        }
    }
    if input.repeat_after_secs != 0 && !(MIN_REPEAT_SECS..=MAX_SECS).contains(&input.repeat_after_secs) {
        return Err(CollabError::bad_request(format!(
            "repeat_after_secs must be 0 (remind once) or between {MIN_REPEAT_SECS} and {MAX_SECS}"
        )));
    }
    if let Some(q) = &input.quiet_hours {
        let ok = (0..1440).contains(&q.start_minute)
            && (0..1440).contains(&q.end_minute)
            && q.start_minute != q.end_minute;
        if !ok {
            return Err(CollabError::bad_request(
                "quiet_hours minutes must be within 0..1439 and start must differ from end",
            ));
        }
    }
    if input.expected_version < 0 {
        return Err(CollabError::bad_request("expected_version must not be negative"));
    }
    Ok(())
}

/// Replace the repository's settings. The caller has established repository
/// admin rights. `expected_version` is 0 for the first save; a retry that
/// finds exactly the requested values already stored succeeds without change.
pub async fn put_settings(
    db: &DatabaseConnection,
    repo_id: i64,
    user_id: i64,
    input: SettingsInput,
    now: DateTime<Utc>,
) -> CollabResult<SettingsView> {
    validate(&input)?;
    let same_content = |v: &SettingsView| {
        v.enabled == input.enabled
            && v.blocked_after_secs == input.blocked_after_secs
            && v.question_after_secs == input.question_after_secs
            && v.handoff_after_secs == input.handoff_after_secs
            && v.repeat_after_secs == input.repeat_after_secs
            && v.quiet_hours == input.quiet_hours
    };
    if let Some(row) = collab_escalation_ops::get(db, repo_id).await? {
        let current = view(&row);
        if same_content(&current) {
            return Ok(current);
        }
    }
    let row = collab_escalation_setting::Model {
        repo_id,
        enabled: input.enabled,
        blocked_after_secs: input.blocked_after_secs,
        question_after_secs: input.question_after_secs,
        handoff_after_secs: input.handoff_after_secs,
        repeat_after_secs: input.repeat_after_secs,
        quiet_start_minute: input.quiet_hours.map(|q| q.start_minute),
        quiet_end_minute: input.quiet_hours.map(|q| q.end_minute),
        version: 0,
        updated_by_user_id: user_id,
        updated_at: now,
    };
    match collab_escalation_ops::save_cas(db, input.expected_version, row).await? {
        SaveOutcome::Saved(saved) => Ok(view(&saved)),
        SaveOutcome::Conflict { current } => {
            let current_version = current.map(|c| c.version).unwrap_or(0);
            Err(CollabError::VersionConflict { current_version })
        }
    }
}

// --------------------------------------------------------------- evaluation

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct Report {
    pub enabled: bool,
    /// Candidates whose condition holds (before thresholds and dedupe).
    pub candidates: u32,
    pub emitted: u32,
    pub resolved: u32,
    /// New reminders were held back by the quiet window.
    pub quiet_window: bool,
}

struct Candidate {
    recipient: String,
    task_id: String,
    source_type: &'static str,
    source_id: String,
    condition: &'static str,
    since: DateTime<Utc>,
    threshold_secs: i64,
    extra: serde_json::Value,
}

struct Lookup<'a> {
    db: &'a DatabaseConnection,
    repo: &'a repository::Model,
    sessions: HashMap<String, Option<agent_session::Model>>,
    access: HashMap<i64, bool>,
}

impl<'a> Lookup<'a> {
    fn new(db: &'a DatabaseConnection, repo: &'a repository::Model) -> Self {
        Self { db, repo, sessions: HashMap::new(), access: HashMap::new() }
    }

    async fn session(&mut self, id: &str) -> CollabResult<Option<agent_session::Model>> {
        if !self.sessions.contains_key(id) {
            let found = agent_session_ops::find_in_repo(self.db, self.repo.id, id).await?;
            self.sessions.insert(id.to_string(), found);
        }
        Ok(self.sessions[id].clone())
    }

    /// A live session of this repository whose account is still in the working group.
    async fn may_notify(&mut self, id: &str, now: DateTime<Utc>) -> CollabResult<bool> {
        let Some(session) = self.session(id).await? else { return Ok(false) };
        if !matches!(state_of(&session, now), "online" | "offline") {
            return Ok(false);
        }
        if !self.access.contains_key(&session.user_id) {
            let ok = access::is_collab_member(self.db, self.repo, session.user_id).await?;
            self.access.insert(session.user_id, ok);
        }
        Ok(self.access[&session.user_id])
    }
}

/// Lazily evaluate the escalations of one session (the one reading its
/// notices or inbox). Only reminders addressed to that session are touched.
pub async fn evaluate_for_session(
    db: &DatabaseConnection,
    repo: &repository::Model,
    session: &agent_session::Model,
    now: DateTime<Utc>,
) -> CollabResult<Report> {
    evaluate(db, repo, Some(&session.id), now).await
}

/// Evaluate every session of a repository. This is what a periodic task
/// should call; nothing in the server calls it on a timer.
pub async fn evaluate_repo(
    db: &DatabaseConnection,
    repo: &repository::Model,
    now: DateTime<Utc>,
) -> CollabResult<Report> {
    evaluate(db, repo, None, now).await
}

/// Evaluate all repositories that have escalation enabled (for a periodic task).
pub async fn evaluate_all_enabled(
    db: &DatabaseConnection,
    now: DateTime<Utc>,
) -> CollabResult<Vec<(i64, Report)>> {
    let mut out = Vec::new();
    for repo_id in collab_escalation_ops::list_enabled_repo_ids(db).await? {
        let Some(repo) = rg_db::ops::repo_ops::find_by_id(db, repo_id).await? else { continue };
        out.push((repo_id, evaluate(db, &repo, None, now).await?));
    }
    Ok(out)
}

async fn evaluate(
    db: &DatabaseConnection,
    repo: &repository::Model,
    scope: Option<&str>,
    now: DateTime<Utc>,
) -> CollabResult<Report> {
    let stored = collab_escalation_ops::get(db, repo.id).await?;
    let enabled = stored.as_ref().is_some_and(|s| s.enabled);
    let mut report = Report { enabled, ..Report::default() };

    report.resolved = resolve_ended(db, repo.id, scope, enabled, now).await?;
    let Some(settings) = stored.filter(|s| s.enabled) else { return Ok(report) };

    let minute = (now.hour() * 60 + now.minute()) as i32;
    if let (Some(start_minute), Some(end_minute)) =
        (settings.quiet_start_minute, settings.quiet_end_minute)
    {
        if (QuietHours { start_minute, end_minute }).contains(minute) {
            report.quiet_window = true;
            return Ok(report);
        }
    }

    let mut lookup = Lookup::new(db, repo);
    let candidates = gather(db, repo.id, scope, &settings, &mut lookup, now).await?;
    report.candidates = candidates.len() as u32;
    for candidate in candidates {
        if (now - candidate.since).num_seconds() < candidate.threshold_secs {
            continue;
        }
        if !lookup.may_notify(&candidate.recipient, now).await? {
            continue;
        }
        let latest = collab_notice_ops::find_for_source(
            db,
            &candidate.recipient,
            kind::ESCALATION,
            candidate.source_type,
            &candidate.source_id,
        )
        .await?;
        let generation = match latest {
            None => 1,
            // The condition was seen to end (or the feature was off) since the
            // last reminder and holds again: a new episode.
            Some(l) if l.resolved_at.is_some() => l.generation + 1,
            // A reminder that still asks for attention is never stacked.
            Some(l) if l.ack_at.is_none() => continue,
            // Same episode, acknowledged: repeat only if configured, measured
            // from the later of creation and acknowledgement.
            Some(l) if settings.repeat_after_secs > 0 => {
                let since_last = l.ack_at.map_or(l.created_at, |ack| ack.max(l.created_at));
                if now >= since_last + Duration::seconds(settings.repeat_after_secs) {
                    l.generation + 1
                } else {
                    continue;
                }
            }
            Some(_) => continue,
        };
        let mut hint = serde_json::json!({
            "condition": candidate.condition,
            "since": candidate.since,
            "threshold_secs": candidate.threshold_secs,
            "settings_version": settings.version,
        });
        if let (Some(extra), Some(map)) = (candidate.extra.as_object(), hint.as_object_mut()) {
            map.extend(extra.clone());
        }
        let emitted = collab_notice_ops::insert_if_new(
            db,
            &NewNotice {
                repo_id: repo.id,
                task_id: candidate.task_id,
                recipient_session_id: candidate.recipient,
                kind: kind::ESCALATION,
                source_type: candidate.source_type,
                source_id: candidate.source_id,
                generation,
                hint_json: hint.to_string(),
                created_at: now,
            },
        )
        .await?;
        if emitted {
            report.emitted += 1;
        }
    }
    Ok(report)
}

async fn gather(
    db: &DatabaseConnection,
    repo_id: i64,
    scope: Option<&str>,
    settings: &collab_escalation_setting::Model,
    lookup: &mut Lookup<'_>,
    now: DateTime<Utc>,
) -> CollabResult<Vec<Candidate>> {
    let mut out = Vec::new();

    for task in collab_escalation_ops::list_stalled_tasks(db, repo_id, scope, CANDIDATE_LIMIT).await? {
        let Some(owner) = task.owner_session_id.clone() else { continue };
        out.push(Candidate {
            recipient: owner,
            task_id: task.id.clone(),
            source_type: source::TASK,
            source_id: task.id.clone(),
            condition: CONDITION_TASK_BLOCKED,
            since: task.updated_at,
            threshold_secs: settings.blocked_after_secs,
            extra: serde_json::json!({ "task_status": task.status }),
        });
    }

    for question in
        collab_question_ops::list_open(db, repo_id, Condition::all(), CANDIDATE_LIMIT).await?
    {
        let mut recipients = Vec::new();
        if question.creator_kind == "session" {
            recipients.push(question.creator_id.clone());
        }
        if question.recipient_kind == "session" {
            if let Some(id) = &question.recipient_id {
                if !recipients.contains(id) {
                    recipients.push(id.clone());
                }
            }
        }
        for recipient in recipients {
            if scope.is_some_and(|s| s != recipient) {
                continue;
            }
            out.push(Candidate {
                recipient,
                task_id: question.task_id.clone(),
                source_type: source::QUESTION,
                source_id: question.id.clone(),
                condition: CONDITION_QUESTION_UNANSWERED,
                since: question.created_at,
                threshold_secs: settings.question_after_secs,
                extra: serde_json::json!({ "recipient_kind": question.recipient_kind }),
            });
        }
    }

    for handoff in
        collab_escalation_ops::list_pending_handoffs(db, repo_id, scope, CANDIDATE_LIMIT).await?
    {
        let Some(target) = lookup.session(&handoff.to_session_id).await? else { continue };
        let target_gone_at = match (target.ended_at, target.expires_at <= now) {
            (Some(ended), true) => Some(ended.min(target.expires_at)),
            (Some(ended), false) => Some(ended),
            (None, true) => Some(target.expires_at),
            (None, false) => None,
        };
        let Some(since) = target_gone_at else { continue };
        out.push(Candidate {
            recipient: handoff.from_session_id.clone(),
            task_id: handoff.task_id.clone(),
            source_type: source::HANDOFF,
            source_id: handoff.id.clone(),
            condition: CONDITION_HANDOFF_TARGET_EXPIRED,
            since,
            threshold_secs: settings.handoff_after_secs,
            extra: serde_json::json!({ "target_session_id": handoff.to_session_id }),
        });
    }
    Ok(out)
}

/// Mark unresolved escalations (acknowledged or not) whose condition no longer
/// holds as resolved; this is what delimits one episode of a condition from the next.
async fn resolve_ended(
    db: &DatabaseConnection,
    repo_id: i64,
    scope: Option<&str>,
    enabled: bool,
    now: DateTime<Utc>,
) -> CollabResult<u32> {
    let mut resolved = 0;
    for notice in collab_notice_ops::open_escalations(db, repo_id, scope).await? {
        if enabled && still_holds(db, repo_id, &notice).await? {
            continue;
        }
        if collab_notice_ops::resolve(db, &notice.id, now).await? {
            resolved += 1;
        }
    }
    Ok(resolved)
}

async fn still_holds(
    db: &DatabaseConnection,
    repo_id: i64,
    notice: &collab_notice::Model,
) -> CollabResult<bool> {
    Ok(match notice.source_type.as_str() {
        source::TASK => collab_task_ops::find_in_repo(db, repo_id, &notice.source_id)
            .await?
            .is_some_and(|t| {
                matches!(t.status.as_str(), "blocked" | "waiting")
                    && t.owner_session_id.as_deref() == Some(notice.recipient_session_id.as_str())
            }),
        source::QUESTION => collab_question_ops::find_in_repo(db, repo_id, &notice.source_id)
            .await?
            .is_some_and(|q| q.status == collab_question_ops::status::OPEN),
        source::HANDOFF => collab_handoff_ops::find_in_repo(db, repo_id, &notice.source_id)
            .await?
            .is_some_and(|h| h.status == collab_handoff_ops::status::PENDING),
        _ => false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quiet_hours_wrap_around_midnight() {
        let day = QuietHours { start_minute: 9 * 60, end_minute: 17 * 60 };
        assert!(day.contains(9 * 60) && day.contains(16 * 60 + 59));
        assert!(!day.contains(17 * 60) && !day.contains(0));
        let night = QuietHours { start_minute: 22 * 60, end_minute: 7 * 60 };
        assert!(night.contains(23 * 60) && night.contains(0) && night.contains(6 * 60 + 59));
        assert!(!night.contains(7 * 60) && !night.contains(12 * 60));
    }

    fn input() -> SettingsInput {
        SettingsInput {
            expected_version: 0,
            enabled: true,
            blocked_after_secs: 3600,
            question_after_secs: 3600,
            handoff_after_secs: 900,
            repeat_after_secs: 0,
            quiet_hours: None,
        }
    }

    #[test]
    fn settings_are_bounded() {
        assert!(validate(&input()).is_ok());
        for mutate in [
            |i: &mut SettingsInput| i.blocked_after_secs = 59,
            |i: &mut SettingsInput| i.question_after_secs = MAX_SECS + 1,
            |i: &mut SettingsInput| i.repeat_after_secs = 299,
            |i: &mut SettingsInput| i.expected_version = -1,
            |i: &mut SettingsInput| {
                i.quiet_hours = Some(QuietHours { start_minute: 5, end_minute: 5 })
            },
            |i: &mut SettingsInput| {
                i.quiet_hours = Some(QuietHours { start_minute: 0, end_minute: 1440 })
            },
        ] {
            let mut i = input();
            mutate(&mut i);
            assert!(validate(&i).is_err());
        }
        let mut i = input();
        i.repeat_after_secs = 600;
        assert!(validate(&i).is_ok());
    }
}
