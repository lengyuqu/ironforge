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

// ── Approving is a human act (§3.4, T20-1 … T20-9) ────────────────────
//
// The gate used to look at the *shape* of the credential — a token limited to
// some branches could not approve — so an agent's token with no branch limit
// approved freely, its owner's pull request included, and the approval counted
// towards the required approvals. It now looks at the account type, which no
// credential disguises, and the approval count only counts people.

mod gate {
    use super::*;
    use common::spawn_test_app_with_root;
    use sea_orm::Set;

    const OWNER: &str = "gate_alice";
    const REVIEWER: &str = "gate_bob";

    fn git(args: &[&str], cwd: Option<&std::path::Path>) -> String {
        let gateway = rg_git::cli_gateway::global_gateway().as_ref().unwrap();
        let out = gateway.run(args, cwd).unwrap();
        out.ensure_success().unwrap();
        out.stdout_str().trim().to_string()
    }

    /// A person, two agents of theirs, and pull requests in a repository whose
    /// `main` requires one approval.
    struct Gate {
        _exclusive: tokio::sync::MutexGuard<'static, ()>,
        base: String,
        db: rg_db::DatabaseConnection,
        owner: String,
        owner_id: i64,
        reviewer: String,
        agent_id: i64,
        /// The agent's token with no branch limit: the shape the old gate let through.
        token: String,
        /// The agent's token limited to one branch: the shape the old gate refused.
        limited_token: String,
        /// An agent's token with the default shape (`repo:write`, no limits).
        default_token: String,
        /// An agent's token with no repository or branch limit either.
        ci_token: String,
        /// Opened by the owner.
        owner_pr: i64,
        /// Opened by `REVIEWER`.
        other_pr: i64,
        /// Opened by the owner's other agent.
        sibling_agent_pr: i64,
    }

    impl Gate {
        fn url(&self, path: &str) -> String {
            format!("{}/api/v1{path}", self.base)
        }

        async fn send(
            &self,
            method: reqwest::Method,
            path: &str,
            auth: &str,
            body: Value,
        ) -> (StatusCode, Value) {
            let resp = Client::new()
                .request(method, self.url(path))
                .bearer_auth(auth)
                .json(&body)
                .send()
                .await
                .unwrap();
            let status = resp.status();
            (status, resp.json().await.unwrap_or(Value::Null))
        }

        async fn post(&self, path: &str, auth: &str, body: Value) -> (StatusCode, Value) {
            self.send(reqwest::Method::POST, path, auth, body).await
        }

        async fn review(&self, pr: i64, auth: &str, action: &str) -> (StatusCode, Value) {
            self.post(
                &format!("/repos/{OWNER}/work/pulls/{pr}/reviews"),
                auth,
                json!({ "action": action, "body": action }),
            )
            .await
        }

        async fn merge(&self, pr: i64, auth: &str) -> (StatusCode, Value) {
            self.post(
                &format!("/repos/{OWNER}/work/pulls/{pr}/merge"),
                auth,
                json!({ "strategy": "merge" }),
            )
            .await
        }

        /// How many approvals the branch protection rule sees.
        async fn counted_approvals(&self, pr_id: i64) -> i64 {
            let pr = rg_db::entities::pull_request::Entity::find_by_id(pr_id)
                .one(&self.db)
                .await
                .unwrap()
                .unwrap();
            rg_db::ops::pr_review_ops::count_current_human_approvals(
                &self.db,
                pr_id,
                pr.head_sha.as_deref(),
            )
            .await
            .unwrap()
        }

        /// Record an approval directly, the way an import or a stale row would:
        /// these tests are about the count, not about the API gate.
        async fn approve_in_db(&self, pr_id: i64, reviewer_id: i64) {
            let pr = rg_db::entities::pull_request::Entity::find_by_id(pr_id)
                .one(&self.db)
                .await
                .unwrap()
                .unwrap();
            rg_db::ops::pr_review_ops::create(
                &self.db,
                rg_db::entities::pr_review::ActiveModel {
                    id: sea_orm::NotSet,
                    pr_id: Set(pr.id),
                    repo_id: Set(pr.repo_id),
                    reviewer_id: Set(reviewer_id),
                    action: Set("approve".to_string()),
                    body: Set(None),
                    commit_id: Set(pr.head_sha.clone()),
                    created_at: Set(chrono::Utc::now()),
                },
            )
            .await
            .unwrap();
        }
    }

