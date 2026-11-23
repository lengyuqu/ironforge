//! R3-P1: the event envelope, the delivery headers, and the "5xx is not a
//! successful delivery" fix.
//!
//! These are integration tests on purpose: they deliver to a loopback receiver,
//! which needs `IRONFORGE_ALLOW_LOCAL_OUTBOUND=1` for the whole process. Keeping
//! them in their own binary means that escape hatch cannot leak into the
//! `net_guard` unit tests, which assert loopback *is* rejected.

use rg_core::webhook::service::{
    create_webhook, deliver_retry_job, looks_like_envelope, redeliver, trigger_event,
    trigger_event_quietly, trigger_tag_created, CreateWebhookRequest, EVENT_SCHEMA_VERSION,
    HEADER_DELIVERY, HEADER_EVENT,
};
use rg_db::entities::background_job::status;
use rg_db::entities::repository;
use rg_db::ops::{background_job_ops, repo_event_ops, user_ops, webhook_ops};
use sea_orm::{ConnectionTrait, DatabaseConnection};
use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;

// ── database ──────────────────────────────────────────────────────────────

/// A migrated SQLite file, built once per test binary (migrations cost seconds;
/// copying the finished file costs milliseconds).
static TEMPLATE: tokio::sync::OnceCell<std::path::PathBuf> = tokio::sync::OnceCell::const_new();

async fn migrated_template() -> &'static std::path::PathBuf {
    TEMPLATE
        .get_or_init(|| async {
            let path = std::env::temp_dir().join(format!(
                "ironforge-webhook-envelope-{}.db",
                std::process::id()
            ));
            let _ = std::fs::remove_file(&path);
            let db = rg_db::connect_with_pool(
                &format!("sqlite://{}?mode=rwc", path.display()),
                5,
                60,
                1,
            )
            .await
            .expect("connect to the template database");
            rg_db::run_migrations(&db).await.expect("migrations");
            db.execute_unprepared("PRAGMA wal_checkpoint(TRUNCATE)")
                .await
                .expect("checkpoint");
            db.close().await.expect("close");
            for suffix in ["-wal", "-shm"] {
                let mut sidecar = path.clone().into_os_string();
                sidecar.push(suffix);
                let _ = std::fs::remove_file(sidecar);
            }
            path
        })
        .await
}

/// A fresh migrated database plus the owner and repository events belong to.
/// Returns the repository id and the owner's user id.
async fn test_db() -> (DatabaseConnection, tempfile::TempDir, i64, i64) {
    // Tests deliver to a loopback receiver; the outbound guard blocks loopback by
    // default (this is this binary's own process, see the module docs).
    std::env::set_var("IRONFORGE_ALLOW_LOCAL_OUTBOUND", "1");
    let template = migrated_template().await;
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("webhook.db");
    std::fs::copy(template, &path).expect("copy the migrated template");
    let db = rg_db::connect_with_pool(&format!("sqlite://{}?mode=rwc", path.display()), 5, 60, 4)
        .await
        .expect("connect");

    let owner = user_ops::create_user(
        &db,
        "hook-owner",
        "hook-owner@example.com",
        "",
        "Hook Owner",
    )
    .await
    .expect("create the owner");
    let now = chrono::Utc::now();
    let repo = rg_db::ops::repo_ops::create(
        &db,
        repository::ActiveModel {
            id: sea_orm::NotSet,
            owner_id: sea_orm::Set(owner.id),
            name: sea_orm::Set(format!("hooks-{}", uuid::Uuid::new_v4().simple())),
            description: sea_orm::Set(None),
            is_private: sea_orm::Set(false),
            default_branch: sea_orm::Set("main".to_string()),
            fork_id: sea_orm::Set(None),
            stars_count: sea_orm::Set(0),
            forks_count: sea_orm::Set(0),
            org_id: sea_orm::Set(None),
            created_at: sea_orm::Set(now),
            updated_at: sea_orm::Set(now),
            deleted_at: sea_orm::Set(None),
            origin_repo_id: sea_orm::Set(None),

            is_template: sea_orm::ActiveValue::Set(false),
        },
    )
    .await
    .expect("create the repository");
    (db, dir, repo.id, owner.id)
}

