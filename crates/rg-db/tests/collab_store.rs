//! Storage-level tests for the multi-agent collaboration tables (C01).
//!
//! Runs against the database named by `IRONFORGE_TEST_DATABASE_URL` (SQLite,
//! PostgreSQL or MySQL); without it a throw-away SQLite file is used. Every test
//! creates its own user/repository with random names, so a shared server
//! database can be reused. To cover all backends:
//!
//! ```text
//! cargo test -p rg-db --test collab_store                                   # SQLite
//! IRONFORGE_TEST_DATABASE_URL=postgres://... cargo test -p rg-db --test collab_store
//! IRONFORGE_TEST_DATABASE_URL=mysql://...    cargo test -p rg-db --test collab_store
//! ```

use std::collections::BTreeSet;
use std::sync::Arc;

use chrono::{Duration, Utc};
use rg_db::entities::{agent_session, collab_handoff, collab_task};
use rg_db::ops::collab_handoff_ops::{self, CreateOutcome};
use rg_db::ops::collab_message_ops::{self, AppendOutcome, NewMessage};
use rg_db::ops::collab_read_ops::{self, AdvanceOutcome};
use rg_db::ops::collab_task_ops::{self, CasOutcome, TaskPatch};
use rg_db::ops::{agent_session_ops, repo_ops, user_ops};
use rg_db::sea_orm::{NotSet, Set};
use rg_db::DatabaseConnection;

static SETUP_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

async fn connect() -> DatabaseConnection {
    let url = std::env::var("IRONFORGE_TEST_DATABASE_URL").unwrap_or_else(|_| {
        let path = std::env::temp_dir().join(format!(
            "ironforge_collab_store_{}.db",
            std::process::id()
        ));
        format!("sqlite://{}?mode=rwc", path.display())
    });
    let _guard = SETUP_LOCK.lock().await;
    let db = rg_db::connect_with_pool(&url, 20, 60, 16)
        .await
        .expect("connect");
    rg_db::run_migrations(&db).await.expect("migrate");
    db
}

fn uid() -> String {
    uuid::Uuid::new_v4().to_string()
}

struct Fixture {
    db: Arc<DatabaseConnection>,
    user_id: i64,
    repo_id: i64,
}

impl Fixture {
    async fn new() -> Self {
        let db = connect().await;
        let suffix = uuid::Uuid::new_v4().simple().to_string();
        let suffix = &suffix[..10];
        let username = format!("collab{suffix}");
        let user = user_ops::create_user(
            &db,
            &username,
            &format!("{username}@example.invalid"),
            "unused",
            "Collab Test",
        )
        .await
        .expect("create user");
        let repo_id = Self::repo(&db, user.id, &format!("repo{suffix}")).await;
        Self {
            db: Arc::new(db),
            user_id: user.id,
            repo_id,
        }
    }

    async fn repo(db: &DatabaseConnection, owner_id: i64, name: &str) -> i64 {
        let now = Utc::now();
        repo_ops::create(
            db,
            rg_db::entities::repository::ActiveModel {
                id: NotSet,
                owner_id: Set(owner_id),
                name: Set(name.to_string()),
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
            },
        )
        .await
        .expect("create repo")
        .id
    }

    async fn other_repo(&self) -> i64 {
        Self::repo(&self.db, self.user_id, &format!("other{}", &uid()[..8])).await
    }

    async fn session_in(&self, repo_id: i64, name: &str) -> agent_session::Model {
        let now = Utc::now();
        agent_session_ops::create(
            &*self.db,
            agent_session::ActiveModel {
                id: Set(uid()),
                repo_id: Set(repo_id),
                user_id: Set(self.user_id),
                secret_hash: Set("0".repeat(64)),
                display_name: Set(name.to_string()),
                client_kind: Set("test".to_string()),
                created_at: Set(now),
                last_seen_at: Set(now),
                expires_at: Set(now + Duration::hours(8)),
                ended_at: Set(None),
            },
        )
        .await
        .expect("create session")
    }

    async fn session(&self, name: &str) -> agent_session::Model {
        self.session_in(self.repo_id, name).await
    }