    async fn gate() -> Gate {
        // Same discipline as the tests above: one at a time, from an empty
        // permission cache, because that cache is process-wide.
        let exclusive = ONE_AT_A_TIME.lock().await;
        rg_core::repo::service::invalidate_perm_cache_all();
        let (base, db, repo_root) = spawn_test_app_with_root().await;

        // The first account is the administrator; these two are ordinary.
        register_user(&base, "gate_root", "gate_root@example.com", PASSWORD).await;
        let (owner, owner_id) = register_full(&base, OWNER, "gate_alice@example.com").await;
        let reviewer = register_user(&base, REVIEWER, "gate_bob@example.com", PASSWORD).await;

        // A repository with a real `main` and three branches one commit ahead,
        // so creating the pull requests and merging one of them both work.
        let resp = Client::new()
            .post(format!("{base}/api/v1/repos"))
            .bearer_auth(&owner)
            .json(&json!({ "name": "work", "is_private": true, "auto_init": true }))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 201, "create repo: {:?}", resp.text().await);
        let bare = repo_root.join(format!("{OWNER}/work.git"));
        let work = tempfile::tempdir().unwrap();
        let work = work.path();
        git(&["clone", bare.to_string_lossy().as_ref(), "."], Some(work));
        git(&["config", "user.name", "Gate"], Some(work));
        git(&["config", "user.email", "gate@example.com"], Some(work));
        // The unrelated signing settings of whoever runs the tests must not
        // reach this fixture.
        git(&["config", "commit.gpgsign", "false"], Some(work));
        git(&["checkout", "main"], Some(work));
        std::fs::write(work.join("value.txt"), "1\n").unwrap();
        git(&["add", "."], Some(work));
        git(&["commit", "-m", "base"], Some(work));
        for branch in ["feature", "other", "sibling"] {
            git(&["checkout", "main"], Some(work));
            git(&["checkout", "-b", branch], Some(work));
            std::fs::write(work.join("value.txt"), format!("{branch}\n")).unwrap();
            git(&["commit", "-am", branch], Some(work));
        }
        git(
            &["push", "origin", "main", "feature", "other", "sibling"],
            Some(work),
        );

        // Two agents of the same person, and one token each.
        let mut agents = Vec::new();
        for name in ["bot", "helper"] {
            let (status, body) = {
                let resp = Client::new()
                    .post(format!("{base}/api/v1/agents"))
                    .bearer_auth(&owner)
                    .json(&json!({ "name": name }))
                    .send()
                    .await
                    .unwrap();
                (resp.status(), resp.json::<Value>().await.unwrap())
            };
            assert_eq!(status, 201, "create agent {name}: {body}");
            let username = body["username"].as_str().unwrap().to_string();
            let id = body["id"].as_i64().unwrap();
            let token = |name: &str, extra: Value| {
                let mut body = json!({ "name": name });
                for (key, value) in extra.as_object().unwrap() {
                    body[key] = value.clone();
                }
                body
            };
            let issue = |body: Value| {
                let client = Client::new();
                let url = format!("{base}/api/v1/agents/{username}/tokens");
                let jwt = owner.clone();
                async move {
                    let resp = client
                        .post(url)
                        .bearer_auth(jwt)
                        .json(&body)
                        .send()
                        .await
                        .unwrap();
                    let status = resp.status();
                    let body: Value = resp.json().await.unwrap();
                    assert_eq!(status, 201, "issue token: {body}");
                    body["token"].as_str().unwrap().to_string()
                }
            };
            let default_shape = issue(token("default", json!({ "scopes": "repo:write" }))).await;
            let unlimited = issue(token("unlimited", json!({}))).await;
            let limited = issue(token(
                "limited",
                json!({ "repos": [format!("{OWNER}/work")], "refs": ["agent/*"] }),
            ))
            .await;
            let ci = issue(token("ci", json!({ "scopes": "repo,repo:write" }))).await;
            agents.push((username, id, unlimited, limited, ci, default_shape));
        }
        let (agent, agent_id, token, limited_token, ci_token, default_token) = agents[0].clone();
        let (sibling, _, sibling_token, _, _, _) = agents[1].clone();

