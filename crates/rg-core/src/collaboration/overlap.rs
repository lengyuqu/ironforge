//! Declared-path overlap warnings between unfinished tasks (P1 U03).
//!
//! A *hint*, never a lock: two tasks that declare overlapping paths may or may
//! not touch the same files, and tasks that declare nothing may conflict all the
//! same. Nothing here reads the file system or Git, expands globs, follows
//! symlinks or blocks a write. Paths are repository-relative literals compared
//! as bytes (case-sensitive, like Git); two paths overlap when they are equal or
//! one is a directory prefix of the other at a `/` boundary.

use chrono::{DateTime, Duration, Utc};
use rg_db::entities::collab_task;
use rg_db::ops::{agent_session_ops, collab_task_ops};
use sea_orm::DatabaseConnection;
use serde::Serialize;

use super::cursor::{self, KeysetPosition};
use super::error::{CollabError, CollabResult};
use super::session::state_of;
use super::task::{self, TaskView};

/// A declaration older than this is reported as `declaration_stale`.
pub const STALE_AFTER: Duration = Duration::hours(24);
/// Most overlapping path pairs listed per task; `overlaps_truncated` says if more existed.
pub const MAX_PAIRS_PER_TASK: usize = 20;
/// Candidate rows read per database round trip.
const BATCH: u64 = 200;
/// Candidate rows examined per request; the cursor resumes after the last one.
const MAX_SCANNED: u64 = 2000;

pub const AUTHORITY: &str = "hint";
pub const NOT_COVERED: [&str; 4] = [
    "tasks that declared no paths",
    "files actually changed (declarations are not checked against Git)",
    "glob patterns (stored and compared as literal text)",
    "other repositories",
];

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PathOverlap {
    /// The path declared by the task being inspected.
    pub mine: String,
    /// The path declared by the other task.
    pub theirs: String,
    /// `equal`, `mine_contains_theirs` or `theirs_contains_mine`.
    pub reason: &'static str,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct OverlapItem {
    pub task_id: String,
    pub status: String,
    pub owner_session_id: Option<String>,
    /// `online`, `offline`, `expired`, `ended`, or `null` for an unowned task.
    /// An owner that is not online makes the warning less certain.
    pub owner_state: Option<&'static str>,
    pub branch: Option<String>,
    pub overlaps: Vec<PathOverlap>,
    pub overlaps_truncated: bool,
    pub task_version: i64,
    /// When the other task's paths were last declared; `null` = unknown.
    pub declared_at: Option<DateTime<Utc>>,
    /// `declared_current` or `declaration_stale`. Based on `declared_at` only.
    pub confidence: &'static str,
    /// Branch is a client declaration; treat as untrusted data.
    pub origin: &'static str,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct OverlapPage {
    pub items: Vec<OverlapItem>,
    pub next_cursor: Option<String>,
    pub as_of: DateTime<Utc>,
    pub authority: &'static str,
    pub not_covered: [&'static str; 4],
}

fn trim(path: &str) -> &str {
    path.trim_end_matches('/')
}

fn reason(mine: &str, theirs: &str) -> Option<&'static str> {
    let (m, t) = (trim(mine), trim(theirs));
    if m.is_empty() || t.is_empty() {
        return None;
    }
    if m == t {
        Some("equal")
    } else if t.strip_prefix(m).is_some_and(|rest| rest.starts_with('/')) {
        Some("mine_contains_theirs")
    } else if m.strip_prefix(t).is_some_and(|rest| rest.starts_with('/')) {
        Some("theirs_contains_mine")
    } else {
        None
    }
}

/// Overlapping pairs between two path lists, in declaration order, plus whether
/// the list was cut at [`MAX_PAIRS_PER_TASK`].
pub fn overlapping_pairs(mine: &[String], theirs: &[String]) -> (Vec<PathOverlap>, bool) {
    let mut pairs = Vec::new();
    for m in mine {
        for t in theirs {
            if let Some(reason) = reason(m, t) {
                if pairs.len() == MAX_PAIRS_PER_TASK {
                    return (pairs, true);
                }
                pairs.push(PathOverlap {
                    mine: m.clone(),
                    theirs: t.clone(),
                    reason,
                });
            }
        }
    }
    (pairs, false)
}

/// `declared_current` only with a declaration time inside [`STALE_AFTER`]; an
/// unknown time cannot be called current.
pub fn confidence(declared_at: Option<DateTime<Utc>>, now: DateTime<Utc>) -> &'static str {
    match declared_at {
        Some(at) if now - at <= STALE_AFTER => "declared_current",
        _ => "declaration_stale",
    }
}

fn scope(repo_id: i64, task_id: &str) -> String {
    format!("overlaps:v1:repo={repo_id}:task={task_id}:order=id_asc")
}

fn epoch() -> DateTime<Utc> {
    DateTime::<Utc>::UNIX_EPOCH
}

