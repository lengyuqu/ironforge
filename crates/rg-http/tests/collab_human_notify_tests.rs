//! A question an agent asks reaches the people it is addressed to as an
//! ordinary in-app notification.
//!
//! Real HTTP server and SQLite database; with `IRONFORGE_TEST_DATABASE_URL` the
//! same tests run on PostgreSQL or MySQL (not run here).

mod collab_common;
mod common;

use collab_common::{Env, Sess};
use reqwest::Method;
use serde_json::{json, Value};

async fn new_task(env: &Env, s: &Sess) -> String {
    let (status, task) = env
        .json(
            Method::POST,
            &env.url("/tasks"),
            &env.owner_token,
            Some(s),
            Some(json!({"title": "task"})),
        )
        .await;
    assert_eq!(status, 201, "{task}");
    task["id"].as_str().unwrap().to_string()
}

async fn ask(
    env: &Env,
    s: &Sess,
    task: &str,
    key: &str,
    recipient: Value,
    title: &str,
) -> (u16, Value) {
    env.json(
        Method::POST,
        &env.url(&format!("/tasks/{task}/questions")),
        &env.owner_token,
        Some(s),
        Some(json!({
            "client_question_id": key,
            "recipient": recipient,
            "title": title,
            "body": "SECRET-BODY-TEXT which must not be copied into a notification",
        })),
    )
    .await
}

