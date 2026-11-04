//! O05 acceptance: notification escalation. Real HTTP server and SQLite; no
//! scheduler is involved — conditions are aged by editing stored timestamps
//! and evaluated by reading notices or calling the evaluation function.

mod common;
mod collab_common;

use std::sync::Arc;

use chrono::{Duration, Timelike, Utc};
use collab_common::{Env, Sess};
use reqwest::Method;
use rg_core::collaboration::escalation;
use rg_db::entities::{agent_session, collab_notice, collab_question, collab_task};
use rg_db::sea_orm::{ActiveModelTrait, EntityTrait, Set};
use serde_json::{json, Value};
use tokio::sync::Barrier;

async fn new_task(env: &Env, s: &Sess) -> String {
    let (status, task) = env
        .json(Method::POST, &env.url("/tasks"), &env.owner_token, Some(s), Some(json!({"title": "task"})))
        .await;
    assert_eq!(status, 201, "{task}");
    task["id"].as_str().unwrap().to_string()
}

async fn put_settings(env: &Env, version: i64, extra: Value) -> (u16, Value) {
    let mut body = json!({
        "expected_version": version, "enabled": true,
        "blocked_after_secs": 600, "question_after_secs": 600, "handoff_after_secs": 600,
        "repeat_after_secs": 0, "quiet_hours": null
    });
    body.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
    env.json(Method::PUT, &env.url("/escalation-settings"), &env.owner_token, None, Some(body)).await
}

async fn ask(env: &Env, tid: &str, who: &Sess, key: &str) -> Value {
    let (status, q) = env
        .json(
            Method::POST,
            &env.url(&format!("/tasks/{tid}/questions")),
            &env.owner_token,
            Some(who),
            Some(json!({"client_question_id": key, "recipient": {"kind": "repo_writers"}, "title": "t", "body": "b"})),
        )
        .await;
    assert_eq!(status, 201, "{q}");
    q
}

async fn age_question(env: &Env, id: &str, secs: i64) {
    let row = collab_question::Entity::find_by_id(id.to_string()).one(&env.db).await.unwrap().unwrap();
    let mut a: collab_question::ActiveModel = row.into();
    a.created_at = Set(Utc::now() - Duration::seconds(secs));
    a.update(&env.db).await.unwrap();
}

async fn set_task(env: &Env, id: &str, status: &str, owner: Option<&str>, age_secs: i64) {
    let row = collab_task::Entity::find_by_id(id.to_string()).one(&env.db).await.unwrap().unwrap();
    let mut a: collab_task::ActiveModel = row.into();
    a.status = Set(status.to_string());
    a.owner_session_id = Set(owner.map(str::to_string));
    a.updated_at = Set(Utc::now() - Duration::seconds(age_secs));
    a.update(&env.db).await.unwrap();
}

async fn age_notice(env: &Env, id: &str, secs: i64) {
    let row = collab_notice::Entity::find_by_id(id.to_string()).one(&env.db).await.unwrap().unwrap();
    let acked = row.ack_at.is_some();
    let mut a: collab_notice::ActiveModel = row.into();
    a.created_at = Set(Utc::now() - Duration::seconds(secs));
    if acked {
        a.ack_at = Set(Some(Utc::now() - Duration::seconds(secs)));
    }
    a.update(&env.db).await.unwrap();
}

async fn expire_session(env: &Env, id: &str, ago_secs: i64) {
    let row = agent_session::Entity::find_by_id(id.to_string()).one(&env.db).await.unwrap().unwrap();
    let mut a: agent_session::ActiveModel = row.into();
    a.expires_at = Set(Utc::now() - Duration::seconds(ago_secs));
    a.update(&env.db).await.unwrap();
}

async fn escalations(env: &Env, who: &Sess, state: &str) -> Vec<Value> {
    let (status, body) = env
        .json(Method::GET, &env.url(&format!("/notices?state={state}&limit=100")), &env.owner_token, Some(who), None)
        .await;
    assert_eq!(status, 200, "{body}");
    body["items"].as_array().unwrap().iter().filter(|n| n["kind"] == "escalation").cloned().collect()
}

async fn repo(env: &Env) -> rg_db::entities::repository::Model {
    rg_db::ops::repo_ops::find_by_id(&env.db, env.repo_id).await.unwrap().unwrap()
}

// ---------------------------------------------------------------- settings

