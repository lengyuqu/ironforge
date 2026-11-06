//! `GET /repos/{o}/{n}/events`: a repository's events by cursor, with long-poll.
//!
//! Every event that would go to a webhook is also in the log, read back in order
//! by id: nothing missed, nothing repeated, and only by someone who can read the
//! repository.

mod common;
use common::{register_user, spawn_test_app_on_env_db};

use reqwest::{Client, StatusCode};
use serde_json::{json, Value};

const PW: &str = "Qz7$wRtm";

struct Fixture {
    base: String,
    jwt: String,
    owner: &'static str,
    client: Client,
}

impl Fixture {
    async fn on(base: String, owner: &'static str, private: bool) -> Fixture {
        let jwt = register_user(&base, owner, &format!("{owner}@example.com"), PW).await;
        let client = Client::new();
        let resp = client
            .post(format!("{base}/api/v1/repos"))
            .bearer_auth(&jwt)
            .json(&json!({"name": "work", "auto_init": true, "is_private": private}))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 201);
        Fixture {
            base,
            jwt,
            owner,
            client,
        }
    }

    async fn new(owner: &'static str, private: bool) -> Fixture {
        Fixture::on(spawn_test_app_on_env_db().await.0, owner, private).await
    }

    fn url(&self, suffix: &str) -> String {
        format!("{}/api/v1/repos/{}/work{suffix}", self.base, self.owner)
    }

    async fn events(&self, bearer: &str, query: &str) -> (StatusCode, Value) {
        let resp = self
            .client
            .get(self.url(&format!("/events{query}")))
            .bearer_auth(bearer)
            .send()
            .await
            .unwrap();
        let status = resp.status();
        (status, resp.json().await.unwrap_or(Value::Null))
    }

    async fn open_issue(&self, title: &str) {
        self.open_issue_with_body(title, "").await
    }

    async fn open_issue_with_body(&self, title: &str, body: &str) {
        let resp = self
            .client
            .post(self.url("/issues"))
            .bearer_auth(&self.jwt)
            .json(&json!({"title": title, "body": body}))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 201);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn events_are_read_back_in_order_by_cursor() {
    let f = Fixture::new("events_a", false).await;

    // No cursor: nothing but where the log ends now.
    let (status, start) = f.events(&f.jwt, "").await;
    assert_eq!(status, StatusCode::OK, "{start}");
    assert_eq!(start["events"], json!([]));
    let cursor = start["next_cursor"].as_i64().unwrap();

    f.open_issue("first").await;
    f.open_issue("second").await;

    let (status, v) = f.events(&f.jwt, &format!("?since={cursor}")).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    let events = v["events"].as_array().unwrap();
    assert_eq!(events.len(), 2, "{v}");
    assert_eq!(events[0]["type"], "issue.opened");
    assert_eq!(events[0]["payload"]["issue"]["title"], "first");
    assert_eq!(events[1]["payload"]["issue"]["title"], "second");
    assert_eq!(events[0]["payload_origin"], "user_content");
    let (a, b) = (
        events[0]["id"].as_i64().unwrap(),
        events[1]["id"].as_i64().unwrap(),
    );
    assert!(cursor < a && a < b);
    assert_eq!(v["next_cursor"], json!(b));
    assert_eq!(v["timed_out"], json!(false));
    assert_eq!(v["retention_days"], json!(7));
    assert!(v["oldest_retained_at"].is_string());

    // Reading again from the new cursor repeats nothing and misses nothing.
    let (_, again) = f.events(&f.jwt, &format!("?since={b}")).await;
    assert_eq!(again["events"], json!([]));
    assert_eq!(again["next_cursor"], json!(b));
    assert_eq!(again["timed_out"], json!(true));
    f.open_issue("third").await;
    let (_, more) = f.events(&f.jwt, &format!("?since={b}")).await;
    assert_eq!(more["events"].as_array().unwrap().len(), 1);
    assert_eq!(more["events"][0]["payload"]["issue"]["title"], "third");

    // limit pages through: the cursor of a page is where the next starts.
    let (_, page1) = f.events(&f.jwt, &format!("?since={cursor}&limit=2")).await;
    assert_eq!(page1["events"].as_array().unwrap().len(), 2);
    let next = page1["next_cursor"].as_i64().unwrap();
    let (_, page2) = f.events(&f.jwt, &format!("?since={next}&limit=2")).await;
    assert_eq!(page2["events"].as_array().unwrap().len(), 1);
    assert_eq!(page2["events"][0]["payload"]["issue"]["title"], "third");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_type_filter_only_returns_those_events() {
    let f = Fixture::new("events_b", false).await;
    let (_, start) = f.events(&f.jwt, "").await;
    let cursor = start["next_cursor"].as_i64().unwrap();
    f.open_issue("an issue").await;

    let (_, only_pr) = f
        .events(
            &f.jwt,
            &format!("?since={cursor}&types=pull_request.opened,push"),
        )
        .await;
    assert_eq!(only_pr["events"], json!([]), "{only_pr}");
    let (_, only_issue) = f
        .events(&f.jwt, &format!("?since={cursor}&types=issue.opened"))
        .await;
    assert_eq!(only_issue["events"].as_array().unwrap().len(), 1);

    // Bad parameters are refused.
    for q in [
        "?since=0&types=BAD NAME",
        "?since=-1",
        &format!(
            "?since=0&types={}",
            (0..21)
                .map(|n| format!("e{n}"))
                .collect::<Vec<_>>()
                .join(",")
        ),
    ] {
        let (status, v) = f.events(&f.jwt, q).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{q}: {v}");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_long_poll_returns_as_soon_as_an_event_arrives() {
    let f = Fixture::new("events_c", false).await;
    let (_, start) = f.events(&f.jwt, "").await;
    let cursor = start["next_cursor"].as_i64().unwrap();

    let started = std::time::Instant::now();
    let query = format!("?since={cursor}&timeout=20");
    let waiter = f.events(&f.jwt, &query);
    let creator = async {
        tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
        f.open_issue("arrives while waiting").await;
    };
    let ((status, v), ()) = tokio::join!(waiter, creator);
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(v["events"].as_array().unwrap().len(), 1, "{v}");
    assert_eq!(v["timed_out"], json!(false));
    let waited = started.elapsed();
    assert!(
        waited >= std::time::Duration::from_millis(1400)
            && waited < std::time::Duration::from_secs(10),
        "returned when the event arrived, not at the timeout: {waited:?}"
    );

    // With nothing coming, it returns at the timeout with timed_out.
    let started = std::time::Instant::now();
    let next = v["next_cursor"].as_i64().unwrap();
    let (_, quiet) = f.events(&f.jwt, &format!("?since={next}&timeout=2")).await;
    assert_eq!(quiet["events"], json!([]));
    assert_eq!(quiet["timed_out"], json!(true));
    assert!(started.elapsed() >= std::time::Duration::from_millis(1800));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn only_someone_who_can_read_the_repository_sees_its_events() {
    let f = Fixture::new("events_d", true).await;
    f.open_issue("private matter").await;
    let stranger = register_user(
        &f.base,
        "events_stranger",
        "events-stranger@example.com",
        PW,
    )
    .await;

    let (status, _) = f.events(&stranger, "?since=0").await;
    assert!(
        status == StatusCode::NOT_FOUND || status == StatusCode::FORBIDDEN,
        "a stranger gets no events from a private repository: {status}"
    );
    let anonymous = f
        .client
        .get(f.url("/events?since=0"))
        .send()
        .await
        .unwrap()
        .status();
    assert!(anonymous.is_client_error(), "{anonymous}");
    let (status, v) = f.events(&f.jwt, "?since=0").await;
    assert_eq!(status, StatusCode::OK);
    assert!(!v["events"].as_array().unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn expired_events_are_deleted_and_a_large_payload_is_kept_as_a_stub() {
    use rg_db::sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};
    let (base, db) = spawn_test_app_on_env_db().await;
    let f = Fixture::on(base, "events_e", false).await;
    f.open_issue("old news").await;
    let (_, v) = f.events(&f.jwt, "?since=0").await;
    let id = v["events"][0]["id"].as_i64().unwrap();

    // Age the event past the retention period and sweep.
    let row = rg_db::entities::repo_event::Entity::find_by_id(id)
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    let mut active: rg_db::entities::repo_event::ActiveModel = row.into();
    active.created_at = Set(chrono::Utc::now() - chrono::Duration::days(8));
    active.update(&db).await.unwrap();
    let deleted = rg_db::ops::repo_event_ops::delete_older_than(
        &db,
        chrono::Utc::now() - chrono::Duration::days(7),
    )
    .await
    .unwrap();
    assert_eq!(deleted, 1);
    let left = rg_db::entities::repo_event::Entity::find()
        .filter(rg_db::entities::repo_event::Column::RepoId.gt(0))
        .all(&db)
        .await
        .unwrap();
    assert!(left.iter().all(|row| row.id != id));

    // A payload over the cap is stored as a stub that says so. (No ordinary
    // action produces one, so the event is triggered directly.)
    let repo: Value = f
        .client
        .get(f.url(""))
        .bearer_auth(&f.jwt)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let repo_id = repo["id"].as_i64().unwrap();
    rg_core::webhook::service::trigger_event(
        &db,
        repo_id,
        "test.big",
        &json!({ "blob": "x".repeat(20_000) }),
    )
    .await
    .unwrap();
    let (_, v) = f.events(&f.jwt, "?since=0&types=test.big").await;
    let event = v["events"].as_array().unwrap().last().unwrap().clone();
    assert_eq!(event["payload"]["truncated"], json!(true), "{event}");
    assert!(event["payload"]["original_bytes"].as_i64().unwrap() > 16 * 1024);
}
