//! Agent accounts: a row in `users` that acts for a person.
//!
//! What these tests pin down:
//! - only a person can create and manage agents, and only their own;
//! - an agent has no human entrance (password, reset, registration namespace)
//!   and its tokens cannot reach the account, agent or administrator APIs;
//! - on a repository an agent can do what it was granted **and** its owner can
//!   do, never administer it, and nothing once it or its owner is disabled;
//! - an agent owns nothing; an owner with agents cannot be deleted.

mod common;
use common::{register_full, register_user, spawn_test_app_with_db};

use reqwest::{Client, StatusCode};
use sea_orm::{ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter};
use serde_json::{json, Value};

const PASSWORD: &str = "Qz7$wRtm";

/// The permission cache is one process-wide map keyed by `(repo id, user id)`.
/// Every test here has its own database, so the same ids mean different things
/// in different tests: run them one at a time, each starting from an empty cache.
static ONE_AT_A_TIME: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

struct World {
    _exclusive: tokio::sync::MutexGuard<'static, ()>,
    base: String,
    db: rg_db::DatabaseConnection,
    admin: String,
    alice: String,
    alice_id: i64,
    bob: String,
}

/// The first account is the administrator, so `alice` and `bob` are ordinary.
async fn world() -> World {
    let exclusive = ONE_AT_A_TIME.lock().await;
    rg_core::repo::service::invalidate_perm_cache_all();
    let (base, db) = spawn_test_app_with_db().await;
    let admin = register_user(&base, "root_admin", "root_admin@example.com", PASSWORD).await;
    let (alice, alice_id) = register_full(&base, "alice", "alice@example.com").await;
    let bob = register_user(&base, "bob", "bob@example.com", PASSWORD).await;
    World {
        _exclusive: exclusive,
        base,
        db,
        admin,
        alice,
        alice_id,
        bob,
    }
}

impl World {
    fn url(&self, path: &str) -> String {
        format!("{}/api/v1{path}", self.base)
    }

    async fn send(
        &self,
        method: reqwest::Method,
        path: &str,
        auth: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let mut req = Client::new()
            .request(method, self.url(path))
            .bearer_auth(auth);
        if let Some(body) = body {
            req = req.json(&body);
        }
        let resp = req.send().await.unwrap();
        let status = resp.status();
        let body = resp.json().await.unwrap_or(Value::Null);
        (status, body)
    }

    async fn get(&self, path: &str, auth: &str) -> (StatusCode, Value) {
        self.send(reqwest::Method::GET, path, auth, None).await
    }

    async fn post(&self, path: &str, auth: &str, body: Value) -> (StatusCode, Value) {
        self.send(reqwest::Method::POST, path, auth, Some(body))
            .await
    }

    async fn patch(&self, path: &str, auth: &str, body: Value) -> (StatusCode, Value) {
        self.send(reqwest::Method::PATCH, path, auth, Some(body))
            .await
    }

    /// Create `<alice>-agent-<name>` and return `(username, id)`.
    async fn agent(&self, owner_jwt: &str, name: &str) -> (String, i64) {
        let (status, body) = self
            .post("/agents", owner_jwt, json!({ "name": name }))
            .await;
        assert_eq!(status, 201, "create agent {name}: {body}");
        (
            body["username"].as_str().unwrap().to_string(),
            body["id"].as_i64().unwrap(),
        )
    }

    async fn agent_token(&self, owner_jwt: &str, agent: &str, scopes: Option<&str>) -> String {
        let mut body = json!({ "name": "t" });
        if let Some(scopes) = scopes {
            body["scopes"] = json!(scopes);
        }
        let (status, body) = self
            .post(&format!("/agents/{agent}/tokens"), owner_jwt, body)
            .await;
        assert_eq!(status, 201, "issue agent token: {body}");
        body["token"].as_str().unwrap().to_string()
    }

    async fn private_repo(&self, jwt: &str, name: &str) {
        let (status, body) = self
            .post(
                "/repos",
                jwt,
                json!({ "name": name, "is_private": true, "auto_init": true }),
            )
            .await;
        assert_eq!(status, 201, "create repo {name}: {body}");
    }

    /// Add a collaborator and return the collaborator row id.
    async fn grant(&self, jwt: &str, repo: &str, username: &str, permission: &str) -> i64 {
        let (status, body) = self
            .post(
                &format!("/repos/{repo}/collaborators"),
                jwt,
                json!({ "username": username, "permission": permission }),
            )
            .await;
        assert!(
            status.is_success(),
            "grant {permission} on {repo}: {status} {body}"
        );
        body["id"]
            .as_i64()
            .unwrap_or_else(|| panic!("collaborator id in {body}"))
    }

