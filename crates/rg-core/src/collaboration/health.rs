//! Collaboration health metrics (R13): blocked tasks, blocked duration, handoff
//! waiting and open questions of one repository, computed from the platform
//! event log (`collab_metric_events`).
//!
//! Design: `ironforge-docs/architecture/agent-collaboration-p2-design-2026-11.md` §5.
//!
//! Rules this module keeps:
//! * Times come from events the server wrote in the business transaction; no
//!   client time is used.
//! * Nothing before the first recorded event is counted as zero or guessed.
//!   `coverage_start` is the earliest event of the repository; objects without
//!   any event (created before the feature, never touched since) have an
//!   *unknown* state until an observation baseline is taken, and a baseline only
//!   says "observed at T", never "since forever".
//! * Task status is a client declaration (`source: user_content`); only the
//!   *timing* of its changes is platform-recorded.
//! * There is no per-session, per-account or per-agent breakdown, and no
//!   message body is read: drill-down is by task only.
//!
//! [`compute`] is a pure function of the loaded facts so the interval maths can
//! be tested without a database.

use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Duration, SecondsFormat, Utc};
use rg_db::entities::collab_metric_event::Model as Event;
use rg_db::ops::collab_metric_ops::{self as ops, event_type as et};
use sea_orm::DatabaseConnection;
use serde::Serialize;
use serde_json::{json, Value};

use super::cursor::{self, KeysetPosition};
use super::error::{CollabError, CollabResult};
use super::{clamp_limit, ORIGIN_USER_CONTENT};

/// Bumped whenever a definition below changes meaning.
pub const METRIC_VERSION: u32 = 1;
pub const DEFAULT_WINDOW_DAYS: i64 = 7;
pub const MAX_WINDOW_DAYS: i64 = 90;

pub const METRIC_BLOCKED_TASKS: &str = "blocked_tasks";
pub const METRIC_BLOCKED_DURATION: &str = "blocked_duration";
pub const METRIC_HANDOFF_WAITING: &str = "handoff_waiting";
pub const METRIC_OPEN_QUESTIONS: &str = "open_questions";
pub const METRIC_STALE_PATH_CLAIMS: &str = "stale_path_claims";
pub const METRICS: [&str; 5] = [
    METRIC_BLOCKED_TASKS,
    METRIC_BLOCKED_DURATION,
    METRIC_HANDOFF_WAITING,
    METRIC_OPEN_QUESTIONS,
    METRIC_STALE_PATH_CLAIMS,
];

pub const DRILL_BLOCKED_AT_TO: &str = "blocked_at_to";
pub const DRILL_ENTERED_BLOCKED: &str = "entered_blocked";
pub const DRILL_UNKNOWN_STATE: &str = "unknown_state";
/// Metrics that can be drilled into; the other ids are shared with [`METRICS`].
pub const DRILL_METRICS: [&str; 6] = [
    DRILL_BLOCKED_AT_TO,
    DRILL_ENTERED_BLOCKED,
    METRIC_BLOCKED_DURATION,
    METRIC_HANDOFF_WAITING,
    METRIC_OPEN_QUESTIONS,
    DRILL_UNKNOWN_STATE,
];

/// UTC half-open window `[from, to)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Window {
    pub from: DateTime<Utc>,
    pub to: DateTime<Utc>,
}

/// Validate a client window: `from < to`, at most 90 days, `to` not in the
/// future; defaults are the last 7 days up to `now`.
pub fn resolve_window(
    from: Option<DateTime<Utc>>,
    to: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> CollabResult<Window> {
    let to = to.unwrap_or(now);
    let from = from.unwrap_or(to - Duration::days(DEFAULT_WINDOW_DAYS));
    if to > now {
        return Err(CollabError::bad_request("`to` must not be in the future"));
    }
    if from >= to {
        return Err(CollabError::bad_request("`from` must be before `to`"));
    }
    if to - from > Duration::days(MAX_WINDOW_DAYS) {
        return Err(CollabError::bad_request(format!(
            "the window may span at most {MAX_WINDOW_DAYS} days"
        )));
    }
    Ok(Window { from, to })
}

/// Parse a comma-separated `metrics` filter; `None` = all metrics.
pub fn parse_metrics(raw: Option<&str>) -> CollabResult<Vec<&'static str>> {
    let Some(raw) = raw.map(str::trim).filter(|r| !r.is_empty()) else {
        return Ok(METRICS.to_vec());
    };
    let mut wanted = Vec::new();
    for part in raw.split(',').map(str::trim) {
        let id = METRICS.iter().find(|m| **m == part).ok_or_else(|| {
            CollabError::bad_request(format!("unknown metric `{part}`; known: {}", METRICS.join(", ")))
        })?;
        if !wanted.contains(id) {
            wanted.push(*id);
        }
    }
    Ok(wanted)
}

// ------------------------------------------------------------------ facts

/// Everything [`compute`] reads.
#[derive(Debug, Default)]
pub struct Facts {
    /// Events with `occurred_at < to`, oldest first.
    pub events: Vec<Event>,
    /// Earliest event time of the repository (including events at or after `to`).
    pub coverage_start: Option<DateTime<Utc>>,
    /// Tasks created before `to`: `(id, created_at)`.
    pub tasks: Vec<(String, DateTime<Utc>)>,
    /// Handoffs created before `to`: `(id, task_id, created_at, status)`.
    pub handoffs: Vec<(String, String, DateTime<Utc>, String)>,
    /// Questions created before `to`: `(id, task_id, created_at, status)`.
    pub questions: Vec<(String, String, DateTime<Utc>, String)>,
}

async fn load_facts(db: &DatabaseConnection, repo_id: i64, to: DateTime<Utc>) -> CollabResult<Facts> {
    let coverage_start = ops::first_occurred_at(db, repo_id).await?;
    let events = ops::load_before(db, repo_id, to).await?.ok_or_else(|| {
        CollabError::coded_conflict(
            "HEALTH_EVENT_LIMIT",
            "too many events to compute this window; use a narrower `to`",
        )
    })?;
    Ok(Facts {
        events,
        coverage_start,
        tasks: ops::task_ids_created_before(db, repo_id, to).await?,
        handoffs: ops::handoff_ids_created_before(db, repo_id, to).await?,
        questions: ops::question_ids_created_before(db, repo_id, to).await?,
    })
}

// ------------------------------------------------------------------ results

