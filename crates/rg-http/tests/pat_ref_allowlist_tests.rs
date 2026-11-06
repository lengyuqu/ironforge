//! A personal access token can be limited to some branches.
//!
//! A push (real `git` over HTTP) may only update the listed branches, never a
//! tag, and a limited token's REST writes are confined to the ones that cannot
//! move a branch. A token without the limit is unchanged.

mod common;
use common::{register_user, spawn_test_app_on_env_db};

use reqwest::{Client, Method, StatusCode};
use serde_json::{json, Value};
use std::path::Path;

const PW: &str = "Qz7$wRtm";

async fn make_token(base: &str, jwt: &str, body: Value) -> (StatusCode, Value) {
    let resp = Client::new()
        .post(format!("{base}/api/v1/users/tokens"))
        .bearer_auth(jwt)
        .json(&body)
        .send()
        .await
        .unwrap();
    let status = resp.status();
    (status, resp.json().await.unwrap_or(Value::Null))
}

/// Run git, returning (succeeded, stderr).
fn git_try(args: &[&str], cwd: Option<&Path>) -> (bool, String) {
    let gateway = rg_git::cli_gateway::global_gateway().as_ref().unwrap();
    let out = gateway.run(args, cwd).unwrap();
    (out.success(), out.stderr_str())
}

fn git(args: &[&str], cwd: Option<&Path>) -> String {
    let gateway = rg_git::cli_gateway::global_gateway().as_ref().unwrap();
    let out = gateway.run(args, cwd).unwrap();
    out.ensure_success().unwrap();
    out.stdout_str().trim().to_string()
}

