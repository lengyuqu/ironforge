//! Outbox sweep — the fallback that closes "event logged, delivery never
//! happened" (#15, step 7).
//!
//! Two ways an event can be written and then never delivered:
//!
//! ```text
//! ① txn path:  INSERT repo_events (in txn) → COMMIT → deliver_event
//!                                                    ↑ crash here = no delivery
//! ② legacy:    INSERT repo_events → deliver_envelope
//!                                    ↑ crash here = no delivery
//! ```
//!
//! Before this module the log had no delivery state, so nothing could tell the
//! two apart from an event that was delivered and only logged. `repo_events.
//! dispatched_at` answers that: `NULL` means nobody ever started a delivery for
//! the row. [`sweep`] finds those rows and calls the ordinary delivery path once
//! more.
//!
//! # What this is not
//!
//! Not an outbox implementation. There is no `pending`/`processing`/`sent`
//! state machine, no idempotency key, no ordering guarantee, and no change to
//! the durable retry queue (`background_jobs`, QUEUE-001) that already handles
//! *failed* deliveries. This only covers deliveries that were **never started**.
//! Delivery therefore stays **at least once**: a crash between the hand-off mark
//! and the actual POST, or a row the sweep re-sends after a failed mark, can
//! reach a receiver twice.
//!
//! # The window, and why
//!
//! ```text
//!   now-15min            now-2min        now
//!      ├────────────────────┤
//!      │  swept             │  left alone
//! ```
//!
//! * **Lower bound `now - 2min`** — an event younger than that is very likely
//!   being delivered right now. Sweeping it would race the normal path and
//!   produce a duplicate for the common case, which is much worse than waiting
//!   two minutes for the rare crash.
//! * **Upper bound `now - 15min`** — bounds the scan (the retention window is
//!   days long) and keeps an old, repeatedly failing row from being re-sent
//!   forever. An event that is still undelivered after fifteen minutes is a
//!   signal for a person, not for a retry loop.
//!
//! Historical rows are excluded by the migration, not by this window: everything
//! that existed before the column was added was back-filled with its
//! `created_at`. A `NULL` row older than the window is therefore either a
//! genuinely old miss or a row the window never reached — both are reported by
//! [`SweepReport::scanned`] staying at zero rather than being replayed.

use anyhow::Result;
use chrono::{Duration, Utc};
use sea_orm::DatabaseConnection;
use serde_json::Value;

use super::service::{deliver_envelope, looks_like_envelope};

/// How long an event is left alone after it was written: it is probably in
/// flight.
pub const SETTLE_SECS: i64 = 2 * 60;

/// How far back the sweep looks. Older misses are for a person to inspect.
pub const LOOKBACK_SECS: i64 = 15 * 60;

/// Default sweep period. `0` (via [`SWEEP_INTERVAL_ENV`]) disables the worker.
pub const DEFAULT_INTERVAL_SECS: u64 = 60;

/// Overrides [`DEFAULT_INTERVAL_SECS`] in seconds; `0` disables the sweep.
/// Follows the `IRONFORGE_GIT_STREAM_TIMEOUT_SECS` style — this project has no
/// general configuration system and this task does not add one.
pub const SWEEP_INTERVAL_ENV: &str = "IRONFORGE_WEBHOOK_OUTBOX_SWEEP_SECS";

/// One pass over the missed-delivery window.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SweepReport {
    /// Events inside the window that were still undispatched.
    pub scanned: u64,
    /// Events handed to delivery again (a `spawn` per active hook).
    pub redelivered: u64,
    /// Events that could not be redelivered; left undispatched for the next
    /// pass.
    pub failed: u64,
}

