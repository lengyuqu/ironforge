//! CI feedback for scripted and AI-agent clients: incremental job logs,
//! change-aware waiting on a pipeline, and a bounded failure summary.
//!
//! The pure helpers (`slice_log`, `tail_log`, `failure_summary`, ...) carry no
//! I/O so the byte-offset and truncation rules are unit-tested directly.

use std::time::{Duration, Instant};

use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::Semaphore;

use crate::api::repo_access;
use crate::error::AppError;
use crate::AppState;

const DEFAULT_LOG_LIMIT: usize = 64 * 1024;
const MAX_LOG_LIMIT: usize = 256 * 1024;
const MAX_TAIL_LINES: usize = 2000;
const SUMMARY_TAIL_LINES: usize = 50;
const MAX_ERROR_LINES: usize = 10;
const MAX_LINE_CHARS: usize = 300;

const DEFAULT_WAIT_SECS: u64 = 30;
const MAX_WAIT_SECS: u64 = 60;
const WAIT_POLL: Duration = Duration::from_secs(1);
/// Concurrent `wait` requests held open per process. Each one polls the
/// database once a second, so the cap bounds that background load.
static WAIT_PERMITS: Semaphore = Semaphore::const_new(64);

/// Substrings that mark a log line as worth surfacing. A heuristic, not a
/// parser: it is meant to point at the first things to read.
const ERROR_MARKERS: &[&str] = &[
    "error[E",
    "error:",
    "Error:",
    "ERROR",
    "FAILED",
    "FAIL ",
    "panicked at",
    "Traceback (most recent call last)",
    "AssertionError",
    "npm ERR!",
    "fatal:",
];

// ── Status helpers ───────────────────────────────────────────────

/// Job statuses that mean the job ran and did not succeed.
pub fn is_failed_job_status(status: &str) -> bool {
    matches!(status, "failed" | "failure" | "error")
}

/// Job statuses after which the log can no longer grow.
pub fn is_terminal_job_status(status: &str) -> bool {
    matches!(
        status,
        "success" | "failed" | "failure" | "error" | "skipped" | "canceled"
    )
}

/// Pipeline statuses after which nothing further will run.
pub fn is_terminal_pipeline_status(status: &str) -> bool {
    matches!(
        status,
        "success" | "failed" | "failure" | "error" | "skipped" | "canceled"
    )
}

// ── Log helpers ──────────────────────────────────────────────────

/// Remove ANSI escape sequences (CSI and OSC) from `input`.
pub fn strip_ansi(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            out.push(c);
            continue;
        }
        match chars.peek() {
            // CSI: ESC [ <params> <final byte 0x40..=0x7e>
            Some('[') => {
                chars.next();
                for next in chars.by_ref() {
                    if ('\u{40}'..='\u{7e}').contains(&next) {
                        break;
                    }
                }
            }
            // OSC: ESC ] ... terminated by BEL or ESC \
            Some(']') => {
                chars.next();
                while let Some(next) = chars.next() {
                    if next == '\u{7}' {
                        break;
                    }
                    if next == '\u{1b}' {
                        if chars.peek() == Some(&'\\') {
                            chars.next();
                        }
                        break;
                    }
                }
            }
            // Any other two-byte escape.
            Some(_) => {
                chars.next();
            }
            None => {}
        }
    }
    out
}

/// A window over a job log, addressed in bytes of the stored text.
#[derive(Debug, PartialEq, Eq)]
pub struct LogSlice {
    /// First byte included in `text` (after clamping and boundary alignment).
    pub offset: usize,
    /// Where the next request should resume.
    pub next_offset: usize,
    pub total_bytes: usize,
    /// The requested offset was past the end of the log (the log was
    /// replaced, e.g. by a rerun), so the slice starts from the beginning.
    pub reset: bool,
    pub text: String,
}

