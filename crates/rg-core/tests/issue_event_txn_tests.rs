//! T13: `issue.opened` is written **inside** the issue transaction and only
//! delivered after the commit.
//!
//! The point of these tests is the ordering, not the envelope shape (that is
//! `webhook_envelope_tests.rs`): an event recorded in a transaction that never
//! commits must leave no log row and must not wake a receiver, while a
//! committed issue must leave exactly one `issue.opened` row and one delivery.
//!
//! Loopback receivers need `IRONFORGE_ALLOW_LOCAL_OUTBOUND=1` for the whole
//! process, so these live in their own test binary.

use rg_core::issue::create_issue;
use rg_core::webhook::service::{
    create_webhook, record_event_in_txn, trigger_event, CreateWebhookRequest,
};
use rg_db::entities::repository;
use rg_db::ops::{issue_ops, repo_event_ops, user_ops, webhook_ops};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, DatabaseConnection, EntityTrait, QueryFilter,
    Set, TransactionTrait,
};
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
                "ironforge-issue-event-txn-{}.db",
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

/// A fresh migrated database plus the owner and repository issues belong to.
async fn test_db() -> (DatabaseConnection, tempfile::TempDir, i64, i64) {
    std::env::set_var("IRONFORGE_ALLOW_LOCAL_OUTBOUND", "1");
    let template = migrated_template().await;
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("issue-event-txn.db");
    std::fs::copy(template, &path).expect("copy the migrated template");
    let db = rg_db::connect_with_pool(&format!("sqlite://{}?mode=rwc", path.display()), 5, 60, 4)
        .await
        .expect("connect");

    let owner = user_ops::create_user(&db, "txn-owner", "txn-owner@example.com", "", "Txn Owner")
        .await
        .expect("create the owner");
    let now = chrono::Utc::now();
    let repo = rg_db::ops::repo_ops::create(
        &db,
        repository::ActiveModel {
            id: sea_orm::NotSet,
            owner_id: sea_orm::Set(owner.id),
            name: sea_orm::Set(format!("txn-{}", uuid::Uuid::new_v4().simple())),
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

async fn logged_events(db: &DatabaseConnection, repo_id: i64) -> Vec<Value> {
    repo_event_ops::list_after(db, repo_id, 0, &[], 100)
        .await
        .expect("read the event log")
        .into_iter()
        .map(|row| serde_json::from_str(row.payload.as_deref().expect("a payload")).expect("json"))
        .collect()
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

// ── the tests ─────────────────────────────────────────────────────────────

/// The event write really happens inside the caller's transaction: it is
/// visible on the transaction, and when the transaction is rolled back because a
/// later business write failed, neither the log nor a receiver ever sees it.
///
/// Before T13 this test cannot even be written: the only event writer took a
/// `&DatabaseConnection`, so an event could be recorded only *after* the
/// business commit — the crash window the issue is about.
#[tokio::test]
async fn an_event_in_a_rolled_back_transaction_is_neither_logged_nor_delivered() {
    let (db, _dir, repo_id, owner_id) = test_db().await;
    let mut receiver = Receiver::spawn(200).await;
    subscribe(&db, repo_id, &receiver.url(), &["issue.opened"]).await;

    let number = issue_ops::next_number(&db, repo_id).await.unwrap();
    let txn = db.begin().await.expect("begin the issue transaction");

    let issue = rg_db::entities::issue::ActiveModel {
        id: sea_orm::NotSet,
        repo_id: Set(repo_id),
        number: Set(number),
        title: Set("rolled back".to_string()),
        body: Set(None),
        state: Set("open".to_string()),
        author_id: Set(owner_id),
        assignee_id: Set(None),
        milestone_id: Set(None),
        labels: Set(None),
        created_at: Set(chrono::Utc::now()),
        updated_at: Set(chrono::Utc::now()),
        closed_at: Set(None),
        deleted_at: Set(None),
    }
    .insert(&txn)
    .await
    .expect("insert the issue inside the transaction");

    let payload = serde_json::json!({
        "id": issue.id,
        "repo_id": issue.repo_id,
        "number": issue.number,
        "title": issue.title,
        "state": issue.state,
        "author_id": issue.author_id,
    });
    record_event_in_txn(&txn, repo_id, "issue.opened", &payload)
        .await
        .expect("record the event inside the transaction");

    // The event is already in the transaction's view of the log — this is what
    // makes it commit or roll back together with the issue.
    let inside: Vec<rg_db::entities::repo_event::Model> =
        rg_db::entities::repo_event::Entity::find()
            .filter(rg_db::entities::repo_event::Column::RepoId.eq(repo_id))
            .all(&txn)
            .await
            .expect("read the log inside the transaction");
    assert_eq!(inside.len(), 1, "the event was written on the transaction");

    // A business write fails after the event write and before the commit.
    let failed = txn
        .execute_unprepared("INSERT INTO repo_events (repo_id, event_type) VALUES (NULL, 'boom')")
        .await;
    assert!(failed.is_err(), "the injected write must fail");
    drop(txn); // never committed → the issue and its event are rolled back

    // Nothing survives: no issue row, no event row.
    let issues = rg_db::entities::issue::Entity::find()
        .filter(rg_db::entities::issue::Column::RepoId.eq(repo_id))
        .all(&db)
        .await
        .expect("read issues back");
    assert!(issues.is_empty(), "the issue was rolled back");
    assert!(
        logged_events(&db, repo_id).await.is_empty(),
        "a rolled back transaction leaves no event in the log"
    );

    // And no receiver was woken: delivery only happens after the commit.
    assert!(
        tokio::time::timeout(
            std::time::Duration::from_millis(500),
            receiver.requests.recv()
        )
        .await
        .is_err(),
        "an event from a rolled back transaction must not be delivered"
    );
}

/// The committed path: one issue → exactly one `issue.opened` row, one delivery,
/// and the cursor moves forward by exactly one.
#[tokio::test]
async fn a_committed_issue_logs_and_delivers_issue_opened_exactly_once() {
    let (db, _dir, repo_id, owner_id) = test_db().await;
    let mut receiver = Receiver::spawn(200).await;
    subscribe(&db, repo_id, &receiver.url(), &["issue.opened"]).await;
    let before = repo_event_ops::latest_id(&db, repo_id).await.unwrap();

    let issue = create_issue(
        &db,
        repo_id,
        owner_id,
        "first".to_string(),
        None,
        None,
        None,
    )
    .await
    .expect("create the issue");

    let events = logged_events(&db, repo_id).await;
    assert_eq!(events.len(), 1, "one issue.opened row, not two: {events:?}");
    let logged = &events[0];
    assert_eq!(logged["kind"], "issue.opened");
    assert_eq!(logged["data"]["issue"]["id"], issue.id);
    assert_eq!(logged["data"]["issue"]["title"], "first");
    // The actor is resolved on the transaction connection as well.
    assert_eq!(logged["actor"]["id"], owner_id);

    let delivered = receiver.next().await;
    assert_eq!(
        delivered
            .header(rg_core::webhook::service::HEADER_EVENT)
            .as_deref(),
        Some("issue.opened")
    );
    let delivered = delivered.json();
    assert_eq!(delivered["kind"], "issue.opened");
    assert_eq!(
        delivered["data"], logged["data"],
        "delivery carries the same business payload"
    );

    let hook = hook_id(&db, repo_id).await;
    assert_eq!(
        wait_for_delivery(&db, hook).await.len(),
        1,
        "exactly one delivery row"
    );

    // The cursor is the id of that one event and moves forward once more.
    let rows = repo_event_ops::list_after(&db, repo_id, 0, &[], 100)
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert!(rows[0].id > before, "the cursor advanced by one event");
    assert_eq!(
        repo_event_ops::latest_id(&db, repo_id).await.unwrap(),
        rows[0].id
    );

    let _second = create_issue(
        &db,
        repo_id,
        owner_id,
        "second".to_string(),
        None,
        None,
        None,
    )
    .await
    .expect("create the second issue");
    let rows = repo_event_ops::list_after(&db, repo_id, 0, &[], 100)
        .await
        .unwrap();
    assert_eq!(rows.len(), 2, "one row per issue, no duplicate writes");
    assert!(rows[1].id > rows[0].id, "the cursor is monotonic");
}

/// Regression guard for the `trigger_event` split: the `trigger_*` sugar
/// functions still write one row and deliver the *same* envelope (same id), i.e.
/// splitting the log write from the delivery changed nothing for them.
#[tokio::test]
async fn trigger_event_still_writes_one_row_and_delivers_the_same_envelope() {
    let (db, _dir, repo_id, owner_id) = test_db().await;
    let mut receiver = Receiver::spawn(200).await;
    subscribe(&db, repo_id, &receiver.url(), &["push"]).await;

    trigger_event(
        &db,
        repo_id,
        "push",
        &serde_json::json!({ "after": "abc", "pushed_by": owner_id }),
    )
    .await
    .expect("trigger the push event");

    let delivered = receiver.next().await.json();
    let events = logged_events(&db, repo_id).await;
    assert_eq!(events.len(), 1, "one log row: {events:?}");
    assert_eq!(
        events[0], delivered,
        "the log and the delivery share one envelope"
    );
}
