//! CI quota and usage accounting (R10, issue #40) — minimal loop.
//!
//! Two metrics, measured by the platform (not reported by the client like the
//! informational `collab_task_*` budget tables):
//!
//! * `ci_minutes` — minutes a subject's pipelines ran on a repository in one
//!   UTC day;
//! * `ci_concurrent_pipelines` — pipelines of that subject that are currently
//!   `pending` / `running` / `manual` / `waiting_approval` on that repository.
//!
//! Both are enforced in [`enforce`], called by `trigger_pipeline` *before* the
//! pipeline row is created, and only for `users.kind = "agent"`. People are
//! never limited (that would hit existing accounts on upgrade). A trigger with
//! `triggered_by = None` (the push hook) carries no subject at all and is
//! therefore neither limited nor counted — see `docs/plans/T05-plan.md`.
//!
//! # Why the counter is an absolute measurement
//!
//! A pipeline can reach a terminal state through several independent paths (the
//! internal runner, `cancel_pipeline_chain`, the runner API's
//! `try_update_pipeline`), and each may be retried. Instead of adding a delta and
//! needing an idempotency ledger, every write recomputes the period from
//! `pipelines` + `pipeline_jobs` and stores the *total* for
//! `(subject, repo, metric, period_start)`. Recomputing is therefore idempotent
//! by construction: the same pipeline counted twice yields the same value. The
//! unique index on that tuple is the arbiter between concurrent writers
//! (`CLAUDE.md` pitfall 19: no `ON CONFLICT` on MySQL — update first, insert on
//! miss, treat a unique violation as "someone else won, update again").
//!
//! # Minute accounting
//!
//! Minutes come from `pipeline_jobs.started_at` / `finished_at` of the
//! pipelines that finished in the period; `skipped` jobs (no timestamps) and
//! jobs without both timestamps contribute nothing. Seconds are summed across
//! the period and rounded up, so a burst of sub-minute pipelines is charged at
//! least one minute in total rather than zero.
//!
//! A pipeline is attributed wholly to the UTC day of its `finished_at`; a job
//! that straddles midnight is not split (the minimal loop does not need that
//! precision).

use anyhow::{Context, Result};
use chrono::{DateTime, Duration, NaiveDate, NaiveDateTime, Utc};
use rg_core::error::CoreError;
use rg_db::entities::{pipeline, pipeline_job, usage_counter};
use sea_orm::sea_query::Expr;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, PaginatorTrait, QueryFilter,
    Set, TransactionTrait,
};

/// Metric name for daily CI minutes.
pub const METRIC_CI_MINUTES: &str = "ci_minutes";
/// Metric name for the currently active pipeline count.
pub const METRIC_CI_CONCURRENT_PIPELINES: &str = "ci_concurrent_pipelines";

/// `usage_counters.subject_kind` for a `users.id` subject.
pub const SUBJECT_KIND_USER: &str = "user";

/// Stable code for a concurrency rejection (409). Kept in one place so tests,
/// docs and the message prefix cannot drift apart.
pub const CODE_CONCURRENCY: &str = "CI_QUOTA_CONCURRENCY";
/// Stable code for a daily-minutes rejection (409).
pub const CODE_MINUTES: &str = "CI_QUOTA_MINUTES";

/// How many pipelines one agent may have active on one repository at a time.
///
/// Four is the size of this deployment's shared runner pool (`docker-compose.yml`
/// starts a single self-hosted runner; four is the number of slots one agent can
/// occupy before it starves other work on the same repository). A single agent
/// holding more than four concurrent pipelines on one repository is almost
/// always a retry storm, not useful parallelism.
pub const MAX_CONCURRENT_PIPELINES_PER_REPO: u64 = 4;

/// How many CI minutes one agent may consume on one repository per UTC day.
///
/// 120 minutes = 2 hours. A typical agent task pushes a handful of commits and
/// finishes each pipeline in a few minutes, so this covers a normal working day
/// on one repository with a wide margin while still stopping a runaway loop
/// (a pipeline every 2 minutes around the clock would exhaust it by mid-morning).
pub const DAILY_MINUTES_PER_REPO: u64 = 120;

/// Statuses that occupy a concurrency slot. Mirrors
/// `pipeline_ops::count_active_pipelines`.
const ACTIVE_STATUSES: [&str; 4] = ["pending", "running", "manual", "waiting_approval"];

const TERMINAL_STATUSES: [&str; 4] = ["success", "failed", "canceled", "error"];