#[tokio::test]
async fn o05_settings_are_admin_managed_versioned_and_validated() {
    let env = Env::new().await;
    let (s, d) = env.json(Method::GET, &env.url("/escalation-settings"), &env.owner_token, None, None).await;
    assert_eq!(s, 200, "{d}");
    assert_eq!((d["enabled"].as_bool(), d["source"].as_str(), d["version"].as_i64()), (Some(false), Some("default"), Some(0)));

    // A writer may read but not change them.
    let (token, uid) = env.user("writer").await;
    env.add_collaborator(uid, "write").await;
    let (s, _) = env.json(Method::GET, &env.url("/escalation-settings"), &token, None, None).await;
    assert_eq!(s, 200);
    let (s, invalid) = env.json(Method::PUT, &env.url("/escalation-settings"), &token, None, Some(json!({}))).await;
    assert_eq!((s, invalid["error"]["code"].as_str()), (400, Some("INVALID_REQUEST_BODY")), "{invalid}");
    let body = json!({"expected_version": 0, "enabled": true, "blocked_after_secs": 600, "question_after_secs": 600,
        "handoff_after_secs": 600, "repeat_after_secs": 0, "quiet_hours": null});
    let (s, _) = env.json(Method::PUT, &env.url("/escalation-settings"), &token, None, Some(body)).await;
    assert_eq!(s, 403);

    let (s, saved) = put_settings(&env, 0, json!({})).await;
    assert_eq!(s, 200, "{saved}");
    assert_eq!((saved["version"].as_i64(), saved["source"].as_str()), (Some(1), Some("stored")));
    // A retry with the same values is a no-op even though its version is stale.
    let (s, again) = put_settings(&env, 0, json!({})).await;
    assert_eq!(s, 200, "{again}");
    assert_eq!(again["version"], 1);
    // Different values with a stale version conflict and name the current one.
    let (s, conflict) = put_settings(&env, 0, json!({"blocked_after_secs": 900})).await;
    assert_eq!(s, 409, "{conflict}");
    assert_eq!(conflict["error"]["code"], "VERSION_CONFLICT");
    let (s, bumped) = put_settings(&env, 1, json!({"blocked_after_secs": 900, "quiet_hours": {"start_minute": 1320, "end_minute": 420}})).await;
    assert_eq!(s, 200, "{bumped}");
    assert_eq!(bumped["version"], 2);
    assert_eq!(bumped["quiet_hours"]["start_minute"], 1320);

    for bad in [
        json!({"blocked_after_secs": 5}),
        json!({"repeat_after_secs": 10}),
        json!({"quiet_hours": {"start_minute": 5, "end_minute": 5}}),
    ] {
        let (s, _) = put_settings(&env, 2, bad).await;
        assert_eq!(s, 400);
    }
}

#[tokio::test]
async fn o05_racing_first_saves_leave_one_winner() {
    let env = Env::new().await;
    let barrier = Arc::new(Barrier::new(6));
    let handles: Vec<_> = (0..6)
        .map(|i| {
            let (url, token, barrier) = (env.url("/escalation-settings"), env.owner_token.clone(), barrier.clone());
            tokio::spawn(async move {
                barrier.wait().await;
                let resp = reqwest::Client::new()
                    .put(url)
                    .bearer_auth(token)
                    .json(&json!({"expected_version": 0, "enabled": true, "blocked_after_secs": 600 + i * 60,
                        "question_after_secs": 600, "handoff_after_secs": 600, "repeat_after_secs": 0, "quiet_hours": null}))
                    .send()
                    .await
                    .unwrap();
                resp.status().as_u16()
            })
        })
        .collect();
    let mut codes = Vec::new();
    for h in handles {
        codes.push(h.await.unwrap());
    }
    assert_eq!(codes.iter().filter(|c| **c == 200).count(), 1, "{codes:?}");
    assert_eq!(codes.iter().filter(|c| **c == 409).count(), 5, "{codes:?}");
}

// -------------------------------------------------------------- evaluation

