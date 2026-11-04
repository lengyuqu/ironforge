//! X03 acceptance: collaboration health metrics (M06–M09).
//!
//! Real HTTP server and SQLite database (with `IRONFORGE_TEST_DATABASE_URL` the
//! same tests run on PostgreSQL or MySQL). Business changes are made through
//! the public API so the platform events are recorded by the real hooks; timed
//! scenarios inject events with chosen timestamps, because the server stamps
//! its own clock and a test cannot move it.

mod common;
mod collab_common;

use chrono::{DateTime, Duration, SecondsFormat, Utc};
use collab_common::{Env, Sess};
use reqwest::Method;
use rg_db::entities::collab_metric_event as ev;
use rg_db::sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder, Set};
use serde_json::{json, Value};

fn at(t: DateTime<Utc>) -> String {
    t.to_rfc3339_opts(SecondsFormat::Millis, true)
}

async fn new_task(env: &Env, s: &Sess, title: &str) -> String {
    let (status, task) = env
        .json(Method::POST, &env.url("/tasks"), &env.owner_token, Some(s), Some(json!({"title": title})))
        .await;
    assert_eq!(status, 201, "{task}");
    task["id"].as_str().unwrap().to_string()
}

async fn set_status(env: &Env, s: &Sess, task_id: &str, version: i64, status: &str) -> (u16, Value) {
    env.json(
        Method::PATCH,
        &env.url(&format!("/tasks/{task_id}")),
        &env.owner_token,
        Some(s),
        Some(json!({"expected_version": version, "status": status})),
    )
    .await
}

async fn health(env: &Env, token: &str, query: &str) -> (u16, Value) {
    env.json(Method::GET, &env.url(&format!("/health{query}")), token, None, None).await
}

async fn health_ok(env: &Env, query: &str) -> Value {
    let (status, body) = health(env, &env.owner_token, query).await;
    assert_eq!(status, 200, "{body}");
    body
}

/// Query for a window that starts at the first recorded event: the default
/// 7-day window reaches back before a fresh test database has any history,
/// which is (correctly) reported as partial.
async fn covered(env: &Env) -> String {
    let first = rg_db::ops::collab_metric_ops::first_occurred_at(&env.db, env.repo_id)
        .await.unwrap().unwrap();
    format!("?from={}", first.to_rfc3339_opts(SecondsFormat::Nanos, true))
}

fn metric<'a>(body: &'a Value, id: &str) -> &'a Value {
    body["metrics"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["id"] == id)
        .unwrap_or_else(|| panic!("metric {id} missing in {body}"))
}

async fn events_of(env: &Env, task_id: &str) -> Vec<ev::Model> {
    ev::Entity::find()
        .filter(ev::Column::TaskId.eq(task_id))
        .order_by_asc(ev::Column::OccurredAt)
        .order_by_asc(ev::Column::TaskVersion)
        .order_by_asc(ev::Column::Id)
        .all(&env.db)
        .await
        .unwrap()
}

async fn count_events(env: &Env, task_id: &str, event_type: &str) -> u64 {
    ev::Entity::find()
        .filter(ev::Column::TaskId.eq(task_id))
        .filter(ev::Column::EventType.eq(event_type))
        .count(&env.db)
        .await
        .unwrap()
}

#[tokio::test]
async fn health_exclusive_boundary_keeps_fractional_endpoints() {
    let env = Env::new().await;
    let session = env.session(&env.owner_token, "boundary").await;
    let task_id = new_task(&env, &session, "boundary").await;
    let event = events_of(&env, &task_id).await.remove(0);
    let ops = &env.db;
    let at_boundary = rg_db::ops::collab_metric_ops::load_before(ops, env.repo_id, event.occurred_at)
        .await.unwrap().unwrap();
    assert!(at_boundary.iter().all(|e| e.id != event.id));
    let after_boundary = rg_db::ops::collab_metric_ops::load_before(
        ops, env.repo_id, event.occurred_at + Duration::milliseconds(1),
    ).await.unwrap().unwrap();
    assert!(after_boundary.iter().any(|e| e.id == event.id));
    let created = rg_db::entities::collab_task::Entity::find_by_id(task_id.clone())
        .one(ops).await.unwrap().unwrap().created_at;
    assert!(!rg_db::ops::collab_metric_ops::task_ids_created_before(ops, env.repo_id, created)
        .await.unwrap().iter().any(|(id, _)| id == &task_id));
    assert!(rg_db::ops::collab_metric_ops::task_ids_created_before(ops, env.repo_id, created + Duration::milliseconds(1))
        .await.unwrap().iter().any(|(id, _)| id == &task_id));
}

/// Simulate data from before the feature: forget everything recorded so far.
async fn forget_events(env: &Env) {
    ev::Entity::delete_many()
        .filter(ev::Column::RepoId.eq(env.repo_id))
        .exec(&env.db)
        .await
        .unwrap();
}

#[allow(clippy::too_many_arguments)]
async fn inject(
    env: &Env,
    task_id: &str,
    kind: &str,
    from: Option<&str>,
    to: Option<&str>,
    version: Option<i64>,
    source: (&str, &str),
    when: DateTime<Utc>,
) {
    ev::ActiveModel {
        id: Set(uuid::Uuid::new_v4().to_string()),
        repo_id: Set(env.repo_id),
        task_id: Set(task_id.to_string()),
        event_type: Set(kind.to_string()),
        from_state: Set(from.map(str::to_string)),
        to_state: Set(to.map(str::to_string)),
        task_version: Set(version),
        source_type: Set(source.0.to_string()),
        source_id: Set(source.1.to_string()),
        occurred_at: Set(when),
        recorded_at: Set(Utc::now()),
    }
    .insert(&env.db)
    .await
    .unwrap();
}

async fn inject_created(env: &Env, task_id: &str, when: DateTime<Utc>) {
    inject(env, task_id, "task_created", None, Some("open"), Some(1), ("task", task_id), when).await;
}