/// Snapshot of a subject's usage for one repository and UTC day.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UsageSnapshot {
    /// Pipelines currently occupying a concurrency slot.
    pub active_pipelines: u64,
    /// Minutes recorded for the day, rounded up.
    pub minutes: u64,
}

/// UTC day `timestamp` falls in.
pub fn utc_day(timestamp: NaiveDateTime) -> NaiveDate {
    timestamp.date()
}

/// Whole minutes charged for `seconds`, rounded up (`0` seconds stays `0`).
pub fn minutes_from_seconds(seconds: i64) -> u64 {
    if seconds <= 0 {
        0
    } else {
        (seconds as u64).div_ceil(60)
    }
}

/// Whether `status` is a terminal pipeline state (usage is charged once it is).
fn is_terminal(status: &str) -> bool {
    TERMINAL_STATUSES.contains(&status)
}

fn conflict(code: &str, message: String) -> anyhow::Error {
    anyhow::Error::new(CoreError::conflict(format!("{code}: {message}")))
}

/// Enforce the CI quota for a trigger, refreshing the stored counters first.
///
/// `triggered_by` is the only subject the CI engine receives. `None` (the push
/// hook) and human accounts are not limited. Returns a `CoreError::Conflict`
/// (HTTP 409) whose message is prefixed with [`CODE_CONCURRENCY`] or
/// [`CODE_MINUTES`].
pub async fn enforce(
    db: &DatabaseConnection,
    repo_id: i64,
    triggered_by: Option<i64>,
) -> Result<()> {
    let Some(subject_id) = triggered_by else {
        return Ok(());
    };
    if !is_agent_subject(db, subject_id).await? {
        return Ok(());
    }

    let period = utc_day(Utc::now().naive_utc());
    let snapshot = refresh(db, subject_id, repo_id, period).await?;

    if snapshot.active_pipelines >= MAX_CONCURRENT_PIPELINES_PER_REPO {
        return Err(conflict(
            CODE_CONCURRENCY,
            format!(
                "agent {subject_id} already has {} active pipeline(s) on repository {repo_id}; \
                 the limit is {MAX_CONCURRENT_PIPELINES_PER_REPO}",
                snapshot.active_pipelines
            ),
        ));
    }
    if snapshot.minutes >= DAILY_MINUTES_PER_REPO {
        return Err(conflict(
            CODE_MINUTES,
            format!(
                "agent {subject_id} has used {} CI minute(s) on repository {repo_id} on {period}; \
                 the daily limit is {DAILY_MINUTES_PER_REPO}",
                snapshot.minutes
            ),
        ));
    }
    Ok(())
}

/// Record the usage of a pipeline that just reached a terminal state.
///
/// Recomputes the day's minutes rather than adding a delta, so calling it more
/// than once (retries, several terminal-state paths) is harmless. Non-terminal
/// pipelines, pipelines with no subject, and human subjects are ignored. Errors
/// are returned for the caller to log; accounting must never fail the pipeline.
pub async fn record_finished_pipeline(db: &DatabaseConnection, pipeline_id: i64) -> Result<()> {
    let Some(pipeline) = rg_db::ops::pipeline_ops::get_pipeline(db, pipeline_id)
        .await
        .with_context(|| format!("db: load pipeline {pipeline_id} for usage accounting"))?
    else {
        return Ok(());
    };
    if !is_terminal(&pipeline.status) {
        return Ok(());
    }
    let Some(subject_id) = pipeline.triggered_by else {
        return Ok(());
    };
    if !is_agent_subject(db, subject_id).await? {
        return Ok(());
    }

    let finished_at = pipeline
        .finished_at
        .unwrap_or_else(|| Utc::now().naive_utc());
    let period = utc_day(finished_at);
    let minutes =
        minutes_from_seconds(seconds_for_day(db, subject_id, pipeline.repo_id, period).await?);
    upsert_counter(
        db,
        subject_id,
        pipeline.repo_id,
        METRIC_CI_MINUTES,
        period,
        minutes,
    )
    .await?;
    // The gauge is "now", not the pipeline's day: a pipeline finishing after
    // midnight frees a slot today.
    record_active_pipelines(db, pipeline.repo_id, Some(subject_id)).await?;
    Ok(())
}

