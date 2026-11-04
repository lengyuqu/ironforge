//! Storage-level tests for coordination groups and links (P2 X02).
//!
//! Same setup as `collab_store.rs`: the database named by
//! `IRONFORGE_TEST_DATABASE_URL` (SQLite, PostgreSQL or MySQL), else a
//! throw-away SQLite file. These are the tests to run on PostgreSQL / MySQL:
//!
//! ```text
//! IRONFORGE_TEST_DATABASE_URL=postgres://... cargo test -p rg-db --test collab_groups_store
//! IRONFORGE_TEST_DATABASE_URL=mysql://...    cargo test -p rg-db --test collab_groups_store
//! ```

use std::sync::Arc;

use chrono::Utc;
use rg_db::entities::{collab_coordination_group as group, collab_coordination_link as link, collab_task};
use rg_db::ops::collab_group_ops::{self, AddLinkOutcome, RemoveInfo, RemoveLinkOutcome};
use rg_db::ops::{collab_task_ops, repo_ops, user_ops};
use rg_db::sea_orm::{ActiveModelTrait, EntityTrait, NotSet, Set};
use rg_db::DatabaseConnection;

static SETUP_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

async fn connect() -> DatabaseConnection {
    let url = std::env::var("IRONFORGE_TEST_DATABASE_URL").unwrap_or_else(|_| {
        let path = std::env::temp_dir().join(format!("ironforge_collab_groups_{}.db", std::process::id()));
        format!("sqlite://{}?mode=rwc", path.display())
    });
    let _guard = SETUP_LOCK.lock().await;
    let db = rg_db::connect_with_pool(&url, 20, 60, 16).await.expect("connect");
    rg_db::run_migrations(&db).await.expect("migrate");
    db
}

fn uid() -> String {
    uuid::Uuid::new_v4().to_string()
}

struct Fx {
    db: Arc<DatabaseConnection>,
    anchor: i64,
    target: i64,
    task: collab_task::Model,
}