/// The notifications of the person holding `token`.
async fn bell(env: &Env, token: &str) -> Vec<Value> {
    let resp = reqwest::Client::new()
        .get(format!("{}/api/v1/notifications", env.base))
        .bearer_auth(token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().await.unwrap();
    body["data"].as_array().unwrap().clone()
}

fn questions(items: &[Value]) -> Vec<&Value> {
    items
        .iter()
        .filter(|n| n["event_type"] == "collab_question")
        .collect()
}

#[tokio::test]
async fn a_question_to_the_writers_notifies_exactly_the_writers() {
    let env = Env::new().await;
    let (writer_token, writer_id) = env.user("writer").await;
    let (reader_token, reader_id) = env.user("reader").await;
    let (stranger_token, _) = env.user("stranger").await;
    env.add_collaborator(writer_id, "write").await;
    env.add_collaborator(reader_id, "read").await;
    let s = env.session(&env.owner_token, "agent").await;
    let task = new_task(&env, &s).await;

    let (status, q) = ask(
        &env,
        &s,
        &task,
        "q-1",
        json!({"kind": "repo_writers"}),
        "Keep SQLite?",
    )
    .await;
    assert_eq!(status, 201, "{q}");

    let owner = bell(&env, &env.owner_token).await;
    let writer = bell(&env, &writer_token).await;
    assert_eq!(
        questions(&owner).len(),
        1,
        "the owner can answer: {owner:?}"
    );
    assert_eq!(
        questions(&writer).len(),
        1,
        "a write collaborator can answer"
    );
    assert!(questions(&bell(&env, &reader_token).await).is_empty());
    assert!(questions(&bell(&env, &stranger_token).await).is_empty());

    let n = questions(&owner)[0];
    assert!(n["title"].as_str().unwrap().contains("Keep SQLite?"));
    assert_eq!(n["repo"], format!("{}/{}", env.owner, env.repo));
    assert_eq!(n["repo_id"], env.repo_id);
    assert_eq!(n["is_read"], false);
    let body = n["body"].as_str().unwrap();
    assert!(body.contains(q["task_id"].as_str().unwrap()));
    assert!(
        !body.contains("SECRET-BODY-TEXT"),
        "the question body is not copied: {body}"
    );
}

#[tokio::test]
async fn a_replayed_request_does_not_notify_again() {
    let env = Env::new().await;
    let s = env.session(&env.owner_token, "agent").await;
    let task = new_task(&env, &s).await;
    let recipient = json!({"kind": "repo_writers"});
    let (first, _) = ask(&env, &s, &task, "q-1", recipient.clone(), "Once").await;
    let (again, _) = ask(&env, &s, &task, "q-1", recipient.clone(), "Once").await;
    assert_eq!((first, again), (201, 200));
    assert_eq!(questions(&bell(&env, &env.owner_token).await).len(), 1);

    // A different key is a different question.
    let (second, _) = ask(&env, &s, &task, "q-2", recipient, "Twice").await;
    assert_eq!(second, 201);
    assert_eq!(questions(&bell(&env, &env.owner_token).await).len(), 2);
}

#[tokio::test]
async fn a_question_to_one_user_notifies_only_that_user_and_only_if_they_can_read() {
    let env = Env::with_repo(json!({"name": "private-repo", "is_private": true})).await;
    let (member_token, member_id) = env.user("member").await;
    let (outsider_token, outsider_id) = env.user("outsider").await;
    env.add_collaborator(member_id, "read").await;
    let s = env.session(&env.owner_token, "agent").await;
    let task = new_task(&env, &s).await;

    let (status, _) = ask(
        &env,
        &s,
        &task,
        "q-member",
        json!({"kind": "user", "id": member_id.to_string()}),
        "For you",
    )
    .await;
    assert_eq!(status, 201);
    assert_eq!(questions(&bell(&env, &member_token).await).len(), 1);
    assert!(
        questions(&bell(&env, &env.owner_token).await).is_empty(),
        "addressed to one user, not to the writers"
    );

    // Someone who cannot read the private repository is told nothing, and
    // neither is an id that belongs to nobody: either would leak a title.
    for (key, id) in [
        ("q-outsider", outsider_id.to_string()),
        ("q-nobody", "99999999".to_string()),
    ] {
        let (status, _) = ask(
            &env,
            &s,
            &task,
            key,
            json!({"kind": "user", "id": id}),
            "Hidden",
        )
        .await;
        assert_eq!(status, 201);
    }
    assert!(questions(&bell(&env, &outsider_token).await).is_empty());
}

#[tokio::test]
async fn a_question_to_a_session_is_not_a_human_notification() {
    let env = Env::new().await;
    let asker = env.session(&env.owner_token, "asker").await;
    let target = env.session(&env.owner_token, "target").await;
    let task = new_task(&env, &asker).await;
    let (status, _) = ask(
        &env,
        &asker,
        &task,
        "q-session",
        json!({"kind": "session", "id": target.id}),
        "For the other agent",
    )
    .await;
    assert_eq!(status, 201);
    assert!(
        questions(&bell(&env, &env.owner_token).await).is_empty(),
        "sessions have notices; people are told only about questions for people"
    );
}

#[tokio::test]
async fn losing_access_hides_the_text_of_an_old_notification() {
    let env = Env::with_repo(json!({"name": "private-repo", "is_private": true})).await;
    let (member_token, member_id) = env.user("member").await;
    env.add_collaborator(member_id, "write").await;
    let s = env.session(&env.owner_token, "agent").await;
    let task = new_task(&env, &s).await;
    ask(
        &env,
        &s,
        &task,
        "q-1",
        json!({"kind": "repo_writers"}),
        "Rotate the production key?",
    )
    .await;
    let before = bell(&env, &member_token).await;
    assert!(questions(&before)[0]["title"]
        .as_str()
        .unwrap()
        .contains("production key"));

    let resp = reqwest::Client::new()
        .delete(format!(
            "{}/api/v1/repos/{}/{}/collaborators/{member_id}",
            env.base, env.owner, env.repo
        ))
        .bearer_auth(&env.owner_token)
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success(), "remove collaborator");

    let after = bell(&env, &member_token).await;
    let n = questions(&after)[0];
    assert!(
        !n["title"].as_str().unwrap().contains("production key"),
        "the title must no longer be shown: {n}"
    );
    assert!(n["body"].is_null());
    assert!(n["repo"].is_null());
}

#[tokio::test]
async fn a_hostile_title_is_stored_and_returned_as_plain_text() {
    let env = Env::new().await;
    let s = env.session(&env.owner_token, "agent").await;
    let task = new_task(&env, &s).await;
    let hostile = "<img src=x onerror=alert(1)>";
    let (status, _) = ask(
        &env,
        &s,
        &task,
        "q-1",
        json!({"kind": "repo_writers"}),
        hostile,
    )
    .await;
    assert_eq!(status, 201);
    let n = bell(&env, &env.owner_token).await;
    // Returned verbatim as a JSON string; rendering it as text is the client's
    // job and the web client does so.
    assert!(questions(&n)[0]["title"]
        .as_str()
        .unwrap()
        .contains(hostile));
}
