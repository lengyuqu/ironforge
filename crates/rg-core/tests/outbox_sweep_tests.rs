//! T19: the fallback sweep that delivers events which were logged but never
//! handed to webhook delivery (#15, step 7).
//!
//! Integration tests on purpose: a redelivery really POSTs, so the binary needs
//! `IRONFORGE_ALLOW_LOCAL_OUTBOUND=1` in its own process. Keeping them together
//! means that escape hatch cannot leak into `net_guard`'s unit tests.
//!
//! The first test is the one that matters: a row that predates the column (or an
//! old row the sweep has given up on) must never be replayed, or deploying this
//! would mail every hook every event of the last week.

use chrono::{Duration, Utc};
use rg_core::webhook::outbox::{sweep, LOOKBACK_SECS, SETTLE_SECS};
use rg_core::webhook::service::{
    create_webhook, deliver_event, record_event_in_txn, trigger_event, trigger_event_quietly,
    CreateWebhookRequest, EventEnvelope,
};
use rg_db::entities::repo_event::{self, Entity as RepoEvents};
use rg_db::entities::repository;
use rg_db::ops::{repo_event_ops, user_ops, webhook_ops};
use sea_orm::sea_query::Expr;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, DatabaseConnection, EntityTrait, QueryFilter,
    Set, TransactionTrait,
};
use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;

// ── database ──────────────────────────────────────────────────────────────

/// A migrated SQLite file, built once per test binary.
static TEMPLATE: tokio::sync::OnceCell<std::path::PathBuf> = tokio::sync::OnceCell::const_new();

