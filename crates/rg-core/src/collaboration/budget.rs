//! Task budgets and usage records (O08).
//!
//! Informational bookkeeping, nothing more:
//!
//! * **Unit and source are required** on every budget and usage record and are
//!   compared verbatim. Amounts are never converted, and **never summed across
//!   different units or sources** — `USD` from `invoice` and `USD` from
//!   `harness-estimate` are two separate ledgers.
//! * **Missing usage is not zero.** A budget with no usage records for its unit
//!   and source reports `status: "unknown"`, `total: null`, a reason, and
//!   `over_threshold: null`.
//! * Usage is reported by sessions (or through a user door) about themselves;
//!   the platform does not measure anything. Every value is `user_content`.
//! * A threshold produces a derived `over_threshold` flag together with the
//!   numbers it was derived from. **Nothing pauses, blocks or stops work**:
//!   enforcement is out of scope, and the flag is a hint for whoever reads it.
//! * Usage records are deltas for their stated period. Records of one unit and
//!   source whose periods overlap may double-count (e.g. a cumulative counter
//!   reported twice); the total is still the plain sum but is flagged
//!   `overlapping_periods` / `total_may_double_count`.
//!
//! Amounts are non-negative decimals with at most 18 integer and 6 fractional
//! digits, stored as normalized text and added as integer micro-units, so there
//! is no floating-point drift.

use std::collections::BTreeMap;

use chrono::{DateTime, Duration, Utc};
use rg_db::entities::{collab_task_budget, collab_task_usage};
use rg_db::ops::collab_budget_ops::{self, AppendUsageOutcome, PutBudgetOutcome};
use sea_orm::{DatabaseConnection, Set};
use serde::Serialize;
use sha2::{Digest, Sha256};

use super::cursor::{self, KeysetPosition};
use super::error::{CollabError, CollabResult};
use super::{task, validation, Actor, Page, ORIGIN_USER_CONTENT};

pub const MAX_BUDGETS_PER_TASK: u64 = 20;
pub const MAX_USAGE_PER_TASK: u64 = 2000;
const MAX_UNIT_CHARS: usize = 32;
const MAX_SOURCE_CHARS: usize = 64;
const MAX_PERIOD_LABEL_CHARS: usize = 64;
/// How far past "now" a usage period may end (clock skew allowance).
const FUTURE_SKEW: Duration = Duration::minutes(5);

pub const ENFORCEMENT: &str = "none: thresholds are informational and never pause, block or stop any work";

// ── amounts ───────────────────────────────────────────────────────────

const MICROS: i128 = 1_000_000;

/// Parse a non-negative decimal (`12`, `0.5`, `1234.567890`) into normalized
/// text (no leading zeros, no trailing fractional zeros) and micro-units.
pub fn parse_amount(field: &str, raw: &str) -> CollabResult<(String, i128)> {
    let bad = || {
        CollabError::bad_request(format!(
            "{field} must be a non-negative decimal with at most 18 integer and 6 fractional digits (no sign, exponent or separators)"
        ))
    };
    let (int_part, frac_part) = match raw.split_once('.') {
        Some((i, f)) => (i, f),
        None => (raw, ""),
    };
    let digits = |s: &str| s.bytes().all(|b| b.is_ascii_digit());
    if int_part.is_empty()
        || int_part.len() > 18
        || !digits(int_part)
        || (int_part.len() > 1 && int_part.starts_with('0'))
        || frac_part.len() > 6
        || !digits(frac_part)
        || (raw.contains('.') && frac_part.is_empty())
    {
        return Err(bad());
    }
    let int: i128 = int_part.parse().map_err(|_| bad())?;
    let mut frac_padded = frac_part.to_string();
    while frac_padded.len() < 6 {
        frac_padded.push('0');
    }
    let frac: i128 = frac_padded.parse().map_err(|_| bad())?;
    let micros = int * MICROS + frac;
    Ok((format_micros(micros), micros))
}

pub fn format_micros(micros: i128) -> String {
    let int = micros / MICROS;
    let frac = micros % MICROS;
    if frac == 0 {
        int.to_string()
    } else {
        let frac = format!("{frac:06}");
        format!("{int}.{}", frac.trim_end_matches('0'))
    }
}

fn micros_of(stored: &str) -> i128 {
    parse_amount("stored amount", stored).map(|(_, m)| m).unwrap_or(0)
}

