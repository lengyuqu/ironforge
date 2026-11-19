//! T16: `pull_request.merged` is written **inside** the merge transaction and
//! only delivered after the commit (#15, step 4).
//!
//! The point is the ordering, not the envelope shape (`webhook_envelope_tests.rs`
//! covers that): a real merge must leave exactly one `pull_request.merged` row
//! and wake a receiver exactly once — `deliver_event` must not write the log a
//! second time. A merge that fails before its transaction leaves no event.
//!
//! Loopback receivers need `IRONFORGE_ALLOW_LOCAL_OUTBOUND=1` for the whole
//! process, so these live in their own test binary.

use std::path::Path;

use rg_core::pull_request::{merge_pr, MergeStrategy};
use rg_core::webhook::service::{create_webhook, CreateWebhookRequest, HEADER_EVENT};
use rg_db::entities::{pull_request, repository};
use rg_db::ops::{repo_event_ops, user_ops, webhook_ops};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, DatabaseConnection, EntityTrait, QueryFilter,
    Set,
};
use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;

const OWNER: &str = "merge-owner";
const REPO: &str = "merge-repo";

// ── database ──────────────────────────────────────────────────────────────

/// A migrated SQLite file, built once per test binary (migrations cost seconds;
/// copying the finished file costs milliseconds).
static TEMPLATE: tokio::sync::OnceCell<std::path::PathBuf> = tokio::sync::OnceCell::const_new();

