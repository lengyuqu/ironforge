//! Router acceptance test; requires the O09 module and route to be registered.
mod collab_common;
mod common;
use collab_common::Env;
use reqwest::Method;
use serde_json::json;

#[tokio::test]
async fn search_authorization_revocation_and_no_cross_repo_leak() {
    let env = Env::new().await;
    let session = env.session(&env.owner_token, "search").await;
    for title in ["needle one", "needle two"] {
        let (status, _) = env
            .json(
                Method::POST,
                &env.url("/tasks"),
                &env.owner_token,
                Some(&session),
                Some(json!({"title": title})),
            )
            .await;
        assert_eq!(status, 201);
    }
    let (reader, reader_id) = env.user("search_reader").await;
    env.add_collaborator(reader_id, "read").await;
    let url = env.url("/search?kind=task&q=needle&limit=1");
    let (status, page) = env.json(Method::GET, &url, &reader, None, None).await;
    assert_eq!(status, 200, "{page}");
    assert_eq!(page["items"].as_array().unwrap().len(), 1);
    assert!(page.get("total").is_none());
    assert!(page["items"][0].get("snippet").is_none());
    let cursor = page["next_cursor"].as_str().unwrap();
    let next = format!("{url}&cursor={cursor}");
    let (_, page2) = env.json(Method::GET, &next, &reader, None, None).await;
    assert_ne!(page["items"][0]["id"], page2["items"][0]["id"]);
    rg_db::ops::repo_collaborator_ops::delete_by_repo_and_user(&env.db, env.repo_id, reader_id)
        .await
        .unwrap();
    let (status, denied) = env.json(Method::GET, &next, &reader, None, None).await;
    assert_eq!(status, 403, "{denied}");
    assert!(denied.get("items").is_none());
    assert!(denied.get("total").is_none());
    common::create_repo(&env.base, &env.owner_token, "search-other").await;
    let other = env.url_in("search-other", "/search?kind=task&q=needle");
    let (status, empty) = env
        .json(Method::GET, &other, &env.owner_token, None, None)
        .await;
    assert_eq!(status, 200, "{empty}");
    assert!(empty["items"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn message_and_work_package_search_obey_access_and_hide_content() {
    let env = Env::new().await;
    let session = env.session(&env.owner_token, "search-content").await;
    let (status, task) = env
        .json(
            Method::POST,
            &env.url("/tasks"),
            &env.owner_token,
            Some(&session),
            Some(json!({"title":"search task"})),
        )
        .await;
    assert_eq!(status, 201, "{task}");
    let task_id = task["id"].as_str().unwrap();
    for n in 0..2 {
        let (status, body) = env.json(Method::POST, &env.url(&format!("/tasks/{task_id}/messages")),
            &env.owner_token, Some(&session), Some(json!({"client_message_id":format!("m{n}"),"kind":"note","body":"needle private-message"}))).await;
        assert_eq!(status, 201, "{body}");
        let (status, body) = env
            .json(
                Method::POST,
                &env.url(&format!("/tasks/{task_id}/work-packages")),
                &env.owner_token,
                Some(&session),
                Some(json!({"client_package_id":format!("w{n}"),"goal":"needle private-package"})),
            )
            .await;
        assert_eq!(status, 201, "{body}");
    }
    let (reader, reader_id) = env.user("content_reader").await;
    env.add_collaborator(reader_id, "read").await;
    for kind in ["message", "work_package"] {
        let url = env.url(&format!("/search?kind={kind}&q=needle&limit=1"));
        let (status, page) = env.json(Method::GET, &url, &reader, None, None).await;
        assert_eq!(status, 200, "{page}");
        assert_eq!(page["items"].as_array().unwrap().len(), 1);
        assert_eq!(page["items"][0]["task_id"], task_id);
        let serialized = page.to_string();
        assert!(!serialized.contains("private-message"));
        assert!(!serialized.contains("private-package"));
        assert!(!serialized.contains("needle"));
        assert!(page.get("total").is_none());
        let next = format!("{url}&cursor={}", page["next_cursor"].as_str().unwrap());
        let (status, second) = env.json(Method::GET, &next, &reader, None, None).await;
        assert_eq!(status, 200, "{second}");
        assert_ne!(page["items"][0]["id"], second["items"][0]["id"]);
        assert!(second["next_cursor"].is_null());
    }
    rg_db::ops::repo_collaborator_ops::delete_by_repo_and_user(&env.db, env.repo_id, reader_id)
        .await
        .unwrap();
    for kind in ["message", "work_package"] {
        let url = env.url(&format!("/search?kind={kind}&q=needle"));
        let (status, denied) = env.json(Method::GET, &url, &reader, None, None).await;
        assert_eq!(status, 403, "{denied}");
        assert!(denied.get("items").is_none());
        assert!(denied.get("total").is_none());
    }
    common::create_repo(&env.base, &env.owner_token, "content-other").await;
    for kind in ["message", "work_package"] {
        let url = env.url_in("content-other", &format!("/search?kind={kind}&q=needle"));
        let (status, page) = env
            .json(Method::GET, &url, &env.owner_token, None, None)
            .await;
        assert_eq!(status, 200, "{page}");
        assert!(page["items"].as_array().unwrap().is_empty());
    }
}
