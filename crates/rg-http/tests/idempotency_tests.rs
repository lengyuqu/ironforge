//! `Idempotency-Key`: a retried create request gets the first response back.
//!
//! The key is scoped to the user and (for a personal access token) to the token;
//! the same key with a different request is refused; a request that ran into a
//! server error does not keep the key; keys expire after 24 hours; and routes
//! whose response carries a secret do not take the header at all.

mod common;
use common::{register_user, spawn_test_app_on_env_db};

use reqwest::{Client, StatusCode};
use serde_json::{json, Value};

const PW: &str = "Qz7$wRtm";

struct Fixture {
    base: String,
    jwt: String,
    client: Client,
}

impl Fixture {
    async fn new(owner: &str) -> Fixture {
        let base = spawn_test_app_on_env_db().await.0;
        Fixture::on(base, owner).await
    }

    async fn on(base: String, owner: &str) -> Fixture {
        let jwt = register_user(&base, owner, &format!("{owner}@example.com"), PW).await;
        let client = Client::new();
        let resp = client
            .post(format!("{base}/api/v1/repos"))
            .bearer_auth(&jwt)
            .json(&json!({"name": "work", "auto_init": true, "is_private": true}))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 201);
        Fixture { base, jwt, client }
    }

    fn url(&self, owner: &str, suffix: &str) -> String {
        format!("{}/api/v1/repos/{owner}/work{suffix}", self.base)
    }

    async fn send(
        &self,
        method: reqwest::Method,
        url: String,
        bearer: &str,
        key: Option<&str>,
        body: Option<Value>,
    ) -> (StatusCode, bool, Value) {
        let mut req = self.client.request(method, url).bearer_auth(bearer);
        if let Some(key) = key {
            req = req.header("idempotency-key", key);
        }
        if let Some(body) = body {
            req = req.json(&body);
        }
        let resp = req.send().await.unwrap();
        let status = resp.status();
        let replayed = resp.headers().get("idempotent-replay").is_some();
        (status, replayed, resp.json().await.unwrap_or(Value::Null))
    }

    async fn issue_count(&self, owner: &str) -> usize {
        let v: Value = self
            .client
            .get(self.url(owner, "/issues"))
            .bearer_auth(&self.jwt)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        v.as_array()
            .cloned()
            .or_else(|| v["data"].as_array().cloned())
            .unwrap_or_default()
            .len()
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_retried_create_returns_the_first_response_and_creates_nothing_more() {
    let f = Fixture::new("idem_a").await;
    let url = f.url("idem_a", "/issues");
    let body = json!({"title": "found a bug"});

    let (s1, replay1, v1) = f
        .send(
            reqwest::Method::POST,
            url.clone(),
            &f.jwt,
            Some("create-bug-1"),
            Some(body.clone()),
        )
        .await;
    assert_eq!(s1, StatusCode::CREATED, "{v1}");
    assert!(!replay1);
    let (s2, replay2, v2) = f
        .send(
            reqwest::Method::POST,
            url.clone(),
            &f.jwt,
            Some("create-bug-1"),
            Some(body.clone()),
        )
        .await;
    assert_eq!(s2, StatusCode::CREATED, "{v2}");
    assert!(replay2, "the second answer is marked as a replay");
    assert_eq!(v1, v2, "the very same response");
    assert_eq!(f.issue_count("idem_a").await, 1);

    // The same key with a different body is refused, and creates nothing.
    let (s3, _, v3) = f
        .send(
            reqwest::Method::POST,
            url.clone(),
            &f.jwt,
            Some("create-bug-1"),
            Some(json!({"title": "something else"})),
        )
        .await;
    assert_eq!(s3, StatusCode::UNPROCESSABLE_ENTITY, "{v3}");
    assert_eq!(v3["error"]["reason"], "IDEMPOTENCY_KEY_REUSED");
    assert_eq!(v3["error"]["retryable"], json!(false));
    assert_eq!(f.issue_count("idem_a").await, 1);

    // A new key is a new request; no key is the old behaviour.
    let (s4, replay4, _) = f
        .send(
            reqwest::Method::POST,
            url.clone(),
            &f.jwt,
            Some("create-bug-2"),
            Some(body.clone()),
        )
        .await;
    assert_eq!(s4, StatusCode::CREATED);
    assert!(!replay4);
    let (s5, _, _) = f
        .send(reqwest::Method::POST, url, &f.jwt, None, Some(body))
        .await;
    assert_eq!(s5, StatusCode::CREATED);
    assert_eq!(f.issue_count("idem_a").await, 3);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn keys_are_scoped_to_the_token_that_sent_them() {
    let f = Fixture::new("idem_b").await;
    let make = |name: &'static str| {
        let f = &f;
        async move {
            let v: Value = f
                .client
                .post(format!("{}/api/v1/users/tokens", f.base))
                .bearer_auth(&f.jwt)
                .json(&json!({"name": name, "scopes": "repo:write"}))
                .send()
                .await
                .unwrap()
                .json()
                .await
                .unwrap();
            v["token"].as_str().unwrap().to_string()
        }
    };
    let (one, two) = (make("one").await, make("two").await);
    let url = f.url("idem_b", "/issues");
    let body = json!({"title": "same key, different clients"});

    let (_, _, a) = f
        .send(
            reqwest::Method::POST,
            url.clone(),
            &one,
            Some("shared-key"),
            Some(body.clone()),
        )
        .await;
    let (_, _, b) = f
        .send(
            reqwest::Method::POST,
            url.clone(),
            &two,
            Some("shared-key"),
            Some(body.clone()),
        )
        .await;
    let (_, _, c) = f
        .send(
            reqwest::Method::POST,
            url.clone(),
            &f.jwt,
            Some("shared-key"),
            Some(body.clone()),
        )
        .await;
    // Three clients, three issues, three different numbers.
    let numbers: Vec<_> = [&a, &b, &c].iter().map(|v| v["number"].clone()).collect();
    assert!(numbers.iter().all(|n| n.is_number()), "{a} {b} {c}");
    assert_eq!(numbers.len(), 3);
    assert!(
        numbers[0] != numbers[1] && numbers[1] != numbers[2] && numbers[0] != numbers[2],
        "{numbers:?}"
    );
    assert_eq!(f.issue_count("idem_b").await, 3);

    // Each client's own retry is replayed.
    let (_, replayed, again) = f
        .send(
            reqwest::Method::POST,
            url,
            &one,
            Some("shared-key"),
            Some(body),
        )
        .await;
    assert!(replayed);
    assert_eq!(again["number"], a["number"]);
    assert_eq!(f.issue_count("idem_b").await, 3);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_client_error_is_replayed_and_a_bad_key_or_route_is_refused() {
    let f = Fixture::new("idem_c").await;
    let url = f.url("idem_c", "/issues");

    // A request the server rejects (no title) is replayed the same way: the retry
    // does not turn into a different answer.
    let (s1, _, v1) = f
        .send(
            reqwest::Method::POST,
            url.clone(),
            &f.jwt,
            Some("bad-body"),
            Some(json!({"title": ""})),
        )
        .await;
    assert!(s1.is_client_error(), "{s1} {v1}");
    let (s2, replayed, v2) = f
        .send(
            reqwest::Method::POST,
            url.clone(),
            &f.jwt,
            Some("bad-body"),
            Some(json!({"title": ""})),
        )
        .await;
    assert_eq!(s2, s1);
    assert!(replayed);
    // The same answer; only the per-request id the server stamps on an error is new.
    let without_request_id = |mut v: Value| {
        v["error"].as_object_mut().map(|e| e.remove("request_id"));
        v
    };
    assert_eq!(without_request_id(v1), without_request_id(v2));

    // A key that is not a plain token is refused before anything runs.
    for key in ["has space", "a/b", &"k".repeat(256)] {
        let (s, _, v) = f
            .send(
                reqwest::Method::POST,
                url.clone(),
                &f.jwt,
                Some(key),
                Some(json!({"title": "t"})),
            )
            .await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{key}: {v}");
    }
    assert_eq!(f.issue_count("idem_c").await, 0);

    // A route whose response carries a secret takes no key: it is refused, not
    // silently ignored, and nothing is created.
    let (s, _, v) = f
        .send(
            reqwest::Method::POST,
            format!("{}/api/v1/users/tokens", f.base),
            &f.jwt,
            Some("token-key"),
            Some(json!({"name": "t", "scopes": "repo:read"})),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
    assert_eq!(v["error"]["reason"], "IDEMPOTENCY_NOT_SUPPORTED");
    let list: Value = f
        .client
        .get(format!("{}/api/v1/users/tokens", f.base))
        .bearer_auth(&f.jwt)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let tokens = list
        .as_array()
        .cloned()
        .or_else(|| list["data"].as_array().cloned())
        .unwrap_or_default();
    assert!(tokens.is_empty(), "{list}");

    // A read with the header is just a read.
    let (s, replayed, _) = f
        .send(reqwest::Method::GET, url, &f.jwt, Some("read-key"), None)
        .await;
    assert_eq!(s, StatusCode::OK);
    assert!(!replayed);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn two_simultaneous_requests_with_one_key_create_one_thing() {
    let f = Fixture::new("idem_d").await;
    let url = f.url("idem_d", "/issues");
    let body = json!({"title": "raced"});
    let (a, b) = tokio::join!(
        f.send(
            reqwest::Method::POST,
            url.clone(),
            &f.jwt,
            Some("race-key"),
            Some(body.clone())
        ),
        f.send(
            reqwest::Method::POST,
            url.clone(),
            &f.jwt,
            Some("race-key"),
            Some(body.clone())
        ),
    );
    // One ran; the other either got its stored answer or was told to try again.
    let outcomes = [a, b];
    let created = outcomes
        .iter()
        .filter(|(s, replayed, _)| *s == StatusCode::CREATED && !replayed)
        .count();
    assert_eq!(created, 1, "exactly one request ran: {outcomes:?}");
    for (status, replayed, v) in &outcomes {
        let ok = (*status == StatusCode::CREATED)
            || (*status == StatusCode::CONFLICT
                && v["error"]["reason"] == "IDEMPOTENCY_IN_PROGRESS");
        assert!(ok, "{status} {replayed} {v}");
    }
    assert_eq!(f.issue_count("idem_d").await, 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_key_older_than_a_day_can_be_used_again() {
    use rg_db::sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};
    let (base, db) = spawn_test_app_on_env_db().await;
    let f = Fixture::on(base, "idem_e").await;
    let url = f.url("idem_e", "/issues");
    let body = json!({"title": "once a day"});
    let (s, _, first) = f
        .send(
            reqwest::Method::POST,
            url.clone(),
            &f.jwt,
            Some("daily"),
            Some(body.clone()),
        )
        .await;
    assert_eq!(s, StatusCode::CREATED);

    // Make the stored key 25 hours old.
    let row = rg_db::entities::idempotency_key::Entity::find()
        .filter(rg_db::entities::idempotency_key::Column::IdemKey.eq("daily"))
        .one(&db)
        .await
        .unwrap()
        .expect("key stored");
    let mut active: rg_db::entities::idempotency_key::ActiveModel = row.into();
    active.created_at = Set(chrono::Utc::now() - chrono::Duration::hours(25));
    active.update(&db).await.unwrap();

    let (s, replayed, second) = f
        .send(
            reqwest::Method::POST,
            url,
            &f.jwt,
            Some("daily"),
            Some(body),
        )
        .await;
    assert_eq!(s, StatusCode::CREATED);
    assert!(!replayed, "an expired key runs the request again");
    assert_ne!(first["number"], second["number"]);
    assert_eq!(f.issue_count("idem_e").await, 2);
}
