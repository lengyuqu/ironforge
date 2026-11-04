//! Who may read collaboration data: the owner, explicit collaborators of any
//! level and writers — not a plain signed-in account on a public repository.

mod collab_common;
mod common;

use collab_common::Env;
use reqwest::Method;
use serde_json::json;

#[tokio::test]
async fn stranger_on_public_repo_cannot_read_collaboration_data() {
    let env = Env::new().await;
    let a = env.session(&env.owner_token, "A").await;
    let (status, task) = env
        .json(
            Method::POST,
            &env.url("/tasks"),
            &env.owner_token,
            Some(&a),
            Some(json!({"title": "t"})),
        )
        .await;
    assert_eq!(status, 201, "{task}");
    let tid = task["id"].as_str().unwrap().to_string();

    let (stranger, _) = env.user("stranger").await;
    for suffix in [
        "/tasks".to_string(),
        format!("/tasks/{tid}"),
        format!("/tasks/{tid}/messages"),
    ] {
        let (status, _) = env
            .json(Method::GET, &env.url(&suffix), &stranger, None, None)
            .await;
        assert_eq!(status, 403, "stranger GET {suffix}");
    }

    // Becoming a collaborator, even read-only, opens the data.
    let (reader, reader_id) = env.user("member").await;
    env.add_collaborator(reader_id, "read").await;
    let (status, _) = env
        .json(Method::GET, &env.url("/tasks"), &reader, None, None)
        .await;
    assert_eq!(status, 200);
}
