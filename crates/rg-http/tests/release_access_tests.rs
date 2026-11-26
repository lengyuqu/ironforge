//! Access rules for release and mirror routes.
//!
//! Guards:
//!   - a browser session (HttpOnly cookie, no Bearer header) is accepted by the
//!     write handlers;
//!   - a private repository's releases and mirror settings need read/write
//!     access, and a mirror row is never public;
//!   - a release or asset id only resolves through the repository it belongs to;
//!   - a mirror URL may not carry credentials.

mod common;

use common::{create_repo, register_user, spawn_test_app};
use reqwest::{Client, StatusCode};

const PW: &str = "Qz7$wRtm";

async fn register(base: &str, name: &str) -> String {
    register_user(base, name, &format!("{name}@example.com"), PW).await
}

async fn create_repo_with(base: &str, token: &str, body: serde_json::Value) {
    let resp = Client::new()
        .post(format!("{base}/api/v1/repos"))
        .bearer_auth(token)
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201, "create_repo failed: {}", resp.status());
}

/// Create a release and return its id.
async fn create_release(base: &str, token: &str, owner: &str, repo: &str, tag: &str) -> i64 {
    let resp = Client::new()
        .post(format!("{base}/api/v1/repos/{owner}/{repo}/releases"))
        .bearer_auth(token)
        .json(&serde_json::json!({"tag_name": tag, "title": "Title"}))
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        201,
        "create release failed: {}",
        resp.status()
    );
    resp.json::<serde_json::Value>().await.unwrap()["id"]
        .as_i64()
        .unwrap()
}