async fn inject_change(env: &Env, task_id: &str, from: &str, to: &str, version: i64, when: DateTime<Utc>) {
    inject(
        env,
        task_id,
        "task_status_changed",
        Some(from),
        Some(to),
        Some(version),
        ("task", &format!("{task_id}:{version}")),
        when,
    )
    .await;
}

fn minutes(base: DateTime<Utc>, m: i64) -> DateTime<Utc> {
    base + Duration::minutes(m)
}

fn base_time() -> DateTime<Utc> {
    // Whole seconds, far enough in the past that every scenario window is not in the future.
    DateTime::from_timestamp((Utc::now() - Duration::hours(10)).timestamp(), 0).unwrap()
}

// ------------------------------------------------------------------- M06

#[tokio::test]
async fn m06_status_changes_are_recorded_by_the_platform_in_the_business_transaction() {
    let env = Env::new().await;
    let a = env.session(&env.owner_token, "A").await;
    let started = Utc::now();
    let tid = new_task(&env, &a, "t").await;

    // open -> blocked -> in_progress -> blocked; a title edit and a no-op status
    // write change nothing worth an event.
    assert_eq!(set_status(&env, &a, &tid, 1, "blocked").await.0, 200);
    assert_eq!(set_status(&env, &a, &tid, 2, "in_progress").await.0, 200);
    assert_eq!(set_status(&env, &a, &tid, 3, "blocked").await.0, 200);
    let (status, _) = env
        .json(Method::PATCH, &env.url(&format!("/tasks/{tid}")), &env.owner_token, Some(&a), Some(json!({"expected_version": 4, "title": "renamed"})))
        .await;
    assert_eq!(status, 200);
    assert_eq!(set_status(&env, &a, &tid, 5, "blocked").await.0, 200, "same status again");
    // A stale write is refused and leaves no event.
    assert_eq!(set_status(&env, &a, &tid, 1, "done").await.0, 409);

    let events = events_of(&env, &tid).await;
    let kinds: Vec<(&str, Option<&str>, Option<&str>)> = events
        .iter()
        .map(|e| (e.event_type.as_str(), e.from_state.as_deref(), e.to_state.as_deref()))
        .collect();
    assert_eq!(
        kinds,
        vec![
            ("task_created", None, Some("open")),
            ("task_status_changed", Some("open"), Some("blocked")),
            ("task_status_changed", Some("blocked"), Some("in_progress")),
            ("task_status_changed", Some("in_progress"), Some("blocked")),
        ]
    );
    let finished = Utc::now() + Duration::seconds(1);
    for e in &events {
        assert!(e.occurred_at >= started - Duration::seconds(1) && e.occurred_at <= finished, "server time");
        assert_eq!(e.repo_id, env.repo_id);
    }
    assert_eq!(events[1].task_version, Some(2));

    let body = health_ok(&env, &covered(&env).await).await;
    assert_eq!(body["metric_version"], 1);
    assert_eq!(body["range"]["timezone"], "UTC");
    assert!(body["coverage_start"].is_string());
    assert_eq!(body["partial"], false, "{body}");
    let tasks = metric(&body, "blocked_tasks");
    assert_eq!(tasks["source"], "user_content", "status is a self-report");
    assert_eq!(tasks["timing_source"], "platform_event");
    assert_eq!(tasks["status"], "complete");
    assert_eq!(tasks["value"]["blocked_at_to"], 1);
    assert_eq!(tasks["value"]["entered_blocked_in_window"], 1, "one task, two entries");
    assert_eq!(tasks["value"]["entered_blocked_events"], 2);
    let duration = metric(&body, "blocked_duration");
    assert_eq!(duration["value"]["right_censored_tasks"], 1);
    // A complete blocked interval can have zero measurable duration when both
    // transitions share MySQL's stored second. Only positive overlaps count.
    let closed_interval = u64::from((events[2].occurred_at - events[1].occurred_at).num_milliseconds() > 0);
    assert_eq!(duration["value"]["intervals"], 1 + closed_interval);
}

