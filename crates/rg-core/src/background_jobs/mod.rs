//! Durable background job queue (QUEUE-001).
//!
//! A thin, database-backed task queue shared by webhook delivery retries,
//! email retries and other asynchronous work. Enqueueing only needs a
//! [`DatabaseConnection`], so any code path that already holds `db` can
//! schedule work without threading a queue handle through layers of APIs.
//!
//! Semantics:
//! - Jobs are rows in `background_jobs` (`pending`/`running`/`succeeded`/`dead`).
//! - Workers claim jobs with a conditional UPDATE (`status = 'pending' AND
//!   `run_at <= now`), which is atomic on SQLite/PostgreSQL/MySQL and safe
//!   with multiple worker processes.
//! - A failed attempt is rescheduled with exponential backoff; after
//!   `max_attempts` attempts the job is dead-lettered (`dead`) with the last
//!   error recorded.
//! - Jobs stuck in `running` (worker crash) are returned to `pending` once
//!   their claim is older than the staleness threshold. Handlers should
//!   therefore be quick or idempotent.

use chrono::{DateTime, Duration, Utc};
use futures::future::BoxFuture;
use rg_db::entities::background_job::{status, Model as Job};
use rg_db::ops::background_job_ops as ops;
use sea_orm::{ActiveValue::Set, DatabaseConnection};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration as StdDuration;

/// Retry a webhook delivery after a transport failure.
pub const TASK_WEBHOOK_DELIVER: &str = "webhook.deliver";
/// Send a notification email with retry.
pub const TASK_EMAIL_SEND: &str = "email.send";

pub const DEFAULT_MAX_ATTEMPTS: i32 = 5;
/// Backoff base for attempt `n`: 30s * 2^(n-1), capped at 1 hour.
const BACKOFF_BASE_SECS: i64 = 30;
const BACKOFF_CAP_SECS: i64 = 3600;

/// Outcome of reporting a failure for a job.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FailOutcome {
    /// The job was rescheduled for another attempt at the given time.
    Retry { run_at: DateTime<Utc> },
    /// The job exhausted its attempts and was dead-lettered.
    Dead,
}

/// Database-backed job queue handle. Cheap to clone by value from a
/// `DatabaseConnection` at any call site.
#[derive(Clone)]
pub struct BackgroundJobQueue {
    db: DatabaseConnection,
}

impl BackgroundJobQueue {
    pub fn new(db: DatabaseConnection) -> Self {
        Self { db }
    }

    /// Enqueue a job eligible to run immediately with the default attempt cap.
    pub async fn enqueue(&self, task_type: &str, payload: &Value) -> anyhow::Result<Job> {
        self.enqueue_with(task_type, payload, DEFAULT_MAX_ATTEMPTS).await
    }

    /// Enqueue a job with an explicit attempt cap.
    pub async fn enqueue_with(
        &self,
        task_type: &str,
        payload: &Value,
        max_attempts: i32,
    ) -> anyhow::Result<Job> {
        if max_attempts < 1 {
            anyhow::bail!("max_attempts must be at least 1");
        }
        let now = Utc::now();
        ops::insert(
            &self.db,
            rg_db::entities::background_job::ActiveModel {
                id: sea_orm::ActiveValue::NotSet,
                task_type: Set(task_type.to_string()),
                payload: Set(payload.to_string()),
                status: Set(status::PENDING.to_string()),
                attempts: Set(0),
                max_attempts: Set(max_attempts),
                run_at: Set(now),
                locked_by: Set(None),
                locked_at: Set(None),
                last_error: Set(None),
                created_at: Set(now),
                updated_at: Set(now),
            },
        )
        .await
    }

    /// Claim the next due pending job for `worker_id`. Returns `None` when
    /// no job is due.
    pub async fn claim_next(&self, worker_id: &str) -> anyhow::Result<Option<Job>> {
        let candidates = ops::list_pending_due(&self.db, 5).await?;
        for candidate in candidates {
            if let Some(job) = ops::claim(&self.db, candidate.id, worker_id).await? {
                return Ok(Some(job));
            }
        }
        Ok(None)
    }

    /// Mark a claimed job as completed.
    pub async fn complete(&self, job_id: i64) -> anyhow::Result<()> {
        let job = ops::find_by_id(&self.db, job_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("background job {job_id} not found"))?;
        let mut model: rg_db::entities::background_job::ActiveModel = job.into();
        model.status = Set(status::SUCCEEDED.to_string());
        model.locked_by = Set(None);
        model.locked_at = Set(None);
        model.updated_at = Set(Utc::now());
        ops::update(&self.db, model).await?;
        Ok(())
    }