/// Return up to `limit` bytes of `log` starting at `offset`, never splitting
/// a UTF-8 character.
pub fn slice_log(log: &str, offset: usize, limit: usize) -> LogSlice {
    let total = log.len();
    let (mut start, reset) = if offset > total {
        (0, true)
    } else {
        (offset, false)
    };
    while start < total && !log.is_char_boundary(start) {
        start += 1;
    }
    let mut end = start.saturating_add(limit.max(1)).min(total);
    while end > start && !log.is_char_boundary(end) {
        end -= 1;
    }
    // A limit smaller than one character must still make progress.
    if end == start && start < total {
        end = start + 1;
        while end < total && !log.is_char_boundary(end) {
            end += 1;
        }
    }
    LogSlice {
        offset: start,
        next_offset: end,
        total_bytes: total,
        reset,
        text: log[start..end].to_string(),
    }
}

/// Return the last `lines` lines of `log`, capped at `max_bytes`.
pub fn tail_log(log: &str, lines: usize, max_bytes: usize) -> LogSlice {
    let total = log.len();
    let body = log.strip_suffix('\n').unwrap_or(log);
    let mut start = body
        .rmatch_indices('\n')
        .nth(lines.saturating_sub(1))
        .map(|(index, _)| index + 1)
        .unwrap_or(0);
    if lines == 0 {
        start = total;
    }
    if total - start > max_bytes {
        start = total - max_bytes;
        while start < total && !log.is_char_boundary(start) {
            start += 1;
        }
    }
    LogSlice {
        offset: start,
        next_offset: total,
        total_bytes: total,
        reset: false,
        text: log[start..].to_string(),
    }
}

fn clip_line(line: &str) -> String {
    if line.chars().count() <= MAX_LINE_CHARS {
        return line.to_string();
    }
    let mut clipped: String = line.chars().take(MAX_LINE_CHARS).collect();
    clipped.push('…');
    clipped
}

/// What an agent needs first when a job fails, without downloading the log.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct FailureSummary {
    pub exit_code: Option<i32>,
    /// Last lines of the log, ANSI-stripped.
    pub tail: Vec<String>,
    /// More log exists before `tail`.
    pub truncated: bool,
    /// Lines anywhere in the log that look like errors. Heuristic.
    pub error_lines: Vec<String>,
}

pub fn failure_summary(log: Option<&str>, exit_code: Option<i32>) -> FailureSummary {
    let Some(log) = log else {
        return FailureSummary {
            exit_code,
            tail: Vec::new(),
            truncated: false,
            error_lines: Vec::new(),
        };
    };
    let clean = strip_ansi(log);
    let lines: Vec<&str> = clean.lines().collect();

    let tail_start = lines.len().saturating_sub(SUMMARY_TAIL_LINES);
    let tail = lines[tail_start..].iter().map(|l| clip_line(l)).collect();

    let mut error_lines: Vec<String> = Vec::new();
    for line in &lines {
        if error_lines.len() >= MAX_ERROR_LINES {
            break;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() || !ERROR_MARKERS.iter().any(|m| trimmed.contains(m)) {
            continue;
        }
        let clipped = clip_line(trimmed);
        if !error_lines.contains(&clipped) {
            error_lines.push(clipped);
        }
    }

    FailureSummary {
        exit_code,
        tail,
        truncated: tail_start > 0,
        error_lines,
    }
}

/// Summary for a job, present only when the job failed.
pub fn summary_for_job(
    status: &str,
    exit_code: Option<i32>,
    log: Option<&str>,
) -> Option<FailureSummary> {
    is_failed_job_status(status).then(|| failure_summary(log, exit_code))
}

/// Stable fingerprint of a pipeline's observable state. A client passes the
/// last one it saw to `wait` and is released when it changes.
pub fn state_etag(pipeline_status: &str, jobs: &[(i64, &str)]) -> String {
    let mut ordered: Vec<(i64, &str)> = jobs.to_vec();
    ordered.sort_unstable();
    let mut hasher = Sha256::new();
    hasher.update(pipeline_status.as_bytes());
    for (id, status) in ordered {
        hasher.update(b"|");
        hasher.update(id.to_string().as_bytes());
        hasher.update(b":");
        hasher.update(status.as_bytes());
    }
    hex::encode(&hasher.finalize()[..8])
}

// ── Handlers ─────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct JobLogQuery {
    /// Byte offset to resume from (default 0).
    #[serde(default)]
    pub offset: Option<usize>,
    /// Maximum bytes to return (default 65536, max 262144).
    #[serde(default)]
    pub limit: Option<usize>,
    /// Return only the last N lines instead (max 2000), still bounded by
    /// `limit` bytes. Mutually exclusive with `offset`.
    #[serde(default)]
    pub tail: Option<usize>,
    /// Strip ANSI escapes from the returned text. Offsets still refer to the
    /// stored (unstripped) log.
    #[serde(default)]
    pub strip_ansi: Option<bool>,
}

