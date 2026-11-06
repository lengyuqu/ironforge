//! A personal access token can be limited to some repositories.
//!
//! The limit is checked where the token is used: the REST middleware (only
//! `/repos/{owner}/{name}…` and `/ai/repos/{owner}/{name}…` for a listed
//! repository), and the git-over-HTTP entry points. A token with no list is
//! unchanged.

mod common;
use common::{register_user, spawn_test_app};

use reqwest::{Client, Method, StatusCode};
use serde_json::{json, Value};

struct Fixture {
    base: String,
    owner: String,
    jwt: String,
    /// Limited to `owner/allowed-repo`.
    limited: String,
    /// Same scope, no limit.
    unlimited: String,
}

async fn make_repo(base: &str, jwt: &str, name: &str) {
    let resp = Client::new()
        .post(format!("{base}/api/v1/repos"))
        .bearer_auth(jwt)
        .json(&json!({"name": name, "is_private": true}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201, "create {name}");
}

async fn make_token(base: &str, jwt: &str, body: Value) -> (StatusCode, Value) {
    let resp = Client::new()
        .post(format!("{base}/api/v1/users/tokens"))
        .bearer_auth(jwt)
        .json(&body)
        .send()
        .await
        .unwrap();
    let status = resp.status();
    (status, resp.json().await.unwrap_or(Value::Null))
}

async fn fixture() -> Fixture {
    let base = spawn_test_app().await;
    let owner = "allow_owner".to_string();
    let jwt = register_user(&base, &owner, "allow@example.com", "Qz7$wRtm").await;
    make_repo(&base, &jwt, "allowed-repo").await;
    make_repo(&base, &jwt, "other-repo").await;
    let (s, limited) = make_token(
        &base,
        &jwt,
        json!({"name": "limited", "scopes": "repo:write", "repos": [format!("{owner}/allowed-repo")]}),
    )
    .await;
    assert_eq!(s, 201, "{limited}");
    let (s, unlimited) = make_token(
        &base,
        &jwt,
        json!({"name": "unlimited", "scopes": "repo:write"}),
    )
    .await;
    assert_eq!(s, 201);
    Fixture {
        base,
        owner,
        jwt,
        limited: limited["token"].as_str().unwrap().to_string(),
        unlimited: unlimited["token"].as_str().unwrap().to_string(),
    }
}

impl Fixture {
    fn api(&self, repo: &str, suffix: &str) -> String {
        format!("{}/api/v1/repos/{}/{repo}{suffix}", self.base, self.owner)
    }

    async fn send(
        &self,
        method: Method,
        url: String,
        token: &str,
        body: Option<Value>,
    ) -> StatusCode {
        let mut req = Client::new().request(method, url).bearer_auth(token);
        if let Some(body) = body {
            req = req.json(&body);
        }
        req.send().await.unwrap().status()
    }
}

#[tokio::test]
async fn the_limit_is_validated_when_the_token_is_created() {
    let base = spawn_test_app().await;
    let jwt = register_user(&base, "allow_valid", "valid@example.com", "Qz7$wRtm").await;
    for (why, body) in [
        (
            "empty list",
            json!({"name": "t", "scopes": "repo", "repos": []}),
        ),
        (
            "no slash",
            json!({"name": "t", "scopes": "repo", "repos": ["demo"]}),
        ),
        (
            "traversal",
            json!({"name": "t", "scopes": "repo", "repos": ["a/.."]}),
        ),
        (
            "odd characters",
            json!({"name": "t", "scopes": "repo", "repos": ["a/b c"]}),
        ),
        (
            "account scope",
            json!({"name": "t", "scopes": "repo,user", "repos": ["a/b"]}),
        ),
        (
            "admin scope",
            json!({"name": "t", "scopes": "admin", "repos": ["a/b"]}),
        ),
    ] {
        let (status, body) = make_token(&base, &jwt, body).await;
        assert_eq!(status, 400, "{why}: {body}");
    }

    let (status, body) = make_token(
        &base,
        &jwt,
        json!({"name": "t", "scopes": "repo:collab", "repos": ["bob/zeta", "alice/demo", "alice/demo"]}),
    )
    .await;
    assert_eq!(status, 201, "{body}");
    assert_eq!(body["repos"], json!(["alice/demo", "bob/zeta"]));

    // The list shows the limit, and an unlimited token has no `repos` at all.
    let (_, plain) = make_token(&base, &jwt, json!({"name": "plain", "scopes": "repo"})).await;
    assert!(plain["repos"].is_null());
    let listed: Value = Client::new()
        .get(format!("{base}/api/v1/users/tokens"))
        .bearer_auth(&jwt)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let limited = listed
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "t")
        .unwrap();
    assert_eq!(limited["repos"], json!(["alice/demo", "bob/zeta"]));
    let plain = listed
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "plain")
        .unwrap();
    assert!(plain.get("repos").is_none());
}