/// Recompute and store the number of pipelines the subject currently has
/// active on `repo_id`. Cheap enough to call after every create and finish; a
/// no-op for `None` and human subjects.
pub async fn record_active_pipelines(
    db: &DatabaseConnection,
    repo_id: i64,
    triggered_by: Option<i64>,
) -> Result<()> {
    let Some(subject_id) = triggered_by else {
        return Ok(());
    };
    if !is_agent_subject(db, subject_id).await? {
        return Ok(());
    }
    let active = count_active_pipelines(db, repo_id, subject_id).await?;
    upsert_counter(
        db,
        subject_id,
        repo_id,
        METRIC_CI_CONCURRENT_PIPELINES,
        utc_day(Utc::now().naive_utc()),
        active,
    )
    .await
}

/// Recompute and store both counters for `(subject, repo, period)`.
async fn refresh(
    db: &DatabaseConnection,
    subject_id: i64,
    repo_id: i64,
    period: NaiveDate,
) -> Result<UsageSnapshot> {
    let active = count_active_pipelines(db, repo_id, subject_id).await?;
    upsert_counter(
        db,
        subject_id,
        repo_id,
        METRIC_CI_CONCURRENT_PIPELINES,
        period,
        active,
    )
    .await?;

    let seconds = seconds_for_day(db, subject_id, repo_id, period).await?;
    let minutes = minutes_from_seconds(seconds);
    upsert_counter(db, subject_id, repo_id, METRIC_CI_MINUTES, period, minutes).await?;

    Ok(UsageSnapshot {
        active_pipelines: active,
        minutes,
    })
}

async fn is_agent_subject(db: &DatabaseConnection, subject_id: i64) -> Result<bool> {
    let user = rg_db::ops::user_ops::find_by_id(db, subject_id)
        .await
        .with_context(|| format!("db: load user {subject_id} for CI quota"))?;
    Ok(user.is_some_and(|user| rg_core::agent::is_agent(&user)))
}

/// Pipelines of `subject_id` on `repo_id` that currently occupy a slot.
async fn count_active_pipelines(
    db: &DatabaseConnection,
    repo_id: i64,
    subject_id: i64,
) -> Result<u64> {
    let count = pipeline::Entity::find()
        .filter(pipeline::Column::RepoId.eq(repo_id))
        .filter(pipeline::Column::TriggeredBy.eq(subject_id))
        .filter(pipeline::Column::Status.is_in(ACTIVE_STATUSES))
        .count(db)
        .await
        .context("db: count active pipelines for CI quota")?;
    Ok(count)
}

/// Seconds the subject's pipelines ran in the UTC day of `period`.
///
/// Shared by the check and the recorder so both always agree.
async fn seconds_for_day(
    db: &DatabaseConnection,
    subject_id: i64,
    repo_id: i64,
    period: NaiveDate,
) -> Result<i64> {
    let day_start = day_start(period);
    let day_end = day_start + Duration::days(1);
    // `pipeline_ops` projects the legacy INTEGER id columns as bigint on
    // PostgreSQL, so go through it instead of `pipeline::Entity` directly. The
    // day and subject filters are applied here: they only need `NaiveDateTime`
    // comparison, which every backend stores identically enough for a day
    // bucket.
    let pipelines = rg_db::ops::pipeline_ops::list_pipelines_by_repo(db, repo_id)
        .await
        .context("db: list pipelines for CI usage")?;

    let mut seconds: i64 = 0;
    for pipeline in pipelines {
        if pipeline.triggered_by != Some(subject_id) || !is_terminal(&pipeline.status) {
            continue;
        }
        let Some(finished_at) = pipeline.finished_at else {
            continue;
        };
        if finished_at < day_start || finished_at >= day_end {
            continue;
        }
        seconds += pipeline_seconds(db, pipeline.id).await?;
    }
    Ok(seconds)
}

/// Seconds the jobs of one pipeline ran. Skipped jobs and jobs missing either
/// timestamp contribute nothing; a negative span (clock skew) counts as zero.
async fn pipeline_seconds(db: &DatabaseConnection, pipeline_id: i64) -> Result<i64> {
    let jobs = rg_db::ops::pipeline_ops::list_jobs_by_pipeline(db, pipeline_id)
        .await
        .with_context(|| format!("db: list jobs of pipeline {pipeline_id} for CI usage"))?;
    Ok(jobs.iter().filter_map(job_seconds).sum())
}

fn job_seconds(job: &pipeline_job::Model) -> Option<i64> {
    if job.status == "skipped" {
        return None;
    }
    let started = job.started_at?;
    let finished = job.finished_at?;
    Some((finished - started).num_seconds().max(0))
}

