//! P1 U03 acceptance: declared-path overlap warnings (N06 path matching and
//! declaration time, N07 hint-only behaviour, paging and access).
//!
//! Real HTTP server and SQLite database; with `IRONFORGE_TEST_DATABASE_URL` the
//! same tests run on PostgreSQL or MySQL (not run here).

mod collab_common;
mod common;

use chrono::{Duration, Utc};
use collab_common::{Env, Sess};
use reqwest::Method;
use rg_db::entities::{agent_session, collab_task};
use rg_db::sea_orm::{ActiveModelTrait, EntityTrait, Set};
use serde_json::{json, Value};

async fn task(env: &Env, s: &Sess, paths: &[&str]) -> String {
    let (status, body) = env
        .json(
            Method::POST,
            &env.url("/tasks"),
            &env.owner_token,
            Some(s),
            Some(json!({"title": "t", "owner_session_id": s.id, "paths": paths})),
        )
        .await;
    assert_eq!(status, 201, "{body}");
    body["id"].as_str().unwrap().to_string()
}

async fn overlaps(env: &Env, id: &str, query: &str) -> (u16, Value) {
    env.json(
        Method::GET,
        &env.url(&format!("/tasks/{id}/overlaps{query}")),
        &env.owner_token,
        None,
        None,
    )
    .await
}