#[tokio::test]
async fn m06_interval_maths_covers_clipping_censoring_and_reopening() {
    let env = Env::new().await;
    let s = env.session(&env.owner_token, "S").await;
    let base = base_time();
    let (a, b, c, d) = (
        new_task(&env, &s, "A").await,
        new_task(&env, &s, "B").await,
        new_task(&env, &s, "C").await,
        new_task(&env, &s, "D").await,
    );
    forget_events(&env).await;

    // A: blocked 10..40, finished. B: blocked from 100, never unblocked.
    // C: existed before the feature; first recorded fact is leaving `blocked` at 30.
    // D: reopened: blocked 5..15 and 25..35.
    inject_created(&env, &a, minutes(base, 0)).await;
    inject_change(&env, &a, "open", "blocked", 2, minutes(base, 10)).await;
    inject_change(&env, &a, "blocked", "done", 3, minutes(base, 40)).await;
    inject_created(&env, &b, minutes(base, 0)).await;
    inject_change(&env, &b, "open", "blocked", 2, minutes(base, 100)).await;
    inject_change(&env, &c, "blocked", "open", 7, minutes(base, 30)).await;
    inject_created(&env, &d, minutes(base, 0)).await;
    inject_change(&env, &d, "open", "blocked", 2, minutes(base, 5)).await;
    inject_change(&env, &d, "blocked", "in_progress", 3, minutes(base, 15)).await;
    inject_change(&env, &d, "in_progress", "blocked", 4, minutes(base, 25)).await;
    inject_change(&env, &d, "blocked", "done", 5, minutes(base, 35)).await;

    // Whole window [0, 120): A 30m, B 20m, C 30m (observed since coverage start,
    // left-censored), D 10m + 10m = 90m.
    let q = format!("?from={}&to={}", at(base), at(minutes(base, 120)));
    let body = health_ok(&env, &q).await;
    assert_eq!(body["coverage_start"], at(base));
    assert_eq!(body["partial"], false);
    let duration = metric(&body, "blocked_duration");
    assert_eq!(duration["value"]["blocked_seconds"], 100 * 60);
    assert_eq!(duration["value"]["intervals"], 5);
    assert_eq!(duration["value"]["tasks"], 4);
    assert_eq!(duration["value"]["left_censored_tasks"], 1);
    assert_eq!(duration["value"]["right_censored_tasks"], 1);
    let tasks = metric(&body, "blocked_tasks");
    assert_eq!(tasks["value"]["blocked_at_to"], 1, "only B is blocked at `to`");
    assert_eq!(tasks["value"]["entered_blocked_in_window"], 3, "A, B and D (C was already blocked)");
    assert_eq!(tasks["value"]["entered_blocked_events"], 4, "D entered twice");

    // Window [20, 60): A 20m (clipped at the start), C 10m (known since the
    // coverage start, which precedes the window, so not censored), D 10m (25..35).
    let q = format!("?from={}&to={}", at(minutes(base, 20)), at(minutes(base, 60)));
    let body = health_ok(&env, &q).await;
    let duration = metric(&body, "blocked_duration");
    assert_eq!(duration["value"]["blocked_seconds"], 40 * 60);
    assert_eq!(duration["value"]["left_censored_tasks"], 0);
    assert_eq!(duration["value"]["right_censored_tasks"], 0);
    let tasks = metric(&body, "blocked_tasks");
    assert_eq!(tasks["value"]["blocked_at_to"], 0);
    assert_eq!(tasks["value"]["entered_blocked_in_window"], 1, "D's second entry only");
    assert_eq!(tasks["value"]["entered_blocked_events"], 1);

    // A window that starts before the first recorded event is partial and only
    // its covered part is measured.
    let q = format!("?from={}&to={}", at(minutes(base, -60)), at(minutes(base, 120)));
    let body = health_ok(&env, &q).await;
    assert_eq!(body["partial"], true);
    let duration = metric(&body, "blocked_duration");
    assert_eq!(duration["status"], "partial");
    assert_eq!(duration["value"]["blocked_seconds"], 100 * 60, "nothing is invented for the uncovered hour");
    assert_eq!(duration["value"]["covered_from"], at(base));

    // `to` in the middle of A's blocked interval: right-censored, cut at `to`.
    let q = format!("?from={}&to={}", at(base), at(minutes(base, 25)));
    let body = health_ok(&env, &q).await;
    let duration = metric(&body, "blocked_duration");
    // A 10..25 = 15m, D 5..15 = 10m. C's first event (minute 30) lies after `to`:
    // only events before `to` are used, so a past window never changes when later
    // events arrive, and C is not guessed.
    assert_eq!(duration["value"]["blocked_seconds"], 25 * 60);
    assert_eq!(duration["value"]["right_censored_tasks"], 1, "A is still blocked at `to`");
    assert_eq!(metric(&body, "blocked_tasks")["value"]["blocked_at_to"], 1);
    // (The rows of this scenario were created "now", so C does not count as an
    // existing-but-unknown task at that past `to`; the unknown path has its own tests.)
}

// ------------------------------------------------------------------- M07

#[tokio::test]
async fn m07_handoffs_and_questions_are_recorded_and_counted_per_state() {
    let env = Env::new().await;
    let a = env.session(&env.owner_token, "A").await;
    let b = env.session(&env.owner_token, "B").await;
    let tid = new_task(&env, &a, "t").await;
    let handoff = |key: &str| json!({
        "client_handoff_id": key, "to_session_id": b.id, "summary": "s", "completed": "", "next_steps": "", "blockers": "",
    });
    let mut ids = Vec::new();
    for key in ["h1", "h2", "h3"] {
        let (status, h) = env
            .json(Method::POST, &env.url(&format!("/tasks/{tid}/handoffs")), &env.owner_token, Some(&a), Some(handoff(key)))
            .await;
        assert_eq!(status, 201, "{h}");
        ids.push(h["id"].as_str().unwrap().to_string());
    }
    // A replayed create writes no second event.
    let (status, _) = env
        .json(Method::POST, &env.url(&format!("/tasks/{tid}/handoffs")), &env.owner_token, Some(&a), Some(handoff("h1")))
        .await;
    assert_eq!(status, 200);
    assert_eq!(count_events(&env, &tid, "handoff_created").await, 3);

    let (s1, _) = env
        .json(Method::POST, &env.url(&format!("/handoffs/{}/accept", ids[0])), &env.owner_token, Some(&b), Some(json!({"expected_version": 1})))
        .await;
    let (s2, _) = env
        .json(Method::POST, &env.url(&format!("/handoffs/{}/cancel", ids[1])), &env.owner_token, Some(&a), Some(json!({"expected_version": 1})))
        .await;
    assert_eq!((s1, s2), (200, 200));
    // A refused transition leaves no event.
    let (s3, _) = env
        .json(Method::POST, &env.url(&format!("/handoffs/{}/accept", ids[0])), &env.owner_token, Some(&b), Some(json!({"expected_version": 1})))
        .await;
    assert_eq!(s3, 409);
    assert_eq!(count_events(&env, &tid, "handoff_accepted").await, 1);
    assert_eq!(count_events(&env, &tid, "handoff_cancelled").await, 1);

    // Questions: one answered, one cancelled, one left open.
    let ask = |n: &str| json!({"client_question_id": n, "recipient": {"kind": "repo_writers"}, "title": "q", "body": "b"});
    let mut qs = Vec::new();
    for n in ["q1", "q2", "q3"] {
        let (status, q) = env
            .json(Method::POST, &env.url(&format!("/tasks/{tid}/questions")), &env.owner_token, Some(&a), Some(ask(n)))
            .await;
        assert_eq!(status, 201, "{q}");
        qs.push(q);
    }
    let (status, answered) = env
        .json(
            Method::POST,
            &env.url(&format!("/questions/{}/answer", qs[0]["id"].as_str().unwrap())),
            &env.owner_token,
            None,
            Some(json!({"client_answer_id": "a1", "expected_version": 1, "body": "yes", "acknowledged_context_hash": qs[0]["context_hash"]})),
        )
        .await;
    assert_eq!(status, 200, "{answered}");
    let (status, _) = env
        .json(Method::POST, &env.url(&format!("/questions/{}/cancel", qs[1]["id"].as_str().unwrap())), &env.owner_token, Some(&a), Some(json!({"expected_version": 1})))
        .await;
    assert_eq!(status, 200);
    assert_eq!(count_events(&env, &tid, "question_opened").await, 3);

    let body = health_ok(&env, &covered(&env).await).await;
    let h = &metric(&body, "handoff_waiting")["value"];
    assert_eq!(h["accepted_in_window"], 1);
    assert_eq!(h["cancelled_in_window"], 1);
    assert_eq!(h["open_at_to"], 1);
    assert_eq!(h["right_censored_handoffs"], 1);
    assert_eq!(h["measured_handoffs"], 3);
    assert_eq!(metric(&body, "handoff_waiting")["source"], "platform_event");
    let q = &metric(&body, "open_questions")["value"];
    assert_eq!(q["open_at_to"], 1);
    assert_eq!(q["answered_in_window"], 1);
    assert_eq!(q["cancelled_in_window"], 1);
    assert_eq!(metric(&body, "handoff_waiting")["status"], "complete");
    assert_eq!(metric(&body, "open_questions")["status"], "complete");
    let notes = metric(&body, "handoff_waiting")["notes"].to_string();
    assert!(notes.contains("nothing about the work being done") || notes.contains("says nothing about the work"), "accepted is not completed");
}