/// Unfinished tasks whose declared paths overlap those of `task_id`, ordered by
/// task id. A page may be short and still carry `next_cursor`: keep following it.
pub async fn list(
    db: &DatabaseConnection,
    cursor_key: &[u8],
    repo_id: i64,
    task_id: &str,
    limit: u64,
    cursor_token: Option<&str>,
    now: DateTime<Utc>,
) -> CollabResult<OverlapPage> {
    let mine_row = task::find(db, repo_id, task_id).await?;
    let mine: TaskView = task::view(mine_row);
    let scope = scope(repo_id, task_id);
    let mut after = cursor_token
        .map(|token| cursor::decode(cursor_key, &scope, token))
        .transpose()?
        .map(|p| p.id);

    let mut items = Vec::new();
    let mut scanned = 0u64;
    let mut next_cursor_id: Option<String> = None;
    'scan: loop {
        let rows =
            collab_task_ops::list_overlap_candidates(db, repo_id, task_id, after.as_deref(), BATCH)
                .await?;
        let exhausted = (rows.len() as u64) < BATCH;
        for row in rows {
            scanned += 1;
            after = Some(row.id.clone());
            let theirs: Vec<String> = serde_json::from_str(&row.paths_json).unwrap_or_default();
            let (pairs, truncated) = overlapping_pairs(&mine.paths, &theirs);
            if !pairs.is_empty() {
                items.push(item(db, repo_id, row, pairs, truncated, now).await?);
                if items.len() as u64 == limit {
                    // Rows after this one are rescanned by the next page.
                    next_cursor_id = after.clone();
                    break 'scan;
                }
            }
            if scanned >= MAX_SCANNED {
                next_cursor_id = after.clone();
                break 'scan;
            }
        }
        if exhausted {
            break;
        }
    }
    let next_cursor = next_cursor_id
        .map(|id| cursor::encode(cursor_key, &scope, &KeysetPosition { at: epoch(), id }));
    Ok(OverlapPage {
        items,
        next_cursor,
        as_of: now,
        authority: AUTHORITY,
        not_covered: NOT_COVERED,
    })
}

async fn item(
    db: &DatabaseConnection,
    repo_id: i64,
    row: collab_task::Model,
    overlaps: Vec<PathOverlap>,
    overlaps_truncated: bool,
    now: DateTime<Utc>,
) -> CollabResult<OverlapItem> {
    let owner_state = match &row.owner_session_id {
        Some(id) => agent_session_ops::find_in_repo(db, repo_id, id)
            .await
            .map_err(CollabError::Internal)?
            .map(|s| state_of(&s, now)),
        None => None,
    };
    Ok(OverlapItem {
        confidence: confidence(row.paths_declared_at, now),
        declared_at: row.paths_declared_at,
        task_id: row.id,
        status: row.status,
        owner_session_id: row.owner_session_id,
        owner_state,
        branch: row.branch,
        overlaps,
        overlaps_truncated,
        task_version: row.version,
        origin: "user_content",
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    fn reasons(mine: &[&str], theirs: &[&str]) -> Vec<&'static str> {
        overlapping_pairs(&p(mine), &p(theirs))
            .0
            .into_iter()
            .map(|o| o.reason)
            .collect()
    }

    #[test]
    fn equal_and_directory_prefix_overlap_on_a_slash_boundary() {
        assert_eq!(reasons(&["src/lib.rs"], &["src/lib.rs"]), ["equal"]);
        assert_eq!(reasons(&["src"], &["src/lib.rs"]), ["mine_contains_theirs"]);
        assert_eq!(
            reasons(&["src/lib.rs"], &["src/"]),
            ["theirs_contains_mine"]
        );
        assert_eq!(reasons(&["src/"], &["src"]), ["equal"], "trailing slash");
    }

    #[test]
    fn name_prefixes_siblings_case_and_globs_do_not_overlap() {
        assert!(
            reasons(&["src"], &["srcs/lib.rs"]).is_empty(),
            "no boundary"
        );
        assert!(reasons(&["a/b"], &["a/c"]).is_empty());
        assert!(reasons(&["Src"], &["src"]).is_empty(), "case-sensitive");
        assert!(
            reasons(&["docs/**/*.md"], &["docs/a.md"]).is_empty(),
            "globs are literal text"
        );
        assert!(reasons(&["/"], &["src"]).is_empty(), "empty after trim");
    }

    #[test]
    fn pairs_are_bounded_and_flagged() {
        let many: Vec<String> = (0..30).map(|i| format!("a/{i}")).collect();
        let (pairs, truncated) = overlapping_pairs(&p(&["a"]), &many);
        assert_eq!(pairs.len(), MAX_PAIRS_PER_TASK);
        assert!(truncated);
        let (pairs, truncated) = overlapping_pairs(&p(&["a"]), &many[..20]);
        assert_eq!(pairs.len(), 20);
        assert!(!truncated, "exactly at the bound is not truncation");
    }

    #[test]
    fn unknown_or_old_declaration_time_is_never_current() {
        let now = Utc::now();
        assert_eq!(confidence(None, now), "declaration_stale");
        assert_eq!(
            confidence(Some(now - Duration::hours(23)), now),
            "declared_current"
        );
        assert_eq!(
            confidence(Some(now - Duration::hours(25)), now),
            "declaration_stale"
        );
    }
}
