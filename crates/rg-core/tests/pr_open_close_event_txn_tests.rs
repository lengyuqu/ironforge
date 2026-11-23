//! T18: `pull_request.opened` is written inside the PR creation transaction and
//! `pull_request.closed` is written inside the update transaction — *after* the
//! update, not before it (#15, step 6).
//!
//! The interesting test here is not "a close delivers an event" (that passed
//! before the change too). It is [`an_update_that_fails_publishes_no_pull_request_closed`]:
//! a database trigger makes the state update fail, and no `pull_request.closed`
//! row and no delivery may exist afterwards. With the old ordering the event was
//! published before `update`, so that test fails on the pre-change code.
//!
//! Loopback receivers need `IRONFORGE_ALLOW_LOCAL_OUTBOUND=1` for the whole
//! process, so these live in their own test binary.

use rg_core::pull_request::{create_pr, update_pr};
use rg_core::webhook::service::{create_webhook, CreateWebhookRequest, HEADER_EVENT};
use rg_db::entities::{pull_request, repository};
use rg_db::ops::{pr_event_ops, repo_event_ops, user_ops, webhook_ops};
use sea_orm::{ActiveModelTrait, ConnectionTrait, DatabaseConnection, EntityTrait, Set};
use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;

const OWNER: &str = "pr-event-owner";
const REPO: &str = "pr-event-repo";

// ── database ──────────────────────────────────────────────────────────────

/// A migrated SQLite file, built once per test binary (migrations cost seconds;
/// copying the finished file costs milliseconds).
static TEMPLATE: tokio::sync::OnceCell<std::path::PathBuf> = tokio::sync::OnceCell::const_new();

async fn migrated_template() -> &'static std::path::PathBuf {
    TEMPLATE
        .get_or_init(|| async {
            let path = std::env::temp_dir()
                .join(format!("ironforge-pr-event-txn-{}.db", std::process::id()));
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
    repo_root: std::path::PathBuf,
    repo_id: i64,
    owner_id: i64,
}

/// A fresh migrated database, the owner/repository the PR belongs to, and the
/// directory the repository's bare git directory would live under (absent here:
/// `create_pr` tolerates an unresolvable head).
async fn test_db() -> Fixture {
    std::env::set_var("IRONFORGE_ALLOW_LOCAL_OUTBOUND", "1");
    let template = migrated_template().await;
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("pr-event-txn.db");
    std::fs::copy(template, &path).expect("copy the migrated template");
    let db = rg_db::connect_with_pool(&format!("sqlite://{}?mode=rwc", path.display()), 5, 60, 4)
        .await
        .expect("connect");
    let repo_root = dir.path().join("repos");
    std::fs::create_dir_all(&repo_root).expect("create the repository root");

    let owner = user_ops::create_user(&db, OWNER, "pr-event@example.com", "", "PR Event Owner")
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
        repo_root,
        repo_id: repo.id,
        owner_id: owner.id,
    }
}