fn day_start(period: NaiveDate) -> NaiveDateTime {
    period
        .and_hms_opt(0, 0, 0)
        .expect("midnight is always a valid time")
}

/// Store `value` as the absolute measurement for the period.
///
/// Short transaction, write-first for SQLite (`CLAUDE.md` pitfall 21), and the
/// update-then-insert-then-update shape that also works on MySQL
/// (`CLAUDE.md` pitfall 19). No `ON CONFLICT`.
async fn upsert_counter(
    db: &DatabaseConnection,
    subject_id: i64,
    repo_id: i64,
    metric: &str,
    period: NaiveDate,
    value: u64,
) -> Result<()> {
    let value = i64::try_from(value).unwrap_or(i64::MAX);
    let now: DateTime<Utc> = Utc::now();
    let txn = db.begin().await.context("db: begin CI usage upsert")?;

    let rows = usage_counter::Entity::update_many()
        .col_expr(usage_counter::Column::Value, Expr::value(value))
        .col_expr(usage_counter::Column::UpdatedAt, Expr::value(now))
        .filter(usage_counter::Column::SubjectKind.eq(SUBJECT_KIND_USER))
        .filter(usage_counter::Column::SubjectId.eq(subject_id))
        .filter(usage_counter::Column::RepoId.eq(repo_id))
        .filter(usage_counter::Column::Metric.eq(metric))
        .filter(usage_counter::Column::PeriodStart.eq(period))
        .exec(&txn)
        .await
        .context("db: update CI usage counter")?;

    if rows.rows_affected == 0 {
        let inserted = usage_counter::ActiveModel {
            id: sea_orm::NotSet,
            subject_kind: Set(SUBJECT_KIND_USER.to_string()),
            subject_id: Set(subject_id),
            repo_id: Set(repo_id),
            metric: Set(metric.to_string()),
            period_start: Set(period),
            value: Set(value),
            updated_at: Set(now),
        }
        .insert(&txn)
        .await;
        match inserted {
            Ok(_) => {}
            // Another writer created the row between our UPDATE and INSERT: it
            // won, so apply our value as an update instead of failing.
            Err(error) if is_unique_violation(&error) => {
                usage_counter::Entity::update_many()
                    .col_expr(usage_counter::Column::Value, Expr::value(value))
                    .col_expr(usage_counter::Column::UpdatedAt, Expr::value(now))
                    .filter(usage_counter::Column::SubjectKind.eq(SUBJECT_KIND_USER))
                    .filter(usage_counter::Column::SubjectId.eq(subject_id))
                    .filter(usage_counter::Column::RepoId.eq(repo_id))
                    .filter(usage_counter::Column::Metric.eq(metric))
                    .filter(usage_counter::Column::PeriodStart.eq(period))
                    .exec(&txn)
                    .await
                    .context("db: update CI usage counter after race")?;
            }
            Err(error) => return Err(error).context("db: insert CI usage counter"),
        }
    }

    txn.commit().await.context("db: commit CI usage upsert")?;
    Ok(())
}

fn is_unique_violation(error: &sea_orm::DbErr) -> bool {
    matches!(
        error.sql_err(),
        Some(sea_orm::SqlErr::UniqueConstraintViolation(_))
    )
}