#[tokio::test]
async fn o05_a_stale_question_reminds_its_asker_once_even_under_concurrent_reads() {
    let env = Env::new().await;
    let a = env.session(&env.owner_token, "A").await;
    let b = env.session(&env.owner_token, "B").await;
    let tid = new_task(&env, &a).await;
    let q = ask(&env, &tid, &a, "q1").await;
    let qid = q["id"].as_str().unwrap();

    // Escalation is off by default: an old question produces nothing.
    age_question(&env, qid, 7200).await;
    assert!(escalations(&env, &a, "all").await.is_empty());

    assert_eq!(put_settings(&env, 0, json!({})).await.0, 200);
    // Young enough: nothing yet.
    age_question(&env, qid, 60).await;
    assert!(escalations(&env, &a, "all").await.is_empty());
    age_question(&env, qid, 7200).await;

    // Eight simultaneous reads create exactly one reminder.
    let barrier = Arc::new(Barrier::new(8));
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let (url, token, barrier, a) = (env.url("/notices?state=all"), env.owner_token.clone(), barrier.clone(), a.clone());
            tokio::spawn(async move {
                barrier.wait().await;
                reqwest::Client::new()
                    .get(url)
                    .bearer_auth(token)
                    .header(collab_common::HDR_SESSION, a.id)
                    .header(collab_common::HDR_SECRET, a.secret)
                    .send()
                    .await
                    .unwrap()
                    .status()
                    .as_u16()
            })
        })
        .collect();
    for h in handles {
        assert_eq!(h.await.unwrap(), 200);
    }
    let got = escalations(&env, &a, "all").await;
    assert_eq!(got.len(), 1, "{got:?}");
    assert_eq!(got[0]["source_id"], qid);
    assert_eq!(got[0]["hint"]["condition"], "question_unanswered");
    assert_eq!(got[0]["generation"], 1);
    assert_eq!(got[0]["authority"], "hint");

    // Further reads, an inbox read and a periodic evaluation add nothing.
    escalations(&env, &a, "all").await;
    let (s, _) = env.json(Method::GET, &env.url("/inbox"), &env.owner_token, Some(&a), None).await;
    assert_eq!(s, 200);
    let report = escalation::evaluate_repo(&env.db, &repo(&env).await, Utc::now()).await.unwrap();
    assert_eq!(report.emitted, 0, "{report:?}");
    assert_eq!(escalations(&env, &a, "all").await.len(), 1);

    // It goes to the asker only.
    assert!(escalations(&env, &b, "all").await.is_empty());

    // Acknowledged and repeat_after_secs = 0: the same episode is never repeated.
    let nid = got[0]["id"].as_str().unwrap();
    let (s, _) = env.json(Method::POST, &env.url(&format!("/notices/{nid}/ack")), &env.owner_token, Some(&a), None).await;
    assert_eq!(s, 200);
    age_notice(&env, nid, 3600).await;
    assert_eq!(escalations(&env, &a, "all").await.len(), 1);
}

#[tokio::test]
async fn o05_acknowledged_reminders_repeat_only_after_the_interval() {
    let env = Env::new().await;
    let a = env.session(&env.owner_token, "A").await;
    let tid = new_task(&env, &a).await;
    let q = ask(&env, &tid, &a, "q1").await;
    let qid = q["id"].as_str().unwrap();
    assert_eq!(put_settings(&env, 0, json!({"repeat_after_secs": 3600})).await.0, 200);
    age_question(&env, qid, 7200).await;

    let first = escalations(&env, &a, "pending").await;
    let nid = first[0]["id"].as_str().unwrap().to_string();
    // Unacknowledged: even long after the interval no second reminder stacks up.
    age_notice(&env, &nid, 7200 - 600).await;
    assert_eq!(escalations(&env, &a, "all").await.len(), 1);
    env.json(Method::POST, &env.url(&format!("/notices/{nid}/ack")), &env.owner_token, Some(&a), None).await;
    // Interval not over (measured from the acknowledgement): no repeat.
    assert_eq!(escalations(&env, &a, "all").await.len(), 1);
    age_notice(&env, &nid, 3600 + 60).await;
    let all = escalations(&env, &a, "all").await;
    assert_eq!(all.len(), 2, "{all:?}");
    assert_eq!(all[1]["generation"], 2);
    // The new one is unacknowledged, so a third is not stacked on top.
    age_notice(&env, all[1]["id"].as_str().unwrap(), 3600 - 600).await;
    assert_eq!(escalations(&env, &a, "all").await.len(), 2);
}