async fn migrated_template() -> &'static std::path::PathBuf {
    TEMPLATE
        .get_or_init(|| async {
            let path = std::env::temp_dir()
                .join(format!("ironforge-outbox-sweep-{}.db", std::process::id()));
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

/// A fresh migrated database plus an owner and a repository.
async fn test_db() -> (DatabaseConnection, tempfile::TempDir, i64, i64) {
    std::env::set_var("IRONFORGE_ALLOW_LOCAL_OUTBOUND", "1");
    let template = migrated_template().await;
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("outbox.db");
    std::fs::copy(template, &path).expect("copy the migrated template");
    let db = rg_db::connect_with_pool(&format!("sqlite://{}?mode=rwc", path.display()), 5, 60, 4)
        .await
        .expect("connect");

    let owner = user_ops::create_user(&db, "sweep-owner", "sweep@example.com", "", "Sweep Owner")
        .await
        .expect("create the owner");
    let now = Utc::now();
    let repo = rg_db::ops::repo_ops::create(
        &db,
        repository::ActiveModel {
            id: sea_orm::NotSet,
            owner_id: Set(owner.id),
            name: Set(format!("sweep-{}", uuid::Uuid::new_v4().simple())),
            description: Set(None),
            is_private: Set(false),
            default_branch: Set("main".to_string()),
            fork_id: Set(None),
            stars_count: Set(0),
            forks_count: Set(0),
            org_id: Set(None),
            created_at: Set(now),
            updated_at: Set(now),
            deleted_at: Set(None),
            origin_repo_id: Set(None),

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

/// Write a log row directly, so a test can shape exactly the state a crash
/// leaves behind: a payload, an age, and whether delivery was ever handed off.
async fn insert_event(
    db: &DatabaseConnection,
    repo_id: i64,
    event_type: &str,
    payload: Option<String>,
    created_at: chrono::DateTime<Utc>,
    dispatched_at: Option<chrono::DateTime<Utc>>,
) -> i64 {
    repo_event::ActiveModel {
        id: sea_orm::NotSet,
        repo_id: Set(repo_id),
        event_type: Set(event_type.to_string()),
        payload: Set(payload),
        created_at: Set(created_at),
        dispatched_at: Set(dispatched_at),
    }
    .insert(db)
    .await
    .expect("insert a repository event")
    .id
}

/// An envelope as the service would store it, with a known id.
fn envelope(repo_id: i64, kind: &str, data: Value) -> (uuid::Uuid, String) {
    let envelope = EventEnvelope::new(repo_id, kind, None, data);
    let id = envelope.id;
    (id, serde_json::to_string(&envelope.to_value()).unwrap())
}

async fn read_event(db: &DatabaseConnection, id: i64) -> repo_event::Model {
    RepoEvents::find_by_id(id)
        .one(db)
        .await
        .expect("read the event row")
        .expect("the event row exists")
}

/// Age a row written by the service so the sweep window can see it.
async fn age_event(db: &DatabaseConnection, id: i64, created_at: chrono::DateTime<Utc>) {
    RepoEvents::update_many()
        .col_expr(repo_event::Column::CreatedAt, Expr::value(created_at))
        .filter(repo_event::Column::Id.eq(id))
        .exec(db)
        .await
        .expect("age the event");
}

// ── a receiver we control ─────────────────────────────────────────────────

struct Received {
    body: String,
}

impl Received {
    fn json(&self) -> Value {
        serde_json::from_str(&self.body).expect("the delivered body is JSON")
    }
}

struct Receiver {
    port: u16,
    requests: mpsc::UnboundedReceiver<Received>,
    _task: tokio::task::JoinHandle<()>,
}

impl Receiver {
    async fn spawn() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind a loopback port");
        let port = listener.local_addr().unwrap().port();
        let (tx, requests) = mpsc::unbounded_channel();
        let task = tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let tx = tx.clone();
                tokio::spawn(async move {
                    let Some(body) = read_body(&mut socket).await else {
                        return;
                    };
                    let _ = tx.send(Received { body });
                    let _ = socket
                        .write_all(
                            b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
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

    /// Assert nothing arrives within a short grace period.
    async fn expect_nothing(&mut self) {
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(500), self.requests.recv())
                .await
                .is_err(),
            "no delivery was expected"
        );
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

async fn read_body(socket: &mut tokio::net::TcpStream) -> Option<String> {
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
                return Some(
                    String::from_utf8_lossy(&buffer[body_start..body_start + length]).to_string(),
                );
            }
        }
    }
}

// ── the five dangerous cases ──────────────────────────────────────────────

/// The dead-line: a `NULL` row the sweep has no business touching must not be
/// replayed. If this fails on a real deployment, every hook receives the whole
/// retained event log.
#[tokio::test]
async fn a_historical_undispatched_row_is_never_replayed() {
    let (db, _dir, repo_id, _owner_id) = test_db().await;
    let mut receiver = Receiver::spawn().await;
    subscribe(&db, repo_id, &receiver.url(), &["push"]).await;

    // Exactly what a pre-migration row looks like if the back-fill were missed:
    // no dispatched_at and ancient.
    let (_id, payload) = envelope(repo_id, "push", serde_json::json!({ "after": "abc" }));
    let event_id = insert_event(
        &db,
        repo_id,
        "push",
        Some(payload),
        Utc::now() - Duration::days(1),
        None,
    )
    .await;

    let report = sweep(&db).await.unwrap();
    assert_eq!(report.scanned, 0, "an ancient row is outside the window");
    assert_eq!(report.redelivered, 0);
    assert_eq!(report.failed, 0);

    receiver.expect_nothing().await;
    assert!(
        read_event(&db, event_id).await.dispatched_at.is_none(),
        "the sweep must not touch a row outside its window either"
    );
}

/// The crash the task exists for: committed, never handed to delivery, still
/// inside the retry window.
#[tokio::test]
async fn a_missed_event_inside_the_window_is_redelivered_once() {
    let (db, _dir, repo_id, _owner_id) = test_db().await;
    let mut receiver = Receiver::spawn().await;
    subscribe(&db, repo_id, &receiver.url(), &["push"]).await;

    let (envelope_id, payload) = envelope(repo_id, "push", serde_json::json!({ "after": "abc" }));
    let event_id = insert_event(
        &db,
        repo_id,
        "push",
        Some(payload),
        Utc::now() - Duration::minutes(5),
        None,
    )
    .await;

    let report = sweep(&db).await.unwrap();
    assert_eq!(report.scanned, 1);
    assert_eq!(report.redelivered, 1, "{report:?}");
    assert_eq!(report.failed, 0);

    // The receiver sees the stored envelope, not a re-wrapped payload.
    let delivered = receiver.next().await.json();
    assert_eq!(delivered["id"], envelope_id.to_string());
    assert_eq!(delivered["data"]["after"], "abc");

    let row = read_event(&db, event_id).await;
    assert!(
        row.dispatched_at.is_some(),
        "the sweep must mark what it handed off"
    );

    // Second pass: nothing left to do (worker idempotence).
    let report = sweep(&db).await.unwrap();
    assert_eq!(report.scanned, 0);
    assert_eq!(report.redelivered, 0);
    receiver.expect_nothing().await;
}

/// The normal path marks its row before the spawn, so the sweep finds nothing
/// even if the row is old enough for the window.
#[tokio::test]
async fn a_normally_triggered_event_is_not_repeated_by_the_sweep() {
    let (db, _dir, repo_id, _owner_id) = test_db().await;
    let mut receiver = Receiver::spawn().await;
    subscribe(&db, repo_id, &receiver.url(), &["push"]).await;

    trigger_event(&db, repo_id, "push", &serde_json::json!({ "after": "abc" }))
        .await
        .unwrap();
    receiver.next().await;

    let event_id = repo_event_ops::latest_id(&db, repo_id).await.unwrap();
    assert!(
        read_event(&db, event_id).await.dispatched_at.is_some(),
        "trigger_event must mark the row as handed off"
    );
    // Put it inside the window anyway: the mark, not the age, is what protects
    // it.
    age_event(&db, event_id, Utc::now() - Duration::minutes(5)).await;

    let report = sweep(&db).await.unwrap();
    assert_eq!(
        report.scanned, 0,
        "a dispatched row is invisible to the sweep"
    );
    assert_eq!(report.redelivered, 0);
    receiver.expect_nothing().await;
}

/// The transaction path's row is also marked, by payload, after the commit.
#[tokio::test]
async fn a_transaction_event_is_marked_after_it_is_delivered() {
    let (db, _dir, repo_id, _owner_id) = test_db().await;
    let mut receiver = Receiver::spawn().await;
    subscribe(&db, repo_id, &receiver.url(), &["issue.opened"]).await;

    let payload = serde_json::json!({
        "event": "issue.opened",
        "issue": { "id": 7, "number": 1, "title": "txn" },
    });
    let txn = db.begin().await.unwrap();
    record_event_in_txn(&txn, repo_id, "issue.opened", &payload)
        .await
        .unwrap();
    txn.commit().await.unwrap();
    deliver_event(&db, repo_id, "issue.opened", &payload)
        .await
        .unwrap();
    receiver.next().await;

    let event_id = repo_event_ops::latest_id(&db, repo_id).await.unwrap();
    assert!(
        read_event(&db, event_id).await.dispatched_at.is_some(),
        "deliver_event must identify the row the transaction wrote"
    );
    age_event(&db, event_id, Utc::now() - Duration::minutes(5)).await;
    let report = sweep(&db).await.unwrap();
    assert_eq!(report.scanned, 0, "a transaction event must not be re-sent");
    receiver.expect_nothing().await;
}

/// T11's promise: a quiet event is a decision, not a missed delivery. The sweep
/// must not turn the agent draft area's silence into webhooks.
#[tokio::test]
async fn a_quiet_event_is_never_swept() {
    let (db, _dir, repo_id, _owner_id) = test_db().await;
    let mut receiver = Receiver::spawn().await;
    subscribe(&db, repo_id, &receiver.url(), &["push"]).await;

    trigger_event_quietly(
        &db,
        repo_id,
        "push",
        &serde_json::json!({ "after": "draft" }),
    )
    .await
    .unwrap();
    let event_id = repo_event_ops::latest_id(&db, repo_id).await.unwrap();
    age_event(&db, event_id, Utc::now() - Duration::minutes(5)).await;

    let report = sweep(&db).await.unwrap();
    assert_eq!(report.scanned, 0, "a deliberately silent event is settled");
    receiver.expect_nothing().await;
}

/// Both edges of the band: too young (still in flight) and too old (for a
/// person, not a retry loop).
#[tokio::test]
async fn the_window_edges_are_excluded() {
    let (db, _dir, repo_id, _owner_id) = test_db().await;
    let mut receiver = Receiver::spawn().await;
    subscribe(&db, repo_id, &receiver.url(), &["push"]).await;

    let (_id, young) = envelope(repo_id, "push", serde_json::json!({ "after": "young" }));
    insert_event(
        &db,
        repo_id,
        "push",
        Some(young),
        Utc::now() - Duration::seconds(10),
        None,
    )
    .await;
    let (_id, old) = envelope(repo_id, "push", serde_json::json!({ "after": "old" }));
    insert_event(
        &db,
        repo_id,
        "push",
        Some(old),
        Utc::now() - Duration::seconds(LOOKBACK_SECS + 300),
        None,
    )
    .await;

    let report = sweep(&db).await.unwrap();
    assert_eq!(report.scanned, 0, "10s is too young, 20min is too old");
    assert_eq!(report.redelivered, 0);
    receiver.expect_nothing().await;

    // And the middle of the band is not: proves the two rows above were excluded
    // by their age, not by something else.
    let (_id, middle) = envelope(repo_id, "push", serde_json::json!({ "after": "mid" }));
    insert_event(
        &db,
        repo_id,
        "push",
        Some(middle),
        Utc::now() - Duration::seconds(SETTLE_SECS + 60),
        None,
    )
    .await;
    let report = sweep(&db).await.unwrap();
    assert_eq!(report.scanned, 1);
    assert_eq!(report.redelivered, 1);
    receiver.next().await;
}

/// A payload nobody can replay is settled with a warning instead of being read
/// again every minute.
#[tokio::test]
async fn an_unparseable_payload_is_settled_instead_of_looping() {
    let (db, _dir, repo_id, _owner_id) = test_db().await;
    let mut receiver = Receiver::spawn().await;
    subscribe(&db, repo_id, &receiver.url(), &["push"]).await;

    let broken = insert_event(
        &db,
        repo_id,
        "push",
        Some("not json at all".to_string()),
        Utc::now() - Duration::minutes(5),
        None,
    )
    .await;
    // The truncation stub `append_envelope` writes for an oversized event is
    // valid JSON but is not an envelope: also unreplayable.
    let truncated = insert_event(
        &db,
        repo_id,
        "push",
        Some(serde_json::json!({ "truncated": true, "original_bytes": 90000 }).to_string()),
        Utc::now() - Duration::minutes(5),
        None,
    )
    .await;

    let report = sweep(&db).await.unwrap();
    assert_eq!(report.scanned, 2);
    assert_eq!(report.redelivered, 0, "nothing sane could be sent");
    assert_eq!(report.failed, 0, "the rows were settled, not failed");
    receiver.expect_nothing().await;

    for id in [broken, truncated] {
        assert!(
            read_event(&db, id).await.dispatched_at.is_some(),
            "an unreplayable row must be marked, or it blocks every pass"
        );
    }

    let report = sweep(&db).await.unwrap();
    assert_eq!(report.scanned, 0, "settled rows leave the window");
}

/// The quiet path, a normal trigger and a transaction event must all leave
/// exactly one delivery behind — the sweep is a fallback, not a second channel.
#[tokio::test]
async fn the_sweep_never_duplicates_a_delivery_it_did_not_make() {
    let (db, _dir, repo_id, _owner_id) = test_db().await;
    let mut receiver = Receiver::spawn().await;
    subscribe(&db, repo_id, &receiver.url(), &["push"]).await;

    trigger_event(&db, repo_id, "push", &serde_json::json!({ "after": "abc" }))
        .await
        .unwrap();
    receiver.next().await;

    let hook = webhook_ops::list_by_repo(&db, repo_id)
        .await
        .unwrap()
        .first()
        .expect("a webhook is registered")
        .id;
    let before = webhook_ops::list_deliveries_by_webhook(&db, hook)
        .await
        .unwrap()
        .len();

    let event_id = repo_event_ops::latest_id(&db, repo_id).await.unwrap();
    age_event(&db, event_id, Utc::now() - Duration::minutes(5)).await;
    let report = sweep(&db).await.unwrap();
    assert_eq!(report.redelivered, 0);

    // Give a wrongly spawned delivery time to be recorded, then compare.
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let after = webhook_ops::list_deliveries_by_webhook(&db, hook)
        .await
        .unwrap()
        .len();
    assert_eq!(before, after, "no second delivery row");
    receiver.expect_nothing().await;
}