    /// Report a failed attempt: reschedule with backoff or dead-letter.
    pub async fn fail(&self, job: &Job, error: &str) -> anyhow::Result<FailOutcome> {
        let attempts = job.attempts + 1;
        let outcome = if attempts >= job.max_attempts {
            FailOutcome::Dead
        } else {
            FailOutcome::Retry {
                run_at: Utc::now() + backoff(attempts),
            }
        };
        let mut model: rg_db::entities::background_job::ActiveModel = job.clone().into();
        model.attempts = Set(attempts);
        model.last_error = Set(Some(error.to_string()));
        model.locked_by = Set(None);
        model.locked_at = Set(None);
        model.updated_at = Set(Utc::now());
        match outcome {
            FailOutcome::Retry { run_at } => {
                model.status = Set(status::PENDING.to_string());
                model.run_at = Set(run_at);
            }
            FailOutcome::Dead => {
                model.status = Set(status::DEAD.to_string());
            }
        }
        ops::update(&self.db, model).await?;
        Ok(outcome)
    }

    /// Return jobs stuck in `running` longer than `older_than` to `pending`.
    /// Returns the number of recovered jobs.
    pub async fn recover_stale(&self, older_than: StdDuration) -> anyhow::Result<usize> {
        let cutoff = Utc::now()
            - Duration::milliseconds(older_than.as_millis() as i64);
        let stale = ops::list_running_locked_before(&self.db, cutoff, 100).await?;
        let mut recovered = 0;
        for job in stale {
            let mut model: rg_db::entities::background_job::ActiveModel = job.into();
            model.status = Set(status::PENDING.to_string());
            model.locked_by = Set(None);
            model.locked_at = Set(None);
            model.updated_at = Set(Utc::now());
            if ops::update(&self.db, model).await.is_ok() {
                recovered += 1;
            }
        }
        Ok(recovered)
    }
}

fn backoff(attempts: i32) -> Duration {
    let shift = (attempts - 1).clamp(0, 16) as u32;
    let secs = (BACKOFF_BASE_SECS.saturating_mul(1i64 << shift)).min(BACKOFF_CAP_SECS);
    Duration::seconds(secs)
}

/// Handler executed for one claimed job. Runs inside the worker task; must
/// be quick or idempotent (stale claims are reset to pending).
pub type JobHandler =
    Arc<dyn Fn(Value) -> BoxFuture<'static, anyhow::Result<()>> + Send + Sync>;

/// Recurring task executed on an interval (no persistence, no retry state —
/// failures are logged and the next tick runs regardless).
pub type PeriodicTask = Arc<dyn Fn() -> BoxFuture<'static, anyhow::Result<()>> + Send + Sync>;

/// Background worker: polls the queue and dispatches to registered handlers,
/// plus optional interval-driven periodic tasks.
pub struct BackgroundJobWorker {
    queue: BackgroundJobQueue,
    handlers: HashMap<String, JobHandler>,
    periodic: Vec<(String, StdDuration, PeriodicTask)>,
    worker_id: String,
    poll_interval: StdDuration,
    stale_after: StdDuration,
}

impl BackgroundJobWorker {
    pub fn new(queue: BackgroundJobQueue) -> Self {
        Self {
            queue,
            handlers: HashMap::new(),
            periodic: Vec::new(),
            worker_id: format!("worker-{}", uuid::Uuid::new_v4()),
            poll_interval: StdDuration::from_secs(5),
            stale_after: StdDuration::from_secs(600),
        }
    }

    /// Register the handler for a task type (builder style).
    pub fn register_handler(
        mut self,
        task_type: impl Into<String>,
        handler: JobHandler,
    ) -> Self {
        self.handlers.insert(task_type.into(), handler);
        self
    }

    /// Register a periodic task run every `interval` (builder style).
    pub fn register_periodic(
        mut self,
        name: &str,
        interval: StdDuration,
        task: PeriodicTask,
    ) -> Self {
        self.periodic
            .push((name.to_string(), interval, task));
        self
    }