    async fn task_in(&self, repo_id: i64) -> collab_task::Model {
        let now = Utc::now();
        collab_task_ops::create(
            &*self.db,
            collab_task::ActiveModel {
                id: Set(uid()),
                repo_id: Set(repo_id),
                title: Set("task".to_string()),
                description: Set(None),
                status: Set("open".to_string()),
                owner_session_id: Set(None),
                branch: Set(None),
                paths_json: Set("[]".to_string()),
                issue_number: Set(None),
                pr_number: Set(None),
                version: Set(1),
                next_seq: Set(0),
                created_at: Set(now),
                updated_at: Set(now),
            },
        )
        .await
        .expect("create task")
    }

    async fn task(&self) -> collab_task::Model {
        self.task_in(self.repo_id).await
    }

    fn msg(&self, task: &collab_task::Model, author: &str, client_id: &str, body: &str) -> NewMessage {
        NewMessage {
            id: uid(),
            repo_id: task.repo_id,
            task_id: task.id.clone(),
            author_session_id: author.to_string(),
            kind: "note".to_string(),
            body: body.to_string(),
            reply_to_id: None,
            target_session_id: None,
            commit_sha: None,
            client_message_id: client_id.to_string(),
            payload_hash: format!("hash-{body}"),
            created_at: Utc::now(),
        }
    }

    async fn handoff_model(
        &self,
        task: &collab_task::Model,
        from: &str,
        to: &str,
        client_id: &str,
    ) -> collab_handoff::ActiveModel {
        collab_handoff::ActiveModel {
            id: Set(uid()),
            repo_id: Set(task.repo_id),
            task_id: Set(task.id.clone()),
            from_session_id: Set(from.to_string()),
            to_session_id: Set(to.to_string()),
            summary: Set("s".to_string()),
            completed: Set("c".to_string()),
            next_steps: Set("n".to_string()),
            blockers: Set("b".to_string()),
            commit_sha: Set(None),
            evidence_json: Set("[]".to_string()),
            client_handoff_id: Set(client_id.to_string()),
            payload_hash: Set("h".to_string()),
            status: Set("pending".to_string()),
            version: Set(1),
            created_at: Set(Utc::now()),
            accepted_at: Set(None),
        }
    }
}

async fn message_count(db: &DatabaseConnection, task_id: &str) -> usize {
    collab_message_ops::list_after(db, task_id, 0, 10_000)
        .await
        .unwrap()
        .len()
}

#[tokio::test]
async fn migration_is_idempotent_and_session_secret_is_not_serialized() {
    let fx = Fixture::new().await;
    rg_db::run_migrations(&fx.db).await.expect("second run is a no-op");
    let session = fx.session("A").await;
    let json = serde_json::to_string(&session).unwrap();
    assert!(!json.contains("secret_hash"), "secret hash leaked: {json}");
}