#[derive(Debug, Clone, Serialize)]
pub struct DrillLink {
    pub metric: &'static str,
    pub href: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct MetricResult {
    pub id: &'static str,
    pub definition_version: u32,
    /// Who vouches for the *values*: `user_content` (a client declaration, e.g.
    /// task status) or `platform_event`.
    pub source: &'static str,
    /// Where the *timestamps* come from: always server-written events.
    pub timing_source: &'static str,
    /// `complete`, `partial` (a lower bound / part of the data is missing) or
    /// `unknown` (`value` is null and `reason` says why).
    pub status: &'static str,
    pub value: Option<Value>,
    pub reason: Option<&'static str>,
    pub notes: Vec<&'static str>,
    pub drill_down: Vec<DrillLink>,
}

/// Counts of records the metrics could not use. A non-zero entry is a reason
/// to read the affected metric as partial, not as zero.
#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct DataQuality {
    /// Tasks with no usable status timeline (created before the feature and
    /// never touched since, no baseline) — their blocked state is unknown.
    pub unknown_state_tasks: u64,
    /// Tasks whose events contradict each other (excluded from blocked metrics).
    pub inconsistent_tasks: u64,
    /// Pending handoffs without any event (state before coverage is unknown).
    pub unobserved_pending_handoffs: u64,
    /// Handoffs that ended before coverage began; they cannot affect a covered window.
    pub handoffs_closed_before_coverage: u64,
    /// Handoffs with an end event but no start event.
    pub handoffs_missing_start: u64,
    pub unobserved_open_questions: u64,
    pub questions_closed_before_coverage: u64,
    pub questions_missing_start: u64,
    /// Intervals whose end precedes their start (excluded).
    pub negative_intervals: u64,
}

#[derive(Debug, Clone)]
pub struct Computed {
    pub coverage_start: Option<DateTime<Utc>>,
    pub partial: bool,
    pub metrics: Vec<MetricResult>,
    pub data_quality: DataQuality,
    /// Per drill-down metric: task id -> why the task is listed.
    pub per_task: BTreeMap<&'static str, BTreeMap<String, Value>>,
}

// ------------------------------------------------------------------ timelines

#[derive(Debug, Clone)]
struct Segment {
    start: DateTime<Utc>,
    /// `None` = still in this state at the end of the loaded events.
    end: Option<DateTime<Utc>>,
    state: String,
    /// The start is where observation began, not when the state began.
    start_censored: bool,
}

fn is_status_event(kind: &str) -> bool {
    matches!(kind, et::TASK_CREATED | et::TASK_STATUS_CHANGED | et::TASK_STATUS_OBSERVED)
}

/// Segments of one task's status, or `Err(())` if its events contradict.
fn build_timeline(events: &[&Event], coverage_start: DateTime<Utc>) -> Result<Vec<Segment>, ()> {
    let mut segments: Vec<Segment> = Vec::new();
    for (i, event) in events.iter().enumerate() {
        let to_state = event.to_state.clone().ok_or(())?;
        match event.event_type.as_str() {
            et::TASK_CREATED | et::TASK_STATUS_OBSERVED => {
                if i != 0 {
                    return Err(());
                }
                segments.push(Segment {
                    start: event.occurred_at,
                    end: None,
                    state: to_state,
                    start_censored: event.event_type == et::TASK_STATUS_OBSERVED,
                });
            }
            _ => {
                let from_state = event.from_state.clone().ok_or(())?;
                match segments.last_mut() {
                    None => {
                        // First thing we know is a change away from `from_state`:
                        // the task was in it since observation began, no longer.
                        if coverage_start < event.occurred_at {
                            segments.push(Segment {
                                start: coverage_start,
                                end: Some(event.occurred_at),
                                state: from_state,
                                start_censored: true,
                            });
                        }
                    }
                    Some(last) => {
                        if last.state != from_state || event.occurred_at < last.start {
                            return Err(());
                        }
                        last.end = Some(event.occurred_at);
                    }
                }
                segments.push(Segment {
                    start: event.occurred_at,
                    end: None,
                    state: to_state,
                    start_censored: false,
                });
            }
        }
    }
    Ok(segments)
}

fn overlap_ms(start: DateTime<Utc>, end: DateTime<Utc>, from: DateTime<Utc>, to: DateTime<Utc>) -> i64 {
    (end.min(to) - start.max(from)).num_milliseconds().max(0)
}

fn secs(ms: i64) -> i64 {
    ms / 1000
}

fn ts(t: DateTime<Utc>) -> String {
    t.to_rfc3339_opts(SecondsFormat::Millis, true)
}

/// One object's life: when it started waiting and how it ended.
struct Life<'a> {
    task_id: &'a str,
    start: Option<&'a Event>,
    end: Option<&'a Event>,
}

fn lives<'a>(events: &'a [Event], start_types: &[&str], end_types: &[&str]) -> BTreeMap<&'a str, Life<'a>> {
    let mut out: BTreeMap<&str, Life> = BTreeMap::new();
    for event in events {
        let is_start = start_types.contains(&event.event_type.as_str());
        let is_end = end_types.contains(&event.event_type.as_str());
        if !is_start && !is_end {
            continue;
        }
        let life = out.entry(event.source_id.as_str()).or_insert(Life {
            task_id: event.task_id.as_str(),
            start: None,
            end: None,
        });
        if is_start {
            life.start = Some(event);
        } else {
            life.end = Some(event);
        }
    }
    out
}

// ------------------------------------------------------------------ compute

/// `true` when a returned metric is a lower bound or unknown. The path-claim
/// metric is unsupported by design (see its reason) and does not count.
fn is_partial(metrics: &[MetricResult]) -> bool {
    metrics
        .iter()
        .any(|m| m.status != "complete" && m.id != METRIC_STALE_PATH_CLAIMS)
}