    /// Run one claim-dispatch pass. Returns how many jobs were processed.
    /// Exposed for tests; production code uses [`Self::spawn`].
    pub async fn run_once(&self) -> anyhow::Result<usize> {
        let mut processed = 0;
        while let Some(job) = self.queue.claim_next(&self.worker_id).await? {
            processed += 1;
            let payload: Value = serde_json::from_str(&job.payload)
                .map_err(|e| anyhow::anyhow!("invalid job payload for {}: {e}", job.task_type))?;
            let result = match self.handlers.get(&job.task_type) {
                Some(handler) => (handler)(payload).await,
                None => Err(anyhow::anyhow!(
                    "no handler registered for task type '{}'",
                    job.task_type
                )),
            };
            match result {
                Ok(()) => self.queue.complete(job.id).await?,
                Err(error) => {
                    let message = format!("{error:#}");
                    tracing::warn!(
                        job_id = job.id,
                        task_type = %job.task_type,
                        attempts = job.attempts + 1,
                        error = %message,
                        "background job failed"
                    );
                    match self.queue.fail(&job, &message).await? {
                        FailOutcome::Retry { run_at } => {
                            tracing::info!(job_id = job.id, run_at = %run_at, "job rescheduled");
                        }
                        FailOutcome::Dead => {
                            tracing::error!(job_id = job.id, "background job dead-lettered");
                        }
                    }
                }
            }
        }
        Ok(processed)
    }

    /// Spawn the worker loops: one poll-dispatch loop for queued jobs and
    /// one interval loop per periodic task. Stale claim recovery runs on the
    /// same cadence as the poll loop.
    pub fn spawn(self) -> Vec<tokio::task::JoinHandle<()>> {
        let mut handles = Vec::new();

        for (name, interval, task) in &self.periodic {
            let task = task.clone();
            let name = name.clone();
            let interval = *interval;
            handles.push(tokio::spawn(async move {
                let mut ticker = tokio::time::interval(interval);
                ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                loop {
                    ticker.tick().await;
                    if let Err(error) = task().await {
                        tracing::warn!(task = %name, error = %format!("{error:#}"), "periodic background task failed");
                    }
                }
            }));
        }

        let queue = self.queue.clone();
        let worker = Arc::new(self);
        handles.push(tokio::spawn(async move {
            let mut ticker = tokio::time::interval(worker.poll_interval);
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                ticker.tick().await;
                if let Err(error) = worker.run_once().await {
                    tracing::warn!(error = %format!("{error:#}"), "background job poll failed");
                }
                if let Err(error) = queue.recover_stale(worker.stale_after).await {
                    tracing::warn!(error = %format!("{error:#}"), "background job stale recovery failed");
                }
            }
        }));

        handles
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::{ConnectOptions, Database};

    async fn test_db() -> DatabaseConnection {
        let dir = tempfile::tempdir().unwrap();
        let db_url = format!("sqlite://{}?mode=rwc", dir.path().join("jobs.db").display());
        let db = Database::connect(ConnectOptions::new(db_url)).await.unwrap();
        rg_db::run_migrations(&db).await.unwrap();
        db
    }

