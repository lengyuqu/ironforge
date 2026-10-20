//! `GET /repos/{owner}/{name}/blame/{path}`: per-line authorship, access
//! control and input rejection.

mod common;

use common::{register_user, spawn_test_app};

const PW: &str = "Qz7$wRtm";

async fn create_repo(base: &str, token: &str, name: &str, private: bool) {
    let resp = reqwest::Client::new()
        .post(format!("{base}/api/v1/repos"))
        .bearer_auth(token)
        .json(&serde_json::json!({"name": name, "auto_init": true, "is_private": private}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201, "create repo failed");
}

#[allow(clippy::too_many_arguments)]
async fn put_file(
    base: &str,
    token: &str,
    owner: &str,
    repo: &str,
    path: &str,
    content: &str,
    message: &str,
    sha: Option<&str>,
) {
    let resp = reqwest::Client::new()
        .post(format!(
            "{base}/api/v1/repos/{owner}/{repo}/contents/{path}"
        ))
        .bearer_auth(token)
        .json(&serde_json::json!({"content": content, "message": message, "sha": sha}))
        .send()
        .await
        .unwrap();
    assert!(
        resp.status().is_success(),
        "write {path} failed: {}",
        resp.status()
    );
}

async fn blob_sha(base: &str, token: &str, owner: &str, repo: &str, path: &str) -> String {
    let body: serde_json::Value = reqwest::Client::new()
        .get(format!("{base}/api/v1/repos/{owner}/{repo}/blob/{path}"))
        .bearer_auth(token)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    body["sha"].as_str().unwrap().to_string()
}

async fn blame(
    base: &str,
    token: Option<&str>,
    owner: &str,
    repo: &str,
    path: &str,
    query: &str,
) -> reqwest::Response {
    let mut req = reqwest::Client::new().get(format!(
        "{base}/api/v1/repos/{owner}/{repo}/blame/{path}{query}"
    ));
    if let Some(t) = token {
        req = req.bearer_auth(t);
    }
    req.send().await.unwrap()
}

#[tokio::test]
async fn blame_attributes_lines_to_introducing_commits() {
    let base = spawn_test_app().await;
    let token = register_user(&base, "blameowner", "blameowner@example.com", PW).await;
    create_repo(&base, &token, "proj", false).await;

    put_file(
        &base,
        &token,
        "blameowner",
        "proj",
        "notes.txt",
        "one\ntwo\nthree\n",
        "add notes",
        None,
    )
    .await;
    let sha = blob_sha(&base, &token, "blameowner", "proj", "notes.txt").await;
    put_file(
        &base,
        &token,
        "blameowner",
        "proj",
        "notes.txt",
        "one\nTWO\nthree\nfour\n",
        "edit two, add four",
        Some(&sha),
    )
    .await;

    let resp = blame(&base, None, "blameowner", "proj", "notes.txt", "?ref=main").await;
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();

    assert_eq!(body["path"], "notes.txt");
    assert_eq!(body["ref"], "main");
    assert_eq!(body["total_lines"], 4);
    assert_eq!(body["commit_sha"].as_str().unwrap().len(), 40);

    let hunks = body["hunks"].as_array().unwrap();
    // Hunks tile the file with no gaps and carry every line's text.
    let mut next = 1;
    let mut text = Vec::new();
    for h in hunks {
        assert_eq!(h["start_line"], next);
        let lines = h["lines"].as_array().unwrap();
        next += lines.len() as i64;
        text.extend(lines.iter().map(|l| l.as_str().unwrap().to_string()));
        assert_eq!(h["commit"]["sha"].as_str().unwrap().len(), 40);
        assert!(!h["commit"]["date"].as_str().unwrap().is_empty());
    }
    assert_eq!(text, ["one", "TWO", "three", "four"]);

    // Lines 1 and 3 predate the edit; lines 2 and 4 belong to it.
    let msg_for = |line: i64| -> String {
        hunks
            .iter()
            .find(|h| {
                let start = h["start_line"].as_i64().unwrap();
                let len = h["lines"].as_array().unwrap().len() as i64;
                (start..start + len).contains(&line)
            })
            .unwrap()["commit"]["message"]
            .as_str()
            .unwrap()
            .to_string()
    };
    assert_eq!(msg_for(1), "add notes");
    assert_eq!(msg_for(2), "edit two, add four");
    assert_eq!(msg_for(3), "add notes");
    assert_eq!(msg_for(4), "edit two, add four");
}

#[tokio::test]
async fn blame_respects_private_repo_access() {
    let base = spawn_test_app().await;
    let token = register_user(&base, "privblame", "privblame@example.com", PW).await;
    let other = register_user(&base, "strangerb", "strangerb@example.com", PW).await;
    create_repo(&base, &token, "secret", true).await;
    put_file(
        &base,
        &token,
        "privblame",
        "secret",
        "k.txt",
        "x\n",
        "add",
        None,
    )
    .await;

    assert_eq!(
        blame(&base, Some(&token), "privblame", "secret", "k.txt", "")
            .await
            .status(),
        200
    );
    assert_eq!(
        blame(&base, None, "privblame", "secret", "k.txt", "")
            .await
            .status(),
        401
    );
    assert_eq!(
        blame(&base, Some(&other), "privblame", "secret", "k.txt", "")
            .await
            .status(),
        403
    );
}

#[tokio::test]
async fn blame_rejects_bad_targets() {
    let base = spawn_test_app().await;
    let token = register_user(&base, "blamebad", "blamebad@example.com", PW).await;
    create_repo(&base, &token, "r", false).await;
    put_file(
        &base,
        &token,
        "blamebad",
        "r",
        "dir/a.txt",
        "a\n",
        "add",
        None,
    )
    .await;
    put_file(
        &base,
        &token,
        "blamebad",
        "r",
        "bin.dat",
        "a\u{0}b\n",
        "bin",
        None,
    )
    .await;

    let status = |path: &'static str, query: &'static str| {
        let (base, token) = (base.clone(), token.clone());
        async move {
            blame(&base, Some(&token), "blamebad", "r", path, query)
                .await
                .status()
        }
    };
    assert_eq!(status("missing.txt", "").await, 404, "unknown path");
    assert_eq!(
        status("dir/a.txt", "?ref=no-such-ref").await,
        404,
        "unknown ref"
    );
    assert_eq!(status("dir", "").await, 400, "directory");
    assert_eq!(status("bin.dat", "").await, 400, "binary file");
    assert_eq!(
        blame(&base, Some(&token), "blamebad", "nope", "a.txt", "")
            .await
            .status(),
        404,
        "unknown repo"
    );
}