async fn migrated_template() -> &'static std::path::PathBuf {
    TEMPLATE
        .get_or_init(|| async {
            let path = std::env::temp_dir().join(format!(
                "ironforge-pr-merge-event-txn-{}.db",
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
    repo_root: std::path::PathBuf,
    repo_id: i64,
    owner_id: i64,
}

/// A fresh migrated database, the owner/repository the PR belongs to, and the
/// directory the repository's bare git directory lives under.
async fn test_db() -> Fixture {
    std::env::set_var("IRONFORGE_ALLOW_LOCAL_OUTBOUND", "1");
    let template = migrated_template().await;
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("pr-merge-event-txn.db");
    std::fs::copy(template, &path).expect("copy the migrated template");
    let db = rg_db::connect_with_pool(&format!("sqlite://{}?mode=rwc", path.display()), 5, 60, 4)
        .await
        .expect("connect");
    let repo_root = dir.path().join("repos");
    std::fs::create_dir_all(&repo_root).expect("create the repository root");

    let owner = user_ops::create_user(&db, OWNER, "merge-owner@example.com", "", "Merge Owner")
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

/// Insert an open PR pointing at `feature` → `main`, as the merge path expects.
async fn insert_pr(db: &DatabaseConnection, repo_id: i64, author_id: i64, head_sha: &str) -> i64 {
    let now = chrono::Utc::now();
    let pr = pull_request::ActiveModel {
        id: sea_orm::NotSet,
        repo_id: Set(repo_id),
        number: Set(1),
        title: Set("Merge the feature".to_string()),
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
        head_sha: Set(Some(head_sha.to_string())),
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

// ── a real git repository ────────────────────────────────────────────────

fn git(args: &[&str], cwd: Option<&Path>) -> String {
    // Goes through the sanctioned gateway: `test_no_raw_git_command_in_crates`
    // forbids spawning `git` directly anywhere under `crates/`.
    let gateway = rg_git::cli_gateway::global_gateway()
        .as_ref()
        .expect("global git gateway");
    let output = gateway.run(args, cwd).expect("run git");
    assert!(
        output.success(),
        "git {:?} failed: {}",
        args,
        output.stderr_str()
    );
    output.stdout_str().trim().to_string()
}

/// Create `<repo_root>/<owner>/<repo>.git` with `main` and a diverged `feature`,
/// and return the `feature` tip.
fn seed_repository(repo_root: &Path) -> String {
    let work = tempfile::tempdir().expect("worktree dir");
    let path = work.path();
    let path_arg = path.to_string_lossy().to_string();
    git(&["init", "--initial-branch=main", &path_arg], None);
    git(&["config", "user.name", "Merge Test"], Some(path));
    git(
        &["config", "user.email", "merge-test@example.com"],
        Some(path),
    );

    std::fs::write(path.join("README.md"), "base\n").unwrap();
    git(&["add", "."], Some(path));
    git(&["commit", "-m", "base"], Some(path));

    git(&["checkout", "-b", "feature"], Some(path));
    std::fs::write(path.join("feature.txt"), "feature\n").unwrap();
    git(&["add", "."], Some(path));
    git(&["commit", "-m", "feature"], Some(path));
    let head_sha = git(&["rev-parse", "refs/heads/feature"], Some(path));

    let target = repo_root.join(OWNER).join(format!("{REPO}.git"));
    std::fs::create_dir_all(target.parent().unwrap()).unwrap();
    let target_arg = target.to_string_lossy().to_string();
    git(&["clone", "--bare", &path_arg, &target_arg], None);
    head_sha
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
            events: vec!["pull_request.merged".to_string()],
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
    repo_event_ops::list_after(db, repo_id, 0, &["pull_request.merged".to_string()], 100)
        .await
        .expect("read the event log")
        .into_iter()
        .map(|row| serde_json::from_str(row.payload.as_deref().expect("a payload")).expect("json"))
        .collect()
}

// ── the tests ─────────────────────────────────────────────────────────────

/// A real merge writes exactly one log row, delivers it once, and the log and
/// the delivery carry the payload the old `trigger_pr_merged` published.
#[tokio::test]
async fn a_merged_pr_logs_and_delivers_pull_request_merged_exactly_once() {
    let fixture = test_db().await;
    let db = &fixture.db;
    let mut receiver = Receiver::spawn(200).await;
    subscribe(db, fixture.repo_id, &receiver.url()).await;

    let head_sha = seed_repository(&fixture.repo_root);
    let pr_id = insert_pr(db, fixture.repo_id, fixture.owner_id, &head_sha).await;

    let merged = merge_pr(db, &fixture.repo_root, OWNER, REPO, 1, MergeStrategy::Merge)
        .await
        .expect("merge the pull request");
    assert_eq!(merged.strategy, "merge");

    // Exactly one row: the transaction writes it once and `deliver_event` must
    // not write it again.
    let events = logged_events(db, fixture.repo_id).await;
    assert_eq!(
        events.len(),
        1,
        "one pull_request.merged row, not two: {events:?}"
    );
    let logged = &events[0];
    assert_eq!(logged["kind"], "pull_request.merged");
    assert_eq!(
        logged["data"]["event"], "pull_request.merged",
        "the business payload keeps its legacy `event` key"
    );
    let pr_payload = &logged["data"]["pull_request"];
    assert_eq!(pr_payload["id"], pr_id);
    assert_eq!(pr_payload["repo_id"], fixture.repo_id);
    assert_eq!(pr_payload["number"], 1);
    assert_eq!(pr_payload["title"], "Merge the feature");
    assert_eq!(pr_payload["merge_commit_sha"], merged.merge_commit_sha);
    assert_eq!(pr_payload["strategy"], "merge");

    // Delivery: one request, same business payload, right event header.
    let delivered = receiver.next().await;
    assert_eq!(
        delivered.header(HEADER_EVENT).as_deref(),
        Some("pull_request.merged")
    );
    let delivered = delivered.json();
    assert_eq!(delivered["kind"], "pull_request.merged");
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

    // The merge itself happened: the base branch is at the merge commit.
    let base_tip = git(
        &["rev-parse", "refs/heads/main"],
        Some(&fixture.repo_root.join(OWNER).join(format!("{REPO}.git"))),
    );
    assert_eq!(base_tip, merged.merge_commit_sha);
}

/// A merge refused before its transaction (here: the PR is no longer open)
/// leaves the log untouched — no event for a merge that did not happen.
///
/// Note: every failure point of this path is *before* the transaction (state and
/// draft gates, merge claim, ref checks, the git merge itself), so a failure
/// *inside* the transaction cannot be constructed from the outside; that half is
/// covered by T13's mechanism test
/// (`issue_event_txn_tests::an_event_in_a_rolled_back_transaction_is_neither_logged_nor_delivered`),
/// which exercises the very same `record_event_in_txn` rollback.
#[tokio::test]
async fn a_refused_merge_writes_no_event() {
    let fixture = test_db().await;
    let db = &fixture.db;

    let head_sha = seed_repository(&fixture.repo_root);
    let pr_id = insert_pr(db, fixture.repo_id, fixture.owner_id, &head_sha).await;
    pull_request::Entity::update_many()
        .col_expr(
            pull_request::Column::State,
            sea_orm::sea_query::Expr::value("closed"),
        )
        .filter(pull_request::Column::Id.eq(pr_id))
        .exec(db)
        .await
        .expect("close the pull request");

    let refused = merge_pr(db, &fixture.repo_root, OWNER, REPO, 1, MergeStrategy::Merge).await;
    assert!(refused.is_err(), "a closed PR must not merge: {refused:?}");
    assert!(
        logged_events(db, fixture.repo_id).await.is_empty(),
        "a refused merge writes no pull_request.merged event"
    );
}
