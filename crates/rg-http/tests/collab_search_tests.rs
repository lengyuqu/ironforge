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
