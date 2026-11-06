//! The repository scopes of a personal access token form a ladder:
//! `repo:read` < `repo:collab` < `repo:write` < `repo` (the historical scope).
//!
//! A token is checked against the request it is used for: reads need
//! `repo:read`, writes inside a repository's collaboration area need
//! `repo:collab`, every other write (and `git push`) needs `repo:write`. The
//! historical `repo` scope satisfies all of them, so old tokens are unchanged.

mod common;
use common::{register_user, spawn_test_app};

use reqwest::{Client, Method, StatusCode};
use serde_json::{json, Value};

struct Fixture {
    base: String,
    owner: String,
    repo: String,
    /// Tokens by the scopes they were issued with.
    read: String,
    collab: String,
    write: String,
    legacy: String,
    admin: String,
}

async fn token(base: &str, jwt: &str, scopes: &str) -> String {
    let resp = Client::new()
        .post(format!("{base}/api/v1/users/tokens"))
        .bearer_auth(jwt)
        .json(&json!({"name": format!("t-{scopes}"), "scopes": scopes}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201, "create {scopes} token");
    let body: Value = resp.json().await.unwrap();
    body["token"].as_str().unwrap().to_string()
}

async fn fixture() -> Fixture {
    let base = spawn_test_app().await;
    let owner = "ladder_owner".to_string();
    let jwt = register_user(&base, &owner, "ladder@example.com", "Qz7$wRtm").await;
    // Private, so that git access proves the token and not anonymous access.
    let resp = Client::new()
        .post(format!("{base}/api/v1/repos"))
        .bearer_auth(&jwt)
        .json(&json!({"name": "private-repo", "is_private": true}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);
    Fixture {
        read: token(&base, &jwt, "repo:read").await,
        collab: token(&base, &jwt, "repo:collab").await,
        write: token(&base, &jwt, "repo:write").await,
        legacy: token(&base, &jwt, "repo").await,
        admin: token(&base, &jwt, "admin").await,
        base,
        owner,
        repo: "private-repo".into(),
    }
}

impl Fixture {
    fn api(&self, suffix: &str) -> String {
        format!(
            "{}/api/v1/repos/{}/{}{suffix}",
            self.base, self.owner, self.repo
        )
    }

    async fn status(
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

fn ok(status: StatusCode) -> bool {
    status.is_success()
}

#[tokio::test]
async fn reading_needs_only_repo_read() {
    let f = fixture().await;
    for (name, tok) in [
        ("read", &f.read),
        ("collab", &f.collab),
        ("write", &f.write),
        ("repo", &f.legacy),
        ("admin", &f.admin),
    ] {
        let s = f.status(Method::GET, f.api(""), tok, None).await;
        assert!(ok(s), "{name} token must read the repository, got {s}");
        let s = f.status(Method::GET, f.api("/issues"), tok, None).await;
        assert!(ok(s), "{name} token must list issues, got {s}");
    }
}

#[tokio::test]
async fn an_ordinary_write_needs_repo_write() {
    let f = fixture().await;
    let issue = json!({"title": "from a token"});
    for (name, tok, allowed) in [
        ("read", &f.read, false),
        ("collab", &f.collab, false),
        ("write", &f.write, true),
        ("repo", &f.legacy, true),
        ("admin", &f.admin, true),
    ] {
        let s = f
            .status(Method::POST, f.api("/issues"), tok, Some(issue.clone()))
            .await;
        if allowed {
            assert_eq!(s, StatusCode::CREATED, "{name} may write");
        } else {
            assert_eq!(s, StatusCode::FORBIDDEN, "{name} must not write");
        }
    }
}

#[tokio::test]
async fn the_collaboration_area_needs_repo_collab() {
    let f = fixture().await;
    let session = json!({"display_name": "agent", "client_kind": "test"});
    for (name, tok, allowed) in [
        ("read", &f.read, false),
        ("collab", &f.collab, true),
        ("write", &f.write, true),
        ("repo", &f.legacy, true),
    ] {
        let s = f
            .status(
                Method::POST,
                f.api("/collaboration/sessions"),
                tok,
                Some(session.clone()),
            )
            .await;
        if allowed {
            assert_eq!(s, StatusCode::CREATED, "{name} may use collaboration");
        } else {
            assert_eq!(
                s,
                StatusCode::FORBIDDEN,
                "{name} must not write collaboration"
            );
        }
    }
    // Reading collaboration needs only read.
    let s = f
        .status(Method::GET, f.api("/collaboration/tasks"), &f.read, None)
        .await;
    assert!(ok(s), "read token lists tasks, got {s}");
}

#[tokio::test]
async fn a_collab_token_cannot_reach_other_writes_through_the_collaboration_path() {
    let f = fixture().await;
    // Lookalike paths are ordinary writes, so a collab token is refused. (An HTTP
    // client may resolve `..` before sending; the path logic itself is covered
    // exhaustively by the unit tests next to `required_pat_scope`.)
    for path in [
        "/collaborationx",
        "/collaboration/../issues",
        "/issues/collaboration",
    ] {
        let s = f
            .status(
                Method::POST,
                f.api(path),
                &f.collab,
                Some(json!({"title": "x"})),
            )
            .await;
        assert_eq!(s, StatusCode::FORBIDDEN, "{path}");
    }
}

#[tokio::test]
async fn git_fetch_needs_read_and_push_needs_write() {
    let f = fixture().await;
    let url = |service: &str| {
        format!(
            "{}/git/{}/{}/info/refs?service={service}",
            f.base, f.owner, f.repo
        )
    };
    let get = |service: &'static str, tok: &str| {
        let req = Client::new().get(url(service)).bearer_auth(tok.to_string());
        async move { req.send().await.unwrap().status() }
    };
    for (name, tok) in [
        ("read", &f.read),
        ("collab", &f.collab),
        ("write", &f.write),
        ("repo", &f.legacy),
    ] {
        let s = get("git-upload-pack", tok).await;
        assert_eq!(s, StatusCode::OK, "{name} may fetch");
    }
    for (name, tok, allowed) in [
        ("read", &f.read, false),
        ("collab", &f.collab, false),
        ("write", &f.write, true),
        ("repo", &f.legacy, true),
    ] {
        let s = get("git-receive-pack", tok).await;
        if allowed {
            assert_eq!(s, StatusCode::OK, "{name} may push");
        } else {
            assert!(
                s == StatusCode::UNAUTHORIZED || s == StatusCode::FORBIDDEN,
                "{name} must not push, got {s}"
            );
        }
    }
}

#[tokio::test]
async fn repository_scopes_do_not_open_the_account_family() {
    let f = fixture().await;
    for tok in [&f.read, &f.collab, &f.write] {
        let s = f
            .status(
                Method::POST,
                format!("{}/api/v1/users/tokens", f.base),
                tok,
                Some(json!({"name": "escalate", "scopes": "admin"})),
            )
            .await;
        assert_eq!(
            s,
            StatusCode::FORBIDDEN,
            "a repo-family token cannot mint tokens"
        );
        let s = f
            .status(
                Method::GET,
                format!("{}/api/v1/users/me", f.base),
                tok,
                None,
            )
            .await;
        assert_eq!(s, StatusCode::FORBIDDEN);
    }
}

#[tokio::test]
async fn scopes_can_be_combined_and_are_stored_canonically() {
    let base = spawn_test_app().await;
    let jwt = register_user(&base, "ladder_combo", "combo@example.com", "Qz7$wRtm").await;
    let resp = Client::new()
        .post(format!("{base}/api/v1/users/tokens"))
        .bearer_auth(&jwt)
        .json(&json!({"name": "combo", "scopes": "user repo:read"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["scopes"], "repo:read,user");
    let tok = body["token"].as_str().unwrap();
    // Both families open, and still read-only.
    let me = Client::new()
        .get(format!("{base}/api/v1/users/me"))
        .bearer_auth(tok)
        .send()
        .await
        .unwrap();
    assert_eq!(me.status(), 200);
    let write = Client::new()
        .post(format!("{base}/api/v1/repos"))
        .bearer_auth(tok)
        .json(&json!({"name": "nope"}))
        .send()
        .await
        .unwrap();
    assert_eq!(write.status(), 403);
}