        let create_pr = |auth: String, head: &str| {
            let client = Client::new();
            let url = format!("{base}/api/v1/repos/{OWNER}/work/pulls");
            let head = head.to_string();
            async move {
                let resp = client
                    .post(url)
                    .bearer_auth(auth)
                    .json(&json!({ "title": &head, "head": head, "base": "main" }))
                    .send()
                    .await
                    .unwrap();
                let status = resp.status();
                let body: Value = resp.json().await.unwrap();
                assert_eq!(status, 201, "open a PR from {head}: {body}");
                body["id"].as_i64().unwrap()
            }
        };
        // Both agents need write access to review on the repository, and the
        // reviewer needs read access to review a pull request the owner opened.
        // (Opening a PR needs write, which is why `other` is opened by the
        // owner: only its author decides what T20-1, T20-2 and T20-9 mean.)
        for username in [&agent, &sibling, &REVIEWER.to_string()] {
            let (status, body) = {
                let resp = Client::new()
                    .post(format!("{base}/api/v1/repos/{OWNER}/work/collaborators"))
                    .bearer_auth(&owner)
                    .json(&json!({ "username": username, "permission": "write" }))
                    .send()
                    .await
                    .unwrap();
                (resp.status(), resp.json::<Value>().await.unwrap())
            };
            assert!(
                status.is_success(),
                "grant write to {username}: {status} {body}"
            );
        }

        let owner_pr = create_pr(owner.clone(), "feature").await;
        let other_pr = create_pr(owner.clone(), "other").await;
        let sibling_agent_pr = create_pr(sibling_token.clone(), "sibling").await;

        let resp = Client::new()
            .post(format!(
                "{base}/api/v1/repos/{OWNER}/work/branches/protection"
            ))
            .bearer_auth(&owner)
            .json(&json!({
                "branch_name": "main",
                "require_approval": true,
                "required_approvals": 1
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 201, "protect main: {:?}", resp.text().await);

        // `other` is the reviewer's pull request: its author is the person who
        // must approve, so the "not your own PR" check is not what this fixture
        // is testing.
        let reviewer_id = rg_db::ops::user_ops::find_by_username(&db, REVIEWER)
            .await
            .unwrap()
            .unwrap()
            .id;
        rg_db::entities::pull_request::Entity::update_many()
            .col_expr(
                rg_db::entities::pull_request::Column::AuthorId,
                sea_orm::sea_query::Expr::value(reviewer_id),
            )
            .filter(rg_db::entities::pull_request::Column::Id.eq(other_pr))
            .exec(&db)
            .await
            .unwrap();

        Gate {
            _exclusive: exclusive,
            base,
            db,
            owner,
            owner_id,
            reviewer,
            agent_id,
            token,
            limited_token,
            ci_token,
            default_token,
            owner_pr,
            other_pr,
            sibling_agent_pr,
        }
    }

    /// The API path of an absolute URL: everything from `/repos/...` on.
    fn request_path(url: &str) -> &str {
        let start = url.find("/api/v1").expect("an API URL") + "/api/v1".len();
        &url[start..]
    }

    fn reason_of(body: &Value) -> (&str, Option<&str>) {
        (
            body["error"]["code"].as_str().unwrap_or_default(),
            body["error"]["reason"].as_str(),
        )
    }

    fn assert_agent_refused(status: StatusCode, body: &Value) {
        assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
        assert_eq!(
            reason_of(body),
            ("FORBIDDEN", Some("AGENT_MAY_NOT_APPROVE")),
            "{body}"
        );
    }

    /// T20-1: the hole itself — an agent token with no branch limit approving.
    #[tokio::test]
    async fn t20_1_an_agent_token_without_a_branch_limit_cannot_approve() {
        let g = gate().await;
        let (status, body) = g.review(g.other_pr, &g.token, "approve").await;
        assert_agent_refused(status, &body);
        assert_eq!(g.counted_approvals(g.other_pr).await, 0);
    }

    /// T20-2: the rule the old gate did have — a branch-limited token — still holds.
    #[tokio::test]
    async fn t20_2_an_agent_token_with_a_branch_limit_cannot_approve() {
        let g = gate().await;
        let (status, body) = g.review(g.other_pr, &g.limited_token, "approve").await;
        assert_agent_refused(status, &body);
        assert_eq!(g.counted_approvals(g.other_pr).await, 0);
    }

    /// T20-3: what made the hole a security bug — an agent approving its own
    /// owner's pull request, which the "not your own PR" check cannot catch
    /// because the author is the person and the reviewer is the agent.
    #[tokio::test]
    async fn t20_3_an_agent_cannot_approve_its_owners_pull_request() {
        let g = gate().await;
        let (status, body) = g.review(g.owner_pr, &g.token, "approve").await;
        assert_agent_refused(status, &body);
        assert_eq!(g.counted_approvals(g.owner_pr).await, 0);
        // The owner cannot merge on the strength of it either.
        let (status, body) = g.merge(g.owner_pr, &g.owner).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    }

    /// T20-4: one of the person's agents approving another one's pull request.
    #[tokio::test]
    async fn t20_4_an_agent_cannot_approve_a_sibling_agents_pull_request() {
        let g = gate().await;
        let (status, body) = g.review(g.sibling_agent_pr, &g.token, "approve").await;
        assert_agent_refused(status, &body);
        assert_eq!(g.counted_approvals(g.sibling_agent_pr).await, 0);
    }

    /// T20-5: only approving is a human act. Talking is still allowed.
    #[tokio::test]
    async fn t20_5_an_agent_may_comment_and_request_changes() {
        let g = gate().await;
        for action in ["comment", "request_changes"] {
            let (status, body) = g.review(g.owner_pr, &g.token, action).await;
            assert_eq!(status, StatusCode::CREATED, "{action}: {body}");
        }
        // Neither of those counts as an approval.
        assert_eq!(g.counted_approvals(g.owner_pr).await, 0);
    }

    /// T20-6: the gate no longer depends on the request carrying a Bearer
    /// token, so a person's cookie session is judged too — and allowed.
    #[tokio::test]
    async fn t20_6_a_person_approving_with_a_cookie_session_is_allowed() {
        let g = gate().await;
        let resp = Client::new()
            .post(g.url("/users/login"))
            .json(&json!({ "login": REVIEWER, "password": PASSWORD }))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200, "login");
        let cookie = resp
            .headers()
            .get("set-cookie")
            .unwrap_or_else(|| panic!("login must set a session cookie"))
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_string();
        let resp = Client::new()
            .post(g.url(&format!("/repos/{OWNER}/work/pulls/{}/reviews", g.owner_pr)))
            .header(reqwest::header::COOKIE, cookie)
            .json(&json!({ "action": "approve", "body": "looks right" }))
            .send()
            .await
            .unwrap();
        let status = resp.status();
        let body: Value = resp.json().await.unwrap();
        assert_eq!(status, StatusCode::CREATED, "{body}");
        assert_eq!(g.counted_approvals(g.owner_pr).await, 1);
    }