    /// Change an existing collaborator's permission.
    async fn regrant(&self, jwt: &str, repo: &str, row_id: i64, permission: &str) {
        let (status, body) = self
            .patch(
                &format!("/repos/{repo}/collaborators/{row_id}"),
                jwt,
                json!({ "permission": permission }),
            )
            .await;
        assert!(
            status.is_success(),
            "regrant {permission} on {repo}: {status} {body}"
        );
    }

    async fn can_read(&self, repo: &str, auth: &str) -> bool {
        self.get(&format!("/repos/{repo}"), auth).await.0 == StatusCode::OK
    }

    /// Whether `auth` can commit a file to `repo` (a write that needs
    /// `can_write_repo`).
    async fn can_write(&self, repo: &str, auth: &str, file: &str) -> bool {
        let (status, _) = self
            .send(
                reqwest::Method::POST,
                &format!("/repos/{repo}/contents/{file}"),
                auth,
                Some(json!({ "content": "x\n", "message": "add" })),
            )
            .await;
        status.is_success()
    }
}

#[tokio::test]
async fn a_person_creates_and_manages_only_their_own_agents() {
    let w = world().await;
    let (username, id) = w.agent(&w.alice, "ci").await;
    assert_eq!(username, "alice-agent-ci");

    let (status, list) = w.get("/agents", &w.alice).await;
    assert_eq!(status, 200);
    assert_eq!(list.as_array().unwrap().len(), 1);
    assert_eq!(list[0]["id"], id);
    assert_eq!(list[0]["kind"], "agent");
    assert_eq!(list[0]["owner_user_id"], w.alice_id);
    assert_eq!(list[0]["is_active"], true);

    // Same name again: conflict. Bad names: rejected.
    let (status, _) = w.post("/agents", &w.alice, json!({ "name": "ci" })).await;
    assert_eq!(status, 409);
    for bad in ["", "-x", "has space", "a-agent-b", "x".repeat(21).as_str()] {
        let (status, _) = w.post("/agents", &w.alice, json!({ "name": bad })).await;
        assert_eq!(status, 400, "name {bad:?}");
    }

    // Someone else's agent looks exactly like one that does not exist.
    for path in [
        "/agents/alice-agent-ci".to_string(),
        "/agents/alice-agent-ci/tokens".to_string(),
        "/agents/alice-agent-nope".to_string(),
        // A person is not an agent, even one's own account.
        "/agents/bob".to_string(),
    ] {
        assert_eq!(w.get(&path, &w.bob).await.0, 404, "{path}");
    }
    let (status, _) = w
        .post(
            "/agents/alice-agent-ci/tokens",
            &w.bob,
            json!({ "name": "t" }),
        )
        .await;
    assert_eq!(status, 404);
    let (status, _) = w
        .patch(
            "/agents/alice-agent-ci",
            &w.bob,
            json!({ "is_active": false }),
        )
        .await;
    assert_eq!(status, 404);
    assert_eq!(w.get("/agents", &w.bob).await.1, json!([]));

    // No credentials at all.
    let resp = Client::new().get(w.url("/agents")).send().await.unwrap();
    assert_eq!(resp.status(), 401);
}

#[tokio::test]
async fn an_agent_token_carries_repository_scopes_only() {
    let w = world().await;
    let (agent, _) = w.agent(&w.alice, "ci").await;
    for scopes in ["user", "admin", "repo,user", "repo:read,admin"] {
        let (status, body) = w
            .post(
                &format!("/agents/{agent}/tokens"),
                &w.alice,
                json!({ "name": "t", "scopes": scopes }),
            )
            .await;
        assert_eq!(status, 400, "scopes {scopes}: {body}");
    }
    // The default is `repo:write`, not the historical full `repo`.
    let (status, body) = w
        .post(
            &format!("/agents/{agent}/tokens"),
            &w.alice,
            json!({ "name": "t" }),
        )
        .await;
    assert_eq!(status, 201);
    assert_eq!(body["scopes"], "repo:write");
    let token_id = body["id"].as_i64().unwrap();

    // The owner sees it under the agent, not among their own tokens.
    let (_, own) = w.get("/users/tokens", &w.alice).await;
    assert_eq!(own, json!([]));
    let (_, listed) = w.get(&format!("/agents/{agent}/tokens"), &w.alice).await;
    assert_eq!(listed[0]["id"], token_id);
    assert!(listed[0].get("token").is_none(), "the secret is shown once");

    // Bob cannot revoke it; alice can.
    let path = format!("/agents/{agent}/tokens/{token_id}");
    let (status, _) = w.send(reqwest::Method::DELETE, &path, &w.bob, None).await;
    assert_eq!(status, 404);
    let (status, _) = w.send(reqwest::Method::DELETE, &path, &w.alice, None).await;
    assert_eq!(status, 204);
    let (_, listed) = w.get(&format!("/agents/{agent}/tokens"), &w.alice).await;
    assert_eq!(listed, json!([]));
}