#[tokio::test]
async fn m07_waiting_time_is_the_interval_intersection() {
    let env = Env::new().await;
    let s = env.session(&env.owner_token, "S").await;
    let tid = new_task(&env, &s, "t").await;
    forget_events(&env).await;
    let base = base_time();
    inject_created(&env, &tid, base).await;
    let id = || uuid::Uuid::new_v4().to_string();
    let (h1, h2, h3, q1, q2) = (id(), id(), id(), id(), id());
    // h1: pending 10..40 then accepted. h2: pending 20..25 then cancelled. h3: pending since 60.
    for (hid, kind, from, to, m) in [
        (&h1, "handoff_created", None, "pending", 10),
        (&h1, "handoff_accepted", Some("pending"), "accepted", 40),
        (&h2, "handoff_created", None, "pending", 20),
        (&h2, "handoff_cancelled", Some("pending"), "cancelled", 25),
        (&h3, "handoff_created", None, "pending", 60),
    ] {
        inject(&env, &tid, kind, from, Some(to), None, ("handoff", hid), minutes(base, m)).await;
    }
    // q1: open 10..30 then answered. q2: open since 20.
    for (qid, kind, from, to, m) in [
        (&q1, "question_opened", None, "open", 10),
        (&q1, "question_answered", Some("open"), "answered", 30),
        (&q2, "question_opened", None, "open", 20),
    ] {
        inject(&env, &tid, kind, from, Some(to), None, ("question", qid), minutes(base, m)).await;
    }

    // [30, 90): h1 10m, h2 none (cancelled before the window), h3 30m.
    let q = format!("?from={}&to={}", at(minutes(base, 30)), at(minutes(base, 90)));
    let body = health_ok(&env, &q).await;
    let h = &metric(&body, "handoff_waiting")["value"];
    assert_eq!(h["waiting_seconds"], 40 * 60);
    assert_eq!(h["accepted_in_window"], 1, "h1 accepted at 40");
    assert_eq!(h["cancelled_in_window"], 0, "h2 was cancelled at 25, before the window");
    assert_eq!(h["open_at_to"], 1);
    assert_eq!(h["measured_handoffs"], 3);
    // q1 answered at 30 is inside [30,90) but waited 0 inside it; q2 60m.
    let qv = &metric(&body, "open_questions")["value"];
    assert_eq!(qv["wait_seconds_in_window"], 60 * 60);
    assert_eq!(qv["open_at_to"], 1);
    assert_eq!(qv["open_wait_seconds_at_to"], 70 * 60, "open since minute 20");
    assert_eq!(qv["answered_in_window"], 1);

    // [0, 120): h1 30m, h2 5m, h3 60m = 95m; q1 20m + q2 100m.
    let q = format!("?from={}&to={}", at(base), at(minutes(base, 120)));
    let body = health_ok(&env, &q).await;
    let h = &metric(&body, "handoff_waiting")["value"];
    assert_eq!(h["waiting_seconds"], 95 * 60);
    assert_eq!(h["accepted_in_window"], 1);
    assert_eq!(h["cancelled_in_window"], 1);
    assert_eq!(h["open_at_to"], 1);
    let qv = &metric(&body, "open_questions")["value"];
    assert_eq!(qv["wait_seconds_in_window"], 120 * 60);
    assert_eq!(qv["open_wait_seconds_at_to"], 100 * 60);

    // `to` inside h1's wait: it is still pending then and cut at `to`.
    let q = format!("?from={}&to={}", at(base), at(minutes(base, 22)));
    let body = health_ok(&env, &q).await;
    let h = &metric(&body, "handoff_waiting")["value"];
    assert_eq!(h["open_at_to"], 2, "h1 and h2 are pending at minute 22");
    assert_eq!(h["waiting_seconds"], (12 + 2) * 60);
    assert_eq!(h["accepted_in_window"], 0, "the accept happens after `to`");
    assert_eq!(h["cancelled_in_window"], 0);
}

// ------------------------------------------------------------------- M08