async fn subscribe(db: &DatabaseConnection, repo_id: i64, url: &str, events: &[&str]) {
    create_webhook(
        db,
        repo_id,
        &CreateWebhookRequest {
            url: url.to_string(),
            content_type: Some("json".to_string()),
            secret: None,
            active: Some(true),
            events: events.iter().map(|event| event.to_string()).collect(),
        },
    )
    .await
    .expect("register the webhook");
}

async fn hook_id(db: &DatabaseConnection, repo_id: i64) -> i64 {
    webhook_ops::list_by_repo(db, repo_id)
        .await
        .unwrap()
        .first()
        .expect("a webhook is registered")
        .id
}

/// Wait (bounded) for a queued `webhook.deliver` job; empty when none appears.
async fn retry_jobs(db: &DatabaseConnection) -> Vec<rg_db::entities::background_job::Model> {
    for _ in 0..100 {
        let pending = background_job_ops::list_by_status(db, status::PENDING, 20)
            .await
            .expect("list pending jobs");
        let webhook_jobs: Vec<_> = pending
            .into_iter()
            .filter(|job| job.task_type == rg_core::background_jobs::TASK_WEBHOOK_DELIVER)
            .collect();
        if !webhook_jobs.is_empty() {
            return webhook_jobs;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    Vec::new()
}

/// Wait (bounded) for the delivery record of an attempt. The attempt is recorded
/// after the retry is queued, so a test that saw the job may not see the row yet.
async fn wait_for_delivery(
    db: &DatabaseConnection,
    hook_id: i64,
) -> Vec<rg_db::entities::webhook_delivery::Model> {
    for _ in 0..100 {
        let deliveries = webhook_ops::list_deliveries_by_webhook(db, hook_id)
            .await
            .expect("list deliveries");
        if !deliveries.is_empty() {
            return deliveries;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    Vec::new()
}

// ── a receiver we control ─────────────────────────────────────────────────

struct Received {
    head: String,
    body: String,
}

impl Received {
    fn header(&self, name: &str) -> Option<String> {
        self.head.lines().find_map(|line| {
            let (key, value) = line.split_once(':')?;
            key.eq_ignore_ascii_case(name)
                .then(|| value.trim().to_string())
        })
    }

    fn json(&self) -> Value {
        serde_json::from_str(&self.body).expect("the delivered body is JSON")
    }
}

/// A loopback HTTP receiver that answers every request with `status` and
/// forwards what it received.
struct Receiver {
    port: u16,
    requests: mpsc::UnboundedReceiver<Received>,
    _task: tokio::task::JoinHandle<()>,
}

impl Receiver {
    async fn spawn(status: u16) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind a loopback port");
        let port = listener.local_addr().unwrap().port();
        let (tx, requests) = mpsc::unbounded_channel();
        let task = tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let tx = tx.clone();
                tokio::spawn(async move {
                    let Some(received) = read_request(&mut socket).await else {
                        return;
                    };
                    let _ = tx.send(received);
                    let _ = socket
                        .write_all(
                            format!(
                                "HTTP/1.1 {status} Receiver\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                            )
                            .as_bytes(),
                        )
                        .await;
                });
            }
        });
        Self {
            port,
            requests,
            _task: task,
        }
    }

    fn url(&self) -> String {
        format!("http://127.0.0.1:{}/hook", self.port)
    }

    async fn next(&mut self) -> Received {
        tokio::time::timeout(std::time::Duration::from_secs(10), self.requests.recv())
            .await
            .expect("a delivery arrived in time")
            .expect("the receiver is still alive")
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

async fn read_request(socket: &mut tokio::net::TcpStream) -> Option<Received> {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let read = socket.read(&mut chunk).await.ok()?;
        if read == 0 {
            return None;
        }
        buffer.extend_from_slice(&chunk[..read]);
        if let Some(end) = find(&buffer, b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&buffer[..end]).to_string();
            let length = head
                .lines()
                .find_map(|line| {
                    let (key, value) = line.split_once(':')?;
                    key.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().ok())?
                })
                .unwrap_or(0);
            let body_start = end + 4;
            if buffer.len() >= body_start + length {
                let body =
                    String::from_utf8_lossy(&buffer[body_start..body_start + length]).to_string();
                return Some(Received { head, body });
            }
        }
    }
}