/// GET /api/v1/repos/:owner/:name/pipelines/:id/jobs/:job_id/log
/// Incremental or tail read of a job's log.
#[utoipa::path(
    get,
    path = "/repos/{owner}/{name}/pipelines/{id}/jobs/{job_id}/log",
    tag = "CI/CD",
    params(
        ("owner" = String, Path, description = "owner"),
        ("name" = String, Path, description = "name"),
        ("id" = i64, Path, description = "pipeline id"),
        ("job_id" = i64, Path, description = "job id"),
        ("offset" = Option<usize>, Query, description = "Byte offset to resume from (default 0)"),
        ("limit" = Option<usize>, Query, description = "Max bytes (default 65536, max 262144)"),
        ("tail" = Option<usize>, Query, description = "Last N lines instead of an offset window (max 2000, still bounded by limit)"),
        ("strip_ansi" = Option<bool>, Query, description = "Strip ANSI escapes from the returned text"),
    ),
    responses(
        (status = 200, description = "Log window with offset, next_offset, total_bytes, eof, reset, text", body = serde_json::Value),
        (status = 400, description = "offset and tail are mutually exclusive", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
        (status = 404, description = "Pipeline or job not found", body = serde_json::Value),
    ),
)]
pub async fn get_job_log(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((owner, name, pipeline_id, job_id)): Path<(String, String, i64, i64)>,
    Query(params): Query<JobLogQuery>,
) -> impl IntoResponse {
    if params.tail.is_some() && params.offset.is_some() {
        return AppError::bad_request("`offset` and `tail` cannot be combined").into_response();
    }
    let repo = match repo_access::require_read(&state, &headers, &owner, &name).await {
        Ok(repo) => repo,
        Err(e) => return e.into_response(),
    };
    match rg_db::ops::pipeline_ops::get_pipeline(&state.db, pipeline_id).await {
        Ok(Some(p)) if p.repo_id == repo.id => {}
        Ok(_) => return AppError::not_found("pipeline not found").into_response(),
        Err(e) => return AppError::from(e).into_response(),
    }
    let job = match rg_db::ops::pipeline_ops::get_job(&state.db, job_id).await {
        Ok(Some(job))
            if crate::api::ci::job_belongs_to_pipeline(&state, pipeline_id, job.stage_id).await =>
        {
            job
        }
        Ok(_) => return AppError::not_found("job not found").into_response(),
        Err(e) => return AppError::from(e).into_response(),
    };

    let log = job.log.as_deref().unwrap_or("");
    let limit = params
        .limit
        .unwrap_or(DEFAULT_LOG_LIMIT)
        .clamp(1, MAX_LOG_LIMIT);
    let slice = match params.tail {
        Some(lines) => tail_log(log, lines.clamp(1, MAX_TAIL_LINES), limit),
        None => slice_log(log, params.offset.unwrap_or(0), limit),
    };
    let eof = is_terminal_job_status(&job.status) && slice.next_offset >= slice.total_bytes;
    let text = if params.strip_ansi.unwrap_or(false) {
        strip_ansi(&slice.text)
    } else {
        slice.text
    };

    Json(serde_json::json!({
        "job_id": job.id,
        "status": job.status,
        "exit_code": job.exit_code,
        "offset": slice.offset,
        "next_offset": slice.next_offset,
        "total_bytes": slice.total_bytes,
        "eof": eof,
        "reset": slice.reset,
        "text": text,
    }))
    .into_response()
}