#[tokio::test]
async fn the_branch_limit_is_validated_when_the_token_is_created() {
    let base = spawn_test_app_on_env_db().await.0;
    let jwt = register_user(&base, "refs_valid", "refs-valid@example.com", PW).await;
    let repos = json!(["refs_valid/demo"]);

    for (why, body) in [
        (
            "branches without repositories",
            json!({"name": "t", "scopes": "repo:write", "refs": ["agent/*"]}),
        ),
        (
            "empty list",
            json!({"name": "t", "scopes": "repo:write", "repos": repos, "refs": []}),
        ),
        (
            "every branch",
            json!({"name": "t", "scopes": "repo:write", "repos": repos, "refs": ["*"]}),
        ),
        (
            "leading wildcard",
            json!({"name": "t", "scopes": "repo:write", "repos": repos, "refs": ["*/x"]}),
        ),
        (
            "traversal",
            json!({"name": "t", "scopes": "repo:write", "repos": repos, "refs": ["agent/../main"]}),
        ),
        (
            "odd characters",
            json!({"name": "t", "scopes": "repo:write", "repos": repos, "refs": ["agent x"]}),
        ),
        (
            "other glob syntax",
            json!({"name": "t", "scopes": "repo:write", "repos": repos, "refs": ["agent/[ab]"]}),
        ),
        (
            "account scope",
            json!({"name": "t", "scopes": "repo:write,user", "repos": repos, "refs": ["agent/*"]}),
        ),
    ] {
        let (status, body) = make_token(&base, &jwt, body).await;
        assert_eq!(status, 400, "{why}: {body}");
    }

    let (status, body) = make_token(
        &base,
        &jwt,
        json!({"name": "ok", "scopes": "repo:write", "repos": repos,
               "refs": ["bots/*", "agent/*", "agent/*"]}),
    )
    .await;
    assert_eq!(status, 201, "{body}");
    assert_eq!(body["refs"], json!(["agent/*", "bots/*"]));

    // The list shows the limit; a token without one has no `refs` at all.
    let (_, plain) = make_token(&base, &jwt, json!({"name": "plain", "scopes": "repo"})).await;
    assert!(plain["refs"].is_null());
    let listed: Value = Client::new()
        .get(format!("{base}/api/v1/users/tokens"))
        .bearer_auth(&jwt)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let find = |name: &str| {
        listed
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["name"] == name)
            .unwrap()
            .clone()
    };
    assert_eq!(find("ok")["refs"], json!(["agent/*", "bots/*"]));
    assert!(find("plain").get("refs").is_none());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_push_may_only_update_the_listed_branches() {
    let base = spawn_test_app_on_env_db().await.0;
    let owner = "refs_push";
    let jwt = register_user(&base, owner, "refs-push@example.com", PW).await;
    let client = Client::new();
    let resp = client
        .post(format!("{base}/api/v1/repos"))
        .bearer_auth(&jwt)
        .json(&json!({"name": "work", "auto_init": true, "is_private": true}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);

    let (s, limited) = make_token(
        &base,
        &jwt,
        json!({"name": "agent", "scopes": "repo:write", "repos": [format!("{owner}/work")],
               "refs": ["agent/*"]}),
    )
    .await;
    assert_eq!(s, 201, "{limited}");
    let limited = limited["token"].as_str().unwrap().to_string();
    let (_, unlimited) = make_token(
        &base,
        &jwt,
        json!({"name": "free", "scopes": "repo:write", "repos": [format!("{owner}/work")]}),
    )
    .await;
    let unlimited = unlimited["token"].as_str().unwrap().to_string();

    let remote = |token: &str| {
        base.replacen("http://", &format!("http://{owner}:{token}@"), 1)
            + &format!("/{owner}/work.git")
    };
    let wt = tempfile::tempdir().unwrap();
    let path = wt.path();
    // A limited token can still read.
    git(&["clone", &remote(&limited), &path.to_string_lossy()], None);
    git(&["config", "user.name", "Agent"], Some(path));
    git(&["config", "user.email", "agent@example.com"], Some(path));
    std::fs::write(path.join("a.txt"), "change\n").unwrap();
    git(&["add", "a.txt"], Some(path));
    git(&["commit", "-m", "agent work"], Some(path));
    let main_before = git(&["rev-parse", "origin/main"], Some(path));
    let local = git(&["rev-parse", "HEAD"], Some(path));

    let heads = |token: &str| -> String { git(&["ls-remote", &remote(token)], None) };

    // Inside the limit.
    let (ok, err) = git_try(
        &["push", &remote(&limited), "HEAD:refs/heads/agent/one"],
        Some(path),
    );
    assert!(ok, "push to agent/one: {err}");
    let (ok, err) = git_try(
        &[
            "push",
            &remote(&limited),
            "HEAD:refs/heads/agent/nested/two",
        ],
        Some(path),
    );
    assert!(ok, "push to agent/nested/two: {err}");

    // Outside it: the default branch, a near miss, a tag, other namespaces.
    for target in [
        "refs/heads/main",
        "refs/heads/agent",
        "refs/heads/agentx/one",
        "refs/heads/other/agent/one",
        "refs/tags/v1",
        "refs/tags/agent/one",
        "refs/notes/commits",
    ] {
        let (ok, err) = git_try(
            &["push", &remote(&limited), &format!("HEAD:{target}")],
            Some(path),
        );
        assert!(!ok, "push to {target} must be refused");
        assert!(err.contains("may not write to this ref"), "{target}: {err}");
    }

    // One push carrying an allowed and a refused ref updates only the allowed.
    let (ok, _) = git_try(
        &[
            "push",
            &remote(&limited),
            "HEAD:refs/heads/agent/three",
            "HEAD:refs/heads/main",
        ],
        Some(path),
    );
    assert!(!ok, "a push with a refused ref reports failure");

    let listing = heads(&jwt);
    for want in [
        "refs/heads/agent/one",
        "refs/heads/agent/nested/two",
        "refs/heads/agent/three",
    ] {
        assert!(listing.contains(want), "{want} missing from\n{listing}");
    }
    assert!(
        listing.contains(&format!("{main_before}\trefs/heads/main")),
        "main must not have moved:\n{listing}"
    );
    for refused in [
        "refs/tags/v1",
        "refs/heads/agentx",
        "refs/heads/other",
        "refs/notes",
    ] {
        assert!(!listing.contains(refused), "{refused} exists:\n{listing}");
    }

    // The same token scope without the branch limit can push main, so the
    // refusals above were the limit.
    let (ok, err) = git_try(
        &["push", &remote(&unlimited), "HEAD:refs/heads/main"],
        Some(path),
    );
    assert!(ok, "unlimited token pushes main: {err}");
    assert!(heads(&jwt).contains(&format!("{local}\trefs/heads/main")));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_limited_tokens_rest_writes_cannot_move_a_branch() {
    let base = spawn_test_app_on_env_db().await.0;
    let owner = "refs_rest";
    let jwt = register_user(&base, owner, "refs-rest@example.com", PW).await;
    let client = Client::new();
    let resp = client
        .post(format!("{base}/api/v1/repos"))
        .bearer_auth(&jwt)
        .json(&json!({"name": "work", "auto_init": true, "is_private": true}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);

    let (_, limited) = make_token(
        &base,
        &jwt,
        json!({"name": "agent", "scopes": "repo:write", "repos": [format!("{owner}/work")],
               "refs": ["agent/*"]}),
    )
    .await;
    let limited = limited["token"].as_str().unwrap().to_string();
    let (_, unlimited) = make_token(
        &base,
        &jwt,
        json!({"name": "free", "scopes": "repo:write", "repos": [format!("{owner}/work")]}),
    )
    .await;
    let unlimited = unlimited["token"].as_str().unwrap().to_string();

    // Put a branch on the server for the pull request.
    let remote = base.replacen("http://", &format!("http://{owner}:{limited}@"), 1)
        + &format!("/{owner}/work.git");
    let wt = tempfile::tempdir().unwrap();
    let path = wt.path();
    git(&["clone", &remote, &path.to_string_lossy()], None);
    git(&["config", "user.name", "Agent"], Some(path));
    git(&["config", "user.email", "agent@example.com"], Some(path));
    std::fs::write(path.join("a.txt"), "change\n").unwrap();
    git(&["add", "a.txt"], Some(path));
    git(&["commit", "-m", "agent work"], Some(path));
    git(&["push", "origin", "HEAD:refs/heads/agent/one"], Some(path));

    let api = |suffix: &str| format!("{base}/api/v1/repos/{owner}/work{suffix}");
    let send = |method: Method, url: String, token: String, body: Option<Value>| {
        let client = client.clone();
        async move {
            let mut req = client.request(method, url).bearer_auth(token);
            if let Some(body) = body {
                req = req.json(&body);
            }
            req.send().await.unwrap()
        }
    };

    // What the limited token may do: read, collaborate, issues, open and discuss
    // a pull request.
    for (method, suffix, body, want) in [
        (Method::GET, "".to_string(), None, StatusCode::OK),
        (Method::GET, "/issues".to_string(), None, StatusCode::OK),
        (
            Method::POST,
            "/issues".to_string(),
            Some(json!({"title": "found a bug"})),
            StatusCode::CREATED,
        ),
        (
            Method::POST,
            "/collaboration/sessions".to_string(),
            Some(json!({"display_name": "a", "client_kind": "t"})),
            StatusCode::CREATED,
        ),
        (
            Method::POST,
            "/pulls".to_string(),
            Some(json!({"title": "agent work", "head": "agent/one", "base": "main"})),
            StatusCode::CREATED,
        ),
    ] {
        let resp = send(method.clone(), api(&suffix), limited.clone(), body).await;
        assert_eq!(
            resp.status(),
            want,
            "{method} {suffix}: {}",
            resp.text().await.unwrap()
        );
    }
    let resp = send(
        Method::POST,
        api("/pulls/1/comments"),
        limited.clone(),
        Some(json!({"path": "a.txt", "line": 1, "body": "looks fine"})),
    )
    .await;
    assert!(
        resp.status().is_success(),
        "comment on the pull request: {} {}",
        resp.status(),
        resp.text().await.unwrap()
    );

    // What it may not: anything that can move a branch or tag, or open a gate.
    for (method, suffix, body) in [
        (
            Method::POST,
            "/contents/new.txt".to_string(),
            Some(json!({"branch": "main", "message": "m", "content": "hello\n"})),
        ),
        (
            Method::POST,
            "/contents/new.txt".to_string(),
            Some(json!({"branch": "agentx/one", "message": "m", "content": "hello\n"})),
        ),
        (
            Method::DELETE,
            "/contents/README.md?branch=main&message=m&sha=0000000000000000000000000000000000000000".to_string(),
            None,
        ),
        (Method::POST, "/pulls/1/merge".to_string(), Some(json!({}))),
        (Method::PUT, "/pulls/1/auto-merge".to_string(), Some(json!({}))),
        (Method::PUT, "/pulls/1/merge-queue".to_string(), Some(json!({}))),
        (Method::POST, "/pulls/1/suggestions/apply".to_string(), Some(json!({}))),
        (
            Method::POST,
            "/releases".to_string(),
            Some(json!({"tag_name": "v1", "name": "v1"})),
        ),
        (Method::POST, "/pipelines".to_string(), Some(json!({}))),
        (Method::POST, "/statuses/abc".to_string(), Some(json!({"state": "success"}))),
        (Method::POST, "/hooks".to_string(), Some(json!({"url": "http://127.0.0.1/x"}))),
        (Method::POST, "/branches/protection".to_string(), Some(json!({"branch_name": "main"}))),
        (Method::POST, "/wiki".to_string(), Some(json!({"title": "t", "content": "c"}))),
        (Method::POST, "/fork".to_string(), Some(json!({}))),
    ] {
        let resp = send(method.clone(), api(&suffix), limited.clone(), body).await;
        assert_eq!(
            resp.status(),
            StatusCode::FORBIDDEN,
            "{method} {suffix} must be refused for a branch-limited token: {}",
            resp.text().await.unwrap()
        );
    }

    // Nothing above changed a branch.
    let heads = git(&["ls-remote", &remote], None);
    assert!(!heads.contains("new.txt"));

    // A file edit on a listed branch is allowed: the handler holds the branch to
    // the token's own limit.
    let resp = send(
        Method::POST,
        api("/contents/agent-note.txt"),
        limited.clone(),
        Some(json!({"branch": "agent/one", "message": "note", "content": "hello\n"})),
    )
    .await;
    assert!(
        resp.status().is_success(),
        "limited token edits a file on a listed branch: {} {}",
        resp.status(),
        resp.text().await.unwrap()
    );
    // A delete on a listed branch gets past the limit (it then fails on the
    // wrong blob sha, which is not a 403).
    let resp = send(
        Method::DELETE,
        api("/contents/a.txt?branch=agent/one&message=m&sha=0000000000000000000000000000000000000000"),
        limited.clone(),
        None,
    )
    .await;
    assert_ne!(
        resp.status(),
        StatusCode::FORBIDDEN,
        "{}",
        resp.text().await.unwrap()
    );
    let branches: Value = send(Method::GET, api("/branches"), jwt.clone(), None)
        .await
        .json()
        .await
        .unwrap();
    let names: Vec<String> = branches
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b.as_str().unwrap().to_string())
        .collect();
    assert!(names.contains(&"agent/one".to_string()), "{names:?}");

    // The same writes are allowed for a token with no branch limit, so the
    // refusals were the limit and not the routes or the data.
    let resp = send(
        Method::POST,
        api("/contents/new.txt"),
        unlimited.clone(),
        Some(json!({"branch": "agent/one", "message": "m", "content": "hello\n"})),
    )
    .await;
    assert!(
        resp.status().is_success(),
        "unlimited token edits a file: {} {}",
        resp.status(),
        resp.text().await.unwrap()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_revoked_token_cannot_keep_using_the_session_it_minted() {
    let base = spawn_test_app_on_env_db().await.0;
    let owner = "refs_revoked";
    let jwt = register_user(&base, owner, "refs-revoked@example.com", PW).await;
    let client = Client::new();
    assert_eq!(
        client
            .post(format!("{base}/api/v1/repos"))
            .bearer_auth(&jwt)
            .json(&json!({"name": "work", "auto_init": true, "is_private": true}))
            .send()
            .await
            .unwrap()
            .status(),
        201
    );
    let (_, created) = make_token(
        &base,
        &jwt,
        json!({"name": "agent", "scopes": "repo:write", "repos": [format!("{owner}/work")],
               "refs": ["agent/*"]}),
    )
    .await;
    let token_id = created["id"].as_i64().unwrap();

    // The session the middleware would mint for this token.
    let claims = rg_core::auth::jwt::validate_token(&jwt, "test-secret-key").unwrap();
    let user_id: i64 = claims.sub.parse().unwrap();
    let minted = rg_core::auth::jwt::generate_token_for(
        user_id,
        owner,
        "test-secret-key",
        1,
        claims.ver,
        Some(token_id),
    )
    .unwrap();

    let url = format!("{base}/api/v1/repos/{owner}/work/contents/x.txt");
    let edit = |branch: &'static str| {
        let client = client.clone();
        let url = url.clone();
        let minted = minted.clone();
        async move {
            client
                .post(url)
                .bearer_auth(minted)
                .json(&json!({"branch": branch, "message": "m", "content": "x\n"}))
                .send()
                .await
                .unwrap()
                .status()
        }
    };
    // While the token exists, the limit applies to the session it minted.
    assert_eq!(edit("main").await, StatusCode::FORBIDDEN);

    // Deleting the token ends the sessions it minted: even a branch the token
    // used to cover is now refused.
    let resp = client
        .delete(format!("{base}/api/v1/users/tokens/{token_id}"))
        .bearer_auth(&jwt)
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success(), "{}", resp.status());
    assert_eq!(edit("agent/anything").await, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn a_token_without_a_branch_limit_behaves_as_before() {
    let base = spawn_test_app_on_env_db().await.0;
    let owner = "refs_plain";
    let jwt = register_user(&base, owner, "refs-plain@example.com", PW).await;
    let client = Client::new();
    assert_eq!(
        client
            .post(format!("{base}/api/v1/repos"))
            .bearer_auth(&jwt)
            .json(&json!({"name": "work", "auto_init": true}))
            .send()
            .await
            .unwrap()
            .status(),
        201
    );
    // A repository-limited token (the previous kind) and an unlimited one.
    for body in [
        json!({"name": "repo-limited", "scopes": "repo:write", "repos": [format!("{owner}/work")]}),
        json!({"name": "plain", "scopes": "repo"}),
    ] {
        let (_, t) = make_token(&base, &jwt, body).await;
        let token = t["token"].as_str().unwrap();
        let resp = client
            .post(format!(
                "{base}/api/v1/repos/{owner}/work/contents/f-{}.txt",
                t["name"].as_str().unwrap()
            ))
            .bearer_auth(token)
            .json(&json!({"branch": "main", "message": "m", "content": "x\n"}))
            .send()
            .await
            .unwrap();
        assert!(
            resp.status().is_success(),
            "{}: {} {}",
            t["name"],
            resp.status(),
            resp.text().await.unwrap()
        );
        // Each token writes its own file so the second is not a conflict.
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_audit_entry_names_the_access_token_that_made_the_request() {
    use rg_db::sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
    let (base, db) = spawn_test_app_on_env_db().await;
    let jwt = register_user(&base, "audit_tok", "audit-tok@example.com", PW).await;
    let client = Client::new();
    let (status, created) =
        make_token(&base, &jwt, json!({"name": "ci-bot", "scopes": "repo"})).await;
    assert_eq!(status, StatusCode::CREATED);
    let token_id = created["id"].as_i64().unwrap();
    let pat = created["token"].as_str().unwrap().to_string();

    let create = |name: &'static str, bearer: String, extra: Option<&'static str>| {
        let client = client.clone();
        let url = format!("{base}/api/v1/repos");
        async move {
            let mut req = client
                .post(url)
                .bearer_auth(bearer)
                .json(&json!({"name": name, "is_private": true}));
            if let Some(forged) = extra {
                req = req.header("x-ironforge-access-token-id", forged);
            }
            req.send().await.unwrap().status()
        }
    };
    // Made with the token: the entry carries its id.
    assert_eq!(
        create("via-token", pat.clone(), None).await,
        StatusCode::CREATED
    );
    // Made with an ordinary login, claiming a token id: nothing is recorded, the
    // client cannot name a token.
    assert_eq!(
        create("via-login", jwt.clone(), Some("999")).await,
        StatusCode::CREATED
    );
    // Made with the token and a forged id: the real one wins.
    assert_eq!(
        create("via-token-forged", pat.clone(), Some("999")).await,
        StatusCode::CREATED
    );

    let details_of = |resource_name: String| {
        let db = db.clone();
        async move {
            let row = rg_db::entities::audit_log::Entity::find()
                .filter(rg_db::entities::audit_log::Column::Action.eq("repo.create"))
                .filter(rg_db::entities::audit_log::Column::ResourceName.eq(resource_name))
                .one(&db)
                .await
                .unwrap()
                .expect("audit entry");
            serde_json::from_str::<Value>(&row.details.unwrap()).unwrap()
        }
    };
    assert_eq!(
        details_of("audit_tok/via-token".into()).await["access_token_id"],
        json!(token_id)
    );
    assert!(details_of("audit_tok/via-login".into())
        .await
        .get("access_token_id")
        .is_none());
    assert_eq!(
        details_of("audit_tok/via-token-forged".into()).await["access_token_id"],
        json!(token_id)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_branch_limited_token_cannot_approve_a_pull_request() {
    let base = spawn_test_app_on_env_db().await.0;
    let owner = "appr_owner";
    let owner_jwt = register_user(&base, owner, "appr-owner@example.com", PW).await;
    let reviewer = "appr_reviewer";
    let reviewer_jwt = register_user(&base, reviewer, "appr-reviewer@example.com", PW).await;
    let client = Client::new();
    assert_eq!(
        client
            .post(format!("{base}/api/v1/repos"))
            .bearer_auth(&owner_jwt)
            .json(&json!({"name": "work", "auto_init": true, "is_private": false}))
            .send()
            .await
            .unwrap()
            .status(),
        201
    );

    // A branch for a pull request, pushed with an unlimited owner token.
    let (_, owner_pat) =
        make_token(&base, &owner_jwt, json!({"name": "push", "scopes": "repo"})).await;
    let remote = base.replacen(
        "http://",
        &format!("http://{owner}:{}@", owner_pat["token"].as_str().unwrap()),
        1,
    ) + &format!("/{owner}/work.git");
    let wt = tempfile::tempdir().unwrap();
    let path = wt.path();
    git(&["clone", &remote, &path.to_string_lossy()], None);
    git(&["config", "user.name", "Owner"], Some(path));
    git(&["config", "user.email", "owner@example.com"], Some(path));
    std::fs::write(path.join("a.txt"), "change\n").unwrap();
    git(&["add", "a.txt"], Some(path));
    git(&["commit", "-m", "work"], Some(path));
    git(&["push", "origin", "HEAD:refs/heads/feature"], Some(path));
    let resp = client
        .post(format!("{base}/api/v1/repos/{owner}/work/pulls"))
        .bearer_auth(&owner_jwt)
        .json(&json!({"title": "work", "head": "feature", "base": "main"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201, "{}", resp.text().await.unwrap());

    // The reviewer's tokens: one limited to a branch (an agent's), one not.
    let repos = json!([format!("{owner}/work")]);
    let (_, agent) = make_token(
        &base,
        &reviewer_jwt,
        json!({"name": "agent", "scopes": "repo:write", "repos": repos, "refs": ["agent/*"]}),
    )
    .await;
    let (_, plain) = make_token(
        &base,
        &reviewer_jwt,
        json!({"name": "plain", "scopes": "repo:write", "repos": repos}),
    )
    .await;
    let review = |bearer: String, action: &'static str| {
        let client = client.clone();
        let url = format!("{base}/api/v1/repos/{owner}/work/pulls/1/reviews");
        async move {
            client
                .post(url)
                .bearer_auth(bearer)
                .json(&json!({"action": action, "body": "reviewed"}))
                .send()
                .await
                .unwrap()
        }
    };
    let agent = agent["token"].as_str().unwrap().to_string();
    let plain = plain["token"].as_str().unwrap().to_string();

    // The agent may comment and ask for changes, never approve.
    let resp = review(agent.clone(), "approve").await;
    assert_eq!(
        resp.status(),
        StatusCode::FORBIDDEN,
        "{}",
        resp.text().await.unwrap()
    );
    let resp = review(agent.clone(), "comment").await;
    assert_eq!(
        resp.status(),
        StatusCode::CREATED,
        "{}",
        resp.text().await.unwrap()
    );
    let resp = review(agent.clone(), "request_changes").await;
    assert_eq!(
        resp.status(),
        StatusCode::CREATED,
        "{}",
        resp.text().await.unwrap()
    );

    // The same person, signed in or with an unlimited token, may approve.
    let resp = review(plain, "approve").await;
    assert_eq!(
        resp.status(),
        StatusCode::CREATED,
        "{}",
        resp.text().await.unwrap()
    );
    let resp = review(reviewer_jwt.clone(), "approve").await;
    assert_eq!(
        resp.status(),
        StatusCode::CREATED,
        "{}",
        resp.text().await.unwrap()
    );
}