#[tokio::test]
async fn m08_data_from_before_the_feature_is_unknown_until_a_baseline_and_never_zero() {
    let env = Env::new().await;
    let s = env.session(&env.owner_token, "S").await;
    let old = new_task(&env, &s, "old").await;
    assert_eq!(set_status(&env, &s, &old, 1, "blocked").await.0, 200);
    forget_events(&env).await; // `old` is now a task from before the feature

    // No event at all: everything is unknown, nothing is reported as zero.
    let body = health_ok(&env, "").await;
    assert_eq!(body["coverage_start"], Value::Null);
    assert_eq!(body["partial"], true);
    for id in ["blocked_tasks", "blocked_duration", "handoff_waiting", "open_questions", "stale_path_claims"] {
        let m = metric(&body, id);
        assert_eq!(m["status"], "unknown", "{id}");
        assert!(m["value"].is_null(), "{id}: {m}");
        assert!(m["reason"].is_string(), "{id}");
    }
    assert_eq!(metric(&body, "stale_path_claims")["reason"], "PATH_DECLARED_AT_NOT_RECORDED");

    // A new task starts the event log; the old one stays unknown (not "not blocked").
    let fresh = new_task(&env, &s, "fresh").await;
    let body = health_ok(&env, "").await;
    assert!(body["coverage_start"].is_string());
    assert_eq!(body["partial"], true);
    let tasks = metric(&body, "blocked_tasks");
    assert_eq!(tasks["status"], "partial");
    assert_eq!(tasks["value"]["blocked_at_to"], 0);
    assert_eq!(tasks["value"]["unknown_state_tasks"], 1);
    assert_eq!(body["data_quality"]["unknown_state_tasks"], 1);
    assert!(metric(&body, "blocked_duration")["notes"].to_string().contains("baseline"));
    let (status, unknown) = env
        .json(Method::GET, &env.url("/health/tasks?metric=unknown_state"), &env.owner_token, None, None)
        .await;
    assert_eq!(status, 200, "{unknown}");
    assert_eq!(unknown["items"][0]["task_id"], old.as_str());
    assert_eq!(unknown["items"].as_array().unwrap().len(), 1);
    assert_ne!(unknown["items"][0]["task_id"], fresh.as_str());

    // The baseline makes the blocked task visible as observed *since now*.
    let before = Utc::now();
    let (status, report) = env
        .json(Method::POST, &env.url("/health/baseline"), &env.owner_token, None, None)
        .await;
    assert_eq!(status, 200, "{report}");
    assert_eq!(report["added"]["tasks"], 1);
    assert_eq!(report["existed_before"], false);
    let events = events_of(&env, &old).await;
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event_type, "task_status_observed");
    assert_eq!(events[0].source_type, "baseline_snapshot");
    assert_eq!(events[0].to_state.as_deref(), Some("blocked"));
    assert!(events[0].occurred_at >= before - Duration::seconds(1), "observed now, not earlier");

    let body = health_ok(&env, &covered(&env).await).await;
    assert_eq!(body["baseline"]["taken_at"], at(events[0].occurred_at));
    let tasks = metric(&body, "blocked_tasks");
    assert_eq!(tasks["status"], "complete");
    assert_eq!(tasks["value"]["blocked_at_to"], 1);
    assert_eq!(tasks["value"]["unknown_state_tasks"], 0);
    assert_eq!(tasks["value"]["entered_blocked_in_window"], 0, "observed, not entered");
    let duration = metric(&body, "blocked_duration");
    assert_eq!(duration["value"]["left_censored_tasks"], 1, "observed since T, never since forever");
    let seconds = duration["value"]["blocked_seconds"].as_i64().unwrap();
    let elapsed = (Utc::now() - events[0].occurred_at).num_seconds();
    assert!((0..=elapsed + 1).contains(&seconds), "{seconds} vs {elapsed}");
    let (_, blocked_now) = env
        .json(Method::GET, &env.url("/health/tasks?metric=blocked_at_to"), &env.owner_token, None, None)
        .await;
    assert_eq!(blocked_now["items"][0]["basis"]["since_is_observation_start"], true);

    // Repeating the baseline adds nothing.
    let (_, again) = env
        .json(Method::POST, &env.url("/health/baseline"), &env.owner_token, None, None)
        .await;
    assert_eq!(again["added"], json!({"tasks": 0, "handoffs": 0, "questions": 0}));
    assert_eq!(again["existed_before"], true);
    assert_eq!(events_of(&env, &old).await.len(), 1);
}

#[tokio::test]
async fn m08_pending_handoffs_and_open_questions_from_before_the_feature_are_partial_until_baselined() {
    let env = Env::new().await;
    let a = env.session(&env.owner_token, "A").await;
    let b = env.session(&env.owner_token, "B").await;
    let tid = new_task(&env, &a, "t").await;
    let (_, h) = env
        .json(
            Method::POST,
            &env.url(&format!("/tasks/{tid}/handoffs")),
            &env.owner_token,
            Some(&a),
            Some(json!({"client_handoff_id": "h", "to_session_id": b.id, "summary": "s", "completed": "", "next_steps": "", "blockers": ""})),
        )
        .await;
    let (_, q) = env
        .json(
            Method::POST,
            &env.url(&format!("/tasks/{tid}/questions")),
            &env.owner_token,
            Some(&a),
            Some(json!({"client_question_id": "q", "recipient": {"kind": "repo_writers"}, "title": "t", "body": "b"})),
        )
        .await;
    forget_events(&env).await;
    // Something recorded afterwards so that coverage exists.
    let _other = new_task(&env, &a, "other").await;

    let body = health_ok(&env, "").await;
    assert_eq!(metric(&body, "handoff_waiting")["status"], "partial");
    assert_eq!(metric(&body, "handoff_waiting")["value"]["open_at_to"], 0, "unknown, not counted");
    assert_eq!(body["data_quality"]["unobserved_pending_handoffs"], 1);
    assert_eq!(body["data_quality"]["unobserved_open_questions"], 1);

    let (_, report) = env
        .json(Method::POST, &env.url("/health/baseline"), &env.owner_token, None, None)
        .await;
    assert_eq!(report["added"]["handoffs"], 1);
    assert_eq!(report["added"]["questions"], 1);
    let body = health_ok(&env, &covered(&env).await).await;
    assert_eq!(metric(&body, "handoff_waiting")["status"], "complete");
    assert_eq!(metric(&body, "handoff_waiting")["value"]["open_at_to"], 1);
    assert_eq!(metric(&body, "handoff_waiting")["value"]["left_censored_handoffs"], 1);
    assert_eq!(metric(&body, "open_questions")["value"]["open_at_to"], 1);

    // Accepting it later keeps one timeline: observed -> accepted.
    let (status, _) = env
        .json(Method::POST, &env.url(&format!("/handoffs/{}/accept", h["id"].as_str().unwrap())), &env.owner_token, Some(&b), Some(json!({"expected_version": 1})))
        .await;
    assert_eq!(status, 200);
    let body = health_ok(&env, &covered(&env).await).await;
    let value = &metric(&body, "handoff_waiting")["value"];
    assert_eq!(value["accepted_in_window"], 1);
    assert_eq!(value["open_at_to"], 0);
    assert_eq!(body["data_quality"]["handoffs_missing_start"], 0);
    let _ = q;
}