    /// T20-7: an agent's approval in the table does not satisfy the rule.
    #[tokio::test]
    async fn t20_7_an_agent_approval_does_not_satisfy_the_approval_rule() {
        let g = gate().await;
        g.approve_in_db(g.owner_pr, g.agent_id).await;
        assert_eq!(g.counted_approvals(g.owner_pr).await, 0);
        let (status, body) = g.merge(g.owner_pr, &g.owner).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
        let message = body["error"]["message"].as_str().unwrap_or_default();
        assert!(message.contains("requires at least 1 approval"), "{body}");
    }

    /// T20-8: a person approving their own agent's pull request is allowed and
    /// counts — the account type, not the relationship, decides.
    #[tokio::test]
    async fn t20_8_the_owner_approving_an_agents_pull_request_counts() {
        let g = gate().await;
        let (status, body) = g.review(g.sibling_agent_pr, &g.owner, "approve").await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        assert_eq!(g.counted_approvals(g.sibling_agent_pr).await, 1);
        let (status, body) = g.merge(g.sibling_agent_pr, &g.owner).await;
        assert_eq!(status, StatusCode::OK, "{body}");
    }

    /// T20-9: an agent's approval next to a person's does not raise the count.
    #[tokio::test]
    async fn t20_9_an_agent_approval_does_not_add_to_a_persons() {
        let g = gate().await;
        let (status, body) = g.review(g.owner_pr, &g.reviewer, "approve").await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        g.approve_in_db(g.owner_pr, g.agent_id).await;
        assert_eq!(g.counted_approvals(g.owner_pr).await, 1);
    }

    // ── The same gate on a protected CI environment ───────────────────
    //
    // Releasing a protected environment is decided by a count of approvals, so
    // it is a person's act for exactly the reason a pull request approval is.
    // The approver list accepted any account id and nothing looked at the
    // account type when the approval was counted, so listing one's own agent
    // made it the only voice the gate needed. (`can_admin_repo` is false for an
    // agent, so it is the list, not the administrator shortcut, that let it in.)