/// Read one stored counter (tests and future read endpoints).
pub async fn counter_value(
    db: &DatabaseConnection,
    subject_id: i64,
    repo_id: i64,
    metric: &str,
    period: NaiveDate,
) -> Result<Option<u64>> {
    let row = usage_counter::Entity::find()
        .filter(usage_counter::Column::SubjectKind.eq(SUBJECT_KIND_USER))
        .filter(usage_counter::Column::SubjectId.eq(subject_id))
        .filter(usage_counter::Column::RepoId.eq(repo_id))
        .filter(usage_counter::Column::Metric.eq(metric))
        .filter(usage_counter::Column::PeriodStart.eq(period))
        .one(db)
        .await
        .context("db: read CI usage counter")?;
    Ok(row.map(|row| u64::try_from(row.value).unwrap_or(0)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::{NotSet, Set};

    async fn test_db() -> (tempfile::TempDir, DatabaseConnection) {
        let dir = tempfile::tempdir().expect("tempdir");
        let url = format!(
            "sqlite://{}?mode=rwc",
            dir.path().join("quota.db").display()
        );
        let db = rg_db::connect(&url).await.expect("connect sqlite");
        rg_db::run_migrations(&db).await.expect("migrate");
        (dir, db)
    }

    async fn user(db: &DatabaseConnection, name: &str) -> i64 {
        rg_db::ops::user_ops::create_user(db, name, &format!("{name}@example.com"), "unused", name)
            .await
            .expect("create user")
            .id
    }

    async fn agent(db: &DatabaseConnection, name: &str, owner_id: i64) -> i64 {
        rg_db::ops::user_ops::create_agent(db, name, &format!("{name}@example.com"), owner_id, None)
            .await
            .expect("create agent")
            .id
    }

    async fn repo(db: &DatabaseConnection, owner_id: i64, name: &str) -> i64 {
        let now = chrono::Utc::now();
        rg_db::ops::repo_ops::create(
            db,
            rg_db::entities::repository::ActiveModel {
                id: NotSet,
                owner_id: Set(owner_id),
                name: Set(name.into()),
                description: Set(None),
                is_private: Set(true),
                default_branch: Set("main".into()),
                fork_id: Set(None),
                stars_count: Set(0),
                forks_count: Set(0),
                org_id: Set(None),
                created_at: Set(now),
                updated_at: Set(now),
                deleted_at: Set(None),
                origin_repo_id: Set(None),
            },
        )
        .await
        .expect("create repo")
        .id
    }

    /// A finished pipeline with one job that ran for `minutes`.
    async fn finished_pipeline(
        db: &DatabaseConnection,
        repo_id: i64,
        subject_id: i64,
        minutes: i64,
    ) -> i64 {
        let pipeline = rg_db::ops::pipeline_ops::create_pipeline(
            db,
            repo_id,
            "deadbeef",
            "refs/heads/main",
            "push",
            Some(subject_id),
        )
        .await
        .expect("create pipeline");
        let stage = rg_db::ops::pipeline_ops::create_stage(db, pipeline.id, "build", 0)
            .await
            .expect("create stage");
        let job = rg_db::ops::pipeline_ops::create_job(
            db, stage.id, "build", "true", None, None, None, None, None, false, None, None, None,
        )
        .await
        .expect("create job");
        let finished = chrono::Utc::now().naive_utc();
        let started = finished - chrono::Duration::minutes(minutes);
        rg_db::ops::pipeline_ops::update_job_result(
            db,
            job.id,
            "success",
            Some(0),
            None,
            Some(started),
            Some(finished),
        )
        .await
        .expect("finish job");
        rg_db::ops::pipeline_ops::update_pipeline_status(
            db,
            pipeline.id,
            "success",
            Some(started),
            Some(finished),
        )
        .await
        .expect("finish pipeline");
        pipeline.id
    }

    #[test]
    fn minutes_round_up_and_zero_stays_zero() {
        assert_eq!(minutes_from_seconds(0), 0);
        assert_eq!(minutes_from_seconds(-5), 0);
        assert_eq!(minutes_from_seconds(1), 1);
        assert_eq!(minutes_from_seconds(60), 1);
        assert_eq!(minutes_from_seconds(61), 2);
        assert_eq!(minutes_from_seconds(3_600), 60);
    }

    #[tokio::test]
    async fn people_and_unattributed_triggers_are_never_limited() {
        let (_dir, db) = test_db().await;
        let owner = user(&db, "owner-human").await;
        let repo_id = repo(&db, owner, "quota-human").await;

        // Ten active pipelines: far past the agent limit.
        for _ in 0..10 {
            rg_db::ops::pipeline_ops::create_pipeline(
                &db,
                repo_id,
                "deadbeef",
                "refs/heads/main",
                "push",
                Some(owner),
            )
            .await
            .unwrap();
        }
        enforce(&db, repo_id, Some(owner))
            .await
            .expect("a human is not limited");
        enforce(&db, repo_id, None)
            .await
            .expect("an unattributed trigger is not limited");

        // Nothing was written for a human subject.
        assert!(counter_value(
            &db,
            owner,
            repo_id,
            METRIC_CI_MINUTES,
            utc_day(chrono::Utc::now().naive_utc()),
        )
        .await
        .unwrap()
        .is_none());
    }

    #[tokio::test]
    async fn agent_concurrency_limit_returns_the_stable_code() {
        let (_dir, db) = test_db().await;
        let owner = user(&db, "owner-agent-c").await;
        let subject = agent(&db, "owner-human-agent-c", owner).await;
        let repo_id = repo(&db, owner, "quota-concurrency").await;

        for _ in 0..MAX_CONCURRENT_PIPELINES_PER_REPO {
            rg_db::ops::pipeline_ops::create_pipeline(
                &db,
                repo_id,
                "deadbeef",
                "refs/heads/main",
                "push",
                Some(subject),
            )
            .await
            .unwrap();
        }
        let error = enforce(&db, repo_id, Some(subject)).await.unwrap_err();
        assert!(matches!(
            error.downcast_ref::<CoreError>(),
            Some(CoreError::Conflict(_))
        ));
        assert!(
            error.to_string().contains(CODE_CONCURRENCY),
            "message must carry the stable code: {error}"
        );

        // The gauge recorded at check time is the live count.
        assert_eq!(
            counter_value(
                &db,
                subject,
                repo_id,
                METRIC_CI_CONCURRENT_PIPELINES,
                utc_day(chrono::Utc::now().naive_utc()),
            )
            .await
            .unwrap(),
            Some(MAX_CONCURRENT_PIPELINES_PER_REPO)
        );
    }

    #[tokio::test]
    async fn daily_minutes_are_recorded_once_and_block_at_the_limit() {
        let (_dir, db) = test_db().await;
        let owner = user(&db, "owner-agent-m").await;
        let subject = agent(&db, "owner-human-agent-m", owner).await;
        let repo_id = repo(&db, owner, "quota-minutes").await;
        let today = utc_day(chrono::Utc::now().naive_utc());

        // 3 × 40 min = 120 min, exactly the limit.
        for _ in 0..3 {
            let pipeline_id = finished_pipeline(&db, repo_id, subject, 40).await;
            record_finished_pipeline(&db, pipeline_id).await.unwrap();
        }
        assert_eq!(
            counter_value(&db, subject, repo_id, METRIC_CI_MINUTES, today)
                .await
                .unwrap(),
            Some(DAILY_MINUTES_PER_REPO)
        );

        let error = enforce(&db, repo_id, Some(subject)).await.unwrap_err();
        assert!(error.to_string().contains(CODE_MINUTES), "{error}");

        // Recording the same pipeline again does not double the total.
        for pipeline in rg_db::ops::pipeline_ops::list_pipelines_by_repo(&db, repo_id)
            .await
            .unwrap()
        {
            record_finished_pipeline(&db, pipeline.id).await.unwrap();
        }
        assert_eq!(
            counter_value(&db, subject, repo_id, METRIC_CI_MINUTES, today)
                .await
                .unwrap(),
            Some(DAILY_MINUTES_PER_REPO),
            "recomputation must be idempotent"
        );
        // One row per (subject, repo, metric, period).
        assert_eq!(
            usage_counter::Entity::find()
                .filter(usage_counter::Column::SubjectId.eq(subject))
                .count(&db)
                .await
                .unwrap(),
            2,
            "one ci_minutes row and one ci_concurrent_pipelines row"
        );
    }

    #[tokio::test]
    async fn non_terminal_and_other_subject_pipelines_are_not_charged() {
        let (_dir, db) = test_db().await;
        let owner = user(&db, "owner-agent-n").await;
        let subject = agent(&db, "owner-human-agent-n", owner).await;
        let other = agent(&db, "owner-human-agent-n2", owner).await;
        let repo_id = repo(&db, owner, "quota-scope").await;
        let today = utc_day(chrono::Utc::now().naive_utc());

        // Abandon the pipeline while it is still `pending` (no timestamps).
        rg_db::ops::pipeline_ops::create_pipeline(
            &db,
            repo_id,
            "deadbeef",
            "refs/heads/main",
            "push",
            Some(subject),
        )
        .await
        .unwrap();
        // Someone else's finished pipeline must not be charged to `subject`.
        let other_pipeline = finished_pipeline(&db, repo_id, other, 40).await;

        record_finished_pipeline(&db, other_pipeline).await.unwrap();
        assert_eq!(
            counter_value(&db, other, repo_id, METRIC_CI_MINUTES, today)
                .await
                .unwrap(),
            Some(40)
        );
        assert!(
            counter_value(&db, subject, repo_id, METRIC_CI_MINUTES, today)
                .await
                .unwrap()
                .is_none()
        );
        // Refreshing for `subject` charges the pending pipeline nothing.
        enforce(&db, repo_id, Some(subject)).await.unwrap();
        assert_eq!(
            counter_value(&db, subject, repo_id, METRIC_CI_MINUTES, today)
                .await
                .unwrap(),
            Some(0)
        );
    }
}
