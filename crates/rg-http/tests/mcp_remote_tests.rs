//! The streamable-HTTP MCP transport (`ironforge-mcp --http`), end to end:
//! a real IronForge server with real personal access tokens, the real remote
//! transport served in this process, and real HTTP between them.
//!
//! The transport holds no authority of its own: what a token may do is decided
//! by the REST API, so these tests check that a limited token is limited
//! through MCP exactly as it is over REST.

mod common;
use common::{register_user, spawn_test_app};

use reqwest::{Client, StatusCode};
use rg_mcp::remote::{serve, RemoteConfig};
use serde_json::{json, Value};
use std::time::{Duration, Instant};

const PW: &str = "Qz7$wRtm";

struct Mcp {
    url: String,
    http: Client,
}

#[derive(Debug)]
struct Reply {
    status: StatusCode,
    session: Option<String>,
    body: String,
}

impl Reply {
    /// The JSON-RPC answer, whether it came as a JSON body or as an SSE event.
    fn json(&self) -> Value {
        if self.body.contains("event: message") {
            return sse_message(&self.body).0;
        }
        serde_json::from_str(&self.body).unwrap_or(Value::Null)
    }
}

impl Mcp {
    async fn start(api: &str, tune: impl FnOnce(&mut RemoteConfig)) -> Mcp {
        let mut cfg = RemoteConfig::new(api);
        tune(&mut cfg);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(serve(listener, cfg));
        Mcp {
            url,
            http: Client::new(),
        }
    }

    async fn post(&self, token: Option<&str>, session: Option<&str>, body: &Value) -> Reply {
        self.post_with(token, session, body, &[]).await
    }

    async fn post_with(
        &self,
        token: Option<&str>,
        session: Option<&str>,
        body: &Value,
        extra: &[(&str, &str)],
    ) -> Reply {
        let mut req = self
            .http
            .post(format!("{}/mcp", self.url))
            .header("accept", "application/json, text/event-stream")
            .json(body);
        if let Some(t) = token {
            req = req.bearer_auth(t);
        }
        if let Some(s) = session {
            req = req.header("mcp-session-id", s);
        }
        for (k, v) in extra {
            req = req.header(*k, *v);
        }
        let resp = req.send().await.unwrap();
        let status = resp.status();
        let session = resp
            .headers()
            .get("mcp-session-id")
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        Reply {
            status,
            session,
            body: resp.text().await.unwrap(),
        }
    }

    /// `initialize` with `token`; the session id when it was granted.
    async fn open(&self, token: &str) -> Result<String, Reply> {
        let reply = self
            .post(
                Some(token),
                None,
                &json!({"jsonrpc": "2.0", "id": 1, "method": "initialize",
                        "params": {"protocolVersion": "2025-06-18", "capabilities": {},
                                   "clientInfo": {"name": "test", "version": "0"}}}),
            )
            .await;
        match (reply.status, reply.session.clone()) {
            (StatusCode::OK, Some(id)) => Ok(id),
            _ => Err(reply),
        }
    }

    async fn delete(&self, token: &str, session: &str) -> StatusCode {
        self.http
            .delete(format!("{}/mcp", self.url))
            .bearer_auth(token)
            .header("mcp-session-id", session)
            .send()
            .await
            .unwrap()
            .status()
    }

    /// Call a tool; `(isError, text)` of the result.
    async fn tool(&self, token: &str, session: &str, name: &str, args: Value) -> (bool, String) {
        let reply = self
            .post(
                Some(token),
                Some(session),
                &json!({"jsonrpc": "2.0", "id": 7, "method": "tools/call",
                        "params": {"name": name, "arguments": args}}),
            )
            .await;
        assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
        parse_tool_result(&reply.json())
    }
}