#[tokio::test]
async fn a_limited_token_reaches_only_the_listed_repository() {
    let f = fixture().await;
    let t = &f.limited;

    // The listed repository: read and write.
    assert!(f
        .send(Method::GET, f.api("allowed-repo", ""), t, None)
        .await
        .is_success());
    assert_eq!(
        f.send(
            Method::POST,
            f.api("allowed-repo", "/issues"),
            t,
            Some(json!({"title": "ok"}))
        )
        .await,
        StatusCode::CREATED
    );
    assert_eq!(
        f.send(
            Method::POST,
            f.api("allowed-repo", "/collaboration/sessions"),
            t,
            Some(json!({"display_name": "a", "client_kind": "t"})),
        )
        .await,
        StatusCode::CREATED
    );

    // Another repository of the same owner, which the user can fully use.
    for (method, suffix, body) in [
        (Method::GET, "", None),
        (Method::GET, "/issues", None),
        (Method::POST, "/issues", Some(json!({"title": "no"}))),
        (Method::GET, "/collaboration/tasks", None),
    ] {
        assert_eq!(
            f.send(method.clone(), f.api("other-repo", suffix), t, body)
                .await,
            StatusCode::FORBIDDEN,
            "{method} other-repo{suffix}"
        );
    }
    // The unlimited token of the same user can, so the refusal is the limit.
    assert!(f
        .send(Method::GET, f.api("other-repo", ""), &f.unlimited, None)
        .await
        .is_success());
}

#[tokio::test]
async fn a_limited_token_cannot_use_routes_that_are_not_tied_to_a_listed_repository() {
    let f = fixture().await;
    let t = &f.limited;
    let base = &f.base;
    for (method, url, body) in [
        (Method::GET, format!("{base}/api/v1/repos"), None),
        (
            Method::POST,
            format!("{base}/api/v1/repos"),
            Some(json!({"name": "new"})),
        ),
        (Method::GET, format!("{base}/api/v1/search?q=x"), None),
        (Method::GET, format!("{base}/api/v1/orgs"), None),
        (Method::GET, format!("{base}/api/v1/imports"), None),
        // The account family needs `user`, which a limited token cannot have.
        (Method::GET, format!("{base}/api/v1/users/me"), None),
        (
            Method::POST,
            format!("{base}/api/v1/users/tokens"),
            Some(json!({"name": "mint", "scopes": "admin"})),
        ),
        // Routes inside the listed repository that reach other repositories.
        (
            Method::POST,
            f.api("allowed-repo", "/fork"),
            Some(json!({})),
        ),
        (Method::GET, f.api("allowed-repo", "/forks"), None),
        (
            Method::GET,
            f.api("allowed-repo", "/collaboration/groups"),
            None,
        ),
    ] {
        assert_eq!(
            f.send(method.clone(), url.clone(), t, body).await,
            StatusCode::FORBIDDEN,
            "{method} {url}"
        );
    }
}

#[tokio::test]
async fn git_over_http_honours_the_limit() {
    let f = fixture().await;
    let refs = |repo: &str, service: &str, tok: &str| {
        let req = Client::new()
            .get(format!(
                "{}/git/{}/{repo}/info/refs?service={service}",
                f.base, f.owner
            ))
            .bearer_auth(tok.to_string());
        async move { req.send().await.unwrap().status() }
    };
    for service in ["git-upload-pack", "git-receive-pack"] {
        assert_eq!(
            refs("allowed-repo", service, &f.limited).await,
            StatusCode::OK,
            "{service}"
        );
        let s = refs("other-repo", service, &f.limited).await;
        assert!(
            s == StatusCode::UNAUTHORIZED || s == StatusCode::FORBIDDEN,
            "{service} on the other repository: {s}"
        );
        // Without the limit the same token scope works on both.
        assert_eq!(
            refs("other-repo", service, &f.unlimited).await,
            StatusCode::OK,
            "{service}"
        );
    }
}

#[tokio::test]
async fn a_token_without_a_limit_behaves_as_before() {
    let f = fixture().await;
    for repo in ["allowed-repo", "other-repo"] {
        assert!(f
            .send(Method::GET, f.api(repo, ""), &f.unlimited, None)
            .await
            .is_success());
    }
    assert_eq!(
        f.send(
            Method::POST,
            format!("{}/api/v1/repos", f.base),
            &f.unlimited,
            Some(json!({"name": "created-by-token"})),
        )
        .await,
        StatusCode::CREATED
    );
    // And the owner's own session is not affected by any token's limit.
    assert!(f
        .send(Method::GET, f.api("other-repo", ""), &f.jwt, None)
        .await
        .is_success());
}