/// Insert an open PR numbered `number` without going through `create_pr`, so a
/// test can look at the update path alone.
async fn insert_open_pr(db: &DatabaseConnection, repo_id: i64, author_id: i64) -> i64 {
    let now = chrono::Utc::now();
    let pr = pull_request::ActiveModel {
        id: sea_orm::NotSet,
        repo_id: Set(repo_id),
        number: Set(1),
        title: Set("Close me".to_string()),
        body: Set(None),
        state: Set("open".to_string()),
        is_draft: Set(false),
        auto_merge_enabled: Set(false),
        auto_merge_strategy: Set(None),
        auto_merge_enabled_by_id: Set(None),
        auto_merge_enabled_at: Set(None),
        author_id: Set(author_id),
        reviewer_id: Set(None),
        head_branch: Set("feature".to_string()),
        base_branch: Set("main".to_string()),
        head_sha: Set(None),
        merge_strategy: Set(None),
        merge_commit_sha: Set(None),
        head_repo_id: Set(None),
        milestone_id: Set(None),
        labels: Set(None),
        created_at: Set(now),
        updated_at: Set(now),
        closed_at: Set(None),
        merged_at: Set(None),
    }
    .insert(db)
    .await
    .expect("insert the pull request");
    pr.id
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

    /// True when nothing arrives within `millis`.
    async fn is_quiet_for(&mut self, millis: u64) -> bool {
        tokio::time::timeout(
            std::time::Duration::from_millis(millis),
            self.requests.recv(),
        )
        .await
        .is_err()
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

async fn subscribe(db: &DatabaseConnection, repo_id: i64, url: &str, event: &str) {
    create_webhook(
        db,
        repo_id,
        &CreateWebhookRequest {
            url: url.to_string(),
            content_type: Some("json".to_string()),
            secret: None,
            active: Some(true),
            events: vec![event.to_string()],
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

async fn logged_events(db: &DatabaseConnection, repo_id: i64, kind: &str) -> Vec<Value> {
    repo_event_ops::list_after(db, repo_id, 0, &[kind.to_string()], 100)
        .await
        .expect("read the event log")
        .into_iter()
        .map(|row| serde_json::from_str(row.payload.as_deref().expect("a payload")).expect("json"))
        .collect()
}

// ── the tests ─────────────────────────────────────────────────────────────

/// Opening a PR writes exactly one log row inside the creation transaction and
/// delivers it once, with the same nine fields `trigger_pr_opened` published.
#[tokio::test]
async fn a_created_pr_logs_and_delivers_pull_request_opened_exactly_once() {
    let fixture = test_db().await;
    let db = &fixture.db;
    let mut receiver = Receiver::spawn(200).await;
    subscribe(db, fixture.repo_id, &receiver.url(), "pull_request.opened").await;

    let pr = create_pr(
        db,
        &fixture.repo_root,
        fixture.repo_id,
        fixture.owner_id,
        "Open me".to_string(),
        Some("body".to_string()),
        "feature".to_string(),
        "main".to_string(),
        None,
        false,
        None,
    )
    .await
    .expect("create the pull request");

    let events = logged_events(db, fixture.repo_id, "pull_request.opened").await;
    assert_eq!(
        events.len(),
        1,
        "one pull_request.opened row, not two: {events:?}"
    );
    let logged = &events[0];
    assert_eq!(logged["kind"], "pull_request.opened");
    assert_eq!(logged["data"]["event"], "pull_request.opened");
    let pr_payload = &logged["data"]["pull_request"];
    assert_eq!(pr_payload["id"], pr.id);
    assert_eq!(pr_payload["repo_id"], fixture.repo_id);
    assert_eq!(pr_payload["number"], 1);
    assert_eq!(pr_payload["title"], "Open me");
    assert_eq!(pr_payload["state"], "open");
    assert_eq!(pr_payload["head_branch"], "feature");
    assert_eq!(pr_payload["base_branch"], "main");
    assert_eq!(pr_payload["head_repo_id"], Value::Null);
    assert_eq!(pr_payload["author_id"], fixture.owner_id);
    assert_eq!(
        pr_payload.as_object().expect("an object").len(),
        9,
        "the opened payload keeps its nine fields: {pr_payload}"
    );

    let delivered = receiver.next().await;
    assert_eq!(
        delivered.header(HEADER_EVENT).as_deref(),
        Some("pull_request.opened")
    );
    let delivered = delivered.json();
    assert_eq!(delivered["kind"], "pull_request.opened");
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
        receiver.is_quiet_for(500).await,
        "the event is delivered once, not twice"
    );

    // The timeline entry is in the same transaction, so it exists too.
    assert_eq!(
        pr_event_ops::list_by_pr(db, pr.id).await.unwrap().len(),
        1,
        "one pull_request_opened timeline entry"
    );
}

/// Closing an open PR writes exactly one `pull_request.closed` row and delivers
/// it once, with the same five fields the old helper published.
#[tokio::test]
async fn a_closed_pr_logs_and_delivers_pull_request_closed_exactly_once() {
    let fixture = test_db().await;
    let db = &fixture.db;
    let mut receiver = Receiver::spawn(200).await;
    subscribe(db, fixture.repo_id, &receiver.url(), "pull_request.closed").await;

    let pr_id = insert_open_pr(db, fixture.repo_id, fixture.owner_id).await;

    let closed = update_pr(
        db,
        OWNER,
        REPO,
        1,
        None,
        None,
        Some("closed".to_string()),
        None,
        None,
        fixture.owner_id,
    )
    .await
    .expect("close the pull request");
    assert_eq!(closed.state, "closed");

    let events = logged_events(db, fixture.repo_id, "pull_request.closed").await;
    assert_eq!(
        events.len(),
        1,
        "one pull_request.closed row, not two: {events:?}"
    );
    let logged = &events[0];
    assert_eq!(logged["kind"], "pull_request.closed");
    assert_eq!(logged["data"]["event"], "pull_request.closed");
    let pr_payload = &logged["data"]["pull_request"];
    assert_eq!(pr_payload["id"], pr_id);
    assert_eq!(pr_payload["repo_id"], fixture.repo_id);
    assert_eq!(pr_payload["number"], 1);
    assert_eq!(pr_payload["title"], "Close me");
    assert_eq!(pr_payload["state"], "closed");
    assert_eq!(
        pr_payload.as_object().expect("an object").len(),
        5,
        "the closed payload keeps its five fields: {pr_payload}"
    );

    let delivered = receiver.next().await;
    assert_eq!(
        delivered.header(HEADER_EVENT).as_deref(),
        Some("pull_request.closed")
    );
    let delivered = delivered.json();
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
        receiver.is_quiet_for(500).await,
        "the event is delivered once, not twice"
    );

    // The row itself really is closed — the event describes a committed fact.
    let row = pull_request::Entity::find_by_id(pr_id)
        .one(db)
        .await
        .unwrap()
        .expect("the PR still exists");
    assert_eq!(row.state, "closed");
    assert!(row.closed_at.is_some());
}

/// The ordering fix, and the only test that can tell the two orders apart: the
/// state update fails, so the close never happened — and therefore nothing may
/// have been published or delivered.
///
/// The failure is injected from outside the process code, with a SQLite trigger
/// on `pull_requests`. Before the change `trigger_pr_closed` ran *before*
/// `update`, so the log row (and, with a receiver, the delivery) existed even
/// though the update returned an error; on the old code this test fails.
#[tokio::test]
async fn an_update_that_fails_publishes_no_pull_request_closed() {
    let fixture = test_db().await;
    let db = &fixture.db;
    let mut receiver = Receiver::spawn(200).await;
    subscribe(db, fixture.repo_id, &receiver.url(), "pull_request.closed").await;

    let pr_id = insert_open_pr(db, fixture.repo_id, fixture.owner_id).await;
    db.execute_unprepared(
        "CREATE TRIGGER refuse_pr_update BEFORE UPDATE ON pull_requests
         BEGIN
             SELECT RAISE(ABORT, 'injected update failure');
         END",
    )
    .await
    .expect("install the failing-update trigger");

    let refused = update_pr(
        db,
        OWNER,
        REPO,
        1,
        None,
        None,
        Some("closed".to_string()),
        None,
        None,
        fixture.owner_id,
    )
    .await;
    assert!(
        refused.is_err(),
        "the injected trigger must make the update fail: {refused:?}"
    );

    db.execute_unprepared("DROP TRIGGER refuse_pr_update")
        .await
        .expect("remove the trigger");

    assert!(
        logged_events(db, fixture.repo_id, "pull_request.closed")
            .await
            .is_empty(),
        "an update that failed writes no pull_request.closed event"
    );
    assert!(
        receiver.is_quiet_for(500).await,
        "an update that failed delivers no pull_request.closed webhook"
    );
    let hook = hook_id(db, fixture.repo_id).await;
    assert!(
        webhook_ops::list_deliveries_by_webhook(db, hook)
            .await
            .unwrap()
            .is_empty(),
        "no delivery row for a close that never happened"
    );

    // And the PR is untouched, which is what makes the missing event right.
    let row = pull_request::Entity::find_by_id(pr_id)
        .one(db)
        .await
        .unwrap()
        .expect("the PR still exists");
    assert_eq!(row.state, "open");
    assert!(row.closed_at.is_none());
}

/// A metadata edit that leaves the state alone must not announce a close.
#[tokio::test]
async fn a_title_only_update_publishes_no_pull_request_closed() {
    let fixture = test_db().await;
    let db = &fixture.db;
    let mut receiver = Receiver::spawn(200).await;
    subscribe(db, fixture.repo_id, &receiver.url(), "pull_request.closed").await;

    let pr_id = insert_open_pr(db, fixture.repo_id, fixture.owner_id).await;

    let updated = update_pr(
        db,
        OWNER,
        REPO,
        1,
        Some("Renamed".to_string()),
        None,
        None,
        None,
        None,
        fixture.owner_id,
    )
    .await
    .expect("rename the pull request");
    assert_eq!(updated.title, "Renamed");
    assert_eq!(updated.state, "open");

    assert!(
        logged_events(db, fixture.repo_id, "pull_request.closed")
            .await
            .is_empty(),
        "a title edit writes no pull_request.closed event"
    );
    assert!(
        receiver.is_quiet_for(500).await,
        "a title edit delivers no pull_request.closed webhook"
    );
    let hook = hook_id(db, fixture.repo_id).await;
    assert!(
        webhook_ops::list_deliveries_by_webhook(db, hook)
            .await
            .unwrap()
            .is_empty(),
        "no delivery row for a title edit"
    );
    assert!(pull_request::Entity::find_by_id(pr_id)
        .one(db)
        .await
        .unwrap()
        .is_some());
}