#[derive(Deserialize)]
pub struct WaitQuery {
    /// Seconds to hold the request open (default 30, max 60).
    #[serde(default)]
    pub timeout: Option<u64>,
    /// The `etag` from a previous response. Without it the call returns
    /// immediately with the current state.
    #[serde(default)]
    pub etag: Option<String>,
    /// `terminal`: only return early once the pipeline has finished, not on
    /// every intermediate change.
    #[serde(default)]
    pub until: Option<String>,
}

struct Snapshot {
    etag: String,
    terminal: bool,
    body: serde_json::Value,
}

async fn load_snapshot(
    state: &AppState,
    repo_id: i64,
    pipeline_id: i64,
) -> Result<Snapshot, AppError> {
    let pipeline = match rg_db::ops::pipeline_ops::get_pipeline(&state.db, pipeline_id).await {
        Ok(Some(p)) if p.repo_id == repo_id => p,
        Ok(_) => return Err(AppError::not_found("pipeline not found")),
        Err(e) => return Err(AppError::from(e)),
    };
    let jobs = rg_db::ops::pipeline_ops::list_jobs_by_pipeline(&state.db, pipeline_id)
        .await
        .map_err(AppError::from)?;

    let pairs: Vec<(i64, &str)> = jobs.iter().map(|j| (j.id, j.status.as_str())).collect();
    let etag = state_etag(&pipeline.status, &pairs);
    let terminal = is_terminal_pipeline_status(&pipeline.status);

    let job_views: Vec<serde_json::Value> = jobs
        .iter()
        .map(|j| {
            let mut view = serde_json::json!({
                "id": j.id,
                "name": j.name,
                "status": j.status,
                "exit_code": j.exit_code,
            });
            if let Some(summary) = summary_for_job(&j.status, j.exit_code, j.log.as_deref()) {
                view["failure_summary"] = serde_json::json!(summary);
            }
            view
        })
        .collect();

    Ok(Snapshot {
        etag,
        terminal,
        body: serde_json::json!({
            "pipeline": {
                "id": pipeline.id,
                "status": pipeline.status,
                "commit_sha": pipeline.commit_sha,
                "ref_name": pipeline.ref_name,
                "started_at": pipeline.started_at.map(|t| t.to_string()),
                "finished_at": pipeline.finished_at.map(|t| t.to_string()),
            },
            "jobs": job_views,
        }),
    })
}