    impl Gate {
        /// A protected environment `production` needing one approval, with a job
        /// paused on it. Returns the approval URL and the environment row.
        async fn environment_awaiting_approval(
            &self,
        ) -> (String, rg_db::entities::ci_environment::Model) {
            let (status, body) = self
                .post(
                    &format!("/repos/{OWNER}/work/actions/environments"),
                    &self.owner,
                    json!({
                        "name": "production",
                        "protected": true,
                        "required_approvals": 1,
                        "allowed_approver_ids": [],
                    }),
                )
                .await;
            assert_eq!(status, StatusCode::CREATED, "{body}");
            let environment =
                rg_db::ops::ci_environment_ops::find_by_id(&self.db, body["id"].as_i64().unwrap())
                    .await
                    .unwrap()
                    .unwrap();
            let pipeline = rg_db::ops::pipeline_ops::create_pipeline(
                &self.db,
                environment.repo_id,
                "abcdefabcdefabcdefabcdefabcdefabcdefabcd",
                "refs/heads/main",
                "push",
                None,
            )
            .await
            .unwrap();
            let stage = rg_db::ops::pipeline_ops::create_stage(&self.db, pipeline.id, "deploy", 0)
                .await
                .unwrap();
            let job = rg_db::ops::pipeline_ops::create_job(
                &self.db,
                stage.id,
                "production",
                "echo deploy",
                None,
                None,
                None,
                None,
                None,
                false,
                None,
                None,
                None,
            )
            .await
            .unwrap();
            rg_db::ops::ci_environment_ops::attach_job(
                &self.db,
                job.id,
                Some(&environment),
                "production",
            )
            .await
            .unwrap();
            assert!(
                rg_db::ops::pipeline_ops::try_pause_stage_at_manual(&self.db, stage.id)
                    .await
                    .unwrap()
            );
            let url = self.url(&format!(
                "/repos/{OWNER}/work/pipelines/{}/jobs/{}/approve",
                pipeline.id, job.id
            ));
            (url, environment)
        }
    }

    /// T20-10: an agent cannot be named as an approver at all.
    #[tokio::test]
    async fn t20_10_an_agent_cannot_be_listed_as_an_environment_approver() {
        let g = gate().await;
        let (status, body) = g
            .post(
                &format!("/repos/{OWNER}/work/actions/environments"),
                &g.owner,
                json!({
                    "name": "production",
                    "protected": true,
                    "required_approvals": 1,
                    "allowed_approver_ids": [g.agent_id],
                }),
            )
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert!(
            body["error"]["message"]
                .as_str()
                .unwrap_or_default()
                .contains("agent"),
            "{body}"
        );
    }

    /// T20-11: a list written before the rule (or by hand) can still name one,
    /// and the count is what releases the job: the approval is refused there too.
    #[tokio::test]
    async fn t20_11_an_agent_named_in_an_old_list_still_cannot_release() {
        let g = gate().await;
        let (url, environment) = g.environment_awaiting_approval().await;
        let mut active: rg_db::entities::ci_environment::ActiveModel = environment.clone().into();
        active.allowed_approver_ids = Set(Some(json!([g.agent_id]).to_string()));
        rg_db::ops::ci_environment_ops::update(&g.db, active)
            .await
            .unwrap();

        // Whatever the token's shape, the release is refused. Which gate answers
        // first depends on the credential (a `repo:write` token is turned away by
        // the scope middleware before the handler runs), so this asserts the
        // outcome rather than the reason; the account-type check is what keeps
        // the answer right for a token that does reach the handler.
        for token in [&g.default_token, &g.ci_token] {
            let (status, body) = g.post(request_path(&url), token, json!({})).await;
            assert_eq!(
                status,
                StatusCode::FORBIDDEN,
                "the approval must be refused: {body}"
            );
        }

        // Nothing was released and nothing was recorded as an approval.
        let job_id: i64 = url
            .rsplit('/')
            .nth(1)
            .unwrap()
            .parse()
            .expect("job id in the approval URL");
        assert_eq!(
            rg_db::ops::ci_environment_ops::count_approvals(&g.db, job_id)
                .await
                .unwrap(),
            0
        );

        // The check must not turn the endpoint into a "no": a person the list
        // names still releases the job.
        use sea_orm::EntityTrait;
        rg_db::entities::ci_environment::Entity::update_many()
            .col_expr(
                rg_db::entities::ci_environment::Column::AllowedApproverIds,
                sea_orm::sea_query::Expr::value(json!([g.owner_id]).to_string()),
            )
            .filter(rg_db::entities::ci_environment::Column::Id.eq(environment.id))
            .exec(&g.db)
            .await
            .unwrap();
        let (status, body) = g.post(request_path(&url), &g.owner, json!({})).await;

        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(
            rg_db::ops::ci_environment_ops::count_approvals(&g.db, job_id)
                .await
                .unwrap(),
            1
        );
    }
}