/// Log in and return the session cookie the browser would hold.
async fn session_cookie(base: &str, username: &str) -> String {
    let resp = Client::new()
        .post(format!("{base}/api/v1/users/login"))
        .json(&serde_json::json!({"username": username, "password": PW}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    resp.headers()
        .get_all(reqwest::header::SET_COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .find_map(|value| value.split(';').next().map(str::to_string))
        .expect("login sets the session cookie")
}

fn assert_status(actual: StatusCode, expected: u16, what: &str) {
    assert_eq!(
        actual.as_u16(),
        expected,
        "{what}: expected {expected}, got {actual}"
    );
}

#[tokio::test]
async fn cookie_session_is_accepted_by_write_handlers() {
    let base = spawn_test_app().await;
    register(&base, "ckowner").await;
    let cookie = session_cookie(&base, "ckowner").await;
    let client = Client::new();

    // No Bearer header anywhere: the browser only sends the cookie.
    let created = client
        .post(format!("{base}/api/v1/repos"))
        .header(reqwest::header::COOKIE, &cookie)
        .json(&serde_json::json!({"name": "ckrepo", "auto_init": true}))
        .send()
        .await
        .unwrap();
    assert_status(created.status(), 201, "create repo with a cookie session");

    let edited = client
        .post(format!(
            "{base}/api/v1/repos/ckowner/ckrepo/contents/hello.txt"
        ))
        .header(reqwest::header::COOKIE, &cookie)
        .json(&serde_json::json!({"message": "add file", "content": "hello"}))
        .send()
        .await
        .unwrap();
    assert_status(edited.status(), 200, "edit a file with a cookie session");

    // Without any credential the same calls are still refused.
    let anonymous = client
        .post(format!("{base}/api/v1/repos"))
        .json(&serde_json::json!({"name": "ckrepo2"}))
        .send()
        .await
        .unwrap();
    assert_status(anonymous.status(), 401, "create repo anonymously");
}

#[tokio::test]
async fn private_releases_and_mirrors_need_access() {
    let base = spawn_test_app().await;
    let owner = register(&base, "pvowner").await;
    let outsider = register(&base, "pvother").await;
    create_repo_with(
        &base,
        &owner,
        serde_json::json!({"name": "pvrepo", "is_private": true}),
    )
    .await;
    let id = create_release(&base, &owner, "pvowner", "pvrepo", "v1.0.0").await;
    let client = Client::new();
    let repo = format!("{base}/api/v1/repos/pvowner/pvrepo");

    for path in [
        format!("{repo}/releases"),
        format!("{repo}/releases/{id}"),
        format!("{repo}/releases/tags/v1.0.0"),
        format!("{repo}/releases/{id}/assets"),
    ] {
        let anonymous = client.get(&path).send().await.unwrap();
        assert_status(anonymous.status(), 401, &path);
        let denied = client
            .get(&path)
            .bearer_auth(&outsider)
            .send()
            .await
            .unwrap();
        assert_status(denied.status(), 403, &path);
        let allowed = client.get(&path).bearer_auth(&owner).send().await.unwrap();
        assert_status(allowed.status(), 200, &path);
    }

    for (path, body) in [
        (
            format!("{repo}/mirror"),
            serde_json::json!({"url": "http://127.0.0.1:9/remote.git", "sync_interval_seconds": 3600}),
        ),
        (
            format!("{repo}/push-mirror"),
            serde_json::json!({"url": "http://127.0.0.1:9/remote.git"}),
        ),
    ] {
        let created = client
            .post(&path)
            .bearer_auth(&owner)
            .json(&body)
            .send()
            .await
            .unwrap();
        assert_status(created.status(), 201, &path);

        let anonymous = client.get(&path).send().await.unwrap();
        assert_status(anonymous.status(), 401, &path);
        let denied = client
            .get(&path)
            .bearer_auth(&outsider)
            .send()
            .await
            .unwrap();
        assert_status(denied.status(), 403, &path);
        let allowed = client.get(&path).bearer_auth(&owner).send().await.unwrap();
        assert_status(allowed.status(), 200, &path);
    }
}

#[tokio::test]
async fn mirror_settings_are_not_public_on_a_public_repo() {
    let base = spawn_test_app().await;
    let owner = register(&base, "pbowner").await;
    let outsider = register(&base, "pbother").await;
    create_repo(&base, &owner, "pbrepo").await;
    let client = Client::new();
    let url = format!("{base}/api/v1/repos/pbowner/pbrepo/mirror");

    assert_status(
        client.get(&url).send().await.unwrap().status(),
        401,
        "anonymous mirror settings on a public repo",
    );
    assert_status(
        client
            .get(&url)
            .bearer_auth(&outsider)
            .send()
            .await
            .unwrap()
            .status(),
        403,
        "reader mirror settings on a public repo",
    );
}

#[tokio::test]
async fn release_ids_resolve_only_through_their_own_repository() {
    let base = spawn_test_app().await;
    let owner = register(&base, "scowner").await;
    let outsider = register(&base, "scother").await;
    create_repo(&base, &owner, "screpo").await;
    create_repo(&base, &owner, "scother").await;
    create_repo(&base, &outsider, "scforeign").await;
    let id = create_release(&base, &owner, "scowner", "screpo", "v1.0.0").await;
    let client = Client::new();
    let repos = format!("{base}/api/v1/repos");
    // Each caller may write to the repository named in the route; the id still
    // must not resolve to another repository's release. The outsider has no
    // write access to the release's own repository.
    for (wrong, token) in [
        ("scowner/scother", &owner),
        ("scother/scforeign", &outsider),
    ] {
        for path in [
            format!("{repos}/{wrong}/releases/{id}"),
            format!("{repos}/{wrong}/releases/{id}/assets"),
        ] {
            let read = client.get(&path).bearer_auth(token).send().await.unwrap();
            assert_status(read.status(), 404, &path);
        }

        let updated = client
            .patch(format!("{repos}/{wrong}/releases/{id}"))
            .bearer_auth(token)
            .json(&serde_json::json!({"title": "rewritten"}))
            .send()
            .await
            .unwrap();
        assert_status(updated.status(), 404, &format!("patch through {wrong}"));

        let deleted = client
            .delete(format!("{repos}/{wrong}/releases/{id}"))
            .bearer_auth(token)
            .send()
            .await
            .unwrap();
        assert_status(deleted.status(), 404, &format!("delete through {wrong}"));

        let uploaded = client
            .post(format!("{repos}/{wrong}/releases/{id}/assets"))
            .bearer_auth(token)
            .header("x-asset-filename", "a.txt")
            .body("payload")
            .send()
            .await
            .unwrap();
        assert_status(uploaded.status(), 404, &format!("upload through {wrong}"));
    }

    // The release is untouched, and its own repository still serves it.
    let own = client
        .get(format!("{repos}/scowner/screpo/releases/{id}"))
        .bearer_auth(&owner)
        .send()
        .await
        .unwrap();
    assert_status(own.status(), 200, "release through its own repository");
    let body: serde_json::Value = own.json().await.unwrap();
    assert_eq!(body["title"], "Title");
    let assets = client
        .get(format!("{repos}/scowner/screpo/releases/{id}/assets"))
        .bearer_auth(&owner)
        .send()
        .await
        .unwrap();
    assert_eq!(
        assets
            .json::<serde_json::Value>()
            .await
            .unwrap()
            .as_array()
            .map(Vec::len),
        Some(0)
    );
}

#[tokio::test]
async fn mirror_urls_may_not_embed_credentials() {
    let base = spawn_test_app().await;
    let owner = register(&base, "crowner").await;
    create_repo(&base, &owner, "crrepo").await;
    let client = Client::new();
    let repos = format!("{base}/api/v1/repos/crowner/crrepo");

    for url in [
        "http://alice:pat@127.0.0.1:9/remote.git",
        "http://alice@127.0.0.1:9/remote.git",
    ] {
        for (path, body) in [
            (
                format!("{repos}/mirror"),
                serde_json::json!({"url": url, "sync_interval_seconds": 3600}),
            ),
            (
                format!("{repos}/push-mirror"),
                serde_json::json!({"url": url}),
            ),
        ] {
            let resp = client
                .post(&path)
                .bearer_auth(&owner)
                .json(&body)
                .send()
                .await
                .unwrap();
            assert_status(resp.status(), 400, &format!("{path} with {url}"));
        }
    }
    assert_eq!(
        client
            .get(format!("{repos}/mirror"))
            .bearer_auth(&owner)
            .send()
            .await
            .unwrap()
            .status(),
        404,
        "no mirror row is written for a rejected URL"
    );

    // An `@` in the path is not a credential.
    let plain = client
        .post(format!("{repos}/mirror"))
        .bearer_auth(&owner)
        .json(&serde_json::json!({
            "url": "http://127.0.0.1:9/owner@scope.git",
            "sync_interval_seconds": 3600
        }))
        .send()
        .await
        .unwrap();
    assert_status(plain.status(), 201, "mirror URL with an @ in the path");
}