fn parse_tool_result(v: &Value) -> (bool, String) {
    assert!(v["error"].is_null(), "JSON-RPC error: {v}");
    assert!(
        v["result"]["content"][0]["text"].is_string(),
        "unexpected reply: {v}"
    );
    (
        v["result"]["isError"].as_bool().unwrap(),
        v["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .to_string(),
    )
}

/// The `data:` payload of the first `message` event of an SSE body, and the
/// number of heartbeat comment lines before it.
fn sse_message(body: &str) -> (Value, usize) {
    let heartbeats = body
        .lines()
        .filter(|l| l.starts_with(": heartbeat"))
        .count();
    let data = body
        .lines()
        .find_map(|l| l.strip_prefix("data:"))
        .unwrap_or_else(|| panic!("no data line in {body:?}"));
    (serde_json::from_str(data.trim()).unwrap(), heartbeats)
}

async fn make_token(base: &str, jwt: &str, body: Value) -> String {
    let resp = Client::new()
        .post(format!("{base}/api/v1/users/tokens"))
        .bearer_auth(jwt)
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201, "create token {body}");
    resp.json::<Value>().await.unwrap()["token"]
        .as_str()
        .unwrap()
        .to_string()
}

struct World {
    api: String,
    owner: String,
    /// The owner's session JWT.
    jwt: String,
    full: String,
    read: String,
    /// `repo:write`, only `owner/demo`.
    one_repo: String,
    /// `repo:write`, only `owner/demo`, only branches `agent/*`.
    one_branch: String,
    /// Another account's token.
    stranger: String,
}