fn token(field: &str, value: &str, max: usize) -> CollabResult<String> {
    let ok = !value.is_empty()
        && value.len() <= max
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b':' | b'/' | b'@' | b'+' | b'-'));
    if ok {
        Ok(value.to_string())
    } else {
        Err(CollabError::bad_request(format!(
            "{field} must be 1-{max} characters of [A-Za-z0-9._:/@+-]; it is compared exactly (case-sensitive) and never converted"
        )))
    }
}

fn hash_of(value: &serde_json::Value) -> String {
    hex::encode(Sha256::digest(value.to_string().as_bytes()))
}

// ── budgets ───────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct SetBudget {
    /// The budget's `version` you last saw; 0 when none exists for this
    /// unit and source.
    pub expected_version: i64,
    pub unit: String,
    pub source: String,
    pub amount: String,
    /// Optional lower threshold in the same unit; `over_threshold` is judged
    /// against it when set, otherwise against `amount`.
    pub alert_at: Option<String>,
    /// What period the source's measurements cover or are sampled over, e.g.
    /// `task_total` or `per_day`. Free text; it is recorded, not interpreted.
    pub sampling_period: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct UsageStatus {
    /// `unknown` (no usage reported for this unit and source) or `reported`.
    pub status: &'static str,
    /// Why the status is `unknown`.
    pub reason: Option<String>,
    /// Sum of the reported records of exactly this unit and source; `null`
    /// (never `"0"`) when nothing was reported.
    pub total: Option<String>,
    pub record_count: u64,
    pub covered_from: Option<DateTime<Utc>>,
    pub covered_to: Option<DateTime<Utc>>,
    pub last_reported_at: Option<DateTime<Utc>>,
    /// Periods of the records overlap, so the sum may count something twice.
    pub overlapping_periods: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ThresholdEvaluation {
    /// `alert_at` or `amount`: which number was used as the threshold.
    pub basis: &'static str,
    pub threshold_amount: String,
    pub used_total: String,
    pub unit: String,
    pub source: String,
    /// How `over_threshold` follows from the numbers.
    pub comparison: &'static str,
    pub total_may_double_count: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct BudgetEntry {
    pub task_id: String,
    pub unit: String,
    pub source: String,
    pub amount: String,
    pub alert_at: Option<String>,
    pub sampling_period: String,
    pub version: i64,
    pub set_by_kind: String,
    pub set_by_id: String,
    pub updated_at: DateTime<Utc>,
    pub usage: UsageStatus,
    /// `null` when usage is unknown; otherwise whether `used_total` reached the
    /// threshold. Informational only.
    pub over_threshold: Option<bool>,
    /// The numbers behind `over_threshold`; `null` when usage is unknown.
    pub threshold: Option<ThresholdEvaluation>,
    pub enforcement: &'static str,
    pub origin: &'static str,
}

/// Usage that has no budget of the same unit and source. Listed so nothing is
/// hidden, but compared with no budget.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct UnmatchedUsage {
    pub unit: String,
    pub source: String,
    pub total: String,
    pub record_count: u64,
    pub covered_from: DateTime<Utc>,
    pub covered_to: DateTime<Utc>,
    pub overlapping_periods: bool,
    pub note: &'static str,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct BudgetSummary {
    pub task_id: String,
    pub as_of: DateTime<Utc>,
    pub budgets: Vec<BudgetEntry>,
    pub unmatched_usage: Vec<UnmatchedUsage>,
    pub enforcement: &'static str,
    /// Plain-language reminders that travel with the numbers.
    pub notes: [&'static str; 3],
}

const NOTES: [&str; 3] = [
    "Amounts of different units or sources are never added, converted or compared.",
    "No usage reported means unknown, not zero.",
    "Budgets and usage are reported by clients, not measured by the platform.",
];

struct Group {
    micros: i128,
    count: u64,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    last_reported: DateTime<Utc>,
    overlap: bool,
}

/// Group the task's usage rows by exact `(unit, source)`. Rows come ordered by
/// period start, which is what overlap detection needs.
fn group_usage(rows: &[collab_task_usage::Model]) -> BTreeMap<(String, String), Group> {
    let mut groups: BTreeMap<(String, String), Group> = BTreeMap::new();
    for row in rows {
        let group = groups
            .entry((row.unit.clone(), row.source.clone()))
            .or_insert_with(|| Group {
                micros: 0,
                count: 0,
                from: row.period_start,
                to: row.period_end,
                last_reported: row.created_at,
                overlap: false,
            });
        // `to` is the latest end seen so far: a record that starts before it
        // overlaps an earlier one.
        if group.count > 0 && row.period_start < group.to {
            group.overlap = true;
        }
        group.micros += micros_of(&row.amount);
        group.count += 1;
        group.from = group.from.min(row.period_start);
        group.to = group.to.max(row.period_end);
        group.last_reported = group.last_reported.max(row.created_at);
    }
    groups
}

fn entry(row: collab_task_budget::Model, group: Option<&Group>) -> BudgetEntry {
    let threshold_text = row.alert_at.clone().unwrap_or_else(|| row.amount.clone());
    let basis = if row.alert_at.is_some() { "alert_at" } else { "amount" };
    let (usage, over, threshold) = match group {
        None => (
            UsageStatus {
                status: "unknown",
                reason: Some(format!(
                    "no usage has been reported for unit '{}' from source '{}'; usage in other units or sources is neither converted nor added (see unmatched_usage)",
                    row.unit, row.source
                )),
                total: None,
                record_count: 0,
                covered_from: None,
                covered_to: None,
                last_reported_at: None,
                overlapping_periods: false,
            },
            None,
            None,
        ),
        Some(g) => {
            let used = format_micros(g.micros);
            let over = g.micros >= micros_of(&threshold_text);
            (
                UsageStatus {
                    status: "reported",
                    reason: None,
                    total: Some(used.clone()),
                    record_count: g.count,
                    covered_from: Some(g.from),
                    covered_to: Some(g.to),
                    last_reported_at: Some(g.last_reported),
                    overlapping_periods: g.overlap,
                },
                Some(over),
                Some(ThresholdEvaluation {
                    basis,
                    threshold_amount: threshold_text,
                    used_total: used,
                    unit: row.unit.clone(),
                    source: row.source.clone(),
                    comparison: "over_threshold = used_total >= threshold_amount",
                    total_may_double_count: g.overlap,
                }),
            )
        }
    };
    BudgetEntry {
        task_id: row.task_id,
        unit: row.unit,
        source: row.source,
        amount: row.amount,
        alert_at: row.alert_at,
        sampling_period: row.sampling_period,
        version: row.version,
        set_by_kind: row.set_by_kind,
        set_by_id: row.set_by_id,
        updated_at: row.updated_at,
        usage,
        over_threshold: over,
        threshold,
        enforcement: ENFORCEMENT,
        origin: ORIGIN_USER_CONTENT,
    }
}

/// Budgets of a task with their matched usage, plus the usage that matches no
/// budget.
pub async fn summary(
    db: &DatabaseConnection,
    repo_id: i64,
    task_id: &str,
    now: DateTime<Utc>,
) -> CollabResult<BudgetSummary> {
    let task = task::find(db, repo_id, task_id).await?;
    let budgets = collab_budget_ops::list_budgets(db, repo_id, &task.id).await?;
    let rows = collab_budget_ops::all_usage(db, repo_id, &task.id).await?;
    let mut groups = group_usage(&rows);

    let mut entries = Vec::with_capacity(budgets.len());
    for budget in budgets {
        let group = groups.remove(&(budget.unit.clone(), budget.source.clone()));
        entries.push(entry(budget, group.as_ref()));
    }
    let unmatched_usage = groups
        .into_iter()
        .map(|((unit, source), g)| UnmatchedUsage {
            unit,
            source,
            total: format_micros(g.micros),
            record_count: g.count,
            covered_from: g.from,
            covered_to: g.to,
            overlapping_periods: g.overlap,
            note: "reported, but this task has no budget of the same unit and source; it is compared with nothing",
        })
        .collect();
    Ok(BudgetSummary {
        task_id: task.id,
        as_of: now,
        budgets: entries,
        unmatched_usage,
        enforcement: ENFORCEMENT,
        notes: NOTES,
    })
}

pub struct SavedBudget {
    pub budget: BudgetEntry,
    pub replayed: bool,
}

/// Set (create or replace) the budget of one unit and source of a task. The
/// caller has established repository write access.
pub async fn set_budget(
    db: &DatabaseConnection,
    repo_id: i64,
    actor: &Actor,
    task_id: &str,
    input: SetBudget,
    now: DateTime<Utc>,
) -> CollabResult<SavedBudget> {
    if input.expected_version < 0 {
        return Err(CollabError::bad_request("expected_version must not be negative"));
    }
    let unit = token("unit", &input.unit, MAX_UNIT_CHARS)?;
    let source = token("source", &input.source, MAX_SOURCE_CHARS)?;
    let (amount, amount_micros) = parse_amount("amount", &input.amount)?;
    if amount_micros == 0 {
        return Err(CollabError::bad_request("amount must be greater than zero"));
    }
    let alert_at = match &input.alert_at {
        None => None,
        Some(raw) => {
            let (text, micros) = parse_amount("alert_at", raw)?;
            if micros > amount_micros {
                return Err(CollabError::bad_request(
                    "alert_at is in the budget's unit and must not exceed amount",
                ));
            }
            Some(text)
        }
    };
    let sampling_period = validation::label("sampling_period", &input.sampling_period, MAX_PERIOD_LABEL_CHARS)?;
    let task = task::find(db, repo_id, task_id).await?;
    if input.expected_version == 0
        && collab_budget_ops::find_budget(db, &task.id, &unit, &source).await?.is_none()
        && collab_budget_ops::count_budgets(db, &task.id).await? >= MAX_BUDGETS_PER_TASK
    {
        return Err(CollabError::coded_conflict(
            "BUDGET_LIMIT_REACHED",
            format!("a task may have at most {MAX_BUDGETS_PER_TASK} budgets"),
        ));
    }
    let hash = hash_of(&serde_json::json!([task.id, unit, source, amount, alert_at, sampling_period]));

    let outcome = collab_budget_ops::put_budget(
        db,
        input.expected_version,
        collab_task_budget::ActiveModel {
            id: Set(uuid::Uuid::new_v4().to_string()),
            repo_id: Set(repo_id),
            task_id: Set(task.id.clone()),
            unit: Set(unit.clone()),
            source: Set(source.clone()),
            amount: Set(amount),
            alert_at: Set(alert_at),
            sampling_period: Set(sampling_period),
            version: Set(input.expected_version + 1),
            set_by_kind: Set(actor.kind.to_string()),
            set_by_id: Set(actor.id.clone()),
            payload_hash: Set(hash),
            created_at: Set(now),
            updated_at: Set(now),
        },
    )
    .await?;
    let (row, replayed) = match outcome {
        PutBudgetOutcome::Saved(row) => (row, false),
        PutBudgetOutcome::Replayed(row) => (row, true),
        PutBudgetOutcome::Conflict { current_version } => {
            return Err(CollabError::VersionConflict { current_version })
        }
    };
    let rows = collab_budget_ops::all_usage(db, repo_id, &task.id).await?;
    let groups = group_usage(&rows);
    let group = groups.get(&(unit, source));
    Ok(SavedBudget { budget: entry(row, group), replayed })
}

// ── usage ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct NewUsage {
    pub client_usage_id: String,
    pub unit: String,
    pub source: String,
    pub amount: String,
    pub period_start: DateTime<Utc>,
    pub period_end: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct UsageView {
    pub id: String,
    pub task_id: String,
    pub unit: String,
    pub source: String,
    pub amount: String,
    pub period_start: DateTime<Utc>,
    pub period_end: DateTime<Utc>,
    pub reported_by_kind: String,
    pub reported_by_id: String,
    pub client_usage_id: String,
    pub created_at: DateTime<Utc>,
    /// The reporter's claim; the platform measured nothing.
    pub origin: &'static str,
}

fn usage_view(row: collab_task_usage::Model) -> UsageView {
    UsageView {
        id: row.id,
        task_id: row.task_id,
        unit: row.unit,
        source: row.source,
        amount: row.amount,
        period_start: row.period_start,
        period_end: row.period_end,
        reported_by_kind: row.reported_by_kind,
        reported_by_id: row.reported_by_id,
        client_usage_id: row.client_usage_id,
        created_at: row.created_at,
        origin: ORIGIN_USER_CONTENT,
    }
}

pub struct CreatedUsage {
    pub usage: UsageView,
    pub replayed: bool,
}

/// Append a usage record. Records are never edited; a wrong one is answered by
/// a correcting record in the same unit and source (negative amounts are not
/// accepted, so corrections are made by the reporting convention of the source).
pub async fn report_usage(
    db: &DatabaseConnection,
    repo_id: i64,
    actor: &Actor,
    task_id: &str,
    input: NewUsage,
    now: DateTime<Utc>,
) -> CollabResult<CreatedUsage> {
    validation::client_key("client_usage_id", &input.client_usage_id)?;
    let unit = token("unit", &input.unit, MAX_UNIT_CHARS)?;
    let source = token("source", &input.source, MAX_SOURCE_CHARS)?;
    let (amount, _) = parse_amount("amount", &input.amount)?;
    if input.period_start > input.period_end {
        return Err(CollabError::bad_request("period_start must not be after period_end"));
    }
    if input.period_end > now + FUTURE_SKEW {
        return Err(CollabError::bad_request("period_end must not be in the future"));
    }
    let task = task::find(db, repo_id, task_id).await?;
    let hash = hash_of(&serde_json::json!([
        task.id,
        unit,
        source,
        amount,
        input.period_start.to_rfc3339(),
        input.period_end.to_rfc3339()
    ]));
    // A retry is answered before the per-task cap is consulted.
    if let Some(existing) =
        collab_budget_ops::find_usage_by_client_id(db, actor.kind, &actor.id, &input.client_usage_id).await?
    {
        return replay(existing, &hash);
    }
    if collab_budget_ops::count_usage(db, &task.id).await? >= MAX_USAGE_PER_TASK {
        return Err(CollabError::coded_conflict(
            "USAGE_LIMIT_REACHED",
            format!("a task may hold at most {MAX_USAGE_PER_TASK} usage records"),
        ));
    }
    match collab_budget_ops::append_usage(
        db,
        collab_task_usage::ActiveModel {
            id: Set(uuid::Uuid::new_v4().to_string()),
            repo_id: Set(repo_id),
            task_id: Set(task.id.clone()),
            unit: Set(unit),
            source: Set(source),
            amount: Set(amount),
            period_start: Set(input.period_start),
            period_end: Set(input.period_end),
            reported_by_kind: Set(actor.kind.to_string()),
            reported_by_id: Set(actor.id.clone()),
            client_usage_id: Set(input.client_usage_id),
            payload_hash: Set(hash.clone()),
            created_at: Set(now),
        },
    )
    .await?
    {
        AppendUsageOutcome::Created(row) => Ok(CreatedUsage { usage: usage_view(row), replayed: false }),
        AppendUsageOutcome::Existing(row) => replay(row, &hash),
    }
}

fn replay(row: collab_task_usage::Model, hash: &str) -> CollabResult<CreatedUsage> {
    if row.payload_hash == hash {
        Ok(CreatedUsage { usage: usage_view(row), replayed: true })
    } else {
        Err(CollabError::IdempotencyKeyReused)
    }
}

fn usage_scope(repo_id: i64, task_id: &str, unit: Option<&str>, source: Option<&str>) -> String {
    format!(
        "task-usage:v1:repo={repo_id}:task={task_id}:unit={}:source={}:order=created_desc,id_desc",
        unit.unwrap_or("*"),
        source.unwrap_or("*")
    )
}

/// Usage records of a task, newest reported first; the filters are exact matches.
pub async fn list_usage(
    db: &DatabaseConnection,
    cursor_key: &[u8],
    repo_id: i64,
    task_id: &str,
    unit: Option<&str>,
    source: Option<&str>,
    limit: u64,
    cursor_token: Option<&str>,
) -> CollabResult<Page<UsageView>> {
    let task = task::find(db, repo_id, task_id).await?;
    let scope = usage_scope(repo_id, &task.id, unit, source);
    let after = cursor_token
        .map(|token| cursor::decode(cursor_key, &scope, token))
        .transpose()?
        .map(|p| (p.at, p.id));
    let mut rows =
        collab_budget_ops::list_usage_page(db, repo_id, &task.id, unit, source, after, limit + 1).await?;
    let has_more = rows.len() as u64 > limit;
    rows.truncate(limit as usize);
    let next_cursor = has_more.then(|| rows.last()).flatten().map(|last| {
        cursor::encode(
            cursor_key,
            &scope,
            &KeysetPosition { at: last.created_at, id: last.id.clone() },
        )
    });
    Ok(Page { items: rows.into_iter().map(usage_view).collect(), next_cursor })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn amounts_are_exact_decimals() {
        assert_eq!(parse_amount("a", "12").unwrap(), ("12".to_string(), 12_000_000));
        assert_eq!(parse_amount("a", "0.5").unwrap().0, "0.5");
        assert_eq!(parse_amount("a", "1.500000").unwrap().0, "1.5");
        assert_eq!(parse_amount("a", "0").unwrap().0, "0");
        assert_eq!(parse_amount("a", "999999999999999999.999999").unwrap().0, "999999999999999999.999999");
        for bad in ["", "-1", "+1", "1e3", "1,000", ".5", "5.", "01", "1.0000001", "NaN", "1 2", "1000000000000000000", "0x10"] {
            assert!(parse_amount("a", bad).is_err(), "{bad:?}");
        }
        // 0.1 + 0.2 is exactly 0.3 here.
        let sum = parse_amount("a", "0.1").unwrap().1 + parse_amount("a", "0.2").unwrap().1;
        assert_eq!(format_micros(sum), "0.3");
    }

    #[test]
    fn units_and_sources_are_exact_tokens() {
        assert!(token("u", "USD", 32).is_ok());
        assert!(token("u", "tokens/min", 32).is_ok());
        assert!(token("u", "", 32).is_err());
        assert!(token("u", "two words", 32).is_err());
        assert!(token("u", &"x".repeat(33), 32).is_err());
    }

    fn usage(unit: &str, source: &str, amount: &str, start: i64, end: i64) -> collab_task_usage::Model {
        let at = |s: i64| Utc.timestamp_opt(1_700_000_000 + s, 0).unwrap();
        collab_task_usage::Model {
            id: uuid::Uuid::new_v4().to_string(),
            repo_id: 1,
            task_id: "t".into(),
            unit: unit.into(),
            source: source.into(),
            amount: amount.into(),
            period_start: at(start),
            period_end: at(end),
            reported_by_kind: "user".into(),
            reported_by_id: "1".into(),
            client_usage_id: "c".into(),
            payload_hash: "h".into(),
            created_at: at(end),
        }
    }

    #[test]
    fn grouping_never_mixes_units_or_sources() {
        let rows = vec![
            usage("USD", "invoice", "1.5", 0, 10),
            usage("USD", "estimate", "9", 0, 10),
            usage("tokens", "invoice", "100", 0, 10),
            usage("USD", "invoice", "2.25", 10, 20),
        ];
        let groups = group_usage(&rows);
        assert_eq!(groups.len(), 3);
        let g = &groups[&("USD".to_string(), "invoice".to_string())];
        assert_eq!((format_micros(g.micros), g.count, g.overlap), ("3.75".to_string(), 2, false));
        assert_eq!(format_micros(groups[&("USD".to_string(), "estimate".to_string())].micros), "9");
    }

    #[test]
    fn overlapping_periods_are_flagged_not_hidden() {
        let rows = vec![usage("USD", "s", "1", 0, 10), usage("USD", "s", "1", 5, 15)];
        assert!(group_usage(&rows)[&("USD".to_string(), "s".to_string())].overlap);
        let touching = vec![usage("USD", "s", "1", 0, 10), usage("USD", "s", "1", 10, 15)];
        assert!(!group_usage(&touching)[&("USD".to_string(), "s".to_string())].overlap);
    }

    #[test]
    fn missing_usage_is_unknown_never_zero() {
        let now = Utc::now();
        let budget = collab_task_budget::Model {
            id: "b".into(),
            repo_id: 1,
            task_id: "t".into(),
            unit: "USD".into(),
            source: "invoice".into(),
            amount: "10".into(),
            alert_at: None,
            sampling_period: "task_total".into(),
            version: 1,
            set_by_kind: "user".into(),
            set_by_id: "1".into(),
            payload_hash: "h".into(),
            created_at: now,
            updated_at: now,
        };
        let e = entry(budget.clone(), None);
        assert_eq!(e.usage.status, "unknown");
        assert!(e.usage.total.is_none() && e.over_threshold.is_none() && e.threshold.is_none());
        assert!(e.usage.reason.unwrap().contains("USD"));

        let rows = vec![usage("USD", "invoice", "10", 0, 10)];
        let groups = group_usage(&rows);
        let e = entry(budget, groups.values().next());
        assert_eq!(e.over_threshold, Some(true), "reaching the amount counts");
        let t = e.threshold.unwrap();
        assert_eq!((t.basis, t.threshold_amount.as_str(), t.used_total.as_str()), ("amount", "10", "10"));
    }
}
