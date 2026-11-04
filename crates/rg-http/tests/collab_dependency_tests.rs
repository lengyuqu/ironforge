//! P1 U01 acceptance: task dependencies (N01 without notices, N02, N03).
//!
//! Real HTTP server and SQLite database; with `IRONFORGE_TEST_DATABASE_URL` the
//! same tests run on PostgreSQL or MySQL.

mod collab_common;
mod common;

use std::collections::HashMap;
use std::sync::Arc;

use chrono::Utc;
use collab_common::{Env, Sess};
use reqwest::Method;
use rg_db::entities::{collab_task, collab_task_dependency};
use rg_db::sea_orm::{
    ActiveModelTrait, ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, Set,
};
use serde_json::{json, Value};
use tokio::sync::Barrier;

/// A task row written straight to the store (task creation itself is not under
/// test and needs a session; the dependency API only sees the row).
async fn new_task(env: &Env, title: &str) -> String {
    let id = uuid::Uuid::new_v4().to_string();
    let now = Utc::now();
    collab_task::ActiveModel {
        id: Set(id.clone()),
        repo_id: Set(env.repo_id),
        title: Set(title.to_string()),
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
    .insert(&env.db)
    .await
    .unwrap();
    id
}

/// Create a task in another repository of the same owner through the API.
async fn task_in(env: &Env, repo: &str, title: &str) -> String {
    let (status, body) = env
        .json(
            Method::POST,
            &env.url_in(repo, "/sessions"),
            &env.owner_token,
            None,
            Some(json!({"display_name": "s", "client_kind": "test-client"})),
        )
        .await;
    assert_eq!(status, 201, "{body}");
    let sess = Sess {
        id: body["session"]["id"].as_str().unwrap().to_string(),
        secret: body["session_secret"].as_str().unwrap().to_string(),
    };
    let (status, task) = env
        .json(
            Method::POST,
            &env.url_in(repo, "/tasks"),
            &env.owner_token,
            Some(&sess),
            Some(json!({"title": title})),
        )
        .await;
    assert_eq!(status, 201, "{task}");
    task["id"].as_str().unwrap().to_string()
}

fn deps_url(env: &Env, task: &str) -> String {
    env.url(&format!("/tasks/{task}/dependencies"))
}

fn add_body(key: &str, dep: &str, version: i64) -> Value {
    json!({"client_request_id": key, "depends_on_task_id": dep, "expected_graph_version": version})
}

fn rm_body(key: &str, version: i64) -> Value {
    json!({"client_request_id": key, "expected_graph_version": version})
}

async fn add(env: &Env, task: &str, dep: &str, key: &str, version: i64) -> (u16, Value) {
    env.json(
        Method::POST,
        &deps_url(env, task),
        &env.owner_token,
        None,
        Some(add_body(key, dep, version)),
    )
    .await
}

async fn remove(env: &Env, task: &str, dep: &str, key: &str, version: i64) -> (u16, Value) {
    env.json(
        Method::DELETE,
        &format!("{}/{dep}", deps_url(env, task)),
        &env.owner_token,
        None,
        Some(rm_body(key, version)),
    )
    .await
}

async fn get(env: &Env, task: &str) -> Value {
    let (status, body) = env
        .json(
            Method::GET,
            &deps_url(env, task),
            &env.owner_token,
            None,
            None,
        )
        .await;
    assert_eq!(status, 200, "{body}");
    body
}

async fn graph_version(env: &Env, task: &str) -> i64 {
    get(env, task).await["graph_version"].as_i64().unwrap()
}

fn code(body: &Value) -> &str {
    body["error"]["code"].as_str().unwrap_or("")
}

async fn set_status(env: &Env, sess: &Sess, task: &str, status: &str) -> Value {
    let url = env.url(&format!("/tasks/{task}"));
    let (_, current) = env
        .json(Method::GET, &url, &env.owner_token, None, None)
        .await;
    let (code, body) = env
        .json(
            Method::PATCH,
            &url,
            &env.owner_token,
            Some(sess),
            Some(json!({"expected_version": current["version"], "status": status})),
        )
        .await;
    assert_eq!(code, 200, "{body}");
    body
}

/// Edges of the whole repository read through the API, and whether they form a cycle.
async fn assert_acyclic(env: &Env, tasks: &[String]) -> HashMap<String, Vec<String>> {
    let mut graph = HashMap::new();
    for t in tasks {
        let deps: Vec<String> = get(env, t).await["dependencies"]
            .as_array()
            .unwrap()
            .iter()
            .map(|d| d["depends_on_task_id"].as_str().unwrap().to_string())
            .collect();
        graph.insert(t.clone(), deps);
    }
    // Kahn's algorithm: every node must be removable.
    let mut indegree: HashMap<&str, usize> = graph.keys().map(|k| (k.as_str(), 0)).collect();
    for deps in graph.values() {
        for d in deps {
            *indegree.get_mut(d.as_str()).unwrap() += 1;
        }
    }
    let mut ready: Vec<&str> = indegree
        .iter()
        .filter(|(_, n)| **n == 0)
        .map(|(k, _)| *k)
        .collect();
    let mut removed = 0;
    while let Some(n) = ready.pop() {
        removed += 1;
        for d in &graph[n] {
            let e = indegree.get_mut(d.as_str()).unwrap();
            *e -= 1;
            if *e == 0 {
                ready.push(d.as_str());
            }
        }
    }
    assert_eq!(
        removed,
        graph.len(),
        "the dependency graph has a cycle: {graph:?}"
    );
    graph
}

// ---------------------------------------------------------------------- N01

#[tokio::test]
async fn n01_dependency_status_is_declared_and_blocked_by_is_derived() {
    let env = Env::new().await;
    let sess = env.session(&env.owner_token, "agent").await;
    let a = new_task(&env, "A").await;
    let b = new_task(&env, "B").await;
    assert_eq!(
        graph_version(&env, &a).await,
        0,
        "no edge yet reads as version 0"
    );

    let (status, added) = add(&env, &a, &b, "k1", 0).await;
    assert_eq!(status, 201, "{added}");
    assert_eq!(added["graph_version"], 1);
    assert_eq!(added["replayed"], false);
    assert_eq!(added["dependency"]["task_id"], a.as_str());
    assert_eq!(added["dependency"]["depends_on_task_id"], b.as_str());
    assert_eq!(added["dependency"]["created_by_kind"], "user");
    assert_eq!(added["dependency"]["origin"], "user_content");

    let listed = get(&env, &a).await;
    assert_eq!(listed["graph_version"], 1);
    assert_eq!(listed["status_source"], "declared");
    assert_eq!(listed["origin"], "user_content");
    assert_eq!(listed["blocked_by"], json!([b]));
    assert_eq!(listed["dependencies"][0]["status"], "open");
    assert_eq!(listed["dependencies"][0]["title"], "B");

    let (_, a_before) = env
        .json(
            Method::GET,
            &env.url(&format!("/tasks/{a}")),
            &env.owner_token,
            None,
            None,
        )
        .await;

    // B declared done: nothing blocks A any more, but A itself is untouched.
    set_status(&env, &sess, &b, "done").await;
    let listed = get(&env, &a).await;
    assert_eq!(listed["blocked_by"], json!([]));
    assert_eq!(listed["dependencies"][0]["status"], "done");

    // B reopened: the derived list is recomputed from the current declaration.
    set_status(&env, &sess, &b, "open").await;
    assert_eq!(get(&env, &a).await["blocked_by"], json!([b]));

    let (_, a_after) = env
        .json(
            Method::GET,
            &env.url(&format!("/tasks/{a}")),
            &env.owner_token,
            None,
            None,
        )
        .await;
    assert_eq!(
        a_before, a_after,
        "dependencies never change the waiting task"
    );

    // The dependency list belongs to the waiting task only.
    assert_eq!(get(&env, &b).await["dependencies"], json!([]));
}

#[tokio::test]
async fn edges_acting_session_and_user_door_are_recorded() {
    let env = Env::new().await;
    let s = env.session(&env.owner_token, "agent").await;
    let (a, b, c) = (
        new_task(&env, "A").await,
        new_task(&env, "B").await,
        new_task(&env, "C").await,
    );
    let (status, added) = env
        .json(
            Method::POST,
            &deps_url(&env, &a),
            &env.owner_token,
            Some(&s),
            Some(add_body("s1", &b, 0)),
        )
        .await;
    assert_eq!(status, 201, "{added}");
    assert_eq!(added["dependency"]["created_by_kind"], "session");
    assert_eq!(added["dependency"]["created_by_id"], s.id.as_str());

    // A wrong secret is the same opaque 403 as everywhere else.
    let bad = Sess {
        id: s.id.clone(),
        secret: "nope".into(),
    };
    let (status, body) = env
        .json(
            Method::POST,
            &deps_url(&env, &a),
            &env.owner_token,
            Some(&bad),
            Some(add_body("s2", &c, 1)),
        )
        .await;
    assert_eq!((status, code(&body)), (403, "SESSION_CREDENTIALS_INVALID"));
    assert_eq!(graph_version(&env, &a).await, 1);
}

#[tokio::test]
async fn replays_conflicts_and_validation() {
    let env = Env::new().await;
    let (a, b, c) = (
        new_task(&env, "A").await,
        new_task(&env, "B").await,
        new_task(&env, "C").await,
    );

    let (status, first) = add(&env, &a, &b, "k1", 0).await;
    assert_eq!(status, 201, "{first}");

    // Same key, same body: replay, even though the version moved on meanwhile.
    let (status, _) = add(&env, &a, &c, "k2", 1).await;
    assert_eq!(status, 201);
    let (status, replay) = add(&env, &a, &b, "k1", 0).await;
    assert_eq!(status, 200, "{replay}");
    assert_eq!(replay["replayed"], true);
    assert_eq!(
        replay["graph_version"], 2,
        "a replay reports the current version"
    );
    assert_eq!(replay["dependency"]["graph_version"], 1);

    // Same key, other body.
    let (status, body) = add(&env, &c, &b, "k1", 0).await;
    assert_eq!(
        (status, code(&body)),
        (409, "IDEMPOTENCY_KEY_REUSED"),
        "{body}"
    );

    // Stale version carries the current one.
    let (status, body) = add(&env, &b, &c, "k3", 1).await;
    assert_eq!((status, code(&body)), (409, "VERSION_CONFLICT"), "{body}");
    assert_eq!(body["error"]["details"]["current_version"], 2);

    // Same edge under another key.
    let (status, body) = add(&env, &a, &b, "k4", 2).await;
    assert_eq!((status, code(&body)), (409, "DEPENDENCY_EXISTS"), "{body}");

    // Self edge, unknown task, malformed input.
    let (status, _) = add(&env, &a, &a, "k5", 2).await;
    assert_eq!(status, 400);
    let (status, _) = add(&env, &a, &uuid::Uuid::new_v4().to_string(), "k6", 2).await;
    assert_eq!(status, 404);
    let (status, _) = add(&env, &a, "not-a-uuid", "k7", 2).await;
    assert_eq!(status, 404);
    let (status, _) = add(&env, &a, &c, "bad key!", 2).await;
    assert_eq!(status, 400);
    let (status, _) = add(&env, &a, &c, "k8", -1).await;
    assert_eq!(status, 400);
    let (status, _) = add(&env, &uuid::Uuid::new_v4().to_string(), &c, "k9", 2).await;
    assert_eq!(status, 404);
    assert_eq!(
        graph_version(&env, &a).await,
        2,
        "refusals never bump the version"
    );
    assert_eq!(
        get(&env, &a).await["dependencies"]
            .as_array()
            .unwrap()
            .len(),
        2
    );

    // Unauthenticated.
    let resp = env
        .send(
            Method::POST,
            &deps_url(&env, &a),
            None,
            None,
            Some(add_body("k10", &c, 2)),
        )
        .await;
    assert_eq!(resp.status(), 401);
}

#[tokio::test]
async fn removal_is_idempotent_and_versioned() {
    let env = Env::new().await;
    let (a, b, c) = (
        new_task(&env, "A").await,
        new_task(&env, "B").await,
        new_task(&env, "C").await,
    );
    add(&env, &a, &b, "k1", 0).await;

    // Stale version while the edge exists.
    let (status, body) = remove(&env, &a, &b, "r1", 0).await;
    assert_eq!((status, code(&body)), (409, "VERSION_CONFLICT"), "{body}");
    assert_eq!(body["error"]["details"]["current_version"], 1);
    assert_eq!(
        get(&env, &a).await["dependencies"]
            .as_array()
            .unwrap()
            .len(),
        1
    );

    let (status, body) = remove(&env, &a, &b, "r1", 1).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(
        (body["removed"].as_bool(), body["graph_version"].as_i64()),
        (Some(true), Some(2))
    );

    // Retry of the same removal, and removal of an edge that never existed: no-ops.
    for (dep, version) in [(&b, 1), (&b, 2), (&c, 2)] {
        let (status, body) = remove(&env, &a, dep, "r1", version).await;
        assert_eq!(status, 200, "{body}");
        assert_eq!(
            (body["removed"].as_bool(), body["graph_version"].as_i64()),
            (Some(false), Some(2))
        );
    }
    let (status, body) = remove(&env, &a, "garbage", "r2", 2).await;
    assert_eq!((status, body["removed"].as_bool()), (200, Some(false)));
    let (status, _) = remove(&env, &uuid::Uuid::new_v4().to_string(), &b, "r3", 2).await;
    assert_eq!(status, 404);
    let (status, _) = remove(&env, &a, &b, "bad key!", 2).await;
    assert_eq!(status, 400);

    // A retried removal after the edge was re-added does not remove it again.
    let (status, _) = add(&env, &a, &b, "k2", 2).await;
    assert_eq!(status, 201);
    let (status, body) = remove(&env, &a, &b, "r1", 1).await;
    assert_eq!((status, code(&body)), (409, "VERSION_CONFLICT"), "{body}");
    assert_eq!(
        get(&env, &a).await["dependencies"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

// ---------------------------------------------------------------------- N02

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn n02_barrier_parallel_opposite_edges_never_form_a_cycle() {
    let env = Arc::new(Env::new().await);
    for round in 0..8 {
        let a = new_task(&env, &format!("A{round}")).await;
        let b = new_task(&env, &format!("B{round}")).await;
        let barrier = Arc::new(Barrier::new(2));
        let mut handles = Vec::new();
        for (i, (from, to)) in [(a.clone(), b.clone()), (b.clone(), a.clone())]
            .into_iter()
            .enumerate()
        {
            let (env, barrier) = (env.clone(), barrier.clone());
            handles.push(tokio::spawn(async move {
                barrier.wait().await;
                add(&env, &from, &to, &format!("race{round}-{i}"), 0).await
            }));
        }
        let results: Vec<(u16, Value)> = futures::future::join_all(handles)
            .await
            .into_iter()
            .map(|r| r.unwrap())
            .collect();
        // Both read the same graph version, so the version row lets exactly one in.
        // (Version 0 is the *first ever* edge of the repository only in round 0, so
        // later rounds start from a higher version and may hit a stale 0 instead.)
        for (status, body) in &results {
            assert!(
                matches!(*status, 201 | 409),
                "round {round}: unexpected {status} {body}"
            );
            if *status == 409 {
                assert!(
                    matches!(code(body), "VERSION_CONFLICT" | "DEPENDENCY_CYCLE"),
                    "round {round}: {body}"
                );
            }
        }
        assert!(
            results.iter().filter(|(s, _)| *s == 201).count() <= 1,
            "{results:?}"
        );
        assert_acyclic(&env, &[a, b]).await;
    }
}

/// Drive `edge` until it either gets in or is refused for a reason other than a
/// stale version; returns the final status and body.
async fn add_with_retries(env: &Env, from: &str, to: &str, key: &str) -> (u16, Value) {
    for _ in 0..50 {
        let version = graph_version(env, from).await;
        let (status, body) = add(env, from, to, key, version).await;
        if !(status == 409 && code(&body) == "VERSION_CONFLICT") {
            return (status, body);
        }
    }
    panic!("starved");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn n02_long_cycle_race_closes_exactly_two_of_three_edges() {
    let env = Arc::new(Env::new().await);
    let tasks: Vec<String> = {
        let mut v = Vec::new();
        for i in 0..6 {
            v.push(new_task(&env, &format!("T{i}")).await);
        }
        v
    };
    // Ring T0 -> T1 -> T2 -> T0 with three clients retrying on stale versions.
    let ring = [(0, 1), (1, 2), (2, 0)];
    let barrier = Arc::new(Barrier::new(ring.len()));
    let mut handles = Vec::new();
    for (i, (from, to)) in ring.into_iter().enumerate() {
        let (env, barrier) = (env.clone(), barrier.clone());
        let (from, to) = (tasks[from].clone(), tasks[to].clone());
        handles.push(tokio::spawn(async move {
            barrier.wait().await;
            add_with_retries(&env, &from, &to, &format!("ring-{i}")).await
        }));
    }
    let results: Vec<(u16, Value)> = futures::future::join_all(handles)
        .await
        .into_iter()
        .map(|r| r.unwrap())
        .collect();
    let created = results.iter().filter(|(s, _)| *s == 201).count();
    let cycles = results
        .iter()
        .filter(|(s, b)| *s == 409 && code(b) == "DEPENDENCY_CYCLE")
        .count();
    assert_eq!((created, cycles), (2, 1), "{results:?}");
    let refused = results.iter().find(|(s, _)| *s == 409).unwrap();
    assert_eq!(
        refused.1["error"]["details"]["cycle_path"]
            .as_array()
            .unwrap()
            .len(),
        3,
        "the cycle is reported as the path that would close it: {refused:?}"
    );
    assert_acyclic(&env, &tasks[..3]).await;

    // A longer chain T3 -> T4 -> T5 built sequentially, then every back edge at once.
    let base = graph_version(&env, &tasks[3]).await;
    assert_eq!(add(&env, &tasks[3], &tasks[4], "c1", base).await.0, 201);
    assert_eq!(add(&env, &tasks[4], &tasks[5], "c2", base + 1).await.0, 201);
    let back = [(5, 3), (5, 4), (4, 3), (5, 3)];
    let barrier = Arc::new(Barrier::new(back.len()));
    let mut handles = Vec::new();
    for (i, (from, to)) in back.into_iter().enumerate() {
        let (env, barrier) = (env.clone(), barrier.clone());
        let (from, to) = (tasks[from].clone(), tasks[to].clone());
        handles.push(tokio::spawn(async move {
            barrier.wait().await;
            add_with_retries(&env, &from, &to, &format!("back-{i}")).await
        }));
    }
    for h in futures::future::join_all(handles).await {
        let (status, body) = h.unwrap();
        assert_eq!((status, code(&body)), (409, "DEPENDENCY_CYCLE"), "{body}");
    }
    assert_acyclic(&env, &tasks).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn identical_parallel_requests_create_one_edge() {
    let env = Arc::new(Env::new().await);
    let (a, b) = (new_task(&env, "A").await, new_task(&env, "B").await);
    let barrier = Arc::new(Barrier::new(4));
    let mut handles = Vec::new();
    for _ in 0..4 {
        let (env, barrier, a, b) = (env.clone(), barrier.clone(), a.clone(), b.clone());
        handles.push(tokio::spawn(async move {
            barrier.wait().await;
            add(&env, &a, &b, "same-key", 0).await
        }));
    }
    let results: Vec<(u16, Value)> = futures::future::join_all(handles)
        .await
        .into_iter()
        .map(|r| r.unwrap())
        .collect();
    assert_eq!(
        results.iter().filter(|(s, _)| *s == 201).count(),
        1,
        "{results:?}"
    );
    for (status, body) in &results {
        assert!(
            matches!(*status, 201 | 200) || (*status == 409 && code(body) == "VERSION_CONFLICT"),
            "{status} {body}"
        );
    }
    let listed = get(&env, &a).await;
    assert_eq!(listed["dependencies"].as_array().unwrap().len(), 1);
    assert_eq!(listed["graph_version"], 1);
}

// ---------------------------------------------------------------------- N03

#[tokio::test]
async fn n03_direct_dependency_limit_fails_closed() {
    let env = Env::new().await;
    let a = new_task(&env, "A").await;
    for i in 0..100 {
        let dep = new_task(&env, &format!("D{i}")).await;
        let (status, body) = add(&env, &a, &dep, &format!("k{i}"), i).await;
        assert_eq!(status, 201, "{body}");
    }
    let extra = new_task(&env, "extra").await;
    let (status, body) = add(&env, &a, &extra, "k-extra", 100).await;
    assert_eq!(
        (status, code(&body)),
        (409, "GRAPH_LIMIT_EXCEEDED"),
        "{body}"
    );
    assert_eq!(body["error"]["details"]["limit"], "direct_dependencies");
    assert_eq!(body["error"]["details"]["max"], 100);
    let listed = get(&env, &a).await;
    assert_eq!(
        listed["graph_version"], 100,
        "the refused write left no trace"
    );
    assert_eq!(listed["dependencies"].as_array().unwrap().len(), 100);
    // Removing one makes room again.
    let first = listed["dependencies"][0]["depends_on_task_id"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(remove(&env, &a, &first, "rm", 100).await.0, 200);
    assert_eq!(add(&env, &a, &extra, "k-extra2", 101).await.0, 201);
}

async fn bulk_tasks(env: &Env, n: usize) -> Vec<String> {
    let now = Utc::now();
    let ids: Vec<String> = (0..n).map(|_| uuid::Uuid::new_v4().to_string()).collect();
    for chunk in ids.chunks(100) {
        collab_task::Entity::insert_many(chunk.iter().enumerate().map(|(i, id)| {
            collab_task::ActiveModel {
                id: Set(id.clone()),
                repo_id: Set(env.repo_id),
                title: Set(format!("bulk {i}")),
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
        }))
        .exec(&env.db)
        .await
        .unwrap();
    }
    ids
}

async fn bulk_edges(env: &Env, edges: &[(String, String)]) {
    let now = Utc::now();
    for chunk in edges.chunks(200) {
        collab_task_dependency::Entity::insert_many(chunk.iter().map(|(from, to)| {
            collab_task_dependency::ActiveModel {
                repo_id: Set(env.repo_id),
                task_id: Set(from.clone()),
                depends_on_task_id: Set(to.clone()),
                created_by_kind: Set("user".into()),
                created_by_id: Set("bulk".into()),
                client_dependency_id: Set(format!("bulk-{from}-{to}")),
                payload_hash: Set("0".into()),
                graph_version: Set(0),
                created_at: Set(now),
            }
        }))
        .exec(&env.db)
        .await
        .unwrap();
    }
}

async fn edge_count(env: &Env) -> u64 {
    collab_task_dependency::Entity::find()
        .filter(collab_task_dependency::Column::RepoId.eq(env.repo_id))
        .count(&env.db)
        .await
        .unwrap()
}

#[tokio::test]
async fn n03_node_limit_of_the_traversal_fails_closed() {
    let env = Env::new().await;
    // A chain of 1001 tasks: walking it from the head visits more than 1000 nodes.
    let chain = bulk_tasks(&env, 1001).await;
    let edges: Vec<(String, String)> = chain
        .windows(2)
        .map(|w| (w[0].clone(), w[1].clone()))
        .collect();
    bulk_edges(&env, &edges).await;
    let newcomer = new_task(&env, "newcomer").await;
    let before = edge_count(&env).await;

    let (status, body) = add(&env, &newcomer, &chain[0], "limit-nodes", 0).await;
    assert_eq!(
        (status, code(&body)),
        (409, "GRAPH_LIMIT_EXCEEDED"),
        "{body}"
    );
    assert_eq!(body["error"]["details"]["limit"], "nodes");
    assert_eq!(body["error"]["details"]["max"], 1000);
    assert_eq!(edge_count(&env).await, before, "no partial write");
    assert_eq!(
        graph_version(&env, &newcomer).await,
        0,
        "the version bump rolled back with it"
    );

    // A chain that fits (1000 nodes) is still checked fully and accepted.
    let (status, body) = add(&env, &newcomer, &chain[1], "fits", 0).await;
    assert_eq!(status, 201, "{body}");
    // ...and closing a cycle through it is still found.
    let (status, body) = add(&env, &chain[1000], &newcomer, "closes", 1).await;
    assert_eq!(status, 409, "{body}");
}

#[tokio::test]
async fn n03_edge_limit_of_the_traversal_fails_closed() {
    let env = Env::new().await;
    // 120 tasks, each depending on up to 100 later ones: far more than 5000 edges
    // among fewer than 1000 nodes.
    let nodes = bulk_tasks(&env, 120).await;
    let mut edges = Vec::new();
    for i in 0..nodes.len() {
        for j in (i + 1)..nodes.len().min(i + 101) {
            edges.push((nodes[i].clone(), nodes[j].clone()));
        }
    }
    assert!(edges.len() > 5000);
    bulk_edges(&env, &edges).await;
    let newcomer = new_task(&env, "newcomer").await;
    let before = edge_count(&env).await;

    let (status, body) = add(&env, &newcomer, &nodes[0], "limit-edges", 0).await;
    assert_eq!(
        (status, code(&body)),
        (409, "GRAPH_LIMIT_EXCEEDED"),
        "{body}"
    );
    assert_eq!(body["error"]["details"]["limit"], "edges");
    assert_eq!(body["error"]["details"]["max"], 5000);
    assert_eq!(edge_count(&env).await, before);
    assert_eq!(graph_version(&env, &newcomer).await, 0);
}

#[tokio::test]
async fn n03_traversal_limits_are_parameters_of_the_store_operation() {
    use rg_db::ops::collab_dependency_ops::{add_edge, AddOutcome, GraphLimits, LimitKind};
    let env = Env::new().await;
    let chain = bulk_tasks(&env, 6).await;
    let edges: Vec<(String, String)> = chain
        .windows(2)
        .map(|w| (w[0].clone(), w[1].clone()))
        .collect();
    bulk_edges(&env, &edges).await;
    let extra = bulk_tasks(&env, 1).await.remove(0);
    let model = |key: &str| collab_task_dependency::ActiveModel {
        repo_id: Set(env.repo_id),
        task_id: Set(extra.clone()),
        depends_on_task_id: Set(chain[0].clone()),
        created_by_kind: Set("user".into()),
        created_by_id: Set("t".into()),
        client_dependency_id: Set(key.into()),
        payload_hash: Set("h".into()),
        graph_version: Set(0),
        created_at: Set(Utc::now()),
    };
    let tight = |max_nodes, max_edges, max_direct| GraphLimits {
        max_nodes,
        max_edges,
        max_direct,
    };
    for (limits, kind) in [
        (tight(4, 100, 100), LimitKind::Nodes),
        (tight(100, 3, 100), LimitKind::Edges),
        (tight(100, 100, 0), LimitKind::DirectDependencies),
    ] {
        let out = add_edge(&env.db, env.repo_id, 0, model("k"), &limits, Utc::now())
            .await
            .unwrap();
        assert!(
            matches!(out, AddOutcome::LimitExceeded { kind: k, .. } if k == kind),
            "{kind:?}: {out:?}"
        );
    }
    // Exactly at the bounds is still fine (6 nodes, 5 edges).
    let out = add_edge(
        &env.db,
        env.repo_id,
        0,
        model("k"),
        &tight(6, 5, 1),
        Utc::now(),
    )
    .await
    .unwrap();
    assert!(
        matches!(
            out,
            AddOutcome::Added {
                graph_version: 1,
                ..
            }
        ),
        "{out:?}"
    );
}

#[tokio::test]
async fn n03_cross_repository_edges_are_refused_without_leaking() {
    let env = Env::new().await;
    // Second repository of the same owner.
    let other_repo = "other-repo";
    common::create_repo(&env.base, &env.owner_token, other_repo).await;
    let foreign = task_in(&env, other_repo, "secret title in another repo").await;
    let a = new_task(&env, "A").await;
    let ghost = uuid::Uuid::new_v4().to_string();

    // Foreign and nonexistent targets are indistinguishable.
    let (s1, b1) = add(&env, &a, &foreign, "x1", 0).await;
    let (s2, b2) = add(&env, &a, &ghost, "x2", 0).await;
    assert_eq!((s1, s2), (404, 404));
    assert_eq!(b1["error"]["message"], b2["error"]["message"]);
    assert!(!b1.to_string().contains("secret"));

    // The foreign task is absent from this repository's endpoints.
    let (status, _) = env
        .json(
            Method::GET,
            &deps_url(&env, &foreign),
            &env.owner_token,
            None,
            None,
        )
        .await;
    assert_eq!(status, 404);
    let (status, _) = add(&env, &foreign, &a, "x3", 0).await;
    assert_eq!(status, 404);
    let (status, _) = remove(&env, &foreign, &a, "x4", 0).await;
    assert_eq!(status, 404);
    assert_eq!(graph_version(&env, &a).await, 0);
    assert_eq!(edge_count(&env).await, 0);

    // The store itself refuses it too (composite foreign keys), whatever the API does.
    let now = Utc::now();
    let forged = collab_task_dependency::ActiveModel {
        repo_id: Set(env.repo_id),
        task_id: Set(a.clone()),
        depends_on_task_id: Set(foreign.clone()),
        created_by_kind: Set("user".into()),
        created_by_id: Set("t".into()),
        client_dependency_id: Set("forged".into()),
        payload_hash: Set("h".into()),
        graph_version: Set(1),
        created_at: Set(now),
    };
    assert!(
        forged.insert(&env.db).await.is_err(),
        "a cross-repository edge must not be storable"
    );
}

#[tokio::test]
async fn n03_access_is_checked_on_every_request_and_revocation_applies_next_request() {
    let env = Env::new().await;
    let (a, b, c) = (
        new_task(&env, "A").await,
        new_task(&env, "B").await,
        new_task(&env, "C").await,
    );
    add(&env, &a, &b, "k1", 0).await;

    let (reader_token, reader_id) = env.user("reader").await;
    let (writer_token, writer_id) = env.user("writer").await;
    let (stranger_token, _) = env.user("stranger").await;
    env.add_collaborator(reader_id, "read").await;
    env.add_collaborator(writer_id, "write").await;

    // Reader sees, cannot write; a signed-in stranger sees nothing.
    let (status, _) = env
        .json(Method::GET, &deps_url(&env, &a), &reader_token, None, None)
        .await;
    assert_eq!(status, 200);
    let (status, _) = env
        .json(
            Method::POST,
            &deps_url(&env, &a),
            &reader_token,
            None,
            Some(add_body("r", &c, 1)),
        )
        .await;
    assert_eq!(status, 403);
    let (status, _) = env
        .json(
            Method::DELETE,
            &format!("{}/{b}", deps_url(&env, &a)),
            &reader_token,
            None,
            Some(rm_body("r", 1)),
        )
        .await;
    assert_eq!(status, 403);
    let (status, _) = env
        .json(
            Method::GET,
            &deps_url(&env, &a),
            &stranger_token,
            None,
            None,
        )
        .await;
    assert_eq!(status, 403);
    let (status, _) = env
        .json(
            Method::POST,
            &deps_url(&env, &a),
            &stranger_token,
            None,
            Some(add_body("s", &c, 1)),
        )
        .await;
    assert_eq!(status, 403);

    // Writer writes...
    let (status, body) = env
        .json(
            Method::POST,
            &deps_url(&env, &a),
            &writer_token,
            None,
            Some(add_body("w1", &c, 1)),
        )
        .await;
    assert_eq!(status, 201, "{body}");
    assert_eq!(body["dependency"]["created_by_id"], writer_id.to_string());

    // ...until revoked: the very next request is refused, reads included.
    let resp = env
        .send(
            Method::DELETE,
            &format!(
                "{}/api/v1/repos/{}/{}/collaborators/{writer_id}",
                env.base, env.owner, env.repo
            ),
            Some(&env.owner_token),
            None,
            None,
        )
        .await;
    assert!(resp.status().is_success(), "{}", resp.status());
    let (status, _) = env
        .json(
            Method::DELETE,
            &format!("{}/{c}", deps_url(&env, &a)),
            &writer_token,
            None,
            Some(rm_body("w2", 2)),
        )
        .await;
    assert_eq!(status, 403);
    let (status, _) = env
        .json(Method::GET, &deps_url(&env, &a), &writer_token, None, None)
        .await;
    assert_eq!(status, 403);
    let (status, _) = env
        .json(
            Method::POST,
            &deps_url(&env, &a),
            &writer_token,
            None,
            Some(add_body("w3", &b, 2)),
        )
        .await;
    assert_eq!(status, 403);
    assert_eq!(
        graph_version(&env, &a).await,
        2,
        "revoked and refused writes changed nothing"
    );
    assert_eq!(edge_count(&env).await, 2);

    // Unauthenticated reads are refused as well.
    let resp = env
        .send(Method::GET, &deps_url(&env, &a), None, None, None)
        .await;
    assert_eq!(resp.status(), 401);
}

#[tokio::test]
async fn graph_versions_are_per_repository() {
    let env = Env::new().await;
    common::create_repo(&env.base, &env.owner_token, "second").await;
    let (a, b) = (new_task(&env, "A").await, new_task(&env, "B").await);
    let x = task_in(&env, "second", "X").await;
    let y = task_in(&env, "second", "Y").await;
    assert_eq!(add(&env, &a, &b, "k1", 0).await.0, 201);
    // The other repository's graph still starts at 0.
    let (status, body) = env
        .json(
            Method::POST,
            &env.url_in("second", &format!("/tasks/{x}/dependencies")),
            &env.owner_token,
            None,
            Some(add_body("k2", &y, 0)),
        )
        .await;
    assert_eq!(status, 201, "{body}");
    assert_eq!(body["graph_version"], 1);
}