/// Every way a person gets a session must refuse an agent. If a new login
/// method is added, add it here.
#[tokio::test]
async fn an_agent_has_no_human_entrance() {
    let w = world().await;
    let (agent, agent_id) = w.agent(&w.alice, "ci").await;
    let row = rg_db::ops::user_ops::find_by_id(&w.db, agent_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row.kind, "agent");
    assert_eq!(row.auth_provider, "agent");
    assert!(!row.is_admin);
    assert!(row.email.ends_with("@agents.invalid"));

    // Password login, by username and by e-mail, with the stored non-hash, an
    // empty password and an ordinary one.
    for login in [agent.as_str(), row.email.as_str()] {
        for password in [row.password_hash.as_str(), "", PASSWORD] {
            let resp = Client::new()
                .post(w.url("/users/login"))
                .json(&json!({ "login": login, "password": password }))
                .send()
                .await
                .unwrap();
            assert!(
                !resp.status().is_success(),
                "login as {login} with {password:?} must fail"
            );
            assert!(resp.headers().get("set-cookie").is_none());
        }
    }

    // Password reset: answers as always, creates no reset token.
    let resp = Client::new()
        .post(w.url("/users/forgot-password"))
        .json(&json!({ "email": row.email }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let reset_tokens = rg_db::entities::password_reset_token::Entity::find()
        .filter(rg_db::entities::password_reset_token::Column::UserId.eq(agent_id))
        .count(&w.db)
        .await
        .unwrap();
    assert_eq!(reset_tokens, 0);

    // The agent namespace is closed to people and organizations.
    let resp = Client::new()
        .post(w.url("/users/register"))
        .json(&json!({
            "username": "alice-agent-evil",
            "email": "evil@example.com",
            "password": PASSWORD,
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 400);
    let (status, _) = w
        .post("/orgs", &w.bob, json!({ "name": "bob-agent-org" }))
        .await;
    assert_eq!(status, 400);

    // An administrator cannot make an agent an administrator.
    let (status, _) = w
        .patch(
            &format!("/admin/users/{agent_id}"),
            &w.admin,
            json!({ "is_admin": true }),
        )
        .await;
    assert_eq!(status, 400);
}

#[tokio::test]
async fn an_agent_token_cannot_reach_account_agent_or_admin_apis() {
    let w = world().await;
    let (agent, _) = w.agent(&w.alice, "ci").await;
    // The widest token an agent can hold.
    let token = w.agent_token(&w.alice, &agent, Some("repo")).await;

    for (method, path, body) in [
        (reqwest::Method::GET, "/users/me".to_string(), None),
        (reqwest::Method::GET, "/users/tokens".to_string(), None),
        (
            reqwest::Method::POST,
            "/users/tokens".to_string(),
            Some(json!({ "name": "self" })),
        ),
        (
            reqwest::Method::POST,
            "/users/ssh-keys".to_string(),
            Some(json!({ "title": "k", "public_key": "ssh-ed25519 AAAA" })),
        ),
        (reqwest::Method::GET, "/agents".to_string(), None),
        (
            reqwest::Method::POST,
            "/agents".to_string(),
            Some(json!({ "name": "child" })),
        ),
        (
            reqwest::Method::POST,
            format!("/agents/{agent}/tokens"),
            Some(json!({ "name": "more" })),
        ),
        (reqwest::Method::GET, "/admin/users".to_string(), None),
    ] {
        let (status, _) = w.send(method.clone(), &path, &token, body).await;
        assert_eq!(status, 403, "{method} {path}");
    }

    // Agents own nothing: no repository, no organization.
    let (status, _) = w.post("/repos", &token, json!({ "name": "mine" })).await;
    assert_eq!(status, 403);
    let (status, _) = w.post("/orgs", &token, json!({ "name": "agentorg" })).await;
    assert_eq!(status, 403);
}

#[tokio::test]
async fn an_agent_can_do_what_it_was_granted_and_its_owner_can_do() {
    let w = world().await;
    let (agent, _) = w.agent(&w.alice, "ci").await;
    let token = w.agent_token(&w.alice, &agent, None).await;

    // Its owner's repository: nothing until it is granted, then what was granted.
    w.private_repo(&w.alice, "mine").await;
    assert!(!w.can_read("alice/mine", &token).await);
    let row = w.grant(&w.alice, "alice/mine", &agent, "read").await;
    assert!(w.can_read("alice/mine", &token).await);
    assert!(!w.can_write("alice/mine", &token, "a.txt").await);
    w.regrant(&w.alice, "alice/mine", row, "write").await;
    assert!(w.can_write("alice/mine", &token, "a.txt").await);

    // Someone else's repository: a grant to the agent alone is not enough,
    // because its owner cannot read the repository.
    w.private_repo(&w.bob, "theirs").await;
    w.grant(&w.bob, "bob/theirs", &agent, "write").await;
    assert!(!w.can_read("bob/theirs", &token).await);
    assert!(!w.can_write("bob/theirs", &token, "a.txt").await);

    // The owner gets read: the agent is capped at read, despite its write grant.
    let alice_row = w.grant(&w.bob, "bob/theirs", "alice", "read").await;
    assert!(w.can_read("bob/theirs", &token).await);
    assert!(!w.can_write("bob/theirs", &token, "b.txt").await);

    // The owner gets write: now the agent's own grant decides.
    w.regrant(&w.bob, "bob/theirs", alice_row, "write").await;
    assert!(w.can_write("bob/theirs", &token, "c.txt").await);

    // The owner is removed: the agent loses the repository on the next request,
    // although its own collaborator row is still there.
    // Removal is addressed by user id (changing a permission, by row id).
    let row_id = w.alice_id;
    let (status, body) = w
        .send(
            reqwest::Method::DELETE,
            &format!("/repos/bob/theirs/collaborators/{row_id}"),
            &w.bob,
            None,
        )
        .await;
    assert!(status.is_success(), "remove alice: {status} {body}");
    assert!(!w.can_read("bob/theirs", &token).await);
    assert!(!w.can_write("bob/theirs", &token, "d.txt").await);
}

#[tokio::test]
async fn an_agent_never_administers_a_repository() {
    let w = world().await;
    let (agent, _) = w.agent(&w.alice, "ci").await;
    let token = w.agent_token(&w.alice, &agent, Some("repo")).await;
    w.private_repo(&w.alice, "mine").await;
    w.grant(&w.alice, "alice/mine", &agent, "admin").await;

    // `admin` still lets it read and write …
    assert!(w.can_read("alice/mine", &token).await);
    assert!(w.can_write("alice/mine", &token, "a.txt").await);
    // … but not manage who has access, or the repository's settings.
    let (status, _) = w
        .post(
            "/repos/alice/mine/collaborators",
            &token,
            json!({ "username": "bob", "permission": "write" }),
        )
        .await;
    assert_eq!(status, 403);
    assert!(!w.can_read("alice/mine", &w.bob).await);
    let (status, _) = w
        .send(reqwest::Method::DELETE, "/repos/alice/mine", &token, None)
        .await;
    assert_eq!(status, 403);
    assert!(w.can_read("alice/mine", &w.alice).await);
}

#[tokio::test]
async fn deactivating_an_agent_or_its_owner_stops_it_at_once() {
    let w = world().await;
    let (agent, _) = w.agent(&w.alice, "ci").await;
    let token = w.agent_token(&w.alice, &agent, None).await;
    w.private_repo(&w.alice, "mine").await;
    w.grant(&w.alice, "alice/mine", &agent, "write").await;
    assert!(w.can_read("alice/mine", &token).await);

    // The agent is deactivated: its token is refused, and no new one is issued.
    let (status, body) = w
        .patch(
            &format!("/agents/{agent}"),
            &w.alice,
            json!({ "is_active": false }),
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(body["is_active"], false);
    assert!(!w.can_read("alice/mine", &token).await);
    let (status, _) = w
        .post(
            &format!("/agents/{agent}/tokens"),
            &w.alice,
            json!({ "name": "t2" }),
        )
        .await;
    assert_eq!(status, 409);

    // Reactivated: the same token works again.
    let (status, _) = w
        .patch(
            &format!("/agents/{agent}"),
            &w.alice,
            json!({ "is_active": true }),
        )
        .await;
    assert_eq!(status, 200);
    assert!(w.can_read("alice/mine", &token).await);

    // The owner is deactivated by an administrator: the agent can do nothing.
    let (status, body) = w
        .patch(
            &format!("/admin/users/{}", w.alice_id),
            &w.admin,
            json!({ "is_active": false }),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert!(!w.can_read("alice/mine", &token).await);
    assert!(!w.can_write("alice/mine", &token, "a.txt").await);
}

#[tokio::test]
async fn an_owner_with_agents_cannot_be_deleted() {
    let w = world().await;
    w.agent(&w.alice, "ci").await;
    let (status, body) = w
        .send(
            reqwest::Method::DELETE,
            &format!("/admin/users/{}", w.alice_id),
            &w.admin,
            None,
        )
        .await;
    assert_eq!(status, 409, "{body}");
    assert!(rg_db::ops::user_ops::find_by_id(&w.db, w.alice_id)
        .await
        .unwrap()
        .is_some());
}
