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

// ---------------------------------------------------------------- blocked task

fn blocked_for(items: &[Value]) -> Vec<&Value> {
    items
        .iter()
        .filter(|n| n["event_type"] == "collab_task_blocked")
        .collect()
}

/// A task owned by `s`, so its account is the one to tell.
async fn owned_task(env: &Env, s: &Sess) -> String {
    let (status, task) = env
        .json(
            Method::POST,
            &env.url("/tasks"),
            &env.owner_token,
            Some(s),
            Some(json!({"title": "owned task", "owner_session_id": s.id})),
        )
        .await;
    assert_eq!(status, 201, "{task}");
    task["id"].as_str().unwrap().to_string()
}

async fn set_status(
    env: &Env,
    s: &Sess,
    token: &str,
    task: &str,
    version: i64,
    status: &str,
) -> (u16, Value) {
    env.json(
        Method::PATCH,
        &env.url(&format!("/tasks/{task}")),
        token,
        Some(s),
        Some(json!({"expected_version": version, "status": status})),
    )
    .await
}

#[tokio::test]
async fn declaring_a_task_blocked_tells_only_the_account_that_owns_it() {
    let env = Env::new().await;
    let (bob_token, bob_id) = env.user("bob").await;
    env.add_collaborator(bob_id, "write").await;
    // Bob's agent owns the task; the repository owner has write access too.
    let bob_session = env.session(&bob_token, "bob-agent").await;
    let (status, task) = env
        .json(
            Method::POST,
            &env.url("/tasks"),
            &bob_token,
            Some(&bob_session),
            Some(json!({"title": "Port the lexer", "owner_session_id": bob_session.id})),
        )
        .await;
    assert_eq!(status, 201, "{task}");
    let id = task["id"].as_str().unwrap();

    let (status, t) = set_status(&env, &bob_session, &bob_token, id, 1, "in_progress").await;
    assert_eq!(status, 200, "{t}");
    assert!(
        blocked_for(&bell(&env, &bob_token).await).is_empty(),
        "not blocked yet"
    );

    let (status, t) = set_status(&env, &bob_session, &bob_token, id, 2, "blocked").await;
    assert_eq!(status, 200, "{t}");
    let bob = bell(&env, &bob_token).await;
    let n = blocked_for(&bob);
    assert_eq!(n.len(), 1, "{bob:?}");
    assert_eq!(n[0]["title"], "Port the lexer");
    assert_eq!(n[0]["repo"], format!("{}/{}", env.owner, env.repo));
    assert!(n[0]["body"].as_str().unwrap().contains(id));
    assert!(
        blocked_for(&bell(&env, &env.owner_token).await).is_empty(),
        "the repository owner is not told about someone else's agent"
    );
}

#[tokio::test]
async fn only_a_move_into_blocked_notifies_and_an_unread_one_is_not_repeated() {
    let env = Env::new().await;
    let s = env.session(&env.owner_token, "agent").await;
    let task = owned_task(&env, &s).await; // version 1, open
    let t = &env.owner_token;

    // open -> blocked notifies.
    assert_eq!(set_status(&env, &s, t, &task, 1, "blocked").await.0, 200);
    assert_eq!(blocked_for(&bell(&env, t).await).len(), 1);

    // blocked -> blocked (another field changed) is not a transition.
    let (status, _) = env
        .json(
            Method::PATCH,
            &env.url(&format!("/tasks/{task}")),
            t,
            Some(&s),
            Some(json!({"expected_version": 2, "status": "blocked", "branch": "fix/x"})),
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(blocked_for(&bell(&env, t).await).len(), 1);

    // Flapping while the first one is still unread adds nothing.
    assert_eq!(
        set_status(&env, &s, t, &task, 3, "in_progress").await.0,
        200
    );
    assert_eq!(set_status(&env, &s, t, &task, 4, "blocked").await.0, 200);
    assert_eq!(blocked_for(&bell(&env, t).await).len(), 1);

    // Once it was read, the next time it gets stuck is news again.
    let first = bell(&env, t).await;
    let id = blocked_for(&first)[0]["id"].as_i64().unwrap();
    let resp = reqwest::Client::new()
        .post(format!("{}/api/v1/notifications/{id}/read", env.base))
        .bearer_auth(t)
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());
    assert_eq!(set_status(&env, &s, t, &task, 5, "waiting").await.0, 200);
    assert_eq!(set_status(&env, &s, t, &task, 6, "blocked").await.0, 200);
    assert_eq!(blocked_for(&bell(&env, t).await).len(), 2);
}

