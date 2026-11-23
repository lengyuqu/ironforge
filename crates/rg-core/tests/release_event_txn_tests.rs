//! T17: `release.created` is written **inside** the release creation
//! transaction and only delivered after the commit (#15, step 5).
//!
//! This is the first path moved by #15 that had **no** transaction of its own.
//! The point is the same as in `pr_merge_event_txn_tests.rs`: a real creation
//! must leave exactly one `release.created` row and wake a receiver exactly
//! once — `deliver_event` must not write the log a second time. A refused
//! creation leaves no event.
//!
//! Loopback receivers need `IRONFORGE_ALLOW_LOCAL_OUTBOUND=1` for the whole
//! process, so these live in their own test binary.

use rg_core::release::service::create_release;
use rg_core::webhook::service::{create_webhook, CreateWebhookRequest, HEADER_EVENT};
use rg_db::entities::repository;
use rg_db::ops::{repo_event_ops, user_ops, webhook_ops};
use sea_orm::{ConnectionTrait, DatabaseConnection};
use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;

const OWNER: &str = "release-owner";
const REPO: &str = "release-repo";

// ── database ──────────────────────────────────────────────────────────────

/// A migrated SQLite file, built once per test binary (migrations cost seconds;
/// copying the finished file costs milliseconds).
static TEMPLATE: tokio::sync::OnceCell<std::path::PathBuf> = tokio::sync::OnceCell::const_new();