/// Compute every metric for `window` from `facts`. `api_base` is the
/// collaboration API path used to build drill-down links.
pub fn compute(window: &Window, facts: &Facts, api_base: &str) -> Computed {
    let Window { from, to } = *window;
    let mut quality = DataQuality::default();
    let mut per_task: BTreeMap<&'static str, BTreeMap<String, Value>> =
        DRILL_METRICS.iter().map(|m| (*m, BTreeMap::new())).collect();

    let covered = facts.coverage_start.filter(|c| *c < to);
    let link = |metric: &'static str| DrillLink {
        metric,
        href: format!(
            "{api_base}/health/tasks?metric={metric}&from={}&to={}",
            ts(from),
            ts(to)
        ),
    };
    let unknown_metric = |id: &'static str, source: &'static str, reason: &'static str| MetricResult {
        id,
        definition_version: METRIC_VERSION,
        source,
        timing_source: "platform_event",
        status: "unknown",
        value: None,
        reason: Some(reason),
        notes: Vec::new(),
        drill_down: Vec::new(),
    };
    let stale_paths = unknown_metric(
        METRIC_STALE_PATH_CLAIMS,
        ORIGIN_USER_CONTENT,
        "PATH_DECLARED_AT_NOT_RECORDED",
    );

    let Some(coverage_start) = covered else {
        // Nothing was recorded before `to`: everything is unknown, nothing is zero.
        let reason = "NO_EVENTS_BEFORE_WINDOW_END";
        return Computed {
            coverage_start: facts.coverage_start,
            partial: true,
            metrics: vec![
                unknown_metric(METRIC_BLOCKED_TASKS, ORIGIN_USER_CONTENT, reason),
                unknown_metric(METRIC_BLOCKED_DURATION, ORIGIN_USER_CONTENT, reason),
                unknown_metric(METRIC_HANDOFF_WAITING, "platform_event", reason),
                unknown_metric(METRIC_OPEN_QUESTIONS, "platform_event", reason),
                stale_paths,
            ],
            data_quality: quality,
            per_task,
        };
    };
    let eff_from = from.max(coverage_start);
    let window_partial = from < coverage_start;

    // ---- task status timelines
    let mut by_task: BTreeMap<&str, Vec<&Event>> = BTreeMap::new();
    for event in facts.events.iter().filter(|e| is_status_event(&e.event_type)) {
        by_task.entry(event.task_id.as_str()).or_default().push(event);
    }
    let mut timelines: BTreeMap<&str, Vec<Segment>> = BTreeMap::new();
    // Times at which a task entered `blocked` from another state.
    let mut entries_by_task: BTreeMap<&str, Vec<DateTime<Utc>>> = BTreeMap::new();
    for (task_id, mut evs) in by_task {
        evs.sort_by(|a, b| {
            (a.occurred_at, a.task_version.unwrap_or(0), &a.id).cmp(&(b.occurred_at, b.task_version.unwrap_or(0), &b.id))
        });
        match build_timeline(&evs, coverage_start) {
            Ok(segments) if !segments.is_empty() => {
                let entries = evs
                    .iter()
                    .filter(|e| {
                        e.event_type == et::TASK_STATUS_CHANGED
                            && e.to_state.as_deref() == Some("blocked")
                            && e.from_state.as_deref() != Some("blocked")
                            && e.occurred_at >= eff_from
                            && e.occurred_at < to
                    })
                    .map(|e| e.occurred_at)
                    .collect();
                entries_by_task.insert(task_id, entries);
                timelines.insert(task_id, segments);
            }
            Ok(_) => {}
            Err(()) => {
                quality.inconsistent_tasks += 1;
                per_task
                    .get_mut(DRILL_UNKNOWN_STATE)
                    .expect("drill map")
                    .insert(task_id.to_string(), json!({"reason": "INCONSISTENT_EVENTS"}));
            }
        }
    }
    for (task_id, _) in &facts.tasks {
        if !timelines.contains_key(task_id.as_str()) && !per_task[DRILL_UNKNOWN_STATE].contains_key(task_id) {
            quality.unknown_state_tasks += 1;
            per_task
                .get_mut(DRILL_UNKNOWN_STATE)
                .expect("drill map")
                .insert(task_id.clone(), json!({"reason": "NO_EVENTS_FOR_TASK"}));
        }
    }

    // ---- blocked tasks and blocked duration
    let mut blocked_at_to = 0u64;
    let mut entered_tasks = 0u64;
    let mut entered_events = 0u64;
    let mut blocked_ms = 0i64;
    let mut blocked_intervals = 0u64;
    let mut blocked_tasks_with_time = 0u64;
    let (mut left_censored, mut right_censored) = (0u64, 0u64);
    for (task_id, segments) in &timelines {
        if let Some(last) = segments.last().filter(|s| s.state == "blocked") {
            blocked_at_to += 1;
            per_task.get_mut(DRILL_BLOCKED_AT_TO).expect("drill map").insert(
                (*task_id).to_string(),
                json!({
                    "state_at_to": "blocked",
                    "blocked_since": ts(last.start),
                    "since_is_observation_start": last.start_censored,
                }),
            );
        }
        let (mut task_ms, mut task_intervals, mut task_left, mut task_right) = (0i64, 0u64, false, false);
        for seg in segments.iter().filter(|s| s.state == "blocked") {
            let ms = overlap_ms(seg.start, seg.end.unwrap_or(to), eff_from, to);
            if ms == 0 {
                continue;
            }
            task_ms += ms;
            task_intervals += 1;
            task_left |= seg.start_censored && seg.start >= from;
            task_right |= seg.end.is_none();
        }
        if task_intervals > 0 {
            blocked_ms += task_ms;
            blocked_intervals += task_intervals;
            blocked_tasks_with_time += 1;
            left_censored += u64::from(task_left);
            right_censored += u64::from(task_right);
            per_task.get_mut(METRIC_BLOCKED_DURATION).expect("drill map").insert(
                (*task_id).to_string(),
                json!({
                    "blocked_seconds_in_window": secs(task_ms),
                    "intervals": task_intervals,
                    "left_censored": task_left,
                    "right_censored": task_right,
                }),
            );
        }
        let entries = &entries_by_task[task_id];
        if !entries.is_empty() {
            entered_tasks += 1;
            entered_events += entries.len() as u64;
            per_task.get_mut(DRILL_ENTERED_BLOCKED).expect("drill map").insert(
                (*task_id).to_string(),
                json!({
                    "entries": entries.len(),
                    "first_entered_at": ts(entries[0]),
                    "last_entered_at": ts(entries[entries.len() - 1]),
                }),
            );
        }
    }
    let blocked_incomplete = window_partial || quality.unknown_state_tasks > 0 || quality.inconsistent_tasks > 0;
    let blocked_status = if blocked_incomplete { "partial" } else { "complete" };
    let mut blocked_notes = vec![
        "Task status is declared by clients (user_content); only the time of each change is platform-recorded.",
    ];
    if quality.unknown_state_tasks > 0 {
        blocked_notes.push("Tasks without any recorded event have an unknown state and are not counted; take an observation baseline to include them from that moment on.");
    }
    if window_partial {
        blocked_notes.push("The window starts before the first recorded event; only the covered part is measured.");
    }
    let blocked_tasks = MetricResult {
        id: METRIC_BLOCKED_TASKS,
        definition_version: METRIC_VERSION,
        source: ORIGIN_USER_CONTENT,
        timing_source: "platform_event",
        status: blocked_status,
        value: Some(json!({
            "blocked_at_to": blocked_at_to,
            "entered_blocked_in_window": entered_tasks,
            "entered_blocked_events": entered_events,
            "unknown_state_tasks": quality.unknown_state_tasks,
            "unit": "tasks",
        })),
        reason: None,
        notes: blocked_notes.clone(),
        drill_down: vec![link(DRILL_BLOCKED_AT_TO), link(DRILL_ENTERED_BLOCKED), link(DRILL_UNKNOWN_STATE)],
    };
    let mut duration_notes = blocked_notes;
    duration_notes.push("Intervals are clipped to the window; an interval still open at `to` is cut at `to` (right-censored).");
    let blocked_duration = MetricResult {
        id: METRIC_BLOCKED_DURATION,
        definition_version: METRIC_VERSION,
        source: ORIGIN_USER_CONTENT,
        timing_source: "platform_event",
        status: blocked_status,
        value: Some(json!({
            "blocked_seconds": secs(blocked_ms),
            "intervals": blocked_intervals,
            "tasks": blocked_tasks_with_time,
            "left_censored_tasks": left_censored,
            "right_censored_tasks": right_censored,
            "covered_from": ts(eff_from),
            "unit": "seconds",
        })),
        reason: None,
        notes: duration_notes,
        drill_down: vec![link(METRIC_BLOCKED_DURATION)],
    };

    // ---- handoffs
    let handoff_lives = lives(
        &facts.events,
        &[et::HANDOFF_CREATED, et::HANDOFF_OBSERVED_PENDING],
        &[et::HANDOFF_ACCEPTED, et::HANDOFF_CANCELLED],
    );
    for (id, _, _, status) in &facts.handoffs {
        if !handoff_lives.contains_key(id.as_str()) {
            if status == "pending" {
                quality.unobserved_pending_handoffs += 1;
            } else {
                quality.handoffs_closed_before_coverage += 1;
            }
        }
    }
    let (mut h_ms, mut h_measured, mut h_open) = (0i64, 0u64, 0u64);
    let (mut h_accepted, mut h_cancelled) = (0u64, 0u64);
    let (mut h_left, mut h_right) = (0u64, 0u64);
    for life in handoff_lives.values() {
        let end_in_window = life
            .end
            .filter(|e| e.occurred_at >= eff_from && e.occurred_at < to);
        let entry = per_task
            .get_mut(METRIC_HANDOFF_WAITING)
            .expect("drill map")
            .entry(life.task_id.to_string())
            .or_insert_with(|| {
                json!({"handoffs": 0, "waiting_seconds_in_window": 0, "accepted": 0, "cancelled": 0,
                       "open_at_to": 0, "left_censored": false, "right_censored": false})
            });
        let bump = |entry: &mut Value, key: &str, by: i64| {
            entry[key] = json!(entry[key].as_i64().unwrap_or(0) + by);
        };
        if let Some(end) = end_in_window {
            if end.event_type == et::HANDOFF_ACCEPTED {
                h_accepted += 1;
                bump(entry, "accepted", 1);
            } else {
                h_cancelled += 1;
                bump(entry, "cancelled", 1);
            }
        }
        let Some(start) = life.start else {
            quality.handoffs_missing_start += 1;
            continue;
        };
        if life.end.is_some_and(|e| e.occurred_at < start.occurred_at) {
            quality.negative_intervals += 1;
            continue;
        }
        let end_at = life.end.map_or(to, |e| e.occurred_at);
        let ms = overlap_ms(start.occurred_at, end_at, eff_from, to);
        let censored_left = start.event_type == et::HANDOFF_OBSERVED_PENDING && start.occurred_at >= from;
        h_measured += 1;
        h_ms += ms;
        bump(entry, "handoffs", 1);
        bump(entry, "waiting_seconds_in_window", secs(ms));
        if life.end.is_none() {
            h_open += 1;
            h_right += 1;
            bump(entry, "open_at_to", 1);
            entry["right_censored"] = json!(true);
        }
        if censored_left && ms > 0 {
            h_left += 1;
            entry["left_censored"] = json!(true);
        }
    }
    // Tasks listed only because a bookkeeping row was created but nothing measured.
    per_task.get_mut(METRIC_HANDOFF_WAITING).expect("drill map").retain(|_, v| {
        v["handoffs"].as_i64().unwrap_or(0) > 0 || v["accepted"].as_i64().unwrap_or(0) > 0 || v["cancelled"].as_i64().unwrap_or(0) > 0
    });
    let handoff_incomplete = window_partial
        || quality.unobserved_pending_handoffs > 0
        || quality.handoffs_missing_start > 0
        || quality.negative_intervals > 0;
    let handoff_waiting = MetricResult {
        id: METRIC_HANDOFF_WAITING,
        definition_version: METRIC_VERSION,
        source: "platform_event",
        timing_source: "platform_event",
        status: if handoff_incomplete { "partial" } else { "complete" },
        value: Some(json!({
            "waiting_seconds": secs(h_ms),
            "measured_handoffs": h_measured,
            "accepted_in_window": h_accepted,
            "cancelled_in_window": h_cancelled,
            "open_at_to": h_open,
            "left_censored_handoffs": h_left,
            "right_censored_handoffs": h_right,
            "unobserved_pending_handoffs": quality.unobserved_pending_handoffs,
            "unit": "seconds",
        })),
        reason: None,
        notes: vec![
            "Waiting is the time from creation to accept/cancel (pending: up to `to`), clipped to the window.",
            "Accepted means the recipient took the handoff over; it says nothing about the work being done.",
        ],
        drill_down: vec![link(METRIC_HANDOFF_WAITING)],
    };

    // ---- questions
    let question_lives = lives(
        &facts.events,
        &[et::QUESTION_OPENED, et::QUESTION_OBSERVED_OPEN],
        &[et::QUESTION_ANSWERED, et::QUESTION_CANCELLED],
    );
    for (id, _, _, status) in &facts.questions {
        if !question_lives.contains_key(id.as_str()) {
            if status == "open" {
                quality.unobserved_open_questions += 1;
            } else {
                quality.questions_closed_before_coverage += 1;
            }
        }
    }
    let (mut q_window_ms, mut q_open_wait_ms, mut q_open) = (0i64, 0i64, 0u64);
    let (mut q_answered, mut q_cancelled) = (0u64, 0u64);
    let (mut q_left, mut q_right) = (0u64, 0u64);
    for life in question_lives.values() {
        let end_in_window = life
            .end
            .filter(|e| e.occurred_at >= eff_from && e.occurred_at < to);
        let entry = per_task
            .get_mut(METRIC_OPEN_QUESTIONS)
            .expect("drill map")
            .entry(life.task_id.to_string())
            .or_insert_with(|| {
                json!({"questions": 0, "wait_seconds_in_window": 0, "open_wait_seconds_at_to": 0,
                       "answered": 0, "cancelled": 0, "open_at_to": 0,
                       "left_censored": false, "right_censored": false})
            });
        let bump = |entry: &mut Value, key: &str, by: i64| {
            entry[key] = json!(entry[key].as_i64().unwrap_or(0) + by);
        };
        if let Some(end) = end_in_window {
            if end.event_type == et::QUESTION_ANSWERED {
                q_answered += 1;
                bump(entry, "answered", 1);
            } else {
                q_cancelled += 1;
                bump(entry, "cancelled", 1);
            }
        }
        let Some(start) = life.start else {
            quality.questions_missing_start += 1;
            continue;
        };
        if life.end.is_some_and(|e| e.occurred_at < start.occurred_at) {
            quality.negative_intervals += 1;
            continue;
        }
        let end_at = life.end.map_or(to, |e| e.occurred_at);
        let ms = overlap_ms(start.occurred_at, end_at, eff_from, to);
        let censored_left = start.event_type == et::QUESTION_OBSERVED_OPEN && start.occurred_at >= from;
        q_window_ms += ms;
        bump(entry, "questions", 1);
        bump(entry, "wait_seconds_in_window", secs(ms));
        if life.end.is_none() {
            let open_ms = (to - start.occurred_at).num_milliseconds().max(0);
            q_open += 1;
            q_right += 1;
            q_open_wait_ms += open_ms;
            bump(entry, "open_at_to", 1);
            bump(entry, "open_wait_seconds_at_to", secs(open_ms));
            entry["right_censored"] = json!(true);
        }
        if censored_left && (ms > 0 || life.end.is_none()) {
            q_left += 1;
            entry["left_censored"] = json!(true);
        }
    }
    per_task.get_mut(METRIC_OPEN_QUESTIONS).expect("drill map").retain(|_, v| {
        v["questions"].as_i64().unwrap_or(0) > 0 || v["answered"].as_i64().unwrap_or(0) > 0 || v["cancelled"].as_i64().unwrap_or(0) > 0
    });
    let question_incomplete = window_partial
        || quality.unobserved_open_questions > 0
        || quality.questions_missing_start > 0
        || quality.negative_intervals > 0;
    let open_questions = MetricResult {
        id: METRIC_OPEN_QUESTIONS,
        definition_version: METRIC_VERSION,
        source: "platform_event",
        timing_source: "platform_event",
        status: if question_incomplete { "partial" } else { "complete" },
        value: Some(json!({
            "open_at_to": q_open,
            "open_wait_seconds_at_to": secs(q_open_wait_ms),
            "wait_seconds_in_window": secs(q_window_ms),
            "answered_in_window": q_answered,
            "cancelled_in_window": q_cancelled,
            "left_censored_questions": q_left,
            "right_censored_questions": q_right,
            "unobserved_open_questions": quality.unobserved_open_questions,
            "unit": "seconds",
        })),
        reason: None,
        notes: vec![
            "A question is open from creation until it is answered or cancelled; who answered is not evidence of a particular human.",
        ],
        drill_down: vec![link(METRIC_OPEN_QUESTIONS)],
    };

    let metrics = vec![blocked_tasks, blocked_duration, handoff_waiting, open_questions, stale_paths];
    let partial = is_partial(&metrics);
    Computed {
        coverage_start: facts.coverage_start,
        partial,
        metrics,
        data_quality: quality,
        per_task,
    }
}