#[tokio::test]
async fn a_rejected_update_and_other_statuses_notify_nobody() {
    let env = Env::new().await;
    let s = env.session(&env.owner_token, "agent").await;
    let task = owned_task(&env, &s).await;
    let t = &env.owner_token;

    // A stale version is refused, so nothing became blocked.
    let (status, _) = set_status(&env, &s, t, &task, 99, "blocked").await;
    assert_eq!(status, 409);
    for (v, st) in [(1, "waiting"), (2, "done")] {
        assert_eq!(set_status(&env, &s, t, &task, v, st).await.0, 200);
    }
    assert!(blocked_for(&bell(&env, t).await).is_empty());
}

#[tokio::test]
async fn a_task_nobody_owns_has_nobody_to_tell() {
    let env = Env::new().await;
    let s = env.session(&env.owner_token, "agent").await;
    let task = new_task(&env, &s).await; // no owner session
    assert_eq!(
        set_status(&env, &s, &env.owner_token, &task, 1, "blocked")
            .await
            .0,
        200
    );
    assert!(blocked_for(&bell(&env, &env.owner_token).await).is_empty());
}

// ------------------------------------------------------- agents (T24): the bell
// rings for the person accountable for an agent, never for the agent itself.

use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};

/// A person `bob` (write collaborator) who owns an agent that is also a write
/// collaborator and holds its own token.
struct AgentSetup {
    owner_token: String,
    owner_id: i64,
    agent_id: i64,
    agent_token: String,
}

async fn agent_setup(env: &Env) -> AgentSetup {
    let (owner_token, owner_id) = env.user("bob").await;
    env.add_collaborator(owner_id, "write").await;
    let (status, agent) = env
        .json(
            Method::POST,
            &format!("{}/api/v1/agents", env.base),
            &owner_token,
            None,
            Some(json!({"name": "ci"})),
        )
        .await;
    assert_eq!(status, 201, "{agent}");
    let agent_id = agent["id"].as_i64().unwrap();
    let username = agent["username"].as_str().unwrap();
    let (status, token) = env
        .json(
            Method::POST,
            &format!("{}/api/v1/agents/{username}/tokens", env.base),
            &owner_token,
            None,
            Some(json!({"name": "t"})),
        )
        .await;
    assert_eq!(status, 201, "{token}");
    env.add_collaborator(agent_id, "write").await;
    AgentSetup {
        owner_token,
        owner_id,
        agent_id,
        agent_token: token["token"].as_str().unwrap().to_string(),
    }
}

/// Collaboration notifications stored for `user_id`, straight from the table
/// (an agent token cannot read the bell, and must not need to).
async fn collab_rows(env: &Env, user_id: i64) -> Vec<rg_db::entities::notification::Model> {
    use rg_db::entities::notification::{Column, Entity};
    Entity::find()
        .filter(Column::UserId.eq(user_id))
        .filter(Column::EventType.like("collab_%"))
        .all(&env.db)
        .await
        .unwrap()
}

/// Guard: no collaboration notification of any kind is stored for any agent
/// account. Every recipient is a person, so nothing can reach an agent's
/// `@agents.invalid` address through a path that reuses the recipient list.
async fn assert_no_agent_was_notified(env: &Env) {
    use rg_db::entities::user;
    let agents: Vec<i64> = user::Entity::find()
        .filter(user::Column::Kind.eq("agent"))
        .all(&env.db)
        .await
        .unwrap()
        .into_iter()
        .map(|u| u.id)
        .collect();
    for id in agents {
        assert!(
            collab_rows(env, id).await.is_empty(),
            "agent {id} must never be a recipient"
        );
    }
}

async fn deactivate(env: &Env, user_id: i64) {
    rg_db::ops::user_ops::update_by_id(&env.db, user_id, None, None, None, Some(false))
        .await
        .unwrap();
    rg_core::repo::service::invalidate_perm_cache_all();
}