async fn ids(env: &Env, id: &str) -> Vec<String> {
    let (status, body) = overlaps(env, id, "").await;
    assert_eq!(status, 200, "{body}");
    let mut found: Vec<String> = body["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["task_id"].as_str().unwrap().to_string())
        .collect();
    found.sort();
    found
}

async fn patch(env: &Env, s: &Sess, id: &str, version: i64, body: Value) -> Value {
    let mut body = body;
    body["expected_version"] = json!(version);
    let (status, task) = env
        .json(
            Method::PATCH,
            &env.url(&format!("/tasks/{id}")),
            &env.owner_token,
            Some(s),
            Some(body),
        )
        .await;
    assert_eq!(status, 200, "{task}");
    task
}

async fn set_declared_at(env: &Env, id: &str, at: Option<chrono::DateTime<Utc>>) {
    let row = collab_task::Entity::find_by_id(id.to_string())
        .one(&env.db)
        .await
        .unwrap()
        .unwrap();
    let mut active: collab_task::ActiveModel = row.into();
    active.paths_declared_at = Set(at);
    active.update(&env.db).await.unwrap();
}

#[tokio::test]
async fn n06_only_equal_or_directory_prefix_paths_overlap() {
    let env = Env::new().await;
    let a = env.session(&env.owner_token, "A").await;
    let b = env.session(&env.owner_token, "B").await;
    let mine = task(&env, &a, &["src/api", "docs/guide.md"]).await;
    let equal = task(&env, &b, &["docs/guide.md"]).await;
    let inside = task(&env, &b, &["src/api/users.rs"]).await;
    let parent = task(&env, &b, &["src/"]).await;
    let _sibling = task(&env, &b, &["src/apis/x.rs", "src/db"]).await;
    let _glob = task(&env, &b, &["src/**/*.rs"]).await;
    let _none = task(&env, &b, &[]).await;

    let mut expected = vec![equal.clone(), inside.clone(), parent.clone()];
    expected.sort();
    assert_eq!(ids(&env, &mine).await, expected);

    let (_, body) = overlaps(&env, &mine, "").await;
    assert_eq!(body["authority"], "hint");
    assert!(body["not_covered"].as_array().unwrap().len() >= 3);
    let by_id = |id: &str| -> Value {
        body["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i["task_id"] == id)
            .unwrap()
            .clone()
    };
    assert_eq!(by_id(&equal)["overlaps"][0]["reason"], "equal");
    assert_eq!(
        by_id(&inside)["overlaps"][0]["reason"],
        "mine_contains_theirs"
    );
    assert_eq!(
        by_id(&parent)["overlaps"][0]["reason"],
        "theirs_contains_mine"
    );
    assert_eq!(by_id(&inside)["overlaps"][0]["mine"], "src/api");
    assert_eq!(by_id(&inside)["overlaps"][0]["theirs"], "src/api/users.rs");
    assert_eq!(by_id(&inside)["origin"], "user_content");
}

#[tokio::test]
async fn n06_finished_tasks_self_and_taskless_inspection_are_excluded() {
    let env = Env::new().await;
    let a = env.session(&env.owner_token, "A").await;
    let b = env.session(&env.owner_token, "B").await;
    let mine = task(&env, &a, &["src"]).await;
    let other = task(&env, &b, &["src/x.rs"]).await;
    assert_eq!(ids(&env, &mine).await, vec![other.clone()]);

    patch(&env, &b, &other, 1, json!({"status": "done"})).await;
    assert!(ids(&env, &mine).await.is_empty(), "done is not a conflict");

    // A task with no declared paths has nothing to overlap, and says so.
    let empty = task(&env, &a, &[]).await;
    let (status, body) = overlaps(&env, &empty, "").await;
    assert_eq!(status, 200);
    assert!(body["items"].as_array().unwrap().is_empty());
    assert_eq!(body["authority"], "hint");

    let (status, _) = overlaps(&env, "00000000-0000-4000-8000-000000000000", "").await;
    assert_eq!(status, 404);
}

#[tokio::test]
async fn n06_declaration_time_moves_only_when_paths_are_declared() {
    let env = Env::new().await;
    let a = env.session(&env.owner_token, "A").await;
    let b = env.session(&env.owner_token, "B").await;
    let mine = task(&env, &a, &["src"]).await;
    let other = task(&env, &b, &["src/x.rs"]).await;

    let item = |body: &Value| body["items"][0].clone();
    let (_, body) = overlaps(&env, &mine, "").await;
    let first = item(&body);
    assert_eq!(first["confidence"], "declared_current");
    let declared = first["declared_at"].as_str().unwrap().to_string();

    // Unrelated edits do not refresh the declaration, however recent they are.
    set_declared_at(&env, &other, Some(Utc::now() - Duration::hours(30))).await;
    let t = patch(&env, &b, &other, 1, json!({"title": "renamed"})).await;
    assert_eq!(t["version"], 2);
    let (_, body) = overlaps(&env, &mine, "").await;
    assert_eq!(item(&body)["confidence"], "declaration_stale");
    assert_ne!(item(&body)["declared_at"].as_str().unwrap(), declared);

    // Declaring paths again does.
    let t = patch(&env, &b, &other, 2, json!({"paths": ["src/x.rs"]})).await;
    assert!(t["paths_declared_at"].is_string());
    let (_, body) = overlaps(&env, &mine, "").await;
    assert_eq!(item(&body)["confidence"], "declared_current");

    // An unknown time (a row from before the column existed) is never "current".
    set_declared_at(&env, &other, None).await;
    let (_, body) = overlaps(&env, &mine, "").await;
    assert_eq!(item(&body)["confidence"], "declaration_stale");
    assert!(item(&body)["declared_at"].is_null());
}

#[tokio::test]
async fn n07_the_warning_names_the_owner_state_and_never_blocks_anything() {
    let env = Env::new().await;
    let a = env.session(&env.owner_token, "A").await;
    let b = env.session(&env.owner_token, "B").await;
    let mine = task(&env, &a, &["src"]).await;
    let other = task(&env, &b, &["src"]).await;

    let (_, body) = overlaps(&env, &mine, "").await;
    assert_eq!(body["items"][0]["owner_state"], "online");
    assert_eq!(body["items"][0]["owner_session_id"], b.id);

    let row = agent_session::Entity::find_by_id(b.id.clone())
        .one(&env.db)
        .await
        .unwrap()
        .unwrap();
    let mut active: agent_session::ActiveModel = row.into();
    active.expires_at = Set(Utc::now() - Duration::minutes(1));
    active.update(&env.db).await.unwrap();
    let (_, body) = overlaps(&env, &mine, "").await;
    assert_eq!(body["items"][0]["owner_state"], "expired");

    // Overlapping declarations take no lock: both owners keep writing.
    patch(&env, &a, &mine, 1, json!({"status": "in_progress"})).await;
    let (status, _) = overlaps(&env, &other, "").await;
    assert_eq!(status, 200);
}

#[tokio::test]
async fn n07_pages_follow_cursors_without_gaps_or_duplicates_and_reject_forged_ones() {
    let env = Env::new().await;
    let a = env.session(&env.owner_token, "A").await;
    let mine = task(&env, &a, &["src"]).await;
    let mut all = Vec::new();
    for i in 0..5 {
        all.push(task(&env, &a, &[&format!("src/f{i}.rs")]).await);
        task(&env, &a, &[&format!("elsewhere/{i}")]).await;
    }
    all.sort();

    let mut seen = Vec::new();
    let mut query = "?limit=2".to_string();
    let mut pages = 0;
    loop {
        let (status, body) = overlaps(&env, &mine, &query).await;
        assert_eq!(status, 200, "{body}");
        assert!(body["items"].as_array().unwrap().len() <= 2);
        seen.extend(
            body["items"]
                .as_array()
                .unwrap()
                .iter()
                .map(|i| i["task_id"].as_str().unwrap().to_string()),
        );
        pages += 1;
        assert!(pages < 20, "cursor must advance");
        match body["next_cursor"].as_str() {
            Some(cursor) => query = format!("?limit=2&cursor={cursor}"),
            None => break,
        }
    }
    assert_eq!(seen, all, "ascending id order, each exactly once");

    let (status, _) = overlaps(&env, &mine, "?cursor=garbage").await;
    assert_eq!(status, 400);
    // A cursor minted for another task is rejected, not reinterpreted.
    let (_, first) = overlaps(&env, &mine, "?limit=1").await;
    let cursor = first["next_cursor"].as_str().unwrap();
    let other = task(&env, &a, &["src"]).await;
    let (status, _) = overlaps(&env, &other, &format!("?cursor={cursor}")).await;
    assert_eq!(status, 400);
}

#[tokio::test]
async fn n07_reading_needs_repository_access() {
    let env = Env::with_repo(json!({"name": "private-repo", "is_private": true})).await;
    let a = env.session(&env.owner_token, "A").await;
    let mine = task(&env, &a, &["src"]).await;
    let (stranger, _) = env.user("stranger").await;
    let (status, _) = env
        .json(
            Method::GET,
            &env.url(&format!("/tasks/{mine}/overlaps")),
            &stranger,
            None,
            None,
        )
        .await;
    assert!(
        status == 403 || status == 404,
        "no access must not read overlaps, got {status}"
    );
    let (status, _) = env
        .json(
            Method::GET,
            &env.url(&format!("/tasks/{mine}/overlaps")),
            "",
            None,
            None,
        )
        .await;
    assert!(status == 401 || status == 403 || status == 404, "{status}");
}
