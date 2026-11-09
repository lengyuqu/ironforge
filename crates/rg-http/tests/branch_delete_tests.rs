//! `DELETE /repos/{owner}/{name}/branches/{branch}`.
//!
//! A branch that is safe to delete goes (and a merged or closed pull request
//! keeps its `refs/pull/<n>/head`); the default branch, a protected branch and a
//! branch an open pull request uses are refused with a stable `reason`; a stale
//! decision (`expected_sha`) cannot discard newer commits; a token limited to some
//! branches stays inside its limit. Runs against PostgreSQL / MySQL too when
//! `IRONFORGE_TEST_DATABASE_URL` is set.

mod common;
use common::{register_user, spawn_test_app_on_env_db_with_root};

use reqwest::{Client, StatusCode};
use serde_json::{json, Value};
use std::path::Path;

const PW: &str = "Qz7$wRtm";

fn git(args: &[&str], cwd: Option<&Path>) -> String {
    let gateway = rg_git::cli_gateway::global_gateway().as_ref().unwrap();
    let out = gateway.run(args, cwd).unwrap();
    out.ensure_success().unwrap();
    out.stdout_str().trim().to_string()
}

struct Fixture {
    base: String,
    jwt: String,
    owner: &'static str,
    client: Client,
    db: rg_db::DatabaseConnection,
}