    #[tokio::test]
    async fn enqueue_claim_complete_round_trip() {
        let db = test_db().await;
        let queue = BackgroundJobQueue::new(db.clone());

        let job = queue
            .enqueue(TASK_EMAIL_SEND, &serde_json::json!({"to": "a@b.c"}))
            .await
            .unwrap();
        assert_eq!(job.status, status::PENDING);
        assert_eq!(job.attempts, 0);

        let claimed = queue.claim_next("w1").await.unwrap().unwrap();
        assert_eq!(claimed.id, job.id);
        assert_eq!(claimed.status, status::RUNNING);
        assert_eq!(claimed.locked_by.as_deref(), Some("w1"));

        // Second claim finds nothing: the job is running.
        assert!(queue.claim_next("w2").await.unwrap().is_none());

        queue.complete(job.id).await.unwrap();
        let done = ops::find_by_id(&db, job.id).await.unwrap().unwrap();
        assert_eq!(done.status, status::SUCCEEDED);
        assert!(done.locked_by.is_none());
        assert!(queue.claim_next("w3").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn fail_reschedules_with_backoff_then_dead_letters() {
        let db = test_db().await;
        let queue = BackgroundJobQueue::new(db.clone());

        let job = queue
            .enqueue_with(TASK_EMAIL_SEND, &serde_json::json!({}), 2)
            .await
            .unwrap();
        let claimed = queue.claim_next("w1").await.unwrap().unwrap();

        // First failure: retry with backoff.
        let outcome = queue.fail(&claimed, "boom").await.unwrap();
        let FailOutcome::Retry { run_at } = outcome else {
            panic!("expected retry, got {outcome:?}");
        };
        assert!(run_at > Utc::now());
        let after = ops::find_by_id(&db, job.id).await.unwrap().unwrap();
        assert_eq!(after.status, status::PENDING);
        assert_eq!(after.attempts, 1);
        assert_eq!(after.last_error.as_deref(), Some("boom"));

        // Not claimable until run_at passes.
        assert!(queue.claim_next("w1").await.unwrap().is_none());

        // Force the retry to become due, claim again, fail again: dead.
        let mut model: rg_db::entities::background_job::ActiveModel = after.into();
        model.run_at = Set(Utc::now() - Duration::seconds(1));
        ops::update(&db, model).await.unwrap();
        let claimed = queue.claim_next("w1").await.unwrap().unwrap();
        let outcome = queue.fail(&claimed, "boom 2").await.unwrap();
        assert_eq!(outcome, FailOutcome::Dead);
        let dead = ops::find_by_id(&db, job.id).await.unwrap().unwrap();
        assert_eq!(dead.status, status::DEAD);
        assert_eq!(dead.attempts, 2);
        assert_eq!(dead.last_error.as_deref(), Some("boom 2"));
    }

    #[tokio::test]
    async fn recover_stale_returns_running_jobs_to_pending() {
        let db = test_db().await;
        let queue = BackgroundJobQueue::new(db.clone());

        let job = queue
            .enqueue(TASK_EMAIL_SEND, &serde_json::json!({}))
            .await
            .unwrap();
        let claimed = queue.claim_next("w1").await.unwrap().unwrap();
        assert_eq!(claimed.status, status::RUNNING);

        // Nothing stale yet.
        assert_eq!(queue.recover_stale(StdDuration::from_secs(60)).await.unwrap(), 0);

        // Age the claim beyond the threshold.
        let mut model: rg_db::entities::background_job::ActiveModel = claimed.into();
        model.locked_at = Set(Some(Utc::now() - Duration::minutes(11)));
        ops::update(&db, model).await.unwrap();

        assert_eq!(queue.recover_stale(StdDuration::from_secs(600)).await.unwrap(), 1);
        let recovered = ops::find_by_id(&db, job.id).await.unwrap().unwrap();
        assert_eq!(recovered.status, status::PENDING);
        assert!(recovered.locked_by.is_none());

        // The recovered job is claimable again.
        let re_claimed = queue.claim_next("w2").await.unwrap().unwrap();
        assert_eq!(re_claimed.id, job.id);
    }

    #[tokio::test]
    async fn worker_dispatches_handlers_and_dead_letters_unknown_types() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let db = test_db().await;
        let queue = BackgroundJobQueue::new(db.clone());

        // Unknown task type: no handler → fails like any other job; with
        // max_attempts = 1 it dead-letters on the first pass.
        queue
            .enqueue_with("no.such.task", &serde_json::json!({}), 1)
            .await
            .unwrap();
        let worker = BackgroundJobWorker::new(queue.clone());
        assert_eq!(worker.run_once().await.unwrap(), 1);
        let jobs = ops::list_by_status(&db, status::DEAD, 10).await.unwrap();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].task_type, "no.such.task");
        assert!(jobs[0].last_error.as_deref().unwrap().contains("no handler"));

        // Registered handler: first attempt fails, retry is scheduled with
        // backoff, second attempt (forced due) succeeds.
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_for_handler = calls.clone();
        let handler: JobHandler = Arc::new(move |_payload| {
            let calls = calls_for_handler.clone();
            Box::pin(async move {
                if calls.fetch_add(1, Ordering::SeqCst) == 0 {
                    anyhow::bail!("first attempt fails")
                }
                Ok(())
            })
        });
        let job = queue
            .enqueue_with("test.flaky", &serde_json::json!({"n": 1}), 3)
            .await
            .unwrap();
        let worker = BackgroundJobWorker::new(queue.clone()).register_handler("test.flaky", handler);
        assert_eq!(worker.run_once().await.unwrap(), 1);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let pending = ops::find_by_id(&db, job.id).await.unwrap().unwrap();
        assert_eq!(pending.status, status::PENDING);

        let mut model: rg_db::entities::background_job::ActiveModel = pending.into();
        model.run_at = Set(Utc::now() - Duration::seconds(1));
        ops::update(&db, model).await.unwrap();

        assert_eq!(worker.run_once().await.unwrap(), 1);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        let done = ops::find_by_id(&db, job.id).await.unwrap().unwrap();
        assert_eq!(done.status, status::SUCCEEDED);
    }
}