/// The envelope every event must carry, checked field by field.
fn assert_envelope(body: &Value, kind: &str, repo_id: i64) {
    assert_eq!(body["schema"], EVENT_SCHEMA_VERSION, "schema: {body}");
    assert!(
        uuid::Uuid::parse_str(body["id"].as_str().expect("id is a string")).is_ok(),
        "id is a UUID: {body}"
    );
    assert_eq!(body["kind"], kind, "kind: {body}");
    assert!(
        chrono::DateTime::parse_from_rfc3339(body["at"].as_str().expect("at is a string")).is_ok(),
        "at is RFC 3339: {body}"
    );
    assert_eq!(body["repo_id"], repo_id, "repo_id: {body}");
    assert!(body["actor"].is_object(), "actor is resolved: {body}");
    assert!(body["actor"]["id"].is_i64(), "actor.id: {body}");
    assert!(body["data"].is_object(), "data is the payload: {body}");
}

// ── envelope ──────────────────────────────────────────────────────────────

#[tokio::test]
async fn webhook_push_event_carries_the_envelope_and_the_old_fields_in_data() {
    let (db, _dir, repo_id, owner_id) = test_db().await;
    let mut receiver = Receiver::spawn(200).await;
    subscribe(&db, repo_id, &receiver.url(), &["push"]).await;

    let payload = serde_json::json!({
        "event": "push",
        "ref": "refs/heads/main",
        "before": "0000",
        "after": "abc",
        "pushed_by": owner_id,
        "repository": { "owner": "hook-owner", "name": "repo" },
    });
    trigger_event(&db, repo_id, "push", &payload).await.unwrap();

    let request = receiver.next().await;
    assert_eq!(request.header(HEADER_EVENT).as_deref(), Some("push"));
    let delivery_header = request.header(HEADER_DELIVERY).expect("delivery header");
    assert!(uuid::Uuid::parse_str(&delivery_header).is_ok());

    let body = request.json();
    assert_envelope(&body, "push", repo_id);
    // The old fields are untouched, and the actor came from `pushed_by`.
    assert_eq!(body["data"], payload);
    assert_eq!(body["actor"]["username"], "hook-owner");
    assert_eq!(body["actor"]["kind"], "human");
}

#[tokio::test]
async fn webhook_pull_request_event_carries_the_envelope_and_the_old_fields_in_data() {
    let (db, _dir, repo_id, owner_id) = test_db().await;
    let mut receiver = Receiver::spawn(200).await;
    subscribe(&db, repo_id, &receiver.url(), &["pull_request.opened"]).await;

    let pr = serde_json::json!({
        "id": 1,
        "number": 7,
        "title": "Envelope",
        "author_id": owner_id,
    });
    // `trigger_pr_opened` was removed with #15, step 6: the creation path now
    // writes this log row inside its transaction. The envelope shape is still
    // worth pinning, so publish the same business payload the helper built.
    let payload = serde_json::json!({
        "event": "pull_request.opened",
        "pull_request": pr,
    });
    trigger_event(&db, repo_id, "pull_request.opened", &payload)
        .await
        .unwrap();

    let body = receiver.next().await.json();
    assert_envelope(&body, "pull_request.opened", repo_id);
    assert_eq!(body["data"]["event"], "pull_request.opened");
    assert_eq!(body["data"]["pull_request"], pr);
    // The account sits one level down: `pull_request.author_id`.
    assert_eq!(body["actor"]["id"], owner_id);
}

#[tokio::test]
async fn webhook_release_event_carries_the_envelope_and_the_old_fields_in_data() {
    let (db, _dir, repo_id, owner_id) = test_db().await;
    let mut receiver = Receiver::spawn(200).await;
    subscribe(&db, repo_id, &receiver.url(), &["release.created"]).await;

    let release = serde_json::json!({ "tag_name": "v1.0.0", "author_id": owner_id });
    // `trigger_release_created` was removed with #15, step 5: the release path
    // now writes this log row inside its creation transaction. The envelope
    // shape is still worth pinning, so publish the same business payload the
    // helper used to build.
    let payload = serde_json::json!({
        "event": "release.created",
        "release": release.clone(),
    });
    trigger_event(&db, repo_id, "release.created", &payload)
        .await
        .unwrap();

    let body = receiver.next().await.json();
    assert_envelope(&body, "release.created", repo_id);
    assert_eq!(body["data"]["release"], release);
}