#[tokio::test]
async fn o05_resolved_or_cancelled_conditions_leave_no_stale_reminder() {
    let env = Env::new().await;
    let a = env.session(&env.owner_token, "A").await;
    let b = env.session(&env.owner_token, "B").await;
    let tid = new_task(&env, &a).await;
    assert_eq!(put_settings(&env, 0, json!({})).await.0, 200);

    // 1. question answered after the reminder
    let q = ask(&env, &tid, &a, "q1").await;
    let qid = q["id"].as_str().unwrap().to_string();
    age_question(&env, &qid, 7200).await;
    assert_eq!(escalations(&env, &a, "pending").await.len(), 1);
    let (_, qfull) = env.json(Method::GET, &env.url(&format!("/questions/{qid}")), &env.owner_token, None, None).await;
    let (s, _) = env
        .json(
            Method::POST,
            &env.url(&format!("/questions/{qid}/answer")),
            &env.owner_token,
            None,
            Some(json!({"client_answer_id": "a1", "expected_version": qfull["version"], "body": "ok", "acknowledged_context_hash": qfull["context_hash"]})),
        )
        .await;
    assert_eq!(s, 200);
    assert!(escalations(&env, &a, "pending").await.is_empty(), "answered: nothing left to remind about");
    let all = escalations(&env, &a, "all").await;
    assert_eq!(all[0]["state"], "resolved");
    assert!(!all[0]["resolved_at"].is_null());

    // 2. question cancelled
    let q2 = ask(&env, &tid, &a, "q2").await;
    let q2id = q2["id"].as_str().unwrap().to_string();
    age_question(&env, &q2id, 7200).await;
    assert_eq!(escalations(&env, &a, "pending").await.len(), 1);
    let (s, _) = env
        .json(Method::POST, &env.url(&format!("/questions/{q2id}/cancel")), &env.owner_token, Some(&a), Some(json!({"expected_version": q2["version"]})))
        .await;
    assert_eq!(s, 200);
    assert!(escalations(&env, &a, "pending").await.is_empty());

    // 3. blocked task that is unblocked
    set_task(&env, &tid, "blocked", Some(&a.id), 7200).await;
    let pending = escalations(&env, &a, "pending").await;
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0]["hint"]["condition"], "task_blocked");
    set_task(&env, &tid, "in_progress", Some(&a.id), 7200).await;
    assert!(escalations(&env, &a, "pending").await.is_empty());
    // ...and re-blocked later: a fresh episode reminds again (generation 2).
    set_task(&env, &tid, "waiting", Some(&a.id), 3600).await;
    let again = escalations(&env, &a, "pending").await;
    assert_eq!(again.len(), 1);
    assert_eq!(again[0]["generation"], 2);
    // Handed to another owner: the old owner's reminder closes, the new owner's opens.
    set_task(&env, &tid, "waiting", Some(&b.id), 3600).await;
    assert!(escalations(&env, &a, "pending").await.is_empty());
    assert_eq!(escalations(&env, &b, "pending").await.len(), 1);

    // 4. handoff whose target expired, then cancelled
    let tid2 = new_task(&env, &a).await;
    let c = env.session(&env.owner_token, "C").await;
    let (s, h) = env
        .json(
            Method::POST,
            &env.url(&format!("/tasks/{tid2}/handoffs")),
            &env.owner_token,
            Some(&a),
            Some(json!({"client_handoff_id": "h1", "to_session_id": c.id, "summary": "s", "completed": "", "next_steps": "", "blockers": "", "evidence": []})),
        )
        .await;
    assert_eq!(s, 201, "{h}");
    assert!(escalations(&env, &a, "pending").await.iter().all(|n| n["source_type"] != "handoff"), "target is alive");
    expire_session(&env, &c.id, 7200).await;
    let pending: Vec<_> = escalations(&env, &a, "pending").await.into_iter().filter(|n| n["source_type"] == "handoff").collect();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0]["hint"]["condition"], "handoff_target_expired");
    assert_eq!(pending[0]["hint"]["target_session_id"], c.id);
    let (s, _) = env
        .json(Method::POST, &env.url(&format!("/handoffs/{}/cancel", h["id"].as_str().unwrap())), &env.owner_token, Some(&a), Some(json!({"expected_version": 1})))
        .await;
    assert_eq!(s, 200);
    assert!(escalations(&env, &a, "pending").await.iter().all(|n| n["source_type"] != "handoff"));
}

#[tokio::test]
async fn o05_switching_the_feature_off_closes_open_reminders() {
    let env = Env::new().await;
    let a = env.session(&env.owner_token, "A").await;
    let tid = new_task(&env, &a).await;
    assert_eq!(put_settings(&env, 0, json!({})).await.0, 200);
    let q = ask(&env, &tid, &a, "q1").await;
    age_question(&env, q["id"].as_str().unwrap(), 7200).await;
    assert_eq!(escalations(&env, &a, "pending").await.len(), 1);
    assert_eq!(put_settings(&env, 1, json!({"enabled": false})).await.0, 200);
    assert!(escalations(&env, &a, "pending").await.is_empty());
}