/// GET /api/v1/repos/:owner/:name/pipelines/:id/wait
/// Long-poll until the pipeline changes (or finishes) instead of polling.
#[utoipa::path(
    get,
    path = "/repos/{owner}/{name}/pipelines/{id}/wait",
    tag = "CI/CD",
    params(
        ("owner" = String, Path, description = "owner"),
        ("name" = String, Path, description = "name"),
        ("id" = i64, Path, description = "pipeline id"),
        ("timeout" = Option<u64>, Query, description = "Seconds to wait (default 30, max 60)"),
        ("etag" = Option<String>, Query, description = "etag from the previous response; omit for an immediate snapshot"),
        ("until" = Option<String>, Query, description = "`terminal` to return only when the pipeline has finished"),
    ),
    responses(
        (status = 200, description = "Current state with etag, changed, terminal, timed_out", body = serde_json::Value),
        (status = 400, description = "Invalid `until`", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
        (status = 404, description = "Pipeline not found", body = serde_json::Value),
        (status = 429, description = "Too many concurrent waits", body = serde_json::Value),
    ),
)]
pub async fn wait_pipeline(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((owner, name, pipeline_id)): Path<(String, String, i64)>,
    Query(params): Query<WaitQuery>,
) -> impl IntoResponse {
    let until_terminal = match params.until.as_deref() {
        None => false,
        Some("terminal") => true,
        Some(_) => return AppError::bad_request("`until` must be `terminal`").into_response(),
    };
    let repo = match repo_access::require_read(&state, &headers, &owner, &name).await {
        Ok(repo) => repo,
        Err(e) => return e.into_response(),
    };

    let timeout = Duration::from_secs(
        params
            .timeout
            .unwrap_or(DEFAULT_WAIT_SECS)
            .min(MAX_WAIT_SECS),
    );
    let deadline = Instant::now() + timeout;

    // Hold a slot only while waiting; an immediate snapshot needs none.
    let mut _permit = None;
    loop {
        let snapshot = match load_snapshot(&state, repo.id, pipeline_id).await {
            Ok(s) => s,
            Err(e) => return e.into_response(),
        };
        let changed = params.etag.as_deref() != Some(snapshot.etag.as_str());
        let satisfied = if until_terminal {
            snapshot.terminal
        } else {
            changed
        };
        let remaining = deadline.saturating_duration_since(Instant::now());
        if satisfied || remaining.is_zero() {
            let mut body = snapshot.body;
            body["etag"] = serde_json::json!(snapshot.etag);
            body["changed"] = serde_json::json!(changed);
            body["terminal"] = serde_json::json!(snapshot.terminal);
            body["timed_out"] = serde_json::json!(!satisfied);
            return Json(body).into_response();
        }
        if _permit.is_none() {
            match WAIT_PERMITS.try_acquire() {
                Ok(permit) => _permit = Some(permit),
                Err(_) => {
                    return AppError::rate_limited("too many concurrent wait requests")
                        .into_response()
                }
            }
        }
        tokio::time::sleep(WAIT_POLL.min(remaining)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_csi_and_osc_sequences() {
        assert_eq!(strip_ansi("\u{1b}[31mred\u{1b}[0m ok"), "red ok");
        assert_eq!(strip_ansi("a\u{1b}]0;title\u{7}b"), "ab");
        assert_eq!(strip_ansi("a\u{1b}]0;title\u{1b}\\b"), "ab");
        assert_eq!(strip_ansi("plain"), "plain");
        assert_eq!(strip_ansi("trailing\u{1b}"), "trailing");
    }

    #[test]
    fn slices_resume_exactly_where_the_previous_window_ended() {
        let log = "line one\nline two\nline three\n";
        let mut assembled = String::new();
        let mut offset = 0;
        loop {
            let slice = slice_log(log, offset, 7);
            assert!(!slice.reset);
            assembled.push_str(&slice.text);
            if slice.next_offset >= slice.total_bytes {
                break;
            }
            assert!(slice.next_offset > offset, "must make progress");
            offset = slice.next_offset;
        }
        assert_eq!(assembled, log);
    }

    #[test]
    fn slices_never_split_a_multibyte_character() {
        let log = "ab日本語cd";
        let mut assembled = String::new();
        let mut offset = 0;
        for _ in 0..32 {
            let slice = slice_log(log, offset, 2);
            assembled.push_str(&slice.text);
            offset = slice.next_offset;
            if offset >= log.len() {
                break;
            }
        }
        assert_eq!(assembled, log);
    }

    #[test]
    fn a_limit_smaller_than_one_character_still_advances() {
        let slice = slice_log("日", 0, 1);
        assert_eq!(slice.text, "日");
        assert_eq!(slice.next_offset, 3);
    }

    #[test]
    fn an_offset_past_the_end_resets_to_the_start() {
        let slice = slice_log("short", 100, 10);
        assert!(slice.reset);
        assert_eq!(slice.offset, 0);
        assert_eq!(slice.text, "short");
    }

    #[test]
    fn an_offset_at_the_end_is_an_empty_window_not_a_reset() {
        let slice = slice_log("abc", 3, 10);
        assert!(!slice.reset);
        assert_eq!(slice.text, "");
        assert_eq!(slice.next_offset, 3);
    }

    #[test]
    fn tail_returns_the_last_lines() {
        let log = "a\nb\nc\nd\n";
        assert_eq!(tail_log(log, 2, 1024).text, "c\nd\n");
        assert_eq!(tail_log(log, 10, 1024).text, log);
        assert_eq!(tail_log("a\nb", 1, 1024).text, "b");
        assert_eq!(tail_log(log, 2, 1024).next_offset, log.len());
    }

    #[test]
    fn tail_is_capped_by_bytes_on_a_char_boundary() {
        let log = "日本語日本語日本語";
        let slice = tail_log(log, 1, 7);
        assert!(slice.text.len() <= 7);
        assert!(log.ends_with(&slice.text));
    }

    #[test]
    fn summary_keeps_the_last_lines_and_flags_truncation() {
        let log: String = (1..=80).map(|n| format!("line {n}\n")).collect();
        let summary = failure_summary(Some(&log), Some(2));
        assert_eq!(summary.exit_code, Some(2));
        assert_eq!(summary.tail.len(), SUMMARY_TAIL_LINES);
        assert_eq!(summary.tail.last().unwrap(), "line 80");
        assert!(summary.truncated);
    }

    #[test]
    fn summary_of_a_short_log_is_not_truncated() {
        let summary = failure_summary(Some("only\ntwo\n"), Some(1));
        assert_eq!(summary.tail, vec!["only", "two"]);
        assert!(!summary.truncated);
    }

    #[test]
    fn summary_surfaces_error_lines_from_before_the_tail() {
        let mut log = String::from("error[E0308]: mismatched types\n");
        for n in 0..100 {
            log.push_str(&format!("noise {n}\n"));
        }
        let summary = failure_summary(Some(&log), Some(101));
        assert_eq!(summary.error_lines, vec!["error[E0308]: mismatched types"]);
        assert!(!summary.tail.iter().any(|l| l.contains("E0308")));
    }

    #[test]
    fn summary_dedupes_and_bounds_error_lines() {
        let log = "FAILED a\nFAILED a\n".to_string()
            + &(0..30)
                .map(|n| format!("FAILED t{n}\n"))
                .collect::<String>();
        let summary = failure_summary(Some(&log), None);
        assert_eq!(summary.error_lines.len(), MAX_ERROR_LINES);
        assert_eq!(
            summary
                .error_lines
                .iter()
                .filter(|l| *l == "FAILED a")
                .count(),
            1
        );
    }

    #[test]
    fn summary_strips_ansi_and_clips_long_lines() {
        let long = "x".repeat(MAX_LINE_CHARS + 50);
        let log = format!("\u{1b}[31merror: boom\u{1b}[0m\n{long}\n");
        let summary = failure_summary(Some(&log), Some(1));
        assert_eq!(summary.tail[0], "error: boom");
        assert_eq!(summary.tail[1].chars().count(), MAX_LINE_CHARS + 1);
        assert!(summary.tail[1].ends_with('…'));
    }

    #[test]
    fn summary_without_a_log_is_empty_but_keeps_the_exit_code() {
        let summary = failure_summary(None, Some(137));
        assert_eq!(summary.exit_code, Some(137));
        assert!(summary.tail.is_empty() && summary.error_lines.is_empty());
    }

    #[test]
    fn only_failed_jobs_get_a_summary() {
        assert!(summary_for_job("failed", Some(1), Some("x")).is_some());
        assert!(summary_for_job("error", None, None).is_some());
        assert!(summary_for_job("success", Some(0), Some("x")).is_none());
        assert!(summary_for_job("running", None, Some("x")).is_none());
        assert!(summary_for_job("canceled", None, Some("x")).is_none());
    }

    #[test]
    fn etag_changes_with_any_status_and_ignores_job_order() {
        let a = state_etag("running", &[(1, "running"), (2, "pending")]);
        assert_eq!(a, state_etag("running", &[(2, "pending"), (1, "running")]));
        assert_ne!(a, state_etag("running", &[(1, "success"), (2, "pending")]));
        assert_ne!(a, state_etag("failed", &[(1, "running"), (2, "pending")]));
        assert_ne!(a, state_etag("running", &[(1, "running")]));
    }

    #[test]
    fn terminal_status_sets_match_the_pipeline_state_machine() {
        for s in ["success", "failed", "canceled"] {
            assert!(is_terminal_pipeline_status(s), "{s}");
            assert!(is_terminal_job_status(s), "{s}");
        }
        for s in ["pending", "running", "manual", "waiting_approval"] {
            assert!(!is_terminal_pipeline_status(s), "{s}");
            assert!(!is_terminal_job_status(s), "{s}");
        }
    }
}
