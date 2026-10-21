//! `GET /repos/{owner}/{name}/log?path=`: file and directory history.

mod common;

use common::{register_user, spawn_test_app};

const PW: &str = "Qz7$wRtm";

async fn create_repo(base: &str, token: &str, name: &str) {
    let resp = reqwest::Client::new()
        .post(format!("{base}/api/v1/repos"))
        .bearer_auth(token)
        .json(&serde_json::json!({"name": name, "auto_init": true}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);
}

async fn put(
    base: &str,
    token: &str,
    owner: &str,
    repo: &str,
    path: &str,
    content: &str,
    msg: &str,
) {
    let client = reqwest::Client::new();
    // An update needs the current blob sha; a create must omit it.
    let existing = client
        .get(format!("{base}/api/v1/repos/{owner}/{repo}/blob/{path}"))
        .bearer_auth(token)
        .send()
        .await
        .unwrap();
    let sha = if existing.status() == 200 {
        existing.json::<serde_json::Value>().await.unwrap()["sha"]
            .as_str()
            .map(str::to_string)
    } else {
        None
    };
    let resp = client
        .post(format!(
            "{base}/api/v1/repos/{owner}/{repo}/contents/{path}"
        ))
        .bearer_auth(token)
        .json(&serde_json::json!({"content": content, "message": msg, "sha": sha}))
        .send()
        .await
        .unwrap();
    assert!(
        resp.status().is_success(),
        "write {path}: {}",
        resp.status()
    );
}

async fn log(
    base: &str,
    token: Option<&str>,
    owner: &str,
    repo: &str,
    query: &str,
) -> reqwest::Response {
    let mut req =
        reqwest::Client::new().get(format!("{base}/api/v1/repos/{owner}/{repo}/log{query}"));
    if let Some(t) = token {
        req = req.bearer_auth(t);
    }
    req.send().await.unwrap()
}

async fn messages(base: &str, token: &str, owner: &str, repo: &str, query: &str) -> Vec<String> {
    let resp = log(base, Some(token), owner, repo, query).await;
    assert_eq!(resp.status(), 200, "log{query}");
    let body: serde_json::Value = resp.json().await.unwrap();
    body["commits"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["message"].as_str().unwrap().to_string())
        .collect()
}

#[tokio::test]
async fn path_filter_returns_only_commits_that_touched_the_path() {
    let base = spawn_test_app().await;
    let token = register_user(&base, "logowner", "logowner@example.com", PW).await;
    create_repo(&base, &token, "proj").await;
    put(&base, &token, "logowner", "proj", "a.txt", "1\n", "add a").await;
    put(
        &base,
        &token,
        "logowner",
        "proj",
        "docs/guide.md",
        "g\n",
        "add guide",
    )
    .await;
    put(&base, &token, "logowner", "proj", "a.txt", "2\n", "edit a").await;
    put(&base, &token, "logowner", "proj", "b.txt", "b\n", "add b").await;

    let all = messages(&base, &token, "logowner", "proj", "").await;
    assert!(all.len() >= 5, "unfiltered log lists every commit: {all:?}");

    assert_eq!(
        messages(&base, &token, "logowner", "proj", "?path=a.txt").await,
        ["edit a", "add a"]
    );
    assert_eq!(
        messages(&base, &token, "logowner", "proj", "?path=b.txt").await,
        ["add b"]
    );
    // A directory lists commits that changed anything inside it.
    assert_eq!(
        messages(&base, &token, "logowner", "proj", "?path=docs").await,
        ["add guide"]
    );
    assert_eq!(
        messages(&base, &token, "logowner", "proj", "?path=docs/").await,
        ["add guide"]
    );
    // Unknown path: empty history, not an error.
    assert!(
        messages(&base, &token, "logowner", "proj", "?path=nope.txt")
            .await
            .is_empty()
    );
}

#[tokio::test]
async fn path_filter_honours_ref_and_limit() {
    let base = spawn_test_app().await;
    let token = register_user(&base, "logref", "logref@example.com", PW).await;
    create_repo(&base, &token, "proj").await;
    for i in 0..4 {
        put(
            &base,
            &token,
            "logref",
            "proj",
            "f.txt",
            &format!("{i}\n"),
            &format!("v{i}"),
        )
        .await;
    }

    assert_eq!(
        messages(&base, &token, "logref", "proj", "?path=f.txt&limit=2").await,
        ["v3", "v2"]
    );
    assert_eq!(
        messages(&base, &token, "logref", "proj", "?path=f.txt&ref=main").await,
        ["v3", "v2", "v1", "v0"]
    );
    // Unknown ref gives an empty history, same as the unfiltered log.
    assert!(messages(
        &base,
        &token,
        "logref",
        "proj",
        "?path=f.txt&ref=no-such-ref"
    )
    .await
    .is_empty());
}

#[tokio::test]
async fn path_filter_rejects_traversal_and_keeps_access_control() {
    let base = spawn_test_app().await;
    let token = register_user(&base, "logsec", "logsec@example.com", PW).await;
    let other = register_user(&base, "logother", "logother@example.com", PW).await;
    // Private repo.
    let resp = reqwest::Client::new()
        .post(format!("{base}/api/v1/repos"))
        .bearer_auth(&token)
        .json(&serde_json::json!({"name": "priv", "auto_init": true, "is_private": true}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);

    assert_eq!(
        log(&base, Some(&token), "logsec", "priv", "?path=../x")
            .await
            .status(),
        400
    );
    assert_eq!(
        log(&base, Some(&token), "logsec", "priv", "?path=a//b")
            .await
            .status(),
        400
    );
    assert_eq!(
        log(&base, None, "logsec", "priv", "?path=README.md")
            .await
            .status(),
        401
    );
    assert_eq!(
        log(&base, Some(&other), "logsec", "priv", "?path=README.md")
            .await
            .status(),
        403
    );
    assert_eq!(
        log(&base, Some(&token), "logsec", "priv", "?path=README.md")
            .await
            .status(),
        200
    );
}