#[tokio::test]
async fn o05_the_quiet_window_defers_instead_of_dropping() {
    let env = Env::new().await;
    let a = env.session(&env.owner_token, "A").await;
    let tid = new_task(&env, &a).await;
    let q = ask(&env, &tid, &a, "q1").await;
    age_question(&env, q["id"].as_str().unwrap(), 7200).await;

    let now = Utc::now();
    let minute = (now.hour() * 60 + now.minute()) as i32;
    let start = (minute + 1440 - 120) % 1440;
    let end = (minute + 120) % 1440;
    let quiet = json!({"quiet_hours": {"start_minute": start, "end_minute": end}});
    assert_eq!(put_settings(&env, 0, quiet).await.0, 200);
    assert!(escalations(&env, &a, "all").await.is_empty(), "inside the quiet window nothing is emitted");
    let report = escalation::evaluate_repo(&env.db, &repo(&env).await, Utc::now()).await.unwrap();
    assert!(report.quiet_window && report.emitted == 0);

    // Window moved away from now: the condition still holds and fires.
    let away_start = (minute + 240) % 1440;
    let away_end = (minute + 300) % 1440;
    let moved = json!({"quiet_hours": {"start_minute": away_start, "end_minute": away_end}});
    assert_eq!(put_settings(&env, 1, moved).await.0, 200);
    assert_eq!(escalations(&env, &a, "all").await.len(), 1);
}

#[tokio::test]
async fn o05_nobody_without_access_is_notified() {
    let env = Env::new().await;
    let a = env.session(&env.owner_token, "A").await;
    let (token, uid) = env.user("writer").await;
    env.add_collaborator(uid, "write").await;
    let cs = env.session(&token, "CS").await;
    let tid = new_task(&env, &a).await;
    assert_eq!(put_settings(&env, 0, json!({})).await.0, 200);

    // The collaborator's session asks; the question goes stale.
    let (s, q) = env
        .json(
            Method::POST,
            &env.url(&format!("/tasks/{tid}/questions")),
            &token,
            Some(&cs),
            Some(json!({"client_question_id": "cq", "recipient": {"kind": "repo_writers"}, "title": "t", "body": "b"})),
        )
        .await;
    assert_eq!(s, 201, "{q}");
    let qid = q["id"].as_str().unwrap();
    age_question(&env, qid, 7200).await;
    // The owner's session asks one as well.
    let own = ask(&env, &tid, &a, "oq").await;
    age_question(&env, own["id"].as_str().unwrap(), 7200).await;

    // Access is revoked before any evaluation happens.
    let resp = env
        .send(Method::DELETE, &format!("{}/api/v1/repos/{}/{}/collaborators/{uid}", env.base, env.owner, env.repo), Some(&env.owner_token), None, None)
        .await;
    assert!(resp.status().is_success());

    // A periodic evaluation covers every session but reaches only those with access.
    let report = escalation::evaluate_repo(&env.db, &repo(&env).await, Utc::now()).await.unwrap();
    assert_eq!(report.emitted, 1, "{report:?}");
    let rows = collab_notice::Entity::find().all(&env.db).await.unwrap();
    assert!(rows.iter().all(|r| r.recipient_session_id != cs.id), "no reminder for the revoked account");
    assert!(rows.iter().any(|r| r.recipient_session_id == a.id));
    let (s, _) = env.json(Method::GET, &env.url("/notices"), &token, Some(&cs), None).await;
    assert_eq!(s, 403);
}

#[tokio::test]
async fn o05_expired_recipients_get_no_reminder() {
    let env = Env::new().await;
    let a = env.session(&env.owner_token, "A").await;
    let a2 = env.session(&env.owner_token, "successor").await;
    let tid = new_task(&env, &a).await;
    assert_eq!(put_settings(&env, 0, json!({})).await.0, 200);
    let q = ask(&env, &tid, &a, "q1").await;
    age_question(&env, q["id"].as_str().unwrap(), 7200).await;
    expire_session(&env, &a.id, 30).await;
    let report = escalation::evaluate_repo(&env.db, &repo(&env).await, Utc::now()).await.unwrap();
    assert_eq!(report.emitted, 0, "{report:?}");
    assert!(escalations(&env, &a2, "all").await.is_empty(), "nothing is passed to a successor");
}