// ------------------------------------------------------------------ service

#[derive(Debug, Clone, Serialize)]
pub struct RangeView {
    pub from: String,
    pub to: String,
    pub timezone: &'static str,
    pub interval: &'static str,
}

fn range_view(window: &Window) -> RangeView {
    RangeView {
        from: ts(window.from),
        to: ts(window.to),
        timezone: "UTC",
        interval: "[from, to)",
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct BaselineView {
    /// `null` = no observation baseline was taken for this repository.
    pub taken_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Health {
    pub range: RangeView,
    pub metric_version: u32,
    /// Time of the earliest recorded event. Nothing before it is measured.
    pub coverage_start: Option<String>,
    /// `true` when any returned metric is a lower bound or unknown.
    pub partial: bool,
    pub computed_at: String,
    pub baseline: BaselineView,
    pub metrics: Vec<MetricResult>,
    pub data_quality: DataQuality,
}

/// Health metrics of one repository for `window`, restricted to `wanted`.
pub async fn health(
    db: &DatabaseConnection,
    repo_id: i64,
    api_base: &str,
    window: &Window,
    wanted: &[&'static str],
    now: DateTime<Utc>,
) -> CollabResult<Health> {
    let facts = load_facts(db, repo_id, window.to).await?;
    let computed = compute(window, &facts, api_base);
    let baseline = ops::baseline_taken_at(db, repo_id).await?;
    let metrics: Vec<MetricResult> = computed
        .metrics
        .into_iter()
        .filter(|m| wanted.contains(&m.id))
        .collect();
    let partial = is_partial(&metrics);
    Ok(Health {
        range: range_view(window),
        metric_version: METRIC_VERSION,
        coverage_start: computed.coverage_start.map(ts),
        partial,
        computed_at: ts(now),
        baseline: BaselineView { taken_at: baseline.map(ts) },
        metrics,
        data_quality: computed.data_quality,
    })
}

#[derive(Debug, Clone, Serialize)]
pub struct DrillItem {
    pub task_id: String,
    pub title: String,
    /// The title is a client declaration.
    pub title_origin: &'static str,
    /// Current self-reported task status.
    pub status: String,
    /// Why the task is listed (the figures this task contributes).
    pub basis: Value,
    pub href: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DrillPage {
    pub range: RangeView,
    pub metric: String,
    pub metric_version: u32,
    pub coverage_start: Option<String>,
    pub partial: bool,
    pub computed_at: String,
    pub items: Vec<DrillItem>,
    pub next_cursor: Option<String>,
}

fn drill_scope(repo_id: i64, metric: &str, window: &Window) -> String {
    format!(
        "health-tasks:v1:repo={repo_id}:metric={metric}:from={}:to={}:order=task_id_asc",
        window.from.to_rfc3339_opts(SecondsFormat::Nanos, true),
        window.to.to_rfc3339_opts(SecondsFormat::Nanos, true)
    )
}

/// Tasks behind one metric, ordered by task id. The page is recomputed from the
/// events on every call; `to` is fixed by the cursor's scope so paging is stable.
#[allow(clippy::too_many_arguments)]
pub async fn drill_down(
    db: &DatabaseConnection,
    cursor_key: &[u8],
    repo_id: i64,
    api_base: &str,
    window: &Window,
    metric: &str,
    limit: Option<u64>,
    cursor_token: Option<&str>,
    now: DateTime<Utc>,
) -> CollabResult<DrillPage> {
    let metric_id = DRILL_METRICS
        .iter()
        .find(|m| **m == metric)
        .copied()
        .ok_or_else(|| {
            CollabError::bad_request(format!(
                "unknown drill-down metric `{metric}`; known: {}",
                DRILL_METRICS.join(", ")
            ))
        })?;
    let limit = clamp_limit(limit);
    let scope = drill_scope(repo_id, metric_id, window);
    let after = cursor_token
        .map(|token| cursor::decode(cursor_key, &scope, token))
        .transpose()?
        .map(|p| p.id);

    let facts = load_facts(db, repo_id, window.to).await?;
    let computed = compute(window, &facts, api_base);
    let ids: BTreeSet<&String> = computed.per_task[metric_id].keys().collect();
    let mut page_ids: Vec<String> = ids
        .into_iter()
        .filter(|id| after.as_ref().is_none_or(|a| id.as_str() > a.as_str()))
        .take(limit as usize + 1)
        .cloned()
        .collect();
    let has_more = page_ids.len() as u64 > limit;
    page_ids.truncate(limit as usize);

    let rows: BTreeMap<String, (String, String)> = ops::task_titles(db, repo_id, &page_ids)
        .await?
        .into_iter()
        .map(|(id, title, status)| (id, (title, status)))
        .collect();
    let items: Vec<DrillItem> = page_ids
        .iter()
        .map(|id| {
            let (title, status) = rows.get(id).cloned().unwrap_or_default();
            DrillItem {
                task_id: id.clone(),
                title,
                title_origin: ORIGIN_USER_CONTENT,
                status,
                basis: computed.per_task[metric_id][id].clone(),
                href: format!("{api_base}/tasks/{id}"),
            }
        })
        .collect();
    let next_cursor = has_more
        .then(|| page_ids.last())
        .flatten()
        .map(|last| {
            cursor::encode(
                cursor_key,
                &scope,
                &KeysetPosition { at: window.to, id: last.clone() },
            )
        });
    Ok(DrillPage {
        range: range_view(window),
        metric: metric_id.to_string(),
        metric_version: METRIC_VERSION,
        coverage_start: computed.coverage_start.map(ts),
        partial: computed.partial,
        computed_at: ts(now),
        items,
        next_cursor,
    })
}

#[derive(Debug, Clone, Serialize)]
pub struct BaselineReport {
    /// When the baseline was taken: this call's time for new observations.
    pub observed_at: String,
    pub added: BaselineAdded,
    /// `true` when a baseline already existed before this call.
    pub existed_before: bool,
    pub note: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct BaselineAdded {
    pub tasks: u64,
    pub handoffs: u64,
    pub questions: u64,
}

/// Record the current state of objects that have no events as "observed at
/// `now`". See [`ops::take_baseline`]; repeating it adds nothing new.
pub async fn take_baseline(
    db: &DatabaseConnection,
    repo_id: i64,
    now: DateTime<Utc>,
) -> CollabResult<BaselineReport> {
    let existed_before = ops::baseline_taken_at(db, repo_id).await?.is_some();
    let counts = ops::take_baseline(db, repo_id, now).await?;
    Ok(BaselineReport {
        observed_at: ts(now),
        added: BaselineAdded {
            tasks: counts.tasks,
            handoffs: counts.handoffs,
            questions: counts.questions,
        },
        existed_before,
        note: "Observation start only: objects are measured from this time on, never as having been in their state earlier.",
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use rg_db::ops::collab_metric_ops::source_type as st;

    fn t(minutes: i64) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 1, 0, 0, 0).unwrap() + Duration::minutes(minutes)
    }

    fn ev(task: &str, kind: &str, from: Option<&str>, to_state: Option<&str>, at: i64, version: Option<i64>, src: (&str, &str)) -> Event {
        Event {
            id: format!("e-{task}-{kind}-{at}-{}", src.1),
            repo_id: 1,
            task_id: task.to_string(),
            event_type: kind.to_string(),
            from_state: from.map(str::to_string),
            to_state: to_state.map(str::to_string),
            task_version: version,
            source_type: src.0.to_string(),
            source_id: src.1.to_string(),
            occurred_at: t(at),
            recorded_at: t(at),
        }
    }

    fn created(task: &str, at: i64) -> Event {
        ev(task, et::TASK_CREATED, None, Some("open"), at, Some(1), (st::TASK, task))
    }

    fn changed(task: &str, from: &str, to: &str, at: i64, v: i64) -> Event {
        ev(task, et::TASK_STATUS_CHANGED, Some(from), Some(to), at, Some(v), (st::TASK, &format!("{task}:{v}")))
    }

    fn observed(task: &str, status: &str, at: i64) -> Event {
        ev(task, et::TASK_STATUS_OBSERVED, None, Some(status), at, Some(1), (st::BASELINE, task))
    }

    fn facts(mut events: Vec<Event>, tasks: &[&str]) -> Facts {
        events.sort_by_key(|e| e.occurred_at);
        let coverage_start = events.first().map(|e| e.occurred_at);
        Facts {
            events,
            coverage_start,
            tasks: tasks.iter().map(|id| (id.to_string(), t(-10_000))).collect(),
            handoffs: Vec::new(),
            questions: Vec::new(),
        }
    }

    fn run(window: (i64, i64), facts: &Facts) -> Computed {
        compute(&Window { from: t(window.0), to: t(window.1) }, facts, "/api/x")
    }

    fn val<'a>(c: &'a Computed, id: &str) -> &'a Value {
        c.metrics.iter().find(|m| m.id == id).unwrap().value.as_ref().unwrap()
    }

    fn metric<'a>(c: &'a Computed, id: &str) -> &'a MetricResult {
        c.metrics.iter().find(|m| m.id == id).unwrap()
    }

    #[test]
    fn closed_interval_inside_the_window_is_measured_exactly() {
        // created 0, blocked 10..40, unblocked, done.
        let f = facts(
            vec![created("a", 0), changed("a", "open", "blocked", 10, 2), changed("a", "blocked", "in_progress", 40, 3), changed("a", "in_progress", "done", 50, 4)],
            &["a"],
        );
        let c = run((0, 100), &f);
        assert_eq!(val(&c, METRIC_BLOCKED_DURATION)["blocked_seconds"], 30 * 60);
        assert_eq!(val(&c, METRIC_BLOCKED_DURATION)["right_censored_tasks"], 0);
        assert_eq!(val(&c, METRIC_BLOCKED_DURATION)["left_censored_tasks"], 0);
        assert_eq!(val(&c, METRIC_BLOCKED_TASKS)["blocked_at_to"], 0);
        assert_eq!(val(&c, METRIC_BLOCKED_TASKS)["entered_blocked_in_window"], 1);
        assert_eq!(metric(&c, METRIC_BLOCKED_TASKS).status, "complete");
        assert!(!c.partial, "the unsupported path metric does not make the result partial");
    }

    #[test]
    fn a_blocked_interval_still_open_is_cut_at_to_and_flagged_right_censored() {
        let f = facts(vec![created("a", 0), changed("a", "open", "blocked", 10, 2)], &["a"]);
        let c = run((0, 70), &f);
        assert_eq!(val(&c, METRIC_BLOCKED_DURATION)["blocked_seconds"], 60 * 60);
        assert_eq!(val(&c, METRIC_BLOCKED_DURATION)["right_censored_tasks"], 1);
        assert_eq!(val(&c, METRIC_BLOCKED_TASKS)["blocked_at_to"], 1);
    }

    #[test]
    fn an_interval_that_began_before_the_window_is_clipped_not_censored() {
        let f = facts(vec![created("a", 0), changed("a", "open", "blocked", 10, 2), changed("a", "blocked", "open", 100, 3)], &["a"]);
        let c = run((50, 200), &f);
        assert_eq!(val(&c, METRIC_BLOCKED_DURATION)["blocked_seconds"], 50 * 60);
        assert_eq!(val(&c, METRIC_BLOCKED_DURATION)["left_censored_tasks"], 0);
        assert_eq!(val(&c, METRIC_BLOCKED_TASKS)["entered_blocked_in_window"], 0, "entered before the window");
        assert_eq!(val(&c, METRIC_BLOCKED_TASKS)["blocked_at_to"], 0);
    }

    #[test]
    fn reopened_task_counts_once_as_a_task_and_twice_as_entries() {
        let f = facts(
            vec![
                created("a", 0),
                changed("a", "open", "blocked", 10, 2),
                changed("a", "blocked", "in_progress", 20, 3),
                changed("a", "in_progress", "blocked", 30, 4),
                changed("a", "blocked", "done", 45, 5),
            ],
            &["a"],
        );
        let c = run((0, 100), &f);
        let tasks = val(&c, METRIC_BLOCKED_TASKS);
        assert_eq!(tasks["entered_blocked_in_window"], 1);
        assert_eq!(tasks["entered_blocked_events"], 2);
        assert_eq!(val(&c, METRIC_BLOCKED_DURATION)["blocked_seconds"], (10 + 15) * 60);
        assert_eq!(val(&c, METRIC_BLOCKED_DURATION)["intervals"], 2);
        assert_eq!(c.per_task[DRILL_ENTERED_BLOCKED]["a"]["entries"], 2);
    }

    #[test]
    fn first_event_is_a_change_out_of_blocked_so_the_start_is_left_censored() {
        // Another task makes coverage start at 0; task "a" (pre-feature) is
        // unblocked at 30 and its blocked state before that is only known since 0.
        let f = facts(vec![created("b", 0), changed("a", "blocked", "open", 30, 7)], &["a", "b"]);
        let c = run((-60, 100), &f);
        let d = val(&c, METRIC_BLOCKED_DURATION);
        assert_eq!(d["blocked_seconds"], 30 * 60, "only the observed part");
        assert_eq!(d["left_censored_tasks"], 1);
        assert_eq!(metric(&c, METRIC_BLOCKED_DURATION).status, "partial", "window starts before coverage");
        assert_eq!(d["covered_from"], ts(t(0)));
    }

    #[test]
    fn baseline_observation_is_observed_since_not_since_forever() {
        let f = facts(vec![observed("a", "blocked", 100)], &["a"]);
        let c = run((0, 160), &f);
        assert_eq!(val(&c, METRIC_BLOCKED_TASKS)["blocked_at_to"], 1);
        assert_eq!(val(&c, METRIC_BLOCKED_DURATION)["blocked_seconds"], 60 * 60, "counted from the observation, not from t=0");
        assert_eq!(val(&c, METRIC_BLOCKED_DURATION)["left_censored_tasks"], 1);
        assert_eq!(c.per_task[DRILL_BLOCKED_AT_TO]["a"]["since_is_observation_start"], true);
        assert_eq!(c.per_task[DRILL_BLOCKED_AT_TO]["a"]["blocked_since"], ts(t(100)));
    }

    #[test]
    fn pre_feature_tasks_without_events_are_unknown_never_zero() {
        let f = facts(vec![created("new", 100)], &["old", "new"]);
        let c = run((50, 200), &f);
        let tasks = val(&c, METRIC_BLOCKED_TASKS);
        assert_eq!(tasks["unknown_state_tasks"], 1);
        assert_eq!(metric(&c, METRIC_BLOCKED_TASKS).status, "partial");
        assert!(c.partial);
        assert!(c.per_task[DRILL_UNKNOWN_STATE].contains_key("old"));
    }

    #[test]
    fn a_window_entirely_before_the_first_event_is_unknown() {
        let f = facts(vec![created("a", 500)], &["a"]);
        let c = run((0, 100), &f);
        for m in &c.metrics {
            assert_eq!(m.status, "unknown", "{}", m.id);
            assert!(m.value.is_none());
        }
        let empty = run((0, 100), &facts(vec![], &["a"]));
        assert_eq!(metric(&empty, METRIC_BLOCKED_TASKS).reason, Some("NO_EVENTS_BEFORE_WINDOW_END"));
    }

    #[test]
    fn contradicting_events_exclude_the_task_and_are_counted() {
        let f = facts(vec![created("a", 0), changed("a", "blocked", "done", 10, 2)], &["a"]);
        let c = run((0, 100), &f);
        assert_eq!(c.data_quality.inconsistent_tasks, 1);
        assert_eq!(metric(&c, METRIC_BLOCKED_TASKS).status, "partial");
        assert_eq!(val(&c, METRIC_BLOCKED_DURATION)["blocked_seconds"], 0);
    }

    fn handoff(task: &str, id: &str, kind: &str, at: i64) -> Event {
        let (from, to_state) = match kind {
            et::HANDOFF_CREATED | et::HANDOFF_OBSERVED_PENDING => (None, "pending"),
            et::HANDOFF_ACCEPTED => (Some("pending"), "accepted"),
            _ => (Some("pending"), "cancelled"),
        };
        let source = if kind == et::HANDOFF_OBSERVED_PENDING { st::BASELINE } else { st::HANDOFF };
        ev(task, kind, from, Some(to_state), at, None, (source, id))
    }

    #[test]
    fn handoff_waiting_intersects_the_window_and_counts_each_outcome() {
        let f = facts(
            vec![
                created("a", 0),
                handoff("a", "h1", et::HANDOFF_CREATED, 10),
                handoff("a", "h1", et::HANDOFF_ACCEPTED, 40),
                handoff("a", "h2", et::HANDOFF_CREATED, 20),
                handoff("a", "h2", et::HANDOFF_CANCELLED, 25),
                handoff("a", "h3", et::HANDOFF_CREATED, 60),
                // Created before the window, accepted inside it.
                handoff("a", "h0", et::HANDOFF_CREATED, 0),
                handoff("a", "h0", et::HANDOFF_ACCEPTED, 55),
            ],
            &["a"],
        );
        let c = run((30, 90), &f);
        let h = val(&c, METRIC_HANDOFF_WAITING);
        // h1: 30..40 = 10m, h2: none, h3: 60..90 = 30m, h0: 30..55 = 25m.
        assert_eq!(h["waiting_seconds"], (10 + 30 + 25) * 60);
        assert_eq!(h["accepted_in_window"], 2);
        assert_eq!(h["cancelled_in_window"], 0, "h2 was cancelled before the window");
        assert_eq!(h["open_at_to"], 1);
        assert_eq!(h["right_censored_handoffs"], 1);
        assert_eq!(metric(&c, METRIC_HANDOFF_WAITING).status, "complete");
        assert_eq!(c.per_task[METRIC_HANDOFF_WAITING]["a"]["open_at_to"], 1);
    }

    #[test]
    fn pending_handoff_without_any_event_makes_the_metric_partial() {
        let mut f = facts(vec![created("a", 0)], &["a"]);
        f.handoffs.push(("old".into(), "a".into(), t(-500), "pending".into()));
        f.handoffs.push(("older".into(), "a".into(), t(-600), "accepted".into()));
        let c = run((0, 100), &f);
        assert_eq!(c.data_quality.unobserved_pending_handoffs, 1);
        assert_eq!(c.data_quality.handoffs_closed_before_coverage, 1);
        assert_eq!(metric(&c, METRIC_HANDOFF_WAITING).status, "partial");
        assert_eq!(val(&c, METRIC_HANDOFF_WAITING)["open_at_to"], 0);
    }

    #[test]
    fn baseline_handoffs_are_left_censored_and_missing_starts_are_counted() {
        let f = facts(
            vec![
                created("a", 0),
                handoff("a", "h1", et::HANDOFF_OBSERVED_PENDING, 20),
                // End without a start: counted as an outcome, not as waiting time.
                handoff("a", "hx", et::HANDOFF_ACCEPTED, 30),
            ],
            &["a"],
        );
        let c = run((0, 60), &f);
        let h = val(&c, METRIC_HANDOFF_WAITING);
        assert_eq!(h["waiting_seconds"], 40 * 60);
        assert_eq!(h["left_censored_handoffs"], 1);
        assert_eq!(h["accepted_in_window"], 1);
        assert_eq!(c.data_quality.handoffs_missing_start, 1);
        assert_eq!(metric(&c, METRIC_HANDOFF_WAITING).status, "partial");
    }

    #[test]
    fn question_wait_and_open_counts() {
        let q = |id: &str, kind: &str, at: i64| {
            let (from, to_state) = match kind {
                et::QUESTION_OPENED | et::QUESTION_OBSERVED_OPEN => (None, "open"),
                et::QUESTION_ANSWERED => (Some("open"), "answered"),
                _ => (Some("open"), "cancelled"),
            };
            let source = if kind == et::QUESTION_OBSERVED_OPEN { st::BASELINE } else { st::QUESTION };
            ev("a", kind, from, Some(to_state), at, None, (source, id))
        };
        let f = facts(
            vec![
                created("a", 0),
                q("q1", et::QUESTION_OPENED, 10),
                q("q1", et::QUESTION_ANSWERED, 30),
                q("q2", et::QUESTION_OPENED, 20),
                q("q3", et::QUESTION_OPENED, 40),
                q("q3", et::QUESTION_CANCELLED, 50),
            ],
            &["a"],
        );
        let c = run((0, 100), &f);
        let v = val(&c, METRIC_OPEN_QUESTIONS);
        assert_eq!(v["open_at_to"], 1);
        assert_eq!(v["open_wait_seconds_at_to"], 80 * 60);
        assert_eq!(v["wait_seconds_in_window"], (20 + 80 + 10) * 60);
        assert_eq!(v["answered_in_window"], 1);
        assert_eq!(v["cancelled_in_window"], 1);
        assert_eq!(metric(&c, METRIC_OPEN_QUESTIONS).status, "complete");
    }

    #[test]
    fn stale_path_claims_is_unknown_with_a_reason() {
        let c = run((0, 100), &facts(vec![created("a", 0)], &["a"]));
        let m = metric(&c, METRIC_STALE_PATH_CLAIMS);
        assert_eq!(m.status, "unknown");
        assert_eq!(m.reason, Some("PATH_DECLARED_AT_NOT_RECORDED"));
        assert!(m.value.is_none());
    }

    #[test]
    fn recompute_is_repeatable() {
        let f = facts(vec![created("a", 0), changed("a", "open", "blocked", 10, 2), handoff("a", "h", et::HANDOFF_CREATED, 5)], &["a"]);
        let a = serde_json::to_string(&run((0, 100), &f).metrics).unwrap();
        let b = serde_json::to_string(&run((0, 100), &f).metrics).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn window_rules() {
        let now = t(1000);
        assert!(resolve_window(None, None, now).is_ok());
        assert!(resolve_window(Some(t(10)), Some(t(10)), now).is_err());
        assert!(resolve_window(Some(t(0)), Some(t(2000)), now).is_err(), "future `to`");
        let long = resolve_window(Some(t(-60 * 24 * 91)), Some(t(0)), now);
        assert!(long.is_err());
        assert!(resolve_window(Some(t(-60 * 24 * 90)), Some(t(0)), now).is_ok());
        assert_eq!(parse_metrics(None).unwrap().len(), METRICS.len());
        assert!(parse_metrics(Some("blocked_tasks,nope")).is_err());
        assert_eq!(parse_metrics(Some("blocked_tasks, blocked_tasks")).unwrap(), vec![METRIC_BLOCKED_TASKS]);
    }
}