async fn world(owner: &str) -> World {
    let api = spawn_test_app().await;
    let jwt = register_user(&api, owner, &format!("{owner}@example.com"), PW).await;
    for name in ["demo", "other"] {
        let resp = Client::new()
            .post(format!("{api}/api/v1/repos"))
            .bearer_auth(&jwt)
            .json(&json!({"name": name, "auto_init": true, "is_private": true}))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 201);
    }
    let repos = json!([format!("{owner}/demo")]);
    let other_jwt = register_user(
        &api,
        &format!("{owner}_x"),
        &format!("{owner}_x@example.com"),
        PW,
    )
    .await;
    World {
        full: make_token(&api, &jwt, json!({"name": "full", "scopes": "repo"})).await,
        read: make_token(&api, &jwt, json!({"name": "read", "scopes": "repo:read"})).await,
        one_repo: make_token(
            &api,
            &jwt,
            json!({"name": "one", "scopes": "repo:write", "repos": repos}),
        )
        .await,
        one_branch: make_token(
            &api,
            &jwt,
            json!({"name": "br", "scopes": "repo:write", "repos": repos, "refs": ["agent/*"]}),
        )
        .await,
        stranger: make_token(&api, &other_jwt, json!({"name": "s", "scopes": "repo"})).await,
        api,
        owner: owner.to_string(),
        jwt,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_request_without_a_valid_token_is_refused() {
    let w = world("mcp_auth").await;
    let mcp = Mcp::start(&w.api, |_| {}).await;
    let init = json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}});

    // No credential at all, and not-a-bearer credentials.
    let none = mcp.post(None, None, &init).await;
    assert_eq!(none.status, StatusCode::UNAUTHORIZED);
    let basic = mcp
        .http
        .post(format!("{}/mcp", mcp.url))
        .header("authorization", "Basic dXNlcjpwYXNz")
        .json(&init)
        .send()
        .await
        .unwrap();
    assert_eq!(basic.status(), StatusCode::UNAUTHORIZED);
    assert!(basic.headers().contains_key("www-authenticate"));

    // A well-formed token the server does not know: refused by the probe.
    let bogus = mcp.open("if_pat_not_a_real_token").await.unwrap_err();
    assert_eq!(bogus.status, StatusCode::UNAUTHORIZED, "{}", bogus.body);

    // A revoked/expired JWT behaves the same way.
    let forged = mcp.open("eyJhbGciOiJIUzI1NiJ9.e30.AAAA").await.unwrap_err();
    assert_eq!(forged.status, StatusCode::UNAUTHORIZED);

    // After sign-in, requests still need the token every time.
    let session = mcp.open(&w.full).await.unwrap();
    let ping = json!({"jsonrpc": "2.0", "id": 2, "method": "ping"});
    assert_eq!(
        mcp.post(None, Some(&session), &ping).await.status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        mcp.post(Some(&w.full), Some(&session), &ping).await.status,
        StatusCode::OK
    );

    // The endpoint offers no server-initiated stream, and health is public.
    let get = mcp
        .http
        .get(format!("{}/mcp", mcp.url))
        .send()
        .await
        .unwrap();
    assert_eq!(get.status(), StatusCode::METHOD_NOT_ALLOWED);
    let health = mcp
        .http
        .get(format!("{}/healthz", mcp.url))
        .send()
        .await
        .unwrap();
    assert_eq!(health.status(), StatusCode::OK);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_tool_set_is_the_stdio_tool_set() {
    let w = world("mcp_tools").await;
    let mcp = Mcp::start(&w.api, |_| {}).await;
    let session = mcp.open(&w.read).await.unwrap();
    let reply = mcp
        .post(
            Some(&w.read),
            Some(&session),
            &json!({"jsonrpc": "2.0", "id": 3, "method": "tools/list"}),
        )
        .await;
    let remote: Vec<String> = reply.json()["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_string())
        .collect();

    // The very same function stdio answers `tools/list` with.
    let state = rg_mcp::AppState {
        api_base: String::new(),
        pat: String::new(),
        collab: std::sync::Arc::new(rg_mcp::collab::SessionStore::in_memory()),
    };
    let req = rg_mcp::protocol::JsonRpcRequest {
        jsonrpc: "2.0".into(),
        id: json!(1),
        method: "tools/list".into(),
        params: None,
    };
    let local = serde_json::to_value(rg_mcp::tools::list_tools(&state, &req)).unwrap();
    let local: Vec<String> = local["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(remote, local);
    for name in rg_mcp::collab::TOOL_NAMES {
        assert!(remote.iter().any(|n| n == name), "{name} missing");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tools_reach_the_rest_api_as_the_caller_over_json_and_sse() {
    let w = world("mcp_call").await;
    let mcp = Mcp::start(&w.api, |_| {}).await;
    let session = mcp.open(&w.full).await.unwrap();

    let (is_error, text) = mcp
        .tool(&w.full, &session, "list_repos", json!({"owner": w.owner}))
        .await;
    assert!(!is_error, "{text}");
    assert!(text.contains("demo") && text.contains("other"), "{text}");

    // Same call as an SSE stream: one `message` event holding the answer.
    let reply = mcp
        .post(
            Some(&w.full),
            Some(&session),
            &json!({"jsonrpc": "2.0", "id": 9, "method": "tools/call",
                    "params": {"name": "list_repos", "arguments": {"owner": w.owner}}}),
        )
        .await;
    assert_eq!(reply.status, StatusCode::OK);
    assert!(reply.body.contains("event: message"), "{}", reply.body);
    let (v, _) = sse_message(&reply.body);
    assert_eq!(v["id"], json!(9));
    assert!(!parse_tool_result(&v).0);

    // A client that does not accept SSE gets a plain JSON body instead.
    let plain = mcp
        .http
        .post(format!("{}/mcp", mcp.url))
        .bearer_auth(&w.full)
        .header("accept", "application/json")
        .header("mcp-session-id", &session)
        .json(&json!({"jsonrpc": "2.0", "id": 10, "method": "tools/call",
                      "params": {"name": "list_repos", "arguments": {"owner": w.owner}}}))
        .send()
        .await
        .unwrap();
    assert!(plain.headers()["content-type"]
        .to_str()
        .unwrap()
        .starts_with("application/json"));
    let v: Value = plain.json().await.unwrap();
    assert_eq!(v["id"], json!(10));
    assert!(!parse_tool_result(&v).0);

    // The caller is the token's owner: a stranger sees nothing of a private repo.
    let theirs = mcp.open(&w.stranger).await.unwrap();
    let (is_error, text) = mcp
        .tool(
            &w.stranger,
            &theirs,
            "read_dir",
            json!({"owner": w.owner, "repo": "demo"}),
        )
        .await;
    assert!(is_error, "a stranger must not read a private repo: {text}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_limited_token_is_limited_through_mcp_as_over_rest() {
    let w = world("mcp_limits").await;
    let mcp = Mcp::start(&w.api, |_| {}).await;
    let (owner, demo) = (w.owner.clone(), json!("demo"));

    // repo:read can read, but the REST scope ladder refuses a write and the
    // collaboration area.
    let read = mcp.open(&w.read).await.unwrap();
    let (e, t) = mcp
        .tool(
            &w.read,
            &read,
            "read_dir",
            json!({"owner": owner, "repo": demo}),
        )
        .await;
    assert!(!e, "{t}");
    let write_args = |branch: &str| {
        json!({"owner": owner, "repo": "demo", "path": "note.txt", "content": "hi",
               "message": "m", "branch": branch})
    };
    let (e, t) = mcp
        .tool(&w.read, &read, "commit_file", write_args("agent/one"))
        .await;
    assert!(e && t.contains("403"), "a read token must not commit: {t}");
    let (e, t) = mcp
        .tool(
            &w.read,
            &read,
            "collab_start_session",
            json!({"owner": owner, "repo": "demo", "display_name": "r"}),
        )
        .await;
    assert!(
        e && t.contains("403"),
        "a read token must not open a collab session: {t}"
    );

    // A token limited to one repository: that one works, every other is refused.
    let one = mcp.open(&w.one_repo).await.unwrap();
    let (e, t) = mcp
        .tool(
            &w.one_repo,
            &one,
            "read_dir",
            json!({"owner": owner, "repo": "demo"}),
        )
        .await;
    assert!(!e, "{t}");
    let (e, t) = mcp
        .tool(
            &w.one_repo,
            &one,
            "read_dir",
            json!({"owner": owner, "repo": "other"}),
        )
        .await;
    assert!(e && t.contains("403"), "outside the allow-list: {t}");
    let (e, _) = mcp.tool(&w.one_repo, &one, "list_repos", json!({})).await;
    assert!(e, "listing repositories is outside a one-repo token");

    // A token limited to `agent/*` writes there and nowhere else.
    let br = mcp.open(&w.one_branch).await.unwrap();
    let (e, t) = mcp
        .tool(&w.one_branch, &br, "commit_file", write_args("main"))
        .await;
    assert!(e && t.contains("403"), "main is outside agent/*: {t}");
    let (e, t) = mcp
        .tool(&w.one_branch, &br, "commit_file", write_args("agent/one"))
        .await;
    assert!(!e, "agent/one is inside the branch limit: {t}");

    // Revoking the token ends what its sessions can do, at the next call.
    let list = Client::new()
        .get(format!("{}/api/v1/users/tokens", w.api))
        .bearer_auth(&w.jwt)
        .send()
        .await
        .unwrap()
        .json::<Value>()
        .await
        .unwrap();
    let id = list
        .as_array()
        .or_else(|| list["tokens"].as_array())
        .unwrap()
        .iter()
        .find(|t| t["name"] == "one")
        .and_then(|t| t["id"].as_i64())
        .unwrap();
    let del = Client::new()
        .delete(format!("{}/api/v1/users/tokens/{id}", w.api))
        .bearer_auth(&w.jwt)
        .send()
        .await
        .unwrap();
    assert!(del.status().is_success(), "{}", del.status());
    let (e, t) = mcp
        .tool(
            &w.one_repo,
            &one,
            "read_dir",
            json!({"owner": owner, "repo": "demo"}),
        )
        .await;
    assert!(
        e && t.contains("401"),
        "a revoked token must stop working: {t}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_session_belongs_to_the_token_that_opened_it() {
    let w = world("mcp_iso").await;
    let mcp = Mcp::start(&w.api, |_| {}).await;
    let mine = mcp.open(&w.full).await.unwrap();
    let ping = json!({"jsonrpc": "2.0", "id": 2, "method": "ping"});

    // Another valid token cannot use, or end, my session; it is told the same
    // thing as for an id that never existed.
    let theirs = mcp.post(Some(&w.stranger), Some(&mine), &ping).await;
    let unknown = mcp
        .post(
            Some(&w.stranger),
            Some("00000000-0000-0000-0000-000000000000"),
            &ping,
        )
        .await;
    assert_eq!(theirs.status, StatusCode::NOT_FOUND);
    assert_eq!(theirs.body, unknown.body);
    assert_eq!(mcp.delete(&w.stranger, &mine).await, StatusCode::NOT_FOUND);
    // Another token of the same account is still another token.
    assert_eq!(
        mcp.post(Some(&w.read), Some(&mine), &ping).await.status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        mcp.post(Some(&w.full), Some(&mine), &ping).await.status,
        StatusCode::OK
    );

    // Two sessions of one token do not share collaboration credentials: the
    // session secret one registered is invisible to the other.
    let a = mcp.open(&w.full).await.unwrap();
    let b = mcp.open(&w.full).await.unwrap();
    let start = json!({"owner": w.owner, "repo": "demo", "display_name": "agent-a"});
    let (e, t) = mcp
        .tool(&w.full, &a, "collab_start_session", start.clone())
        .await;
    assert!(!e, "{t}");
    assert!(
        !t.contains("ifs_"),
        "the session secret must not reach the model: {t}"
    );
    let (e, t) = mcp
        .tool(
            &w.full,
            &b,
            "collab_heartbeat",
            json!({"owner": w.owner, "repo": "demo"}),
        )
        .await;
    assert!(
        e && t.contains("NO_ACTIVE_SESSION"),
        "B must not see A's session: {t}"
    );
    let (e, t) = mcp
        .tool(
            &w.full,
            &a,
            "collab_heartbeat",
            json!({"owner": w.owner, "repo": "demo"}),
        )
        .await;
    assert!(!e, "A keeps its own: {t}");

    // Ending a session is final.
    assert_eq!(mcp.delete(&w.full, &a).await, StatusCode::NO_CONTENT);
    assert_eq!(
        mcp.post(Some(&w.full), Some(&a), &ping).await.status,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_client_can_reconnect_and_start_over() {
    let w = world("mcp_recon").await;
    let mcp = Mcp::start(&w.api, |_| {}).await;
    let session = mcp.open(&w.full).await.unwrap();
    let args = json!({"owner": w.owner});
    let (e, _) = mcp
        .tool(&w.full, &session, "list_repos", args.clone())
        .await;
    assert!(!e);

    // A new TCP connection (the old client is gone) continues the session: the
    // session is not tied to a connection.
    let fresh = Mcp {
        url: mcp.url.clone(),
        http: Client::builder().pool_max_idle_per_host(0).build().unwrap(),
    };
    let (e, t) = fresh
        .tool(&w.full, &session, "list_repos", args.clone())
        .await;
    assert!(!e, "{t}");

    // A restarted transport has no such session: 404, and initialize starts over.
    let restarted = Mcp::start(&w.api, |_| {}).await;
    let reply = restarted
        .post(
            Some(&w.full),
            Some(&session),
            &json!({"jsonrpc": "2.0", "id": 5, "method": "tools/list"}),
        )
        .await;
    assert_eq!(reply.status, StatusCode::NOT_FOUND);
    let again = restarted.open(&w.full).await.unwrap();
    let (e, _) = restarted.tool(&w.full, &again, "list_repos", args).await;
    assert!(!e);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_long_poll_returns_on_the_event_and_keeps_the_connection_warm() {
    let w = world("mcp_poll").await;
    let mcp = Mcp::start(&w.api, |c| c.heartbeat = Duration::from_millis(250)).await;
    let session = mcp.open(&w.full).await.unwrap();
    let args = |extra: Value| {
        let mut base = json!({"owner": w.owner, "repo": "demo"});
        base.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        base
    };

    // Without `since` the tool answers at once with the current cursor.
    let (e, t) = mcp
        .tool(&w.full, &session, "wait_repo_events", args(json!({})))
        .await;
    assert!(!e, "{t}");
    let cursor = serde_json::from_str::<Value>(&t).unwrap()["next_cursor"]
        .as_i64()
        .unwrap();

    // An event that arrives while the call waits ends it early, and the SSE
    // response carried heartbeats while it did.
    let started = Instant::now();
    let poll = json!({"jsonrpc": "2.0", "id": 11, "method": "tools/call",
                "params": {"name": "wait_repo_events",
                           "arguments": args(json!({"since": cursor, "timeout": 20}))}});
    let call = mcp.post(Some(&w.full), Some(&session), &poll);
    let creator = async {
        tokio::time::sleep(Duration::from_millis(1500)).await;
        let resp = Client::new()
            .post(format!("{}/api/v1/repos/{}/demo/issues", w.api, w.owner))
            .bearer_auth(&w.full)
            .json(&json!({"title": "wakes the poll"}))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 201);
    };
    let (reply, ()) = tokio::join!(call, creator);
    let waited = started.elapsed();
    assert_eq!(reply.status, StatusCode::OK);
    let (v, heartbeats) = sse_message(&reply.body);
    let (e, text) = parse_tool_result(&v);
    assert!(!e, "{text}");
    let events: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(events["timed_out"], json!(false), "{text}");
    assert!(!events["events"].as_array().unwrap().is_empty());
    assert!(
        waited >= Duration::from_millis(1400) && waited < Duration::from_secs(10),
        "returned on the event, not at the timeout: {waited:?}"
    );
    assert!(
        heartbeats >= 2,
        "expected heartbeats while waiting, got {heartbeats}"
    );

    // Nothing happening: it returns at the timeout, flagged as such.
    let next = events["next_cursor"].as_i64().unwrap();
    let started = Instant::now();
    let (e, text) = mcp
        .tool(
            &w.full,
            &session,
            "wait_repo_events",
            args(json!({"since": next, "timeout": 2})),
        )
        .await;
    assert!(!e, "{text}");
    assert_eq!(
        serde_json::from_str::<Value>(&text).unwrap()["timed_out"],
        json!(true)
    );
    assert!(started.elapsed() >= Duration::from_millis(1800));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_dropped_call_keeps_its_slot_until_it_has_really_ended() {
    let w = world("mcp_drop").await;
    let mcp = Mcp::start(&w.api, |c| c.max_inflight_per_session = 1).await;
    let session = mcp.open(&w.full).await.unwrap();
    let wait = |timeout: u64, since: i64| {
        json!({"jsonrpc": "2.0", "id": 21, "method": "tools/call",
               "params": {"name": "wait_repo_events",
                          "arguments": {"owner": w.owner, "repo": "demo",
                                        "since": since, "timeout": timeout}}})
    };

    // The client gives up after 300 ms of a 3 s wait.
    let abandoned = tokio::time::timeout(
        Duration::from_millis(300),
        mcp.post(Some(&w.full), Some(&session), &wait(3, 0)),
    )
    .await;
    assert!(
        abandoned.is_err(),
        "the call should still have been waiting"
    );

    // The work is still running upstream, so it still counts against the cap.
    let busy = mcp
        .post(
            Some(&w.full),
            Some(&session),
            &json!({"jsonrpc": "2.0", "id": 22, "method": "tools/list"}),
        )
        .await;
    assert_eq!(busy.status, StatusCode::TOO_MANY_REQUESTS, "{}", busy.body);
    assert!(busy.body.contains("too many calls"));

    // When it has ended the slot is free again.
    tokio::time::sleep(Duration::from_millis(3500)).await;
    let ok = mcp
        .post(
            Some(&w.full),
            Some(&session),
            &json!({"jsonrpc": "2.0", "id": 23, "method": "tools/list"}),
        )
        .await;
    assert_eq!(ok.status, StatusCode::OK, "{}", ok.body);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn protocol_edges_and_limits() {
    let w = world("mcp_edges").await;
    let mcp = Mcp::start(&w.api, |c| c.max_sessions_per_token = 2).await;
    let ping = json!({"jsonrpc": "2.0", "id": 2, "method": "ping"});

    // Sessions per token are capped; ending one makes room.
    let s1 = mcp.open(&w.full).await.unwrap();
    let _s2 = mcp.open(&w.full).await.unwrap();
    let third = mcp.open(&w.full).await.unwrap_err();
    assert_eq!(
        third.status,
        StatusCode::TOO_MANY_REQUESTS,
        "{}",
        third.body
    );
    // A different token is not affected by that cap.
    mcp.open(&w.stranger).await.unwrap();
    assert_eq!(mcp.delete(&w.full, &s1).await, StatusCode::NO_CONTENT);
    mcp.open(&w.full).await.unwrap();

    let session = mcp.open(&w.stranger).await.unwrap();
    // Everything after initialize needs the session header.
    assert_eq!(
        mcp.post(Some(&w.stranger), None, &ping).await.status,
        StatusCode::BAD_REQUEST
    );
    // An unsupported protocol version header is refused.
    let bad = mcp
        .post_with(
            Some(&w.stranger),
            Some(&session),
            &ping,
            &[("mcp-protocol-version", "1999-01-01")],
        )
        .await;
    assert_eq!(bad.status, StatusCode::BAD_REQUEST);
    // Notifications get 202 and no body; so do the client's own answers.
    let note = mcp
        .post(
            Some(&w.stranger),
            Some(&session),
            &json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        )
        .await;
    assert_eq!(note.status, StatusCode::ACCEPTED);
    assert!(note.body.is_empty());
    // A batch is answered as a batch, notifications in it are not.
    let batch = mcp
        .post(
            Some(&w.stranger),
            Some(&session),
            &json!([
                {"jsonrpc": "2.0", "id": 1, "method": "ping"},
                {"jsonrpc": "2.0", "method": "notifications/initialized"},
                {"jsonrpc": "2.0", "id": 2, "method": "no/such/method"}
            ]),
        )
        .await;
    let answers = batch.json();
    assert_eq!(answers.as_array().unwrap().len(), 2, "{}", batch.body);
    assert_eq!(answers[1]["error"]["code"], json!(-32601));
    // Garbage is a parse error, not a crash.
    let junk = mcp
        .http
        .post(format!("{}/mcp", mcp.url))
        .bearer_auth(&w.stranger)
        .header("mcp-session-id", &session)
        .body("{not json")
        .send()
        .await
        .unwrap();
    assert_eq!(junk.status(), StatusCode::BAD_REQUEST);
    // A second `initialize` inside a batch is refused.
    let nested = mcp
        .post(
            Some(&w.stranger),
            Some(&session),
            &json!([{"jsonrpc": "2.0", "id": 1, "method": "initialize"}]),
        )
        .await;
    assert_eq!(nested.json()[0]["error"]["code"], json!(-32600));

    // A browser origin is refused unless the operator listed it.
    let origin = [("origin", "https://evil.example")];
    let refused = mcp
        .post_with(Some(&w.stranger), Some(&session), &ping, &origin)
        .await;
    assert_eq!(refused.status, StatusCode::FORBIDDEN);
    let allowing = Mcp::start(&w.api, |c| {
        c.allowed_origins = vec!["https://evil.example".into()]
    })
    .await;
    let s = allowing.open(&w.stranger).await.unwrap();
    let allowed = allowing
        .post_with(Some(&w.stranger), Some(&s), &ping, &origin)
        .await;
    assert_eq!(allowed.status, StatusCode::OK);

    // The probe's answer decides initialize, so a server that is down is a 502,
    // not a session.
    let dead = Mcp::start("http://127.0.0.1:1", |_| {}).await;
    let reply = dead.open(&w.full).await.unwrap_err();
    assert_eq!(reply.status, StatusCode::BAD_GATEWAY);
}