#[tokio::test]
async fn webhook_event_log_holds_the_same_envelope_that_was_delivered() {
    let (db, _dir, repo_id, _owner_id) = test_db().await;
    let mut receiver = Receiver::spawn(200).await;
    subscribe(&db, repo_id, &receiver.url(), &["tag.created"]).await;

    trigger_tag_created(&db, repo_id, "v1").await.unwrap();
    let body = receiver.next().await.json();

    let rows = repo_event_ops::list_after(&db, repo_id, 0, &[], 10)
        .await
        .unwrap();
    let latest = rows.last().expect("the event was logged");
    let logged: Value = serde_json::from_str(latest.payload.as_deref().unwrap()).unwrap();
    assert_eq!(
        logged, body,
        "one envelope shared by the event log and the delivery"
    );
}

/// The stored payload can be read back as an envelope — the repo_events reader
/// relies on this to serve rows written before and after the envelope existed.
#[tokio::test]
async fn webhook_stored_payload_is_an_envelope_a_reader_can_unwrap() {
    let (db, _dir, repo_id, owner_id) = test_db().await;
    let mut receiver = Receiver::spawn(200).await;
    subscribe(&db, repo_id, &receiver.url(), &["push"]).await;

    let payload = serde_json::json!({ "after": "abc", "pushed_by": owner_id });
    trigger_event(&db, repo_id, "push", &payload).await.unwrap();
    receiver.next().await;

    let rows = repo_event_ops::list_after(&db, repo_id, 0, &[], 10)
        .await
        .unwrap();
    let stored: Value =
        serde_json::from_str(rows.last().unwrap().payload.as_deref().unwrap()).unwrap();
    assert!(looks_like_envelope(&stored));
    assert_eq!(stored["data"], payload);
}

/// T11: the quiet path records the same envelope and delivers nothing.
///
/// The agent draft area relies on exactly this split: the agent waits for its
/// own push in `repo_events`, while no receiver, retry job or delivery row is
/// created for it.
#[tokio::test]
async fn a_quiet_event_is_logged_but_never_delivered() {
    let (db, _dir, repo_id, owner_id) = test_db().await;
    let mut receiver = Receiver::spawn(200).await;
    subscribe(&db, repo_id, &receiver.url(), &["push"]).await;

    let payload = serde_json::json!({ "after": "abc", "pushed_by": owner_id });
    trigger_event_quietly(&db, repo_id, "push", &payload)
        .await
        .unwrap();

    let rows = repo_event_ops::list_after(&db, repo_id, 0, &[], 10)
        .await
        .unwrap();
    let stored: Value = serde_json::from_str(
        rows.last()
            .expect("the event was logged")
            .payload
            .as_deref()
            .unwrap(),
    )
    .unwrap();
    assert!(
        looks_like_envelope(&stored),
        "the quiet path writes the same envelope shape: {stored}"
    );
    assert_eq!(stored["data"], payload);
    assert_eq!(stored["kind"], "push");

    // Nothing was delivered, nothing was queued: no receiver request, no
    // delivery row, no durable retry.
    assert!(
        tokio::time::timeout(
            std::time::Duration::from_millis(400),
            receiver.requests.recv()
        )
        .await
        .is_err(),
        "a quiet event must not reach the receiver"
    );
    let hook = hook_id(&db, repo_id).await;
    assert!(
        webhook_ops::list_deliveries_by_webhook(&db, hook)
            .await
            .unwrap()
            .is_empty(),
        "a quiet event must not create a delivery row"
    );
    assert!(
        retry_jobs(&db).await.is_empty(),
        "a quiet event must not queue a retry"
    );
}

// ── receiver status → retry ───────────────────────────────────────────────

#[tokio::test]
async fn webhook_5xx_receiver_is_queued_for_retry_instead_of_counted_as_delivered() {
    let (db, _dir, repo_id, _owner_id) = test_db().await;
    let mut receiver = Receiver::spawn(503).await;
    subscribe(&db, repo_id, &receiver.url(), &["push"]).await;

    trigger_event(&db, repo_id, "push", &serde_json::json!({ "after": "abc" }))
        .await
        .unwrap();
    receiver.next().await;
    let jobs = retry_jobs(&db).await;
    assert!(!jobs.is_empty(), "a 503 must enqueue a durable retry");

    let hook = hook_id(&db, repo_id).await;
    let deliveries = wait_for_delivery(&db, hook).await;
    let latest = deliveries.first().expect("the attempt was recorded");
    assert_eq!(latest.response_status, Some(503));
    assert!(latest
        .response_body
        .as_deref()
        .unwrap()
        .contains("retryable"));

    // The queued retry keeps the delivered envelope, so the receiver sees the
    // same event again rather than a body it has to re-parse into a new shape.
    let job: Value = serde_json::from_str(&jobs[0].payload).unwrap();
    let body: Value = serde_json::from_str(job["payload"].as_str().unwrap()).unwrap();
    assert!(looks_like_envelope(&body), "the retry carries the envelope");
}