// ------------------------------------------------------------------- M09

#[tokio::test]
async fn m09_only_repository_admins_can_read_or_baseline() {
    let env = Env::new().await;
    let a = env.session(&env.owner_token, "A").await;
    let tid = new_task(&env, &a, "secret-titled task").await;
    let (writer, writer_id) = env.user("writer").await;
    env.add_collaborator(writer_id, "write").await;
    let (reader, reader_id) = env.user("reader").await;
    env.add_collaborator(reader_id, "read").await;
    let (stranger, _) = env.user("stranger").await;

    let url = env.url("/health");
    let tasks_url = env.url("/health/tasks?metric=blocked_at_to");
    for token in [&writer, &reader, &stranger] {
        assert_eq!(env.json(Method::GET, &url, token, None, None).await.0, 403);
        assert_eq!(env.json(Method::GET, &tasks_url, token, None, None).await.0, 403);
        assert_eq!(env.json(Method::POST, &env.url("/health/baseline"), token, None, None).await.0, 403);
    }
    let unauth = env.send(Method::GET, &url, None, None, None).await;
    assert_eq!(unauth.status().as_u16(), 401);
    // The refused baseline wrote nothing beyond what the task creation recorded.
    assert_eq!(events_of(&env, &tid).await.len(), 1);

    // The owner is an admin; an admin collaborator is too.
    assert_eq!(env.json(Method::GET, &url, &env.owner_token, None, None).await.0, 200);
    let (admin, admin_id) = env.user("adm").await;
    env.add_collaborator(admin_id, "admin").await;
    assert_eq!(env.json(Method::GET, &url, &admin, None, None).await.0, 200);
}

