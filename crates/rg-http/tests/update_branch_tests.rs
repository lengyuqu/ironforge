//! `POST /repos/{o}/{n}/pulls/{number}/update-branch`: merge the base branch
//! into the pull request's head branch, pinned to `expected_head_sha`.
//!
//! What must hold: the head moves to one merge commit built from exactly the
//! commit the caller named (or does not move); a conflicting base changes no ref;
//! a repeat is a no-op; it is held to the permission of a push to that branch;
//! and a successful update is announced like a push (pull-request head, event
//! log) and audited. Runs against PostgreSQL / MySQL too when
//! `IRONFORGE_TEST_DATABASE_URL` is set.

mod common;
use common::{register_user, spawn_test_app_on_env_db_with_root};

use reqwest::{Client, StatusCode};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use serde_json::{json, Value};

const PW: &str = "Qz7$wRtm";

struct World {
    base: String,
    db: rg_db::DatabaseConnection,
    root: std::path::PathBuf,
    client: Client,
    owner: String,
    jwt: String,
}

impl World {
    async fn new(owner: &str) -> World {
        let (base, db, root) = spawn_test_app_on_env_db_with_root().await;
        let client = Client::new();
        let jwt = register_user(&base, owner, &format!("{owner}@example.com"), PW).await;
        let resp = client
            .post(format!("{base}/api/v1/repos"))
            .bearer_auth(&jwt)
            .json(&json!({"name": "work", "auto_init": true, "is_private": false}))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 201);
        World {
            base,
            db,
            root,
            client,
            owner: owner.to_string(),
            jwt,
        }
    }

    fn url(&self, suffix: &str) -> String {
        format!("{}/api/v1/repos/{}/work{suffix}", self.base, self.owner)
    }

    fn git_dir(&self) -> std::path::PathBuf {
        self.root.join(format!("{}/work.git", self.owner))
    }

    fn rev(&self, spec: &str) -> Option<String> {
        rg_git::ops::try_rev_parse(&self.git_dir(), spec)
            .ok()
            .flatten()
    }

    fn blob_sha(&self, branch: &str, path: &str) -> String {
        rg_git::ops::rev_parse(&self.git_dir(), &format!("refs/heads/{branch}:{path}")).unwrap()
    }

    async fn write(&self, branch: &str, path: &str, content: &str, sha: Option<&str>) -> String {
        let mut body =
            json!({"branch": branch, "message": format!("write {path}"), "content": content});
        if let Some(sha) = sha {
            body["sha"] = json!(sha);
        }
        let resp = self
            .client
            .post(self.url(&format!("/contents/{path}")))
            .bearer_auth(&self.jwt)
            .json(&body)
            .send()
            .await
            .unwrap();
        let status = resp.status();
        let v: Value = resp.json().await.unwrap_or(Value::Null);
        assert_eq!(status, StatusCode::OK, "{v}");
        v["commit_sha"].as_str().unwrap().to_string()
    }

    async fn open_pr(&self, head: &str) {
        let resp = self
            .client
            .post(self.url("/pulls"))
            .bearer_auth(&self.jwt)
            .json(&json!({"title": "Change", "head": head, "base": "main"}))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
    }

    /// `POST update-branch` as `bearer` (none: anonymous).
    async fn update_as(&self, bearer: Option<&str>, body: Value) -> (StatusCode, Value) {
        let mut req = self
            .client
            .post(self.url("/pulls/1/update-branch"))
            .json(&body);
        if let Some(token) = bearer {
            req = req.bearer_auth(token);
        }
        let resp = req.send().await.unwrap();
        let status = resp.status();
        (status, resp.json().await.unwrap_or(Value::Null))
    }

    async fn update(&self, expected: &str) -> (StatusCode, Value) {
        self.update_as(Some(&self.jwt), json!({"expected_head_sha": expected}))
            .await
    }

    async fn pr(&self) -> Value {
        self.client
            .get(self.url("/pulls/1"))
            .bearer_auth(&self.jwt)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap()
    }

    async fn events(&self, since: i64) -> Value {
        self.client
            .get(self.url(&format!("/events?since={since}")))
            .bearer_auth(&self.jwt)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap()
    }

    async fn make_token(&self, body: Value) -> String {
        let resp = self
            .client
            .post(format!("{}/api/v1/users/tokens", self.base))
            .bearer_auth(&self.jwt)
            .json(&body)
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 201);
        resp.json::<Value>().await.unwrap()["token"]
            .as_str()
            .unwrap()
            .to_string()
    }

    /// A pull request from `agent/x` whose base has moved on cleanly: the head
    /// is behind. Returns `(head, base)`.
    async fn behind(&self) -> (String, String) {
        let head = self.write("agent/x", "one.txt", "1\n", None).await;
        self.open_pr("agent/x").await;
        let base = self.write("main", "other.txt", "o\n", None).await;
        (head, base)
    }

    /// This world's update-branch audit entries. Filtered by resource: on
    /// PostgreSQL/MySQL every test of a target shares one database.
    async fn audit_rows(&self) -> Vec<rg_db::entities::audit_log::Model> {
        rg_db::entities::audit_log::Entity::find()
            .filter(rg_db::entities::audit_log::Column::Action.eq("pull_request.update_branch"))
            .filter(
                rg_db::entities::audit_log::Column::ResourceName
                    .eq(format!("{}/work#1", self.owner)),
            )
            .all(&self.db)
            .await
            .unwrap()
    }

    fn parents(&self, commit: &str) -> Vec<String> {
        let gateway = rg_git::cli_gateway::global_gateway().as_ref().unwrap();
        let out = gateway
            .run(&["log", "-1", "--format=%P", commit], Some(&self.git_dir()))
            .unwrap();
        out.stdout_str()
            .split_whitespace()
            .map(str::to_string)
            .collect()
    }

    fn author(&self, commit: &str) -> String {
        let gateway = rg_git::cli_gateway::global_gateway().as_ref().unwrap();
        gateway
            .run(
                &["log", "-1", "--format=%an <%ae>", commit],
                Some(&self.git_dir()),
            )
            .unwrap()
            .stdout_str()
            .trim()
            .to_string()
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn updating_merges_the_base_into_the_head_and_announces_it_like_a_push() {
    let w = World::new("ub_ok").await;
    let (head, base) = w.behind().await;
    let events_before = w.events(0).await["next_cursor"].as_i64().unwrap();

    let (status, v) = w.update(&head).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(v["outcome"], "updated");
    assert_eq!(v["head_sha_before"], json!(head));
    assert_eq!(v["base_sha"], json!(base));
    assert_eq!(v["created_commits"], 1);
    let merged = v["merge_commit_sha"].as_str().unwrap().to_string();
    assert_eq!(v["head_sha_after"], json!(merged));

    // The head branch is at the merge commit: parents are the old head first,
    // then the base tip; the base branch did not move; the author is the caller.
    assert_eq!(w.rev("refs/heads/agent/x"), Some(merged.clone()));
    assert_eq!(w.parents(&merged), vec![head.clone(), base.clone()]);
    assert_eq!(w.rev("refs/heads/main"), Some(base.clone()));
    assert_eq!(w.author(&merged), "ub_ok <ub_ok@example.com>");

    // Announced like a push: the pull request follows its head, the
    // `refs/pull/<n>/head` ref moves, and the event log has the push.
    let mut synced = false;
    for _ in 0..50 {
        if w.pr().await["head_sha"] == json!(merged)
            && w.rev("refs/pull/1/head") == Some(merged.clone())
        {
            synced = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert!(synced, "the pull request did not follow the new head");
    let mut pushed = false;
    for _ in 0..50 {
        let events = w.events(events_before).await;
        if events["events"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["type"] == "push" && e["payload"].to_string().contains(&merged))
        {
            pushed = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert!(pushed, "no push event for the update");

    // Audited with what moved.
    let rows = w.audit_rows().await;
    assert_eq!(rows.len(), 1);
    let details = rows[0].details.clone().unwrap_or_default();
    assert!(
        details.contains(&merged) && details.contains(&head),
        "{details}"
    );

    // Readiness now sees a head that contains its base.
    let v: Value = w
        .client
        .get(w.url("/pulls/1/merge-readiness"))
        .bearer_auth(&w.jwt)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(v["merge"]["head_contains_base"], true);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_second_call_on_the_new_head_is_a_no_op_and_the_old_head_is_stale() {
    let w = World::new("ub_twice").await;
    let (head, _) = w.behind().await;
    let (_, v) = w.update(&head).await;
    let merged = v["head_sha_after"].as_str().unwrap().to_string();

    // Same head as now: already up to date; nothing written, nothing audited.
    let (status, v) = w.update(&merged).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(v["outcome"], "already_up_to_date");
    assert_eq!(v["created_commits"], 0);
    assert_eq!(v["head_sha_after"], json!(merged));
    assert_eq!(w.rev("refs/heads/agent/x"), Some(merged.clone()));
    let rows = w.audit_rows().await;
    assert_eq!(rows.len(), 1);

    // The commit the first call was based on is not the head any more.
    let (status, v) = w.update(&head).await;
    assert_eq!(status, StatusCode::CONFLICT, "{v}");
    assert_eq!(v["error"]["reason"], "REF_ADVANCED");
    assert_eq!(v["error"]["details"]["current_sha"], json!(merged));
    assert_eq!(w.rev("refs/heads/agent/x"), Some(merged));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_stale_expected_head_changes_nothing() {
    let w = World::new("ub_stale").await;
    let (head, _) = w.behind().await;
    let newer = w.write("agent/x", "two.txt", "2\n", None).await;

    let (status, v) = w.update(&head).await;
    assert_eq!(status, StatusCode::CONFLICT, "{v}");
    assert_eq!(v["error"]["reason"], "REF_ADVANCED");
    assert_eq!(v["error"]["retryable"], true);
    assert_eq!(v["error"]["details"]["current_sha"], json!(newer));
    assert_eq!(v["error"]["details"]["expected_sha"], json!(head));
    assert_eq!(w.rev("refs/heads/agent/x"), Some(newer));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_conflict_is_a_409_with_the_files_and_no_ref_moves() {
    let w = World::new("ub_conflict").await;
    w.write("main", "f.txt", "top\nmiddle\nbottom\n", None)
        .await;
    let sha = w.blob_sha("main", "f.txt");
    let head = w
        .write("agent/x", "f.txt", "top\nhead side\nbottom\n", Some(&sha))
        .await;
    w.open_pr("agent/x").await;
    let base = w
        .write("main", "f.txt", "top\nbase side\nbottom\n", Some(&sha))
        .await;
    let before = (
        w.rev("refs/heads/agent/x"),
        w.rev("refs/heads/main"),
        w.rev("refs/pull/1/head"),
    );

    let (status, v) = w.update(&head).await;
    assert_eq!(status, StatusCode::CONFLICT, "{v}");
    assert_eq!(v["error"]["reason"], "MERGE_CONFLICTS");
    assert_eq!(v["error"]["retryable"], false);
    let details = &v["error"]["details"];
    assert_eq!(details["outcome"], "conflicts");
    assert_eq!(details["head_sha"], json!(head));
    assert_eq!(details["base_sha"], json!(base));
    assert_eq!(details["merge"]["conflict_files"], 1);
    assert_eq!(details["merge"]["conflicts"][0]["path"], "f.txt");
    assert_eq!(details["merge"]["conflicts"][0]["kind"], "content");
    assert_eq!(
        details["merge"]["conflicts"][0]["blocks"][0]["theirs"]["excerpt"],
        "head side\n"
    );

    let after = (
        w.rev("refs/heads/agent/x"),
        w.rev("refs/heads/main"),
        w.rev("refs/pull/1/head"),
    );
    assert_eq!(before, after, "a conflict must not move any ref");
    assert_eq!(w.rev("refs/heads/agent/x"), Some(head));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_dry_run_reports_and_writes_nothing() {
    let w = World::new("ub_dry").await;
    let (head, base) = w.behind().await;
    let before = (
        w.rev("refs/heads/agent/x"),
        w.rev("refs/heads/main"),
        w.rev("refs/pull/1/head"),
    );

    let (status, v) = w
        .update_as(
            Some(&w.jwt),
            json!({"expected_head_sha": head, "dry_run": true}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(v["dry_run"], true);
    assert_eq!(v["outcome"], "would_update");
    assert_eq!(v["would"][0]["effect"], "merge_commit");
    assert_eq!(v["would"][0]["parents"], json!([head, base]));

    // The header means the same.
    let resp = w
        .client
        .post(w.url("/pulls/1/update-branch"))
        .bearer_auth(&w.jwt)
        .header("X-Dry-Run", "1")
        .json(&json!({"expected_head_sha": head}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    assert_eq!(resp.json::<Value>().await.unwrap()["dry_run"], true);

    let after = (
        w.rev("refs/heads/agent/x"),
        w.rev("refs/heads/main"),
        w.rev("refs/pull/1/head"),
    );
    assert_eq!(before, after);
    let rows = w.audit_rows().await;
    assert!(rows.is_empty(), "a dry run is not audited as a write");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_token_limited_to_some_branches_is_refused_and_a_plain_write_token_is_not() {
    let w = World::new("ub_token").await;
    let (head, _) = w.behind().await;
    let repos = json!([format!("{}/work", w.owner)]);

    // Limited to `agent/*` — the very branch the pull request's head is on. It is
    // still refused: updating a branch is not among what such a token may do.
    let limited = w
        .make_token(
            json!({"name": "limited", "scopes": "repo:write", "repos": repos, "refs": ["agent/*"]}),
        )
        .await;
    let (status, v) = w
        .update_as(Some(&limited), json!({"expected_head_sha": head}))
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{v}");
    assert_eq!(w.rev("refs/heads/agent/x"), Some(head.clone()));

    // A read-only token cannot write either.
    let read_only = w
        .make_token(json!({"name": "ro", "scopes": "repo:read", "repos": repos}))
        .await;
    let (status, _) = w
        .update_as(Some(&read_only), json!({"expected_head_sha": head}))
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // A write token with no branch limit works.
    let plain = w
        .make_token(json!({"name": "plain", "scopes": "repo:write", "repos": repos}))
        .await;
    let (status, v) = w
        .update_as(Some(&plain), json!({"expected_head_sha": head}))
        .await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(v["outcome"], "updated");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn without_write_access_there_is_no_update() {
    let w = World::new("ub_perm").await;
    let (head, _) = w.behind().await;

    let (status, _) = w.update_as(None, json!({"expected_head_sha": head})).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // Can read the public repository, cannot write it.
    let stranger = register_user(&w.base, "ub_stranger", "ub-stranger@example.com", PW).await;
    let (status, _) = w
        .update_as(Some(&stranger), json!({"expected_head_sha": head}))
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(w.rev("refs/heads/agent/x"), Some(head.clone()));

    // The body must say which commit it is based on.
    let (status, _) = w.update_as(Some(&w.jwt), json!({})).await;
    assert!(status.is_client_error(), "{status}");
    assert_eq!(w.rev("refs/heads/agent/x"), Some(head));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_protected_head_branch_refuses_what_a_push_would_refuse() {
    let w = World::new("ub_protected").await;
    let (head, _) = w.behind().await;
    let resp = w
        .client
        .post(w.url("/branches/protection"))
        .bearer_auth(&w.jwt)
        .json(&json!({"branch_name": "agent/*", "require_pr": true}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);

    let (status, v) = w.update(&head).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{v}");
    assert_eq!(w.rev("refs/heads/agent/x"), Some(head));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_closed_or_merged_pull_request_is_not_updated() {
    let w = World::new("ub_closed").await;
    let (head, _) = w.behind().await;
    let resp = w
        .client
        .post(w.url("/pulls/1/merge"))
        .bearer_auth(&w.jwt)
        .json(&json!({"strategy": "merge"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let (status, v) = w.update(&head).await;
    assert_eq!(status, StatusCode::CONFLICT, "{v}");
    assert_eq!(w.rev("refs/heads/agent/x"), Some(head));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_fork_pull_request_is_refused_with_a_stable_reason() {
    let w = World::new("ub_fork").await;
    let bob = register_user(&w.base, "ub_forker", "ub-forker@example.com", PW).await;
    let resp = w
        .client
        .post(format!("{}/api/v1/repos/ub_fork/work/fork", w.base))
        .bearer_auth(&bob)
        .json(&json!({}))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success(), "{}", resp.status());
    let resp = w
        .client
        .post(format!(
            "{}/api/v1/repos/ub_forker/work/contents/one.txt",
            w.base
        ))
        .bearer_auth(&bob)
        .json(&json!({"branch": "feat", "message": "add", "content": "1\n"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let head = resp.json::<Value>().await.unwrap()["commit_sha"]
        .as_str()
        .unwrap()
        .to_string();
    let resp = w
        .client
        .post(w.url("/pulls"))
        .bearer_auth(&bob)
        .json(&json!({"title": "From a fork", "head": "ub_forker:feat", "base": "main"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    w.write("main", "other.txt", "o\n", None).await;

    // The base repository's owner may write here, but the head branch is not here.
    let (status, v) = w.update(&head).await;
    assert_eq!(status, StatusCode::CONFLICT, "{v}");
    assert_eq!(v["error"]["reason"], "FORK_PR_UNSUPPORTED");
    assert_eq!(v["error"]["retryable"], false);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_agent_updates_the_branch_under_its_own_name() {
    let w = World::new("ub_agent").await;
    let (head, _) = w.behind().await;
    let post = |path: String, body: Value| {
        let (client, base, jwt) = (w.client.clone(), w.base.clone(), w.jwt.clone());
        async move {
            let resp = client
                .post(format!("{base}/api/v1{path}"))
                .bearer_auth(jwt)
                .json(&body)
                .send()
                .await
                .unwrap();
            let status = resp.status();
            (status, resp.json::<Value>().await.unwrap_or(Value::Null))
        }
    };
    let (status, agent) = post("/agents".to_string(), json!({"name": "bot"})).await;
    assert_eq!(status, 201, "{agent}");
    let username = agent["username"].as_str().unwrap().to_string();
    let (status, body) = post(
        "/repos/ub_agent/work/collaborators".to_string(),
        json!({"username": username, "permission": "write"}),
    )
    .await;
    assert!(status.is_success(), "{body}");
    let (status, token) = post(format!("/agents/{username}/tokens"), json!({"name": "t"})).await;
    assert_eq!(status, 201, "{token}");
    let token = token["token"].as_str().unwrap().to_string();

    let (status, v) = w
        .update_as(Some(&token), json!({"expected_head_sha": head}))
        .await;
    assert_eq!(status, StatusCode::OK, "{v}");
    let merged = v["merge_commit_sha"].as_str().unwrap().to_string();
    assert_eq!(
        w.author(&merged),
        format!("{username} <{username}@agents.invalid>")
    );

    // The audit entry names the agent's owner.
    let rows = w.audit_rows().await;
    assert_eq!(rows.len(), 1);
    assert!(rows[0].on_behalf_of_user_id.is_some());
}