async fn repo(db: &DatabaseConnection, owner: i64, name: &str) -> i64 {
    let now = Utc::now();
    repo_ops::create(
        db,
        rg_db::entities::repository::ActiveModel {
            id: NotSet,
            owner_id: Set(owner),
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

impl Fx {
    async fn new() -> Self {
        let db = connect().await;
        let suffix = &uid()[..10];
        let username = format!("grp{suffix}");
        let user = user_ops::create_user(&db, &username, &format!("{username}@example.invalid"), "unused", "G")
            .await
            .expect("user");
        let anchor = repo(&db, user.id, &format!("anchor{suffix}")).await;
        let target = repo(&db, user.id, &format!("target{suffix}")).await;
        let now = Utc::now();
        let task = collab_task_ops::create(
            &db,
            collab_task::ActiveModel {
                id: Set(uid()),
                repo_id: Set(target),
                title: Set("t".to_string()),
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
        .expect("task");
        Self { db: Arc::new(db), anchor, target, task }
    }

    async fn group(&self) -> group::Model {
        let now = Utc::now();
        group::ActiveModel {
            id: Set(uid()),
            anchor_repo_id: Set(self.anchor),
            title: Set("g".into()),
            description: Set(String::new()),
            created_by_user_id: Set(1),
            actor_kind: Set("user".into()),
            actor_id: Set("1".into()),
            version: Set(1),
            client_group_id: Set(uid()),
            payload_hash: Set("h".into()),
            created_at: Set(now),
            updated_at: Set(now),
        }
        .insert(&*self.db)
        .await
        .expect("group")
    }

    fn link(&self, group_id: &str, client_id: &str, task: Option<&str>, key: &str) -> link::ActiveModel {
        link::ActiveModel {
            id: Set(uid()),
            group_id: Set(group_id.to_string()),
            anchor_repo_id: Set(self.anchor),
            target_repo_id: Set(self.target),
            target_task_id: Set(task.map(str::to_string)),
            commit_sha: Set(None),
            active_key: Set(Some(key.to_string())),
            created_by_user_id: Set(1),
            actor_kind: Set("user".into()),
            actor_id: Set("1".into()),
            client_link_id: Set(client_id.to_string()),
            payload_hash: Set(format!("h-{client_id}")),
            created_at: Set(Utc::now()),
            removed_at: Set(None),
            removed_by_user_id: Set(None),
            removed_actor_kind: Set(None),
            removed_actor_id: Set(None),
            remove_client_id: Set(None),
            remove_payload_hash: Set(None),
        }
    }
}

#[tokio::test]
async fn the_database_keeps_links_in_the_anchor_and_tasks_in_their_repository() {
    let fx = Fx::new().await;
    let g = fx.group().await;
    // A task id that does not belong to the target repository is refused by the
    // composite foreign key, not by application code.
    let mut bad = fx.link(&g.id, &uid(), Some(&uid()), &uid());
    assert!(bad.clone().insert(&*fx.db).await.is_err(), "unknown task");
    // A link whose anchor does not match its group's anchor is refused too.
    bad.target_task_id = Set(Some(fx.task.id.clone()));
    bad.anchor_repo_id = Set(fx.target);
    assert!(bad.insert(&*fx.db).await.is_err(), "anchor must match the group's");
    let ok = fx.link(&g.id, &uid(), Some(&fx.task.id), &uid());
    assert!(ok.insert(&*fx.db).await.is_ok());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_adds_with_one_expected_version_have_a_single_winner() {
    let fx = Fx::new().await;
    let g = fx.group().await;
    let mut joins = Vec::new();
    for i in 0..6 {
        let db = fx.db.clone();
        let model = fx.link(&g.id, &format!("c{i}-{}", uid()), Some(&fx.task.id), &format!("k{i}-{}", uid()));
        let (anchor, gid) = (fx.anchor, g.id.clone());
        joins.push(tokio::spawn(async move {
            collab_group_ops::add_link_cas(&db, anchor, &gid, 1, model, 1000, Utc::now()).await.unwrap()
        }));
    }
    let mut added = 0;
    for j in joins {
        match j.await.unwrap() {
            AddLinkOutcome::Added { group, .. } => {
                added += 1;
                assert_eq!(group.version, 2);
            }
            AddLinkOutcome::Conflict { current_version } => assert_eq!(current_version, 2),
            other => panic!("{other:?}"),
        }
    }
    assert_eq!(added, 1);
    assert_eq!(collab_group_ops::count_active(&*fx.db, &g.id).await.unwrap(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_racing_copy_of_one_request_is_a_replay_not_a_conflict() {
    let fx = Fx::new().await;
    let g = fx.group().await;
    let client_id = uid();
    let key = uid();
    let mut joins = Vec::new();
    for _ in 0..6 {
        let db = fx.db.clone();
        let model = fx.link(&g.id, &client_id, Some(&fx.task.id), &key);
        let (anchor, gid) = (fx.anchor, g.id.clone());
        joins.push(tokio::spawn(async move {
            collab_group_ops::add_link_cas(&db, anchor, &gid, 1, model, 1000, Utc::now()).await.unwrap()
        }));
    }
    let (mut added, mut existing) = (0, 0);
    for j in joins {
        match j.await.unwrap() {
            AddLinkOutcome::Added { .. } => added += 1,
            AddLinkOutcome::Existing(_) => existing += 1,
            other => panic!("{other:?}"),
        }
    }
    assert_eq!((added, existing), (1, 5));
    let stored = group::Entity::find_by_id(g.id.clone()).one(&*fx.db).await.unwrap().unwrap();
    assert_eq!(stored.version, 2);
}

#[tokio::test]
async fn duplicate_active_targets_are_refused_and_do_not_move_the_version() {
    let fx = Fx::new().await;
    let g = fx.group().await;
    let key = uid();
    let first = fx.link(&g.id, &uid(), Some(&fx.task.id), &key);
    let first_id = match collab_group_ops::add_link_cas(&fx.db, fx.anchor, &g.id, 1, first, 1000, Utc::now()).await.unwrap() {
        AddLinkOutcome::Added { link, .. } => link.id,
        other => panic!("{other:?}"),
    };
    let again = fx.link(&g.id, &uid(), Some(&fx.task.id), &key);
    match collab_group_ops::add_link_cas(&fx.db, fx.anchor, &g.id, 2, again, 1000, Utc::now()).await.unwrap() {
        AddLinkOutcome::Duplicate { link_id } => assert_eq!(link_id, first_id),
        other => panic!("{other:?}"),
    }
    let stored = group::Entity::find_by_id(g.id.clone()).one(&*fx.db).await.unwrap().unwrap();
    assert_eq!(stored.version, 2, "rolled back with the failed insert");

    // The per-group cap counts active links.
    let third = fx.link(&g.id, &uid(), Some(&fx.task.id), &uid());
    assert_eq!(
        collab_group_ops::add_link_cas(&fx.db, fx.anchor, &g.id, 2, third, 1, Utc::now()).await.unwrap(),
        AddLinkOutcome::LimitReached
    );
    let stored = group::Entity::find_by_id(g.id).one(&*fx.db).await.unwrap().unwrap();
    assert_eq!(stored.version, 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn removal_frees_the_target_and_races_have_a_single_winner() {
    let fx = Fx::new().await;
    let g = fx.group().await;
    let key = uid();
    let l = match collab_group_ops::add_link_cas(&fx.db, fx.anchor, &g.id, 1, fx.link(&g.id, &uid(), Some(&fx.task.id), &key), 1000, Utc::now())
        .await
        .unwrap()
    {
        AddLinkOutcome::Added { link, .. } => link,
        other => panic!("{other:?}"),
    };
    let mut joins = Vec::new();
    for i in 0..5 {
        let db = fx.db.clone();
        let (anchor, gid, lid) = (fx.anchor, g.id.clone(), l.id.clone());
        joins.push(tokio::spawn(async move {
            let by = RemoveInfo {
                user_id: 1,
                actor_kind: "user".into(),
                actor_id: "1".into(),
                client_id: format!("rm{i}-{}", uid()),
                payload_hash: "h".into(),
            };
            collab_group_ops::remove_link_cas(&db, anchor, &gid, &lid, 2, &by, Utc::now()).await.unwrap()
        }));
    }
    let mut removed = 0;
    for j in joins {
        match j.await.unwrap() {
            RemoveLinkOutcome::Removed { link, group } => {
                removed += 1;
                assert!(link.removed_at.is_some() && link.active_key.is_none());
                assert_eq!(group.version, 3);
            }
            RemoveLinkOutcome::Conflict { current_version } => assert_eq!(current_version, 3),
            other => panic!("{other:?}"),
        }
    }
    assert_eq!(removed, 1);
    assert_eq!(collab_group_ops::count_active(&*fx.db, &g.id).await.unwrap(), 0);

    // The same target can be linked again (unique key is NULL for removed rows).
    let again = fx.link(&g.id, &uid(), Some(&fx.task.id), &key);
    assert!(matches!(
        collab_group_ops::add_link_cas(&fx.db, fx.anchor, &g.id, 3, again, 1000, Utc::now()).await.unwrap(),
        AddLinkOutcome::Added { .. }
    ));
}