#[tokio::test]
async fn webhook_429_receiver_is_queued_for_retry() {
    let (db, _dir, repo_id, _owner_id) = test_db().await;
    let mut receiver = Receiver::spawn(429).await;
    subscribe(&db, repo_id, &receiver.url(), &["push"]).await;

    trigger_event(&db, repo_id, "push", &serde_json::json!({ "after": "abc" }))
        .await
        .unwrap();
    receiver.next().await;
    assert!(!retry_jobs(&db).await.is_empty(), "a 429 must retry");
}

#[tokio::test]
async fn webhook_4xx_receiver_is_a_final_answer_and_not_retried() {
    let (db, _dir, repo_id, _owner_id) = test_db().await;
    let mut receiver = Receiver::spawn(404).await;
    subscribe(&db, repo_id, &receiver.url(), &["push"]).await;

    trigger_event(&db, repo_id, "push", &serde_json::json!({ "after": "abc" }))
        .await
        .unwrap();
    receiver.next().await;
    assert!(
        retry_jobs(&db).await.is_empty(),
        "a 404 is the receiver's final answer"
    );

    let hook = hook_id(&db, repo_id).await;
    let deliveries = wait_for_delivery(&db, hook).await;
    assert_eq!(deliveries.first().unwrap().response_status, Some(404));
}

#[tokio::test]
async fn webhook_retry_meeting_a_5xx_fails_so_the_queue_backs_off() {
    let (db, _dir, repo_id, _owner_id) = test_db().await;
    let mut receiver = Receiver::spawn(500).await;
    subscribe(&db, repo_id, &receiver.url(), &["push"]).await;
    let hook = hook_id(&db, repo_id).await;

    let job = serde_json::json!({ "webhook_id": hook, "event": "push", "payload": "{}" });
    let result = deliver_retry_job(&db, &job).await;
    receiver.next().await;
    assert!(result.is_err(), "a 500 must reschedule the job");

    let deliveries = webhook_ops::list_deliveries_by_webhook(&db, hook)
        .await
        .unwrap();
    assert_eq!(deliveries.first().unwrap().response_status, Some(500));
}

#[tokio::test]
async fn webhook_retry_meeting_a_4xx_succeeds_without_another_attempt() {
    let (db, _dir, repo_id, _owner_id) = test_db().await;
    let mut receiver = Receiver::spawn(400).await;
    subscribe(&db, repo_id, &receiver.url(), &["push"]).await;
    let hook = hook_id(&db, repo_id).await;

    let job = serde_json::json!({ "webhook_id": hook, "event": "push", "payload": "{}" });
    assert!(
        deliver_retry_job(&db, &job).await.is_ok(),
        "a 400 is a final answer, not a transient failure"
    );
    receiver.next().await;
}

// ── redelivery ────────────────────────────────────────────────────────────

#[tokio::test]
async fn webhook_redelivery_reuses_the_envelope_instead_of_wrapping_it_twice() {
    let (db, _dir, repo_id, _owner_id) = test_db().await;
    let mut receiver = Receiver::spawn(200).await;
    subscribe(&db, repo_id, &receiver.url(), &["push"]).await;

    trigger_event(&db, repo_id, "push", &serde_json::json!({ "after": "abc" }))
        .await
        .unwrap();
    let first = receiver.next().await.json();

    let hook = hook_id(&db, repo_id).await;
    let delivery = wait_for_delivery(&db, hook)
        .await
        .first()
        .expect("the first attempt was recorded")
        .clone();
    redeliver(&db, delivery.id).await.unwrap();

    let replay = receiver.next().await.json();
    assert_eq!(replay["id"], first["id"], "the event keeps its identity");
    assert!(
        replay["data"].get("data").is_none(),
        "no second envelope layer: {replay}"
    );
    assert_eq!(replay["data"]["after"], "abc");
}
