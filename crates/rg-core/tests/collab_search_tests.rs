//! Search service integration tests with a temporary SQLite database.
pub use rg_core::collaboration::{clamp_limit, cursor, error, ORIGIN_USER_CONTENT};
#[path = "../src/collaboration/search.rs"]
mod search;

use rg_db::entities::{
    collab_decision, collab_handoff, collab_message, collab_question, collab_task,
    collab_work_package,
};
use sea_orm::{ActiveModelTrait, ConnectionTrait, Database, DbBackend, Schema, Set, Statement};

#[tokio::test]
async fn search_is_scoped_paginated_live_and_cursor_bound() {
    let db = Database::connect("sqlite::memory:").await.unwrap();
    db.execute(Statement::from_string(
        DbBackend::Sqlite,
        "PRAGMA foreign_keys = OFF".to_string(),
    ))
    .await
    .unwrap();
    let schema = Schema::new(DbBackend::Sqlite);
    for table in [
        schema.create_table_from_entity(collab_task::Entity),
        schema.create_table_from_entity(collab_question::Entity),
        schema.create_table_from_entity(collab_decision::Entity),
        schema.create_table_from_entity(collab_handoff::Entity),
        schema.create_table_from_entity(collab_message::Entity),
        schema.create_table_from_entity(collab_work_package::Entity),
    ] {
        db.execute(db.get_database_backend().build(&table))
            .await
            .unwrap();
    }
    let now = chrono::Utc::now();
    for (id, repo_id) in [("t1", 1), ("t2", 1), ("hidden", 2)] {
        collab_task::ActiveModel {
            id: Set(id.into()),
            repo_id: Set(repo_id),
            title: Set("needle".into()),
            description: Set(None),
            status: Set("open".into()),
            owner_session_id: Set(None),
            branch: Set(None),
            paths_json: Set("[]".into()),
            issue_number: Set(None),
            pr_number: Set(None),
            version: Set(1),
            next_seq: Set(0),
            created_at: Set(now),
            updated_at: Set(now),
        }
        .insert(&db)
        .await
        .unwrap();
    }
    collab_question::ActiveModel {
        id: Set("q1".into()),
        repo_id: Set(1),
        task_id: Set("t1".into()),
        creator_kind: Set("user".into()),
        creator_id: Set("1".into()),
        recipient_kind: Set("repo_writers".into()),
        recipient_id: Set(None),
        title: Set("question".into()),
        body: Set("body".into()),
        options_json: Set("[]".into()),
        context_json: Set("{\"head_sha\":\"abc123\"}".into()),
        status: Set("open".into()),
        version: Set(1),
        client_question_id: Set("q".into()),
        payload_hash: Set("h".into()),
        created_at: Set(now),
        updated_at: Set(now),
    }
    .insert(&db)
    .await
    .unwrap();
    collab_decision::ActiveModel {
        id: Set("d1".into()),
        repo_id: Set(1),
        task_id: Set("t1".into()),
        seq: Set(1),
        question_id: Set(None),
        answer_id: Set(None),
        title: Set("decision".into()),
        rationale: Set("reason".into()),
        context_json: Set("{\"base_sha\":\"abc123\"}".into()),
        actor_kind: Set("user".into()),
        actor_id: Set("1".into()),
        client_decision_id: Set("d".into()),
        payload_hash: Set("h".into()),
        created_at: Set(now),
    }
    .insert(&db)
    .await
    .unwrap();
    collab_handoff::ActiveModel {
        id: Set("h1".into()),
        repo_id: Set(1),
        task_id: Set("t1".into()),
        from_session_id: Set("a".into()),
        to_session_id: Set("b".into()),
        summary: Set("handoff".into()),
        completed: Set("done".into()),
        next_steps: Set("next".into()),
        blockers: Set("none".into()),
        commit_sha: Set(Some("abc123".into())),
        evidence_json: Set("[]".into()),
        client_handoff_id: Set("h".into()),
        payload_hash: Set("h".into()),
        status: Set("pending".into()),
        version: Set(1),
        created_at: Set(now),
        accepted_at: Set(None),
    }
    .insert(&db)
    .await
    .unwrap();
    for (id, repo_id) in [("1", 1), ("2", 1), ("hidden", 2)] {
        collab_message::ActiveModel {
            id: Set(format!("m{id}")),
            repo_id: Set(repo_id),
            task_id: Set("t1".into()),
            seq: Set(1),
            author_session_id: Set("author".into()),
            kind: Set("note".into()),
            body: Set("needle confidential-content".into()),
            reply_to_id: Set(None),
            target_session_id: Set(None),
            commit_sha: Set(Some("abc123".into())),
            client_message_id: Set(format!("client{id}")),
            payload_hash: Set("internal-hash".into()),
            created_at: Set(now),
        }
        .insert(&db)
        .await
        .unwrap();
        collab_work_package::ActiveModel {
            id: Set(format!("w{id}")),
            repo_id: Set(repo_id),
            task_id: Set("t1".into()),
            revision: Set(1),
            author_kind: Set("user".into()),
            author_id: Set("1".into()),
            task_version: Set(1),
            commit_sha: Set(Some("abc123".into())),
            body_json: Set("{\"goal\":\"needle confidential-content\"}".into()),
            client_package_id: Set(format!("client{id}")),
            payload_hash: Set("internal-hash".into()),
            created_at: Set(now),
        }
        .insert(&db)
        .await
        .unwrap();
    }
    for kind in ["message", "work_package"] {
        for q in ["needle", "abc123"] {
            let first = search::search(&db, 1, b"key", kind, q, Some(1), None)
                .await
                .unwrap();
            assert_eq!(first.items.len(), 1);
            assert!(first.items[0].id.ends_with('2'));
            assert_eq!(first.items[0].task_id, "t1");
            let serialized = serde_json::to_string(&first).unwrap();
            assert!(!serialized.contains("confidential-content"));
            assert!(!serialized.contains("internal-hash"));
            assert!(!serialized.contains(q));
            let token = first.next_cursor.unwrap();
            let second = search::search(&db, 1, b"key", kind, q, Some(1), Some(&token))
                .await
                .unwrap();
            assert!(second.items[0].id.ends_with('1'));
            assert!(second.next_cursor.is_none());
            assert!(
                search::search(&db, 2, b"key", kind, q, Some(1), Some(&token))
                    .await
                    .is_err()
            );
            let other = if kind == "message" {
                "work_package"
            } else {
                "message"
            };
            assert!(
                search::search(&db, 1, b"key", other, q, Some(1), Some(&token))
                    .await
                    .is_err()
            );
            let hidden = search::search(&db, 2, b"key", kind, q, None, None)
                .await
                .unwrap();
            assert_eq!(hidden.items.len(), 1);
            assert!(hidden.items[0].id.ends_with("hidden"));
        }
        for excluded in ["internal-hash", "client1", "author"] {
            assert!(search::search(&db, 1, b"key", kind, excluded, None, None)
                .await
                .unwrap()
                .items
                .is_empty());
        }
    }
    for (kind, expected) in [("question", "q1"), ("decision", "d1"), ("handoff", "h1")] {
        let found = search::search(&db, 1, b"key", kind, "abc123", None, None)
            .await
            .unwrap();
        assert_eq!(found.items[0].id, expected);
        assert!(search::search(&db, 2, b"key", kind, "abc123", None, None)
            .await
            .unwrap()
            .items
            .is_empty());
    }
    let p = search::search(&db, 1, b"key", "task", "needle", Some(1), None)
        .await
        .unwrap();
    assert_eq!(p.items.len(), 1);
    assert_eq!(p.items[0].id, "t2");
    assert_eq!(p.consistency, "live_query_not_snapshot");
    let token = p.next_cursor.unwrap();
    let p2 = search::search(&db, 1, b"key", "task", "needle", Some(1), Some(&token))
        .await
        .unwrap();
    assert_eq!(p2.items[0].id, "t1");
    assert!(p2.next_cursor.is_none());
    for (repo, kind, q) in [
        (2, "task", "needle"),
        (1, "task", "other"),
        (1, "question", "needle"),
    ] {
        assert!(
            search::search(&db, repo, b"key", kind, q, Some(1), Some(&token))
                .await
                .is_err()
        );
    }
    for q in ["", "%", "_", "\\"] {
        assert!(search::search(&db, 1, b"key", "task", q, None, None)
            .await
            .is_err());
    }
    let serialized = serde_json::to_value(p2).unwrap();
    assert!(serialized.get("total").is_none());
    assert!(serialized["items"][0].get("snippet").is_none());
}