impl Fixture {
    async fn new(owner: &'static str) -> Fixture {
        let (base, db, _root) = spawn_test_app_on_env_db_with_root().await;
        let jwt = register_user(&base, owner, &format!("{owner}@example.com"), PW).await;
        let client = Client::new();
        let resp = client
            .post(format!("{base}/api/v1/repos"))
            .bearer_auth(&jwt)
            .json(&json!({"name": "work", "auto_init": true, "is_private": false}))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 201);
        Fixture {
            base,
            jwt,
            owner,
            client,
            db,
        }
    }

    fn api(&self, suffix: &str) -> String {
        format!("{}/api/v1/repos/{}/work{suffix}", self.base, self.owner)
    }

    async fn call(
        &self,
        method: reqwest::Method,
        suffix: &str,
        bearer: Option<&str>,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let mut req = self.client.request(method, self.api(suffix));
        if let Some(token) = bearer {
            req = req.bearer_auth(token);
        }
        if let Some(body) = body {
            req = req.json(&body);
        }
        let resp = req.send().await.unwrap();
        let status = resp.status();
        (status, resp.json().await.unwrap_or(Value::Null))
    }

    /// Commit a file to `branch` (creating it from the default branch).
    async fn commit(&self, bearer: &str, branch: &str, path: &str) -> String {
        let (status, v) = self
            .call(
                reqwest::Method::POST,
                &format!("/contents/{path}"),
                Some(bearer),
                Some(json!({"branch": branch, "message": format!("add {path}"), "content": "x\n"})),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{v}");
        v["commit_sha"].as_str().unwrap().to_string()
    }

    async fn delete(&self, bearer: Option<&str>, branch: &str, query: &str) -> (StatusCode, Value) {
        self.call(
            reqwest::Method::DELETE,
            &format!("/branches/{branch}{query}"),
            bearer,
            None,
        )
        .await
    }

    async fn branches(&self) -> Vec<String> {
        let (status, v) = self
            .call(reqwest::Method::GET, "/branches", Some(&self.jwt), None)
            .await;
        assert_eq!(status, StatusCode::OK);
        v.as_array()
            .unwrap()
            .iter()
            .map(|b| b.as_str().unwrap().to_string())
            .collect()
    }

    /// A write token for the owner, optionally limited to repositories and branches.
    async fn token(&self, limited: bool) -> (i64, String) {
        let mut body = json!({"name": "agent", "scopes": "repo:write"});
        if limited {
            body["repos"] = json!([format!("{}/work", self.owner)]);
            body["refs"] = json!(["agent/*"]);
        }
        let resp = self
            .client
            .post(format!("{}/api/v1/users/tokens", self.base))
            .bearer_auth(&self.jwt)
            .json(&body)
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 201);
        let v: Value = resp.json().await.unwrap();
        (
            v["id"].as_i64().unwrap(),
            v["token"].as_str().unwrap().to_string(),
        )
    }

    async fn remote(&self) -> String {
        let (_, token) = self.token(false).await;
        self.base
            .replacen("http://", &format!("http://{}:{token}@", self.owner), 1)
            + &format!("/{}/work.git", self.owner)
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_branch_with_an_open_pull_request_is_kept_and_a_closed_one_leaves_its_ref() {
    let f = Fixture::new("bd_flow").await;
    let tip = f.commit(&f.jwt, "agent/a", "a.txt").await;
    let (status, pr) = f
        .call(
            reqwest::Method::POST,
            "/pulls",
            Some(&f.jwt),
            Some(json!({"title": "Add a", "head": "agent/a", "base": "main"})),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{pr}");

    // An open pull request still uses the branch.
    let (status, body) = f.delete(Some(&f.jwt), "agent/a", "").await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["error"]["reason"], "OPEN_PULL_REQUESTS", "{body}");
    assert_eq!(body["error"]["details"]["pull_requests"][0]["number"], 1);
    assert!(f.branches().await.contains(&"agent/a".to_string()));

    // Closed: the branch can go, and the pull request keeps its head.
    let (status, v) = f
        .call(
            reqwest::Method::PATCH,
            "/pulls/1",
            Some(&f.jwt),
            Some(json!({"state": "closed"})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{v}");
    let (status, body) = f.delete(Some(&f.jwt), "agent/a", "").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["deleted"], true);
    assert_eq!(body["sha"], tip.as_str());
    assert!(!f.branches().await.contains(&"agent/a".to_string()));
    let remote = f.remote().await;
    let listed = git(&["ls-remote", &remote, "refs/pull/1/head"], None);
    assert!(
        listed.starts_with(&tip),
        "the pull request must keep its head after the branch is deleted: {listed}"
    );

    // Gone is gone.
    let (status, _) = f.delete(Some(&f.jwt), "agent/a", "").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn default_protected_unknown_and_malformed_branches_are_refused() {
    let f = Fixture::new("bd_refuse").await;

    let (status, body) = f.delete(Some(&f.jwt), "main", "").await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["error"]["reason"], "DEFAULT_BRANCH");

    // A protection rule with a wildcard covers a branch that already exists under
    // it. (The branch is made first: the rule would refuse direct commits.)
    f.commit(&f.jwt, "feature", "f.txt").await;
    f.commit(&f.jwt, "release/1", "r.txt").await;
    let (status, v) = f
        .call(
            reqwest::Method::POST,
            "/branches/protection",
            Some(&f.jwt),
            Some(json!({"branch_name": "release/*", "require_pr": true})),
        )
        .await;
    assert!(status.is_success(), "{status} {v}");
    let (status, body) = f.delete(Some(&f.jwt), "release/1", "").await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["error"]["reason"], "BRANCH_PROTECTED", "{body}");
    assert_eq!(body["error"]["details"]["rule"], "release/*");
    assert!(f.branches().await.contains(&"release/1".to_string()));

    let (status, _) = f.delete(Some(&f.jwt), "no-such-branch", "").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = f.delete(Some(&f.jwt), "a%20b", "").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = f.delete(Some(&f.jwt), "a~b", "").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(f.branches().await.contains(&"feature".to_string()));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn expected_sha_and_dry_run_protect_the_branch() {
    let f = Fixture::new("bd_cas").await;
    let first = f.commit(&f.jwt, "agent/c", "one.txt").await;
    let second = f.commit(&f.jwt, "agent/c", "two.txt").await;

    // The caller looked when the branch was at `first`; it has moved.
    let (status, body) = f
        .delete(Some(&f.jwt), "agent/c", &format!("?expected_sha={first}"))
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["error"]["reason"], "REF_ADVANCED");
    assert_eq!(body["error"]["details"]["current_sha"], second.as_str());
    assert!(f.branches().await.contains(&"agent/c".to_string()));

    // A dry run, by query and by header, reports and deletes nothing.
    let (status, body) = f
        .delete(
            Some(&f.jwt),
            "agent/c",
            &format!("?expected_sha={second}&dry_run=true"),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["dry_run"], true);
    assert_eq!(body["would_delete"]["sha"], second.as_str());
    let resp = f
        .client
        .delete(f.api("/branches/agent/c"))
        .bearer_auth(&f.jwt)
        .header("X-Dry-Run", "1")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(f.branches().await.contains(&"agent/c".to_string()));

    // At the commit the caller saw, it goes.
    let (status, body) = f
        .delete(Some(&f.jwt), "agent/c", &format!("?expected_sha={second}"))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(!f.branches().await.contains(&"agent/c".to_string()));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn only_someone_with_write_access_can_delete() {
    let f = Fixture::new("bd_authz").await;
    f.commit(&f.jwt, "agent/z", "z.txt").await;
    let stranger = register_user(&f.base, "bd_stranger", "bd_stranger@example.com", PW).await;

    let (status, _) = f.delete(None, "agent/z", "").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = f.delete(Some(&stranger), "agent/z", "").await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(f.branches().await.contains(&"agent/z".to_string()));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_branch_limited_token_deletes_only_inside_its_limit_and_is_audited() {
    use rg_db::sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
    let f = Fixture::new("bd_token").await;
    let (token_id, pat) = f.token(true).await;
    f.commit(&pat, "agent/ok", "a.txt").await;
    f.commit(&f.jwt, "feature", "f.txt").await;

    let (status, body) = f.delete(Some(&pat), "feature", "").await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert!(f.branches().await.contains(&"feature".to_string()));

    // The protection-rule settings are not reachable with a limited token.
    let (status, _) = f
        .call(
            reqwest::Method::DELETE,
            "/branches/protection/1",
            Some(&pat),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, body) = f.delete(Some(&pat), "agent/ok", "").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(!f.branches().await.contains(&"agent/ok".to_string()));

    let row = rg_db::entities::audit_log::Entity::find()
        .filter(rg_db::entities::audit_log::Column::Action.eq("repo.branch_delete"))
        // PostgreSQL / MySQL runs share one database between the tests of a file.
        .filter(rg_db::entities::audit_log::Column::Username.eq("bd_token"))
        .one(&f.db)
        .await
        .unwrap()
        .expect("the delete must be audited");
    let details: Value = serde_json::from_str(&row.details.unwrap()).unwrap();
    assert_eq!(details["branch"], "agent/ok");
    assert_eq!(details["access_token_id"], json!(token_id));
}