async fn migrated_template() -> &'static std::path::PathBuf {
    TEMPLATE
        .get_or_init(|| async {
            let path = std::env::temp_dir().join(format!(
                "ironforge-release-event-txn-{}.db",
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

struct Fixture {
    db: DatabaseConnection,
    _dir: tempfile::TempDir,
    repo_id: i64,
    owner_id: i64,
}

/// A fresh migrated database plus the owner and repository the release belongs
/// to.
async fn test_db() -> Fixture {
    std::env::set_var("IRONFORGE_ALLOW_LOCAL_OUTBOUND", "1");
    let template = migrated_template().await;
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("release-event-txn.db");
    std::fs::copy(template, &path).expect("copy the migrated template");
    let db = rg_db::connect_with_pool(&format!("sqlite://{}?mode=rwc", path.display()), 5, 60, 4)
        .await
        .expect("connect");

    let owner = user_ops::create_user(&db, OWNER, "release-owner@example.com", "", "Release Owner")
        .await
        .expect("create the owner");
    let now = chrono::Utc::now();
    let repo = rg_db::ops::repo_ops::create(
        &db,
        repository::ActiveModel {
            id: sea_orm::NotSet,
            owner_id: sea_orm::Set(owner.id),
            name: sea_orm::Set(REPO.to_string()),
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

    Fixture {
        db,
        _dir: dir,
        repo_id: repo.id,
        owner_id: owner.id,
    }
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

async fn subscribe(db: &DatabaseConnection, repo_id: i64, url: &str) {
    create_webhook(
        db,
        repo_id,
        &CreateWebhookRequest {
            url: url.to_string(),
            content_type: Some("json".to_string()),
            secret: None,
            active: Some(true),
            events: vec!["release.created".to_string()],
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
    repo_event_ops::list_after(db, repo_id, 0, &["release.created".to_string()], 100)
        .await
        .expect("read the event log")
        .into_iter()
        .map(|row| serde_json::from_str(row.payload.as_deref().expect("a payload")).expect("json"))
        .collect()
}

// ── the tests ─────────────────────────────────────────────────────────────

/// A real creation writes exactly one log row, delivers it once, and the log
/// and the delivery carry the payload the old `trigger_release_created`
/// published — all eight fields, matching the row in `releases`.
#[tokio::test]
async fn a_created_release_logs_and_delivers_release_created_exactly_once() {
    let fixture = test_db().await;
    let db = &fixture.db;
    let mut receiver = Receiver::spawn(200).await;
    subscribe(db, fixture.repo_id, &receiver.url()).await;

    let repo_path = fixture._dir.path().join("repo");
    std::fs::create_dir_all(&repo_path).expect("create the repository path");

    let release = create_release(
        db,
        fixture.repo_id,
        fixture.owner_id,
        "v1.0.0",
        "First Release",
        Some("release notes"),
        "main",
        false,
        true,
        &repo_path,
    )
    .await
    .expect("create the release");

    // Exactly one row: the transaction writes it once and `deliver_event` must
    // not write it again.
    let events = logged_events(db, fixture.repo_id).await;
    assert_eq!(
        events.len(),
        1,
        "one release.created row, not two: {events:?}"
    );
    let logged = &events[0];
    assert_eq!(logged["kind"], "release.created");
    assert_eq!(
        logged["data"]["event"], "release.created",
        "the business payload keeps its legacy `event` key"
    );

    // The eight legacy fields, byte-for-byte, and they agree with the DB row.
    let payload = &logged["data"]["release"];
    assert_eq!(payload["id"], release.id);
    assert_eq!(payload["repo_id"], release.repo_id);
    assert_eq!(payload["tag_name"], "v1.0.0");
    assert_eq!(payload["title"], "First Release");
    assert_eq!(payload["body"], "release notes");
    assert_eq!(payload["is_draft"], false);
    assert_eq!(payload["is_prerelease"], true);
    assert_eq!(payload["author_id"], fixture.owner_id);
    assert_eq!(
        payload.as_object().map(|object| object.len()),
        Some(8),
        "the release payload keeps exactly its eight legacy fields: {payload:?}"
    );

    // Delivery: one request, same business payload, right event header.
    let delivered = receiver.next().await;
    assert_eq!(
        delivered.header(HEADER_EVENT).as_deref(),
        Some("release.created")
    );
    let delivered = delivered.json();
    assert_eq!(delivered["kind"], "release.created");
    assert_eq!(
        delivered["data"], logged["data"],
        "delivery carries the same business payload as the log"
    );

    let hook = hook_id(db, fixture.repo_id).await;
    assert_eq!(
        wait_for_delivery(db, hook).await.len(),
        1,
        "exactly one delivery row"
    );
    assert!(
        tokio::time::timeout(
            std::time::Duration::from_millis(500),
            receiver.requests.recv()
        )
        .await
        .is_err(),
        "the event is delivered once, not twice"
    );
}

/// A creation refused before its transaction (here: the tag already exists)
/// leaves the log untouched — no event for a release that was not created.
///
/// Note: every failure point of this path is *before* the transaction (the
/// empty tag/title checks and the duplicate-tag lookup), so a failure *inside*
/// the transaction cannot be constructed from the outside; that half is covered
/// by T13's mechanism test
/// (`issue_event_txn_tests::an_event_in_a_rolled_back_transaction_is_neither_logged_nor_delivered`),
/// which exercises the very same `record_event_in_txn` rollback.
#[tokio::test]
async fn a_refused_release_writes_no_event() {
    let fixture = test_db().await;
    let db = &fixture.db;
    let repo_path = fixture._dir.path().join("repo");
    std::fs::create_dir_all(&repo_path).expect("create the repository path");

    create_release(
        db,
        fixture.repo_id,
        fixture.owner_id,
        "v1.0.0",
        "First Release",
        None,
        "main",
        false,
        false,
        &repo_path,
    )
    .await
    .expect("create the first release");
    assert_eq!(logged_events(db, fixture.repo_id).await.len(), 1);

    let refused = create_release(
        db,
        fixture.repo_id,
        fixture.owner_id,
        "v1.0.0",
        "Duplicate Release",
        None,
        "main",
        false,
        false,
        &repo_path,
    )
    .await;
    assert!(
        matches!(refused, Err(rg_core::error::CoreError::Conflict(_))),
        "a duplicate tag is a conflict: {refused:?}"
    );
    assert_eq!(
        logged_events(db, fixture.repo_id).await.len(),
        1,
        "the refused creation adds no release.created event"
    );
}