/// Redeliver events that were logged but never handed to delivery.
///
/// Errors are per event, not per pass: one bad row never stops the others, and a
/// row whose delivery fails keeps `dispatched_at = NULL` so the next pass tries
/// again. A payload that cannot be replayed (unparseable, or an oversized event
/// stored as its truncation stub) is **marked** anyway — otherwise it would sit
/// in the window and be re-read every minute forever — after a `warn!` so it is
/// visible.
pub async fn sweep(db: &DatabaseConnection) -> Result<SweepReport> {
    let now = Utc::now();
    let rows = rg_db::ops::repo_event_ops::list_undispatched_between(
        db,
        now - Duration::seconds(LOOKBACK_SECS),
        now - Duration::seconds(SETTLE_SECS),
    )
    .await?;

    let mut report = SweepReport {
        scanned: rows.len() as u64,
        ..SweepReport::default()
    };

    for row in rows {
        let Some(payload) = row.payload.as_deref() else {
            tracing::warn!(
                event_id = row.id,
                repo_id = row.repo_id,
                event = %row.event_type,
                "undispatched event has no payload; nothing to replay"
            );
            settle(db, row.id, &mut report).await;
            continue;
        };

        match serde_json::from_str::<Value>(payload) {
            Ok(envelope) if looks_like_envelope(&envelope) => {
                match deliver_envelope(db, row.repo_id, &row.event_type, envelope, Some(row.id))
                    .await
                {
                    Ok(()) => {
                        report.redelivered += 1;
                        tracing::warn!(
                            event_id = row.id,
                            repo_id = row.repo_id,
                            event = %row.event_type,
                            created_at = %row.created_at,
                            "redelivering a repository event that was never dispatched"
                        );
                    }
                    Err(error) => {
                        report.failed += 1;
                        tracing::warn!(
                            event_id = row.id,
                            repo_id = row.repo_id,
                            event = %row.event_type,
                            %error,
                            "could not redeliver an undispatched event; retrying next pass"
                        );
                    }
                }
            }
            Ok(_) => {
                // Valid JSON that is not an envelope: a pre-envelope payload or
                // the truncation stub `append_envelope` writes for an oversized
                // event. Sending either would be a surprise to the receiver, so
                // it is dropped rather than replayed — but settled, or it would
                // be revisited every minute.
                tracing::warn!(
                    event_id = row.id,
                    repo_id = row.repo_id,
                    event = %row.event_type,
                    "undispatched event payload is not an envelope; skipping it"
                );
                settle(db, row.id, &mut report).await;
            }
            Err(error) => {
                tracing::warn!(
                    event_id = row.id,
                    repo_id = row.repo_id,
                    event = %row.event_type,
                    %error,
                    "undispatched event payload cannot be parsed; skipping it"
                );
                settle(db, row.id, &mut report).await;
            }
        }
    }

    Ok(report)
}

/// Mark an event the sweep has nothing sane to send, counting a failure to do so
/// as `failed` (the next pass will read it again, which is the honest outcome).
async fn settle(db: &DatabaseConnection, event_id: i64, report: &mut SweepReport) {
    match rg_db::ops::repo_event_ops::mark_dispatched(db, event_id).await {
        Ok(_) => {}
        Err(error) => {
            report.failed += 1;
            tracing::warn!(event_id, %error, "could not mark an unreplayable event as handled");
        }
    }
}

/// Run [`sweep`] forever, every [`SWEEP_INTERVAL_ENV`] seconds (default
/// [`DEFAULT_INTERVAL_SECS`]).
///
/// A failed pass is logged and the loop continues: the next one is a minute
/// away, and giving up would leave the module's whole purpose unserved. The
/// first tick fires immediately, so a restart catches up on anything the
/// previous process dropped in its last fifteen minutes.
pub async fn run_sweep_loop(db: DatabaseConnection) {
    let interval = sweep_interval();
    if interval.is_zero() {
        tracing::info!("webhook outbox sweep is disabled");
        // The task is supervised; returning would be restarted with an error log
        // every second. Park instead.
        std::future::pending::<()>().await;
        return;
    }

    let mut ticker = tokio::time::interval(interval);
    loop {
        ticker.tick().await;
        match sweep(&db).await {
            Ok(report) if report.redelivered > 0 || report.failed > 0 => {
                tracing::info!(
                    scanned = report.scanned,
                    redelivered = report.redelivered,
                    failed = report.failed,
                    "webhook outbox sweep completed"
                );
            }
            Ok(_) => {}
            Err(error) => tracing::warn!(%error, "webhook outbox sweep failed"),
        }
    }
}

/// The configured period; zero means "do not run".
fn sweep_interval() -> std::time::Duration {
    let secs = std::env::var(SWEEP_INTERVAL_ENV)
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .unwrap_or(DEFAULT_INTERVAL_SECS);
    std::time::Duration::from_secs(secs)
}