#[tokio::test]
async fn a_question_addressed_to_an_agent_reaches_its_owner_not_the_agent() {
    let env = Env::new().await;
    let a = agent_setup(&env).await;
    let s = env.session(&env.owner_token, "human-side").await;
    let task = new_task(&env, &s).await;

    let (status, q) = ask(
        &env,
        &s,
        &task,
        "q-agent",
        json!({"kind": "user", "id": a.agent_id.to_string()}),
        "Can you take this?",
    )
    .await;
    assert_eq!(status, 201, "{q}");

    let bob = bell(&env, &a.owner_token).await;
    assert_eq!(questions(&bob).len(), 1, "{bob:?}");
    assert!(collab_rows(&env, a.agent_id).await.is_empty());
    assert!(questions(&bell(&env, &env.owner_token).await).is_empty());
    assert_no_agent_was_notified(&env).await;
}

#[tokio::test]
async fn a_question_asked_by_an_agent_to_the_writers_tells_people_only_and_once() {
    let env = Env::new().await;
    let a = agent_setup(&env).await;
    // Both the owner and the agent are in the writers list.
    let s = env.session(&a.agent_token, "the-agent").await;
    let task = {
        let (status, task) = env
            .json(
                Method::POST,
                &env.url("/tasks"),
                &a.agent_token,
                Some(&s),
                Some(json!({"title": "agent task"})),
            )
            .await;
        assert_eq!(status, 201, "{task}");
        task["id"].as_str().unwrap().to_string()
    };
    let (status, q) = env
        .json(
            Method::POST,
            &env.url(&format!("/tasks/{task}/questions")),
            &a.agent_token,
            Some(&s),
            Some(json!({
                "client_question_id": "q-w",
                "recipient": {"kind": "repo_writers"},
                "title": "Which database?",
                "body": "b",
            })),
        )
        .await;
    assert_eq!(status, 201, "{q}");

    assert_eq!(
        questions(&bell(&env, &a.owner_token).await).len(),
        1,
        "owner and agent are one person: one notification"
    );
    assert_eq!(
        questions(&bell(&env, &env.owner_token).await).len(),
        1,
        "the repository owner is a writer too"
    );
    assert!(collab_rows(&env, a.agent_id).await.is_empty());
    assert_no_agent_was_notified(&env).await;
}

#[tokio::test]
async fn an_agent_in_the_writers_list_does_not_take_a_slot_from_its_owner_twice() {
    // The agent is the only candidate that maps to its owner here: the owner is
    // not itself a collaborator. Mapping happens before de-duplication, so the
    // owner still gets exactly one.
    let env = Env::new().await;
    let (owner_token, owner_id) = env.user("carol").await;
    env.add_collaborator(owner_id, "write").await;
    let a = {
        // reuse the helper's steps but without a second human collaborator row
        let (status, agent) = env
            .json(
                Method::POST,
                &format!("{}/api/v1/agents", env.base),
                &owner_token,
                None,
                Some(json!({"name": "x"})),
            )
            .await;
        assert_eq!(status, 201, "{agent}");
        agent["id"].as_i64().unwrap()
    };
    env.add_collaborator(a, "write").await;
    let s = env.session(&env.owner_token, "human-side").await;
    let task = new_task(&env, &s).await;
    let (status, _) = ask(
        &env,
        &s,
        &task,
        "q-dup",
        json!({"kind": "repo_writers"}),
        "Dup?",
    )
    .await;
    assert_eq!(status, 201);
    assert_eq!(questions(&bell(&env, &owner_token).await).len(), 1);
    assert!(collab_rows(&env, a).await.is_empty());
    assert_no_agent_was_notified(&env).await;
}

#[tokio::test]
async fn a_blocked_task_of_an_agent_session_tells_the_owner() {
    let env = Env::new().await;
    let a = agent_setup(&env).await;
    let s = env.session(&a.agent_token, "the-agent").await;
    let (status, task) = env
        .json(
            Method::POST,
            &env.url("/tasks"),
            &a.agent_token,
            Some(&s),
            Some(json!({"title": "Port the parser", "owner_session_id": s.id})),
        )
        .await;
    assert_eq!(status, 201, "{task}");
    let id = task["id"].as_str().unwrap();
    assert_eq!(
        set_status(&env, &s, &a.agent_token, id, 1, "blocked")
            .await
            .0,
        200
    );

    let bob = bell(&env, &a.owner_token).await;
    let n = blocked_for(&bob);
    assert_eq!(n.len(), 1, "{bob:?}");
    assert_eq!(n[0]["title"], "Port the parser");
    assert!(
        blocked_for(&bell(&env, &env.owner_token).await).is_empty(),
        "no broadcast to the repository writers"
    );
    assert!(collab_rows(&env, a.agent_id).await.is_empty());
    assert_no_agent_was_notified(&env).await;
}