#[tokio::test]
async fn m09_private_repository_of_someone_else_is_refused_and_repos_do_not_mix() {
    let env = Env::with_repo(json!({"name": "collab-repo", "is_private": true})).await;
    let a = env.session(&env.owner_token, "A").await;
    let tid = new_task(&env, &a, "t").await;
    assert_eq!(set_status(&env, &a, &tid, 1, "blocked").await.0, 200);

    // Someone with no access learns nothing (and nothing about existence beyond the repo route).
    let (other, _) = env.user("other").await;
    let (status, body) = env.json(Method::GET, &env.url("/health"), &other, None, None).await;
    assert!(status == 403 || status == 404, "{status} {body}");
    assert!(!body.to_string().contains(&tid));

    // A second repository of the same owner has its own, separate numbers.
    let resp = reqwest::Client::new()
        .post(format!("{}/api/v1/repos", env.base))
        .bearer_auth(&env.owner_token)
        .json(&json!({"name": "second-repo"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);
    let (status, second) = env
        .json(Method::GET, &env.url_in("second-repo", "/health"), &env.owner_token, None, None)
        .await;
    assert_eq!(status, 200, "{second}");
    assert_eq!(second["coverage_start"], Value::Null, "no events in the other repository");
    assert_eq!(metric(&second, "blocked_tasks")["status"], "unknown");
    let (_, first) = env.json(Method::GET, &env.url("/health"), &env.owner_token, None, None).await;
    assert_eq!(metric(&first, "blocked_tasks")["value"]["blocked_at_to"], 1);
}

#[tokio::test]
async fn m09_responses_have_no_agent_ranking_and_no_message_bodies() {
    let env = Env::new().await;
    let a = env.session(&env.owner_token, "Alice-Agent").await;
    let b = env.session(&env.owner_token, "Bob-Agent").await;
    let tid = new_task(&env, &a, "t").await;
    let (status, _) = env
        .json(
            Method::POST,
            &env.url(&format!("/tasks/{tid}/messages")),
            &env.owner_token,
            Some(&a),
            Some(json!({"client_message_id": "m1", "kind": "note", "body": "MESSAGE-BODY-MARKER"})),
        )
        .await;
    assert!(status == 201 || status == 200);
    assert_eq!(set_status(&env, &a, &tid, 1, "blocked").await.0, 200);
    let (status, _) = env
        .json(
            Method::POST,
            &env.url(&format!("/tasks/{tid}/questions")),
            &env.owner_token,
            Some(&b),
            Some(json!({"client_question_id": "q", "recipient": {"kind": "repo_writers"}, "title": "t", "body": "QUESTION-BODY-MARKER"})),
        )
        .await;
    assert_eq!(status, 201);

    let mut all = health_ok(&env, "").await.to_string();
    for metric in ["blocked_at_to", "entered_blocked", "blocked_duration", "handoff_waiting", "open_questions", "unknown_state"] {
        let (status, page) = env
            .json(Method::GET, &env.url(&format!("/health/tasks?metric={metric}")), &env.owner_token, None, None)
            .await;
        assert_eq!(status, 200, "{page}");
        all.push_str(&page.to_string());
    }
    for forbidden in [a.id.as_str(), b.id.as_str(), "Alice-Agent", "Bob-Agent", "MESSAGE-BODY-MARKER", "QUESTION-BODY-MARKER", "session", "agent", "rank", "owner_session"] {
        assert!(!all.contains(forbidden), "response leaks `{forbidden}`");
    }
}

#[tokio::test]
async fn m09_concurrent_changes_record_exactly_one_event_each() {
    let env = Env::new().await;
    let a = env.session(&env.owner_token, "A").await;
    let b = env.session(&env.owner_token, "B").await;
    let tid = new_task(&env, &a, "t").await;

    // 8 racing writers with the same expected version: one wins, one event.
    let mut handles = Vec::new();
    for _ in 0..8 {
        let (env_base, token, sess, url) = (
            env.base.clone(),
            env.owner_token.clone(),
            a.clone(),
            env.url(&format!("/tasks/{tid}")),
        );
        let _ = env_base;
        handles.push(tokio::spawn(async move {
            reqwest::Client::new()
                .patch(url)
                .bearer_auth(token)
                .header(collab_common::HDR_SESSION, &sess.id)
                .header(collab_common::HDR_SECRET, &sess.secret)
                .json(&json!({"expected_version": 1, "status": "blocked"}))
                .send()
                .await
                .unwrap()
                .status()
                .as_u16()
        }));
    }
    let mut statuses = Vec::new();
    for h in handles {
        statuses.push(h.await.unwrap());
    }
    assert_eq!(statuses.iter().filter(|s| **s == 200).count(), 1, "{statuses:?}");
    assert!(statuses.iter().all(|s| *s == 200 || *s == 409), "{statuses:?}");
    assert_eq!(count_events(&env, &tid, "task_status_changed").await, 1);

    // Accept and cancel race on one handoff: one terminal event.
    let (_, h) = env
        .json(
            Method::POST,
            &env.url(&format!("/tasks/{tid}/handoffs")),
            &env.owner_token,
            Some(&a),
            Some(json!({"client_handoff_id": "h", "to_session_id": b.id, "summary": "s", "completed": "", "next_steps": "", "blockers": ""})),
        )
        .await;
    let hid = h["id"].as_str().unwrap().to_string();
    let (accept_url, cancel_url) = (env.url(&format!("/handoffs/{hid}/accept")), env.url(&format!("/handoffs/{hid}/cancel")));
    let accept = env.json(Method::POST, &accept_url, &env.owner_token, Some(&b), Some(json!({"expected_version": 1})));
    let cancel = env.json(Method::POST, &cancel_url, &env.owner_token, Some(&a), Some(json!({"expected_version": 1})));
    let (x, y) = tokio::join!(accept, cancel);
    assert_eq!([x.0, y.0].iter().filter(|s| **s == 200).count(), 1);
    let terminal = count_events(&env, &tid, "handoff_accepted").await + count_events(&env, &tid, "handoff_cancelled").await;
    assert_eq!(terminal, 1);

    // Racing creates with one idempotency key: one row, one event.
    let mut handles = Vec::new();
    for _ in 0..6 {
        let (token, sess, url) = (env.owner_token.clone(), b.clone(), env.url(&format!("/tasks/{tid}/questions")));
        handles.push(tokio::spawn(async move {
            reqwest::Client::new()
                .post(url)
                .bearer_auth(token)
                .header(collab_common::HDR_SESSION, &sess.id)
                .header(collab_common::HDR_SECRET, &sess.secret)
                .json(&json!({"client_question_id": "same", "recipient": {"kind": "repo_writers"}, "title": "t", "body": "b"}))
                .send()
                .await
                .unwrap()
                .status()
                .as_u16()
        }));
    }
    for h in handles {
        let status = h.await.unwrap();
        assert!(status == 200 || status == 201, "{status}");
    }
    assert_eq!(count_events(&env, &tid, "question_opened").await, 1);

    // Racing baselines on pre-feature data: one observation per object.
    forget_events(&env).await;
    let mut handles = Vec::new();
    for _ in 0..5 {
        let (token, url) = (env.owner_token.clone(), env.url("/health/baseline"));
        handles.push(tokio::spawn(async move {
            reqwest::Client::new().post(url).bearer_auth(token).send().await.unwrap().status().as_u16()
        }));
    }
    for h in handles {
        assert_eq!(h.await.unwrap(), 200);
    }
    assert_eq!(count_events(&env, &tid, "task_status_observed").await, 1);
    assert_eq!(count_events(&env, &tid, "question_observed_open").await, 1);
}

#[tokio::test]
async fn m09_an_event_that_cannot_be_recorded_rolls_the_business_change_back() {
    let env = Env::new().await;
    let a = env.session(&env.owner_token, "A").await;
    let tid = new_task(&env, &a, "t").await;
    // Occupy the exact event the next status change would write.
    inject_change(&env, &tid, "open", "blocked", 2, Utc::now()).await;
    let (status, _) = set_status(&env, &a, &tid, 1, "blocked").await;
    assert_eq!(status, 500, "the event write failed, so the change must fail");
    let (_, task) = env.json(Method::GET, &env.url(&format!("/tasks/{tid}")), &env.owner_token, None, None).await;
    assert_eq!(task["status"], "open");
    assert_eq!(task["version"], 1, "the status change was rolled back with the event");
}

#[tokio::test]
async fn m09_takeover_is_recorded_once_and_replays_add_nothing() {
    let env = Env::new().await;
    let a = env.session(&env.owner_token, "A").await;
    let b = env.session(&env.owner_token, "B").await;
    let tid = new_task(&env, &a, "t").await;
    let body = json!({"client_takeover_id": "k", "expected_task_version": 1, "expected_owner_session_id": null, "reason": "r"});
    let url = env.url(&format!("/tasks/{tid}/takeovers"));
    assert_eq!(env.json(Method::POST, &url, &env.owner_token, Some(&b), Some(body.clone())).await.0, 201);
    assert_eq!(env.json(Method::POST, &url, &env.owner_token, Some(&b), Some(body)).await.0, 200);
    assert_eq!(count_events(&env, &tid, "takeover").await, 1);
}

#[tokio::test]
async fn m09_recompute_is_repeatable_and_drill_down_pages_stably() {
    let env = Env::new().await;
    let s = env.session(&env.owner_token, "S").await;
    let mut ids = Vec::new();
    for i in 0..5 {
        let id = new_task(&env, &s, &format!("t{i}")).await;
        assert_eq!(set_status(&env, &s, &id, 1, "blocked").await.0, 200);
        ids.push(id);
    }
    ids.sort();
    let to = at(Utc::now());
    let q = format!("?to={to}");
    let first = health_ok(&env, &q).await;
    let second = health_ok(&env, &q).await;
    assert_eq!(first["metrics"], second["metrics"], "same events, same window, same numbers");
    assert_eq!(first["data_quality"], second["data_quality"]);
    assert_eq!(metric(&first, "blocked_tasks")["value"]["blocked_at_to"], 5);

    let mut seen = Vec::new();
    let mut cursor: Option<String> = None;
    let mut pages = 0;
    loop {
        let mut url = format!("/health/tasks?metric=blocked_at_to&to={to}&limit=2");
        if let Some(c) = &cursor {
            url.push_str(&format!("&cursor={c}"));
        }
        let (status, page) = env.json(Method::GET, &env.url(&url), &env.owner_token, None, None).await;
        assert_eq!(status, 200, "{page}");
        pages += 1;
        for item in page["items"].as_array().unwrap() {
            assert_eq!(item["title_origin"], "user_content");
            assert_eq!(item["status"], "blocked");
            assert!(item["href"].as_str().unwrap().ends_with(item["task_id"].as_str().unwrap()));
            seen.push(item["task_id"].as_str().unwrap().to_string());
        }
        cursor = page["next_cursor"].as_str().map(str::to_string);
        if cursor.is_none() {
            break;
        }
    }
    assert_eq!(pages, 3);
    assert_eq!(seen, ids, "every task exactly once, in id order");

    // A cursor is bound to its metric and window; tampering is refused.
    let (_, page) = env
        .json(Method::GET, &env.url(&format!("/health/tasks?metric=blocked_at_to&to={to}&limit=2")), &env.owner_token, None, None)
        .await;
    let c = page["next_cursor"].as_str().unwrap();
    let other_metric = env
        .json(Method::GET, &env.url(&format!("/health/tasks?metric=entered_blocked&to={to}&limit=2&cursor={c}")), &env.owner_token, None, None)
        .await;
    assert_eq!(other_metric.0, 400);
    let tampered = env
        .json(Method::GET, &env.url(&format!("/health/tasks?metric=blocked_at_to&to={to}&limit=2&cursor={c}x")), &env.owner_token, None, None)
        .await;
    assert_eq!(tampered.0, 400);
}

#[tokio::test]
async fn m09_window_and_metric_parameters_are_validated() {
    let env = Env::new().await;
    let s = env.session(&env.owner_token, "S").await;
    new_task(&env, &s, "t").await;
    let now = Utc::now();
    let cases = [
        format!("?from={}&to={}", at(now), at(now)),
        format!("?to={}", at(now + Duration::hours(1))),
        format!("?from={}&to={}", at(now - Duration::days(91)), at(now)),
        "?from=yesterday".to_string(),
        "?metrics=blocked_tasks,nonsense".to_string(),
    ];
    for q in cases {
        assert_eq!(health(&env, &env.owner_token, &q).await.0, 400, "{q}");
    }
    assert_eq!(
        health(&env, &env.owner_token, &format!("?from={}&to={}", at(now - Duration::days(90)), at(now))).await.0,
        200
    );
    // Default window: the last 7 days; metrics filter.
    let body = health_ok(&env, "?metrics=open_questions").await;
    let from: DateTime<Utc> = body["range"]["from"].as_str().unwrap().parse().unwrap();
    let to: DateTime<Utc> = body["range"]["to"].as_str().unwrap().parse().unwrap();
    assert_eq!(to - from, Duration::days(7));
    assert_eq!(body["metrics"].as_array().unwrap().len(), 1);
    // Drill-down requires a known metric.
    assert_eq!(env.json(Method::GET, &env.url("/health/tasks"), &env.owner_token, None, None).await.0, 400);
    assert_eq!(env.json(Method::GET, &env.url("/health/tasks?metric=agents"), &env.owner_token, None, None).await.0, 400);
}

#[tokio::test]
async fn m09_the_same_platform_event_cannot_be_stored_twice() {
    let env = Env::new().await;
    let s = env.session(&env.owner_token, "S").await;
    let tid = new_task(&env, &s, "t").await;
    // `task_created` for this task already exists (written by the real hook).
    let again = ev::ActiveModel {
        id: Set(uuid::Uuid::new_v4().to_string()),
        repo_id: Set(env.repo_id),
        task_id: Set(tid.clone()),
        event_type: Set("task_created".to_string()),
        from_state: Set(None),
        to_state: Set(Some("open".to_string())),
        task_version: Set(Some(1)),
        source_type: Set("task".to_string()),
        source_id: Set(tid.clone()),
        occurred_at: Set(Utc::now()),
        recorded_at: Set(Utc::now()),
    }
    .insert(&env.db)
    .await;
    assert!(again.is_err(), "(repo, source_type, source_id, event_type) is unique");
    assert_eq!(count_events(&env, &tid, "task_created").await, 1);
}