#[tokio::test]
async fn same_display_name_gets_distinct_sessions() {
    let fx = Fixture::new().await;
    let a = fx.session("same name").await;
    let b = fx.session("same name").await;
    assert_ne!(a.id, b.id);
    let found = agent_session_ops::find_in_repo(&*fx.db, fx.repo_id, &a.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(found.id, a.id);
    // Another repository cannot see the id at all.
    let other = fx.other_repo().await;
    assert!(agent_session_ops::find_in_repo(&*fx.db, other, &a.id)
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn heartbeat_does_not_extend_expiry_and_ended_or_expired_sessions_cannot_renew() {
    let fx = Fixture::new().await;
    let s = fx.session("hb").await;
    let later = Utc::now() + Duration::seconds(30);
    assert_eq!(
        agent_session_ops::touch(&*fx.db, fx.repo_id, &s.id, later).await.unwrap(),
        1
    );
    let after = agent_session_ops::find_in_repo(&*fx.db, fx.repo_id, &s.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(after.expires_at, s.expires_at, "hard expiry must not move");

    // Past the hard expiry: not renewable.
    let past_expiry = s.expires_at + Duration::seconds(1);
    assert_eq!(
        agent_session_ops::touch(&*fx.db, fx.repo_id, &s.id, past_expiry).await.unwrap(),
        0
    );

    // Ended: not renewable, and a second end keeps the first end time.
    agent_session_ops::end(&*fx.db, fx.repo_id, &s.id, Utc::now()).await.unwrap();
    let first_end = agent_session_ops::find_in_repo(&*fx.db, fx.repo_id, &s.id)
        .await
        .unwrap()
        .unwrap()
        .ended_at
        .expect("ended");
    agent_session_ops::end(&*fx.db, fx.repo_id, &s.id, Utc::now() + Duration::seconds(60))
        .await
        .unwrap();
    let second = agent_session_ops::find_in_repo(&*fx.db, fx.repo_id, &s.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(second.ended_at, Some(first_end));
    assert_eq!(
        agent_session_ops::touch(&*fx.db, fx.repo_id, &s.id, Utc::now()).await.unwrap(),
        0
    );
}

#[tokio::test]
async fn session_pages_cover_every_row_exactly_once() {
    let fx = Fixture::new().await;
    for i in 0..7 {
        fx.session(&format!("s{i}")).await;
    }
    let mut seen = BTreeSet::new();
    let mut after = None;
    let mut pages = 0;
    loop {
        let page = agent_session_ops::list_page(&*fx.db, fx.repo_id, after.clone(), 3)
            .await
            .unwrap();
        if page.is_empty() {
            break;
        }
        pages += 1;
        for row in &page {
            assert!(seen.insert(row.id.clone()), "duplicate across pages");
        }
        let last = page.last().unwrap();
        after = Some((last.last_seen_at, last.id.clone()));
    }
    assert_eq!(seen.len(), 7);
    assert_eq!(pages, 3);
}

/// T10 (storage part): objects from another repository cannot be linked, and a
/// rejected write leaves nothing behind (including the sequence counter).
#[tokio::test]
async fn cross_repo_references_are_rejected_by_the_database() {
    let fx = Fixture::new().await;
    let other_repo = fx.other_repo().await;
    let task = fx.task().await;
    let author = fx.session("author").await;
    let foreign_session = fx.session_in(other_repo, "foreign").await;
    let foreign_task = fx.task_in(other_repo).await;

    // Author from another repository.
    assert!(collab_message_ops::append(&fx.db, fx.msg(&task, &foreign_session.id, "c1", "x"))
        .await
        .is_err());

    // Target session from another repository.
    let mut m = fx.msg(&task, &author.id, "c2", "x");
    m.target_session_id = Some(foreign_session.id.clone());
    assert!(collab_message_ops::append(&fx.db, m).await.is_err());

    // reply_to pointing at a message of a different task (same repo and other repo).
    let sibling = fx.task().await;
    let sibling_msg = match collab_message_ops::append(&fx.db, fx.msg(&sibling, &author.id, "c3", "y"))
        .await
        .unwrap()
        .unwrap()
    {
        AppendOutcome::Created(m) => m,
        other => panic!("{other:?}"),
    };
    let mut m = fx.msg(&task, &author.id, "c4", "x");
    m.reply_to_id = Some(sibling_msg.id.clone());
    assert!(collab_message_ops::append(&fx.db, m).await.is_err());

    // A task id from another repository reads as absent, not as an error.
    assert!(collab_message_ops::append(
        &fx.db,
        NewMessage {
            repo_id: fx.repo_id,
            ..fx.msg(&foreign_task, &author.id, "c5", "x")
        }
    )
    .await
    .unwrap()
    .is_none());

    // Handoff to a session of another repository.
    assert!(collab_handoff_ops::create_idempotent(
        &fx.db,
        fx.handoff_model(&task, &author.id, &foreign_session.id, "h1").await,
        &author.id,
        "h1",
    )
    .await
    .is_err());

    // Task owner from another repository.
    let owner_patch = TaskPatch {
        owner_session_id: Some(Some(foreign_session.id.clone())),
        ..Default::default()
    };
    assert!(collab_task_ops::update_cas(&fx.db, fx.repo_id, &task.id, 1, owner_patch, Utc::now())
        .await
        .is_err());

    // Nothing was written, and the failed appends consumed no sequence numbers.
    assert_eq!(message_count(&fx.db, &task.id).await, 0);
    let reloaded = collab_task_ops::find_in_repo(&*fx.db, fx.repo_id, &task.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(reloaded.next_seq, 0);
    assert_eq!(reloaded.version, 1);
    assert!(reloaded.owner_session_id.is_none());
}

#[tokio::test]
async fn reply_within_the_same_task_is_accepted() {
    let fx = Fixture::new().await;
    let task = fx.task().await;
    let a = fx.session("a").await;
    let b = fx.session("b").await;
    let first = match collab_message_ops::append(&fx.db, fx.msg(&task, &a.id, "m1", "hello"))
        .await
        .unwrap()
        .unwrap()
    {
        AppendOutcome::Created(m) => m,
        other => panic!("{other:?}"),
    };
    assert_eq!(first.seq, 1);
    let mut reply = fx.msg(&task, &b.id, "m1", "re");
    reply.reply_to_id = Some(first.id.clone());
    reply.target_session_id = Some(a.id.clone());
    match collab_message_ops::append(&fx.db, reply).await.unwrap().unwrap() {
        // Same client id as `a` used, but a different author: keys are per session.
        AppendOutcome::Created(m) => assert_eq!(m.seq, 2),
        other => panic!("{other:?}"),
    }
}

/// T05 (storage part): concurrent retries of one client id create one message.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_same_client_message_id_creates_exactly_one_message() {
    let fx = Fixture::new().await;
    let task = fx.task().await;
    let author = fx.session("a").await;

    let mut joins = Vec::new();
    for _ in 0..8 {
        let db = fx.db.clone();
        let m = fx.msg(&task, &author.id, "same-key", "same body");
        joins.push(tokio::spawn(async move {
            collab_message_ops::append(&db, m).await.unwrap().unwrap()
        }));
    }
    let mut created = Vec::new();
    let mut existing = Vec::new();
    for j in joins {
        match j.await.unwrap() {
            AppendOutcome::Created(m) => created.push(m),
            AppendOutcome::Existing(m) => existing.push(m),
        }
    }
    assert_eq!(created.len(), 1, "exactly one writer creates the message");
    assert_eq!(existing.len(), 7);
    assert!(existing.iter().all(|m| m.id == created[0].id));
    assert_eq!(message_count(&fx.db, &task.id).await, 1);

    // Rolled-back duplicates must not leave a hole in the sequence.
    let next = match collab_message_ops::append(&fx.db, fx.msg(&task, &author.id, "k2", "next"))
        .await
        .unwrap()
        .unwrap()
    {
        AppendOutcome::Created(m) => m,
        other => panic!("{other:?}"),
    };
    assert_eq!(next.seq, 2);

    // Same key, different body: still reported as Existing; the caller sees the
    // stored hash differs and answers IDEMPOTENCY_KEY_REUSED.
    let mut other_body = fx.msg(&task, &author.id, "same-key", "different body");
    other_body.payload_hash = "other".to_string();
    match collab_message_ops::append(&fx.db, other_body).await.unwrap().unwrap() {
        AppendOutcome::Existing(m) => assert_ne!(m.payload_hash, "other"),
        other => panic!("{other:?}"),
    }
    assert_eq!(message_count(&fx.db, &task.id).await, 2);
}

/// T06 (storage part): sequence numbers are issued in commit order, so a reader
/// paging with `after_seq` never skips a message that commits later.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_appends_are_visible_in_sequence_order_without_gaps() {
    let fx = Fixture::new().await;
    let task = fx.task().await;
    let writers = 6usize;
    let per_writer = 15usize;
    let total = (writers * per_writer) as i64;

    let mut sessions = Vec::new();
    for i in 0..writers {
        sessions.push(fx.session(&format!("w{i}")).await);
    }

    let mut joins = Vec::new();
    for (w, session) in sessions.iter().enumerate() {
        let db = fx.db.clone();
        let task_id = task.id.clone();
        let repo_id = fx.repo_id;
        let session_id = session.id.clone();
        joins.push(tokio::spawn(async move {
            for n in 0..per_writer {
                let m = NewMessage {
                    id: uid(),
                    repo_id,
                    task_id: task_id.clone(),
                    author_session_id: session_id.clone(),
                    kind: "note".into(),
                    body: format!("{w}-{n}"),
                    reply_to_id: None,
                    target_session_id: None,
                    commit_sha: None,
                    client_message_id: format!("{w}-{n}"),
                    payload_hash: "h".into(),
                    created_at: Utc::now(),
                };
                collab_message_ops::append(&db, m).await.unwrap().unwrap();
            }
        }));
    }

    // Poller running concurrently with the writers.
    let poll_db = fx.db.clone();
    let poll_task = task.id.clone();
    let poller = tokio::spawn(async move {
        let mut cursor = 0i64;
        let mut seen = Vec::new();
        let mut idle = 0;
        while cursor < total {
            let page = collab_message_ops::list_after(&*poll_db, &poll_task, cursor, 7)
                .await
                .unwrap();
            if page.is_empty() {
                idle += 1;
                assert!(idle < 20_000, "poller starved at cursor {cursor}");
                tokio::task::yield_now().await;
                continue;
            }
            for (i, m) in page.iter().enumerate() {
                // Contiguity is the commit-order property: a gap here would mean a
                // lower seq committed after a higher one became visible.
                assert_eq!(m.seq, cursor + 1 + i as i64, "gap or reorder in page");
            }
            cursor = page.last().unwrap().seq;
            seen.extend(page.into_iter().map(|m| m.seq));
        }
        seen
    });

    for j in joins {
        j.await.unwrap();
    }
    let seen = poller.await.unwrap();
    assert_eq!(seen, (1..=total).collect::<Vec<_>>());
    let reloaded = collab_task_ops::find_in_repo(&*fx.db, fx.repo_id, &task.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(reloaded.next_seq, total);
    assert_eq!(message_count(&fx.db, &task.id).await as i64, total);
}

/// T04 (storage part).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_task_updates_with_one_version_have_a_single_winner() {
    let fx = Fixture::new().await;
    let task = fx.task().await;

    let mut joins = Vec::new();
    for i in 0..6 {
        let db = fx.db.clone();
        let (repo_id, id) = (fx.repo_id, task.id.clone());
        joins.push(tokio::spawn(async move {
            let patch = TaskPatch {
                title: Some(format!("title-{i}")),
                status: Some("in_progress".into()),
                ..Default::default()
            };
            collab_task_ops::update_cas(&db, repo_id, &id, 1, patch, Utc::now())
                .await
                .unwrap()
        }));
    }
    let mut winners = Vec::new();
    for j in joins {
        match j.await.unwrap() {
            CasOutcome::Updated(t) => winners.push(t),
            CasOutcome::Conflict { current } => assert_eq!(current.version, 2),
            CasOutcome::NotFound => panic!("task exists"),
        }
    }
    assert_eq!(winners.len(), 1);
    let stored = collab_task_ops::find_in_repo(&*fx.db, fx.repo_id, &task.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.version, 2);
    assert_eq!(stored.title, winners[0].title, "the winner's write survives intact");

    // Omitted fields stay, explicit null clears.
    let patch = TaskPatch {
        branch: Some(Some("feat/x".into())),
        description: Some(Some("d".into())),
        ..Default::default()
    };
    let t = match collab_task_ops::update_cas(&fx.db, fx.repo_id, &task.id, 2, patch, Utc::now())
        .await
        .unwrap()
    {
        CasOutcome::Updated(t) => t,
        other => panic!("{other:?}"),
    };
    assert_eq!(t.title, stored.title);
    assert_eq!(t.branch.as_deref(), Some("feat/x"));
    let cleared = TaskPatch {
        branch: Some(None),
        ..Default::default()
    };
    match collab_task_ops::update_cas(&fx.db, fx.repo_id, &task.id, 3, cleared, Utc::now())
        .await
        .unwrap()
    {
        CasOutcome::Updated(t) => {
            assert!(t.branch.is_none());
            assert_eq!(t.description.as_deref(), Some("d"));
            assert_eq!(t.version, 4);
        }
        other => panic!("{other:?}"),
    }

    // Unknown task / task of another repository.
    let other_repo = fx.other_repo().await;
    assert_eq!(
        collab_task_ops::update_cas(&fx.db, other_repo, &task.id, 4, TaskPatch::default(), Utc::now())
            .await
            .unwrap(),
        CasOutcome::NotFound
    );
}

#[tokio::test]
async fn task_pages_filter_by_status_and_cover_every_row_once() {
    let fx = Fixture::new().await;
    let mut ids = BTreeSet::new();
    for _ in 0..5 {
        ids.insert(fx.task().await.id);
    }
    let one = ids.iter().next().unwrap().clone();
    collab_task_ops::update_cas(
        &fx.db,
        fx.repo_id,
        &one,
        1,
        TaskPatch {
            status: Some("blocked".into()),
            ..Default::default()
        },
        Utc::now() + Duration::seconds(5),
    )
    .await
    .unwrap();

    let blocked = collab_task_ops::list_page(&*fx.db, fx.repo_id, Some("blocked"), None, 10)
        .await
        .unwrap();
    assert_eq!(blocked.iter().map(|t| &t.id).collect::<Vec<_>>(), vec![&one]);

    let mut seen = BTreeSet::new();
    let mut after = None;
    loop {
        let page = collab_task_ops::list_page(&*fx.db, fx.repo_id, None, after.clone(), 2)
            .await
            .unwrap();
        if page.is_empty() {
            break;
        }
        for t in &page {
            assert!(seen.insert(t.id.clone()));
        }
        let last = page.last().unwrap();
        after = Some((last.updated_at, last.id.clone()));
    }
    assert_eq!(seen, ids);
}

/// T07 (storage part).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_accept_and_cancel_leave_exactly_one_terminal_state() {
    let fx = Fixture::new().await;
    for round in 0..5 {
        let task = fx.task().await;
        let from = fx.session("from").await;
        let to = fx.session("to").await;
        let handoff = match collab_handoff_ops::create_idempotent(
            &fx.db,
            fx.handoff_model(&task, &from.id, &to.id, &format!("h{round}")).await,
            &from.id,
            &format!("h{round}"),
        )
        .await
        .unwrap()
        {
            CreateOutcome::Created(h) => h,
            other => panic!("{other:?}"),
        };
        assert_eq!(handoff.status, "pending");
        assert_eq!(handoff.version, 1);

        let (db1, db2) = (fx.db.clone(), fx.db.clone());
        let (repo_id, id1, id2) = (fx.repo_id, handoff.id.clone(), handoff.id.clone());
        let accept = tokio::spawn(async move {
            collab_handoff_ops::transition_pending(&db1, repo_id, &id1, 1, "accepted", Utc::now())
                .await
                .unwrap()
        });
        let cancel = tokio::spawn(async move {
            collab_handoff_ops::transition_pending(&db2, repo_id, &id2, 1, "cancelled", Utc::now())
                .await
                .unwrap()
        });
        let (a, c) = (accept.await.unwrap(), cancel.await.unwrap());
        let stored = collab_handoff_ops::find_in_repo(&*fx.db, fx.repo_id, &handoff.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stored.version, 2);
        match (a, c) {
            (CasOutcome::Updated(w), CasOutcome::Conflict { current })
            | (CasOutcome::Conflict { current }, CasOutcome::Updated(w)) => {
                assert_eq!(w.status, stored.status);
                assert_eq!(current.status, stored.status, "loser sees the winner's state");
                assert_eq!(stored.accepted_at.is_some(), stored.status == "accepted");
            }
            other => panic!("expected exactly one winner, got {other:?}"),
        }

        // A terminal handoff cannot move again, even with the right version.
        match collab_handoff_ops::transition_pending(&fx.db, fx.repo_id, &handoff.id, 2, "accepted", Utc::now())
            .await
            .unwrap()
        {
            CasOutcome::Conflict { current } => assert_eq!(current.status, stored.status),
            other => panic!("{other:?}"),
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_handoff_creates_with_one_client_id_create_one_row() {
    let fx = Fixture::new().await;
    let task = fx.task().await;
    let from = fx.session("from").await;
    let to = fx.session("to").await;

    let mut joins = Vec::new();
    for _ in 0..6 {
        let db = fx.db.clone();
        let model = fx.handoff_model(&task, &from.id, &to.id, "same-handoff").await;
        let from_id = from.id.clone();
        joins.push(tokio::spawn(async move {
            collab_handoff_ops::create_idempotent(&db, model, &from_id, "same-handoff")
                .await
                .unwrap()
        }));
    }
    let mut ids = BTreeSet::new();
    let mut created = 0;
    for j in joins {
        match j.await.unwrap() {
            CreateOutcome::Created(h) => {
                created += 1;
                ids.insert(h.id);
            }
            CreateOutcome::Existing(h) => {
                ids.insert(h.id);
            }
        }
    }
    assert_eq!(created, 1);
    assert_eq!(ids.len(), 1);
    let page = collab_handoff_ops::list_page(&*fx.db, fx.repo_id, &task.id, None, 10)
        .await
        .unwrap();
    assert_eq!(page.len(), 1);
}

/// T08 (storage part).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn read_watermark_is_monotonic_bounded_and_per_session() {
    let fx = Fixture::new().await;
    let task = fx.task().await;
    let a = fx.session("a").await;
    let b = fx.session("b").await;
    for n in 0..5 {
        collab_message_ops::append(&fx.db, fx.msg(&task, &a.id, &format!("m{n}"), "x"))
            .await
            .unwrap()
            .unwrap();
    }
    let now = Utc::now;
    let adv = |sid: String, seq: i64| {
        let (db, repo, task) = (fx.db.clone(), fx.repo_id, task.id.clone());
        async move { collab_read_ops::advance(&db, repo, &task, &sid, seq, now()).await.unwrap() }
    };

    assert_eq!(adv(a.id.clone(), 3).await, AdvanceOutcome::Current(3));
    // Out of order: a lower value never moves the watermark back.
    assert_eq!(adv(a.id.clone(), 1).await, AdvanceOutcome::Current(3));
    // Beyond the last committed message.
    assert_eq!(adv(a.id.clone(), 6).await, AdvanceOutcome::OutOfRange { max_seq: 5 });
    assert_eq!(adv(a.id.clone(), 5).await, AdvanceOutcome::Current(5));
    // Independent per session.
    assert_eq!(adv(b.id.clone(), 2).await, AdvanceOutcome::Current(2));
    let row_a = collab_read_ops::get(&*fx.db, &task.id, &a.id).await.unwrap().unwrap();
    assert_eq!(row_a.last_read_seq, 5);

    // Concurrent advances converge on the maximum, whatever the order.
    let c = fx.session("c").await;
    let mut joins = Vec::new();
    for seq in [2, 5, 1, 4, 3, 5, 0, 2] {
        let sid = c.id.clone();
        let (db, repo, task_id) = (fx.db.clone(), fx.repo_id, task.id.clone());
        joins.push(tokio::spawn(async move {
            collab_read_ops::advance(&db, repo, &task_id, &sid, seq, Utc::now())
                .await
                .unwrap()
        }));
    }
    for j in joins {
        assert!(matches!(j.await.unwrap(), AdvanceOutcome::Current(_)));
    }
    let row_c = collab_read_ops::get(&*fx.db, &task.id, &c.id).await.unwrap().unwrap();
    assert_eq!(row_c.last_read_seq, 5);

    // Unknown task / task of another repository.
    let other_repo = fx.other_repo().await;
    assert_eq!(
        collab_read_ops::advance(&fx.db, other_repo, &task.id, &a.id, 1, Utc::now())
            .await
            .unwrap(),
        AdvanceOutcome::TaskNotFound
    );
}

#[tokio::test]
async fn deleting_a_repository_removes_its_collaboration_rows() {
    use rg_db::entities::repository;
    use rg_db::sea_orm::{EntityTrait, ModelTrait};

    let fx = Fixture::new().await;
    let repo_id = fx.other_repo().await;
    let task = fx.task_in(repo_id).await;
    let session = fx.session_in(repo_id, "s").await;
    collab_message_ops::append(
        &fx.db,
        NewMessage {
            repo_id,
            ..fx.msg(&task, &session.id, "m", "x")
        },
    )
    .await
    .unwrap()
    .unwrap();

    let repo = repository::Entity::find_by_id(repo_id).one(&*fx.db).await.unwrap().unwrap();
    repo.delete(&*fx.db).await.expect("repository delete cascades without FK errors");
    assert!(collab_task_ops::find_in_repo(&*fx.db, repo_id, &task.id).await.unwrap().is_none());
    assert!(agent_session_ops::find_in_repo(&*fx.db, repo_id, &session.id).await.unwrap().is_none());
    assert_eq!(message_count(&fx.db, &task.id).await, 0);
}