#[tokio::test]
async fn nobody_is_told_when_the_owner_of_an_agent_is_disabled_and_nothing_fails() {
    let env = Env::new().await;
    let a = agent_setup(&env).await;
    // The agent owns a task through its own session, registered while its owner
    // was still active.
    let agent_session = env.session(&a.agent_token, "the-agent").await;
    let (status, task) = env
        .json(
            Method::POST,
            &env.url("/tasks"),
            &a.agent_token,
            Some(&agent_session),
            Some(json!({"title": "agent task", "owner_session_id": agent_session.id})),
        )
        .await;
    assert_eq!(status, 201, "{task}");
    let task_id = task["id"].as_str().unwrap().to_string();

    deactivate(&env, a.owner_id).await;

    // A question to the agent: created, nobody told.
    let s = env.session(&env.owner_token, "human-side").await;
    let qtask = new_task(&env, &s).await;
    let (status, _) = ask(
        &env,
        &s,
        &qtask,
        "q-gone",
        json!({"kind": "user", "id": a.agent_id.to_string()}),
        "Anyone?",
    )
    .await;
    assert_eq!(status, 201);
    // To the writers: the repository owner is told, the disabled owner and its
    // agent are not.
    let (status, _) = ask(
        &env,
        &s,
        &qtask,
        "q-writers",
        json!({"kind": "repo_writers"}),
        "Writers?",
    )
    .await;
    assert_eq!(status, 201);
    assert_eq!(
        questions(&bell(&env, &env.owner_token).await).len(),
        1,
        "only the repository owner is told about the writers question"
    );
    assert!(collab_rows(&env, a.owner_id).await.is_empty());

    // The repository owner declares the agent's task blocked: no fallback to
    // anyone else.
    let (status, body) = set_status(&env, &s, &env.owner_token, &task_id, 1, "blocked").await;
    assert_eq!(status, 200, "{body}");
    assert!(blocked_for(&bell(&env, &env.owner_token).await).is_empty());
    assert!(collab_rows(&env, a.owner_id).await.is_empty());
    assert_no_agent_was_notified(&env).await;
}

#[tokio::test]
async fn a_deactivated_person_is_not_told_but_other_people_are_unaffected() {
    let env = Env::new().await;
    let (gone_token, gone_id) = env.user("gone").await;
    let (kept_token, kept_id) = env.user("kept").await;
    env.add_collaborator(gone_id, "write").await;
    env.add_collaborator(kept_id, "write").await;
    deactivate(&env, gone_id).await;

    let s = env.session(&env.owner_token, "human-side").await;
    let task = new_task(&env, &s).await;
    for (key, recipient) in [
        ("q1", json!({"kind": "user", "id": gone_id.to_string()})),
        ("q2", json!({"kind": "user", "id": kept_id.to_string()})),
        ("q3", json!({"kind": "repo_writers"})),
    ] {
        assert_eq!(ask(&env, &s, &task, key, recipient, "t").await.0, 201);
    }
    assert!(collab_rows(&env, gone_id).await.is_empty());
    let _ = gone_token;
    assert_eq!(questions(&bell(&env, &kept_token).await).len(), 2);
    assert_eq!(questions(&bell(&env, &env.owner_token).await).len(), 1);
}

#[tokio::test]
async fn accountable_user_maps_agents_to_active_people_only() {
    let env = Env::new().await;
    let a = agent_setup(&env).await;
    async fn at(env: &Env, id: i64) -> Option<i64> {
        rg_core::agent::accountable_user(&env.db, id).await.unwrap()
    }
    assert_eq!(
        at(&env, a.owner_id).await,
        Some(a.owner_id),
        "a person is themself"
    );
    assert_eq!(
        at(&env, a.agent_id).await,
        Some(a.owner_id),
        "an agent is its owner"
    );
    assert_eq!(
        at(&env, 99_999_999).await,
        None,
        "an unknown account is nobody"
    );

    deactivate(&env, a.owner_id).await;
    assert_eq!(
        at(&env, a.owner_id).await,
        None,
        "a disabled person is nobody"
    );
    assert_eq!(
        at(&env, a.agent_id).await,
        None,
        "an agent whose owner is disabled has nobody behind it"
    );
}
