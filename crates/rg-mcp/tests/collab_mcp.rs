//! T11 (and the MCP part of T09): the real `ironforge-mcp` binary over stdio,
//! against a recording stub of the IronForge REST API.
//!
//! The stub lets the tests assert exactly what the tools send (paths, headers,
//! bodies) and inject error responses. Behaviour against the real server is
//! covered by the C06 end-to-end run.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};

const SECRET: &str = "ifs_0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const SESSION_ID: &str = "11111111-1111-4111-8111-111111111111";

// ── stub REST API ─────────────────────────────────────────────────────

#[derive(Clone, Debug)]
struct Req {
    method: String,
    path: String,
    headers: HashMap<String, String>,
    body: Value,
}

type Handler = Box<dyn Fn(&Req) -> (u16, String) + Send + Sync>;

struct Stub {
    url: String,
    requests: Arc<Mutex<Vec<Req>>>,
}

impl Stub {
    fn spawn(handler: Handler) -> Stub {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let log = requests.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                serve(stream, &handler, &log);
            }
        });
        Stub { url, requests }
    }

    fn requests(&self) -> Vec<Req> {
        self.requests.lock().unwrap().clone()
    }
}

fn serve(stream: TcpStream, handler: &Handler, log: &Arc<Mutex<Vec<Req>>>) {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut line = String::new();
    if reader.read_line(&mut line).unwrap_or(0) == 0 {
        return;
    }
    let mut parts = line.split_whitespace();
    let (method, path) = (parts.next().unwrap_or("").to_string(), parts.next().unwrap_or("").to_string());
    let mut headers = HashMap::new();
    loop {
        let mut h = String::new();
        reader.read_line(&mut h).unwrap();
        let h = h.trim_end();
        if h.is_empty() {
            break;
        }
        if let Some((k, v)) = h.split_once(':') {
            headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
        }
    }
    let len: usize = headers.get("content-length").and_then(|v| v.parse().ok()).unwrap_or(0);
    let mut raw = vec![0u8; len];
    reader.read_exact(&mut raw).unwrap();
    let body = serde_json::from_slice(&raw).unwrap_or(Value::Null);
    let req = Req { method, path, headers, body };
    let (status, text) = handler(&req);
    log.lock().unwrap().push(req);
    let mut stream = stream;
    let _ = write!(
        stream,
        "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}",
        text.len()
    );
}

/// A server that behaves like a minimal collaboration API.
fn happy_handler() -> Handler {
    Box::new(|req| {
        let p = req.path.as_str();
        if req.method == "POST" && p.ends_with("/collaboration/sessions") {
            (201, json!({"session": {"id": SESSION_ID, "display_name": req.body["display_name"], "client_kind": req.body["client_kind"], "state": "online", "origin": "user_content"}, "session_secret": SECRET}).to_string())
        } else if p.ends_with("/heartbeat") {
            (200, json!({"id": SESSION_ID, "state": "online"}).to_string())
        } else if p.ends_with("/end") {
            (204, String::new())
        } else if p.contains("/messages") && req.method == "POST" {
            (201, json!({"id": "m1", "seq": 1, "client_message_id": req.body["client_message_id"]}).to_string())
        } else {
            (200, json!({"ok": true, "path": p}).to_string())
        }
    })
}

// ── MCP client over stdio ───────────────────────────────────────────

struct Mcp {
    child: Child,
    reader: BufReader<std::process::ChildStdout>,
    next_id: u64,
}

impl Mcp {
    fn start(api: &str, session_file: Option<&std::path::Path>) -> Mcp {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_ironforge-mcp"));
        cmd.env("IRONFORGE_URL", api)
            .env("IRONFORGE_PAT", "pat-for-tests")
            .env("RUST_LOG", "trace")
            .env_remove("IRONFORGE_COLLAB_SESSION_FILE")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(file) = session_file {
            cmd.env("IRONFORGE_COLLAB_SESSION_FILE", file);
        }
        let mut child = cmd.spawn().unwrap();
        let reader = BufReader::new(child.stdout.take().unwrap());
        Mcp { child, reader, next_id: 1 }
    }

    fn rpc(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        let line = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        let stdin = self.child.stdin.as_mut().unwrap();
        writeln!(stdin, "{line}").unwrap();
        stdin.flush().unwrap();
        let mut out = String::new();
        self.reader.read_line(&mut out).unwrap();
        let resp: Value = serde_json::from_str(&out).unwrap_or_else(|_| panic!("bad response {out:?}"));
        assert_eq!(resp["id"], id);
        resp
    }

    /// Call a tool; returns `(isError, text)`.
    fn tool(&mut self, name: &str, args: Value) -> (bool, String) {
        let resp = self.rpc("tools/call", json!({"name": name, "arguments": args}));
        let result = &resp["result"];
        assert!(result.is_object(), "no result: {resp}");
        (
            result["isError"].as_bool().unwrap_or(false),
            result["content"][0]["text"].as_str().unwrap().to_string(),
        )
    }

    fn tool_json(&mut self, name: &str, args: Value) -> (bool, Value) {
        let (is_error, text) = self.tool(name, args);
        (is_error, serde_json::from_str(&text).unwrap_or_else(|_| panic!("not JSON: {text}")))
    }

    /// Stop the process and return everything it wrote to stderr.
    fn stop(mut self) -> String {
        drop(self.child.stdin.take());
        let _ = self.child.wait();
        let mut stderr = String::new();
        self.child.stderr.take().unwrap().read_to_string(&mut stderr).unwrap();
        stderr
    }
}

fn start_args() -> Value {
    json!({"owner": "o", "repo": "r", "display_name": "Agent A", "client_kind": "test"})
}

// ── tests ───────────────────────────────────────────────────────────

#[test]
fn t11_tools_list_advertises_complete_schemas() {
    let stub = Stub::spawn(happy_handler());
    let mut mcp = Mcp::start(&stub.url, None);
    let resp = mcp.rpc("tools/list", json!({}));
    let tools = resp["result"]["tools"].as_array().unwrap();
    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    for expected in [
        "list_repos", "read_file", "collab_start_session", "collab_list_sessions", "collab_create_task",
        "collab_get_task", "collab_update_task", "collab_list_messages", "collab_send_message",
        "collab_mark_read", "collab_create_handoff", "collab_list_handoffs", "collab_accept_handoff",
        "collab_cancel_handoff", "collab_heartbeat", "collab_end_session", "collab_list_tasks",
        "collab_get_rules", "collab_get_inbox", "collab_claim_task", "collab_list_takeovers",
        "collab_ask_question", "collab_list_questions", "collab_get_question", "collab_answer_question",
        "collab_cancel_question", "collab_create_work_package", "collab_list_work_packages",
        "collab_get_work_package", "collab_get_completion_status",
    ] {
        assert!(names.contains(&expected), "missing {expected}: {names:?}");
    }
    let by = |n: &str| tools.iter().find(|t| t["name"] == n).unwrap();

    let update = &by("collab_update_task")["inputSchema"];
    assert!(update["required"].as_array().unwrap().contains(&json!("expected_version")), "version condition");
    assert_eq!(update["properties"]["status"]["enum"], json!(["open", "in_progress", "waiting", "blocked", "done"]));
    let send = &by("collab_send_message")["inputSchema"];
    assert_eq!(send["properties"]["kind"]["enum"], json!(["note", "help", "blocker"]));
    assert_eq!(send["properties"]["body"]["maxLength"], 32768);
    assert_eq!(send["properties"]["body"]["minLength"], 1);
    assert_eq!(by("collab_create_task")["inputSchema"]["properties"]["title"]["maxLength"], 200);
    for t in tools.iter().filter(|t| t["name"].as_str().unwrap().starts_with("collab_")) {
        assert_eq!(t["inputSchema"]["additionalProperties"], false, "{}", t["name"]);
    }
    // The secret is not an input of any tool.
    assert!(!resp.to_string().contains("session_secret"));
    mcp.stop();
}

/// T09 (MCP): the secret never reaches the model, logs or later tool output,
/// yet later calls authenticate with it.
#[test]
fn t09_t11_secret_is_used_but_never_shown() {
    let stub = Stub::spawn(happy_handler());
    let mut mcp = Mcp::start(&stub.url, None);
    let mut seen = Vec::new();

    let (err, start) = mcp.tool_json("collab_start_session", start_args());
    assert!(!err, "{start}");
    assert_eq!((start["id"].as_str(), start["resumed"].as_bool()), (Some(SESSION_ID), Some(false)));
    seen.push(start.to_string());

    for (tool, args) in [
        ("collab_create_task", json!({"owner": "o", "repo": "r", "title": "t", "owner_session_id": "self"})),
        ("collab_send_message", json!({"owner": "o", "repo": "r", "task_id": "t1", "kind": "note", "body": "hi"})),
        ("collab_heartbeat", json!({"owner": "o", "repo": "r"})),
        ("collab_list_sessions", json!({"owner": "o", "repo": "r"})),
    ] {
        let (err, out) = mcp.tool(tool, args);
        assert!(!err, "{tool}: {out}");
        seen.push(out);
    }

    for text in &seen {
        assert!(!text.contains(SECRET), "tool output leaked the secret: {text}");
        assert!(!text.contains("session_secret"), "{text}");
    }

    let reqs = stub.requests();
    // Registration carried only the account PAT; later calls carried the session pair.
    assert_eq!(reqs[0].headers.get("authorization").map(String::as_str), Some("Bearer pat-for-tests"));
    assert!(!reqs[0].headers.contains_key("x-ironforge-session-secret"));
    for r in &reqs[1..] {
        if r.method == "GET" && r.path.contains("/sessions") {
            continue; // discovery needs no session
        }
        assert_eq!(r.headers.get("x-ironforge-session-secret").map(String::as_str), Some(SECRET), "{} {}", r.method, r.path);
        assert_eq!(r.headers.get("x-ironforge-session").map(String::as_str), Some(SESSION_ID));
        assert!(!r.path.contains(SECRET), "secret in URL");
        assert!(!r.body.to_string().contains(SECRET), "secret in body");
    }
    // "self" was resolved to the real session id before it left the process.
    let create = reqs.iter().find(|r| r.path.ends_with("/tasks") && r.method == "POST").unwrap();
    assert_eq!(create.body["owner_session_id"], SESSION_ID);

    let stderr = mcp.stop();
    assert!(!stderr.contains(SECRET), "process log leaked the secret");
}

#[test]
fn t11_server_errors_become_structured_tool_errors() {
    let stub = Stub::spawn(Box::new(|req| match req.path.as_str() {
        p if p.ends_with("/sessions") && req.method == "POST" => (201, json!({"session": {"id": SESSION_ID}, "session_secret": SECRET}).to_string()),
        p if p.contains("/tasks/conflict") => (409, json!({"error": {"code": "VERSION_CONFLICT", "message": "stale", "details": {"current_version": 3}}}).to_string()),
        p if p.contains("/tasks/echo") => (400, json!({"error": {"code": "BAD_REQUEST", "message": format!("you sent {SECRET}")}}).to_string()),
        p if p.contains("/tasks/html") => (502, "<html>bad gateway</html>".to_string()),
        p if p.contains("/tasks/missing") => (404, json!({"error": {"code": "NOT_FOUND", "message": "task not found"}}).to_string()),
        _ => (200, "{}".to_string()),
    }));
    let mut mcp = Mcp::start(&stub.url, None);
    mcp.tool("collab_start_session", start_args());

    let (is_error, v) = mcp.tool_json("collab_get_task", json!({"owner": "o", "repo": "r", "task_id": "conflict"}));
    assert!(is_error, "errors must set isError");
    assert_eq!(v["error"]["status"], 409);
    assert_eq!(v["error"]["code"], "VERSION_CONFLICT");
    assert_eq!(v["error"]["details"]["current_version"], 3);

    let (is_error, v) = mcp.tool_json("collab_get_task", json!({"owner": "o", "repo": "r", "task_id": "missing"}));
    assert!(is_error);
    assert_eq!((v["error"]["status"].as_i64(), v["error"]["code"].as_str()), (Some(404), Some("NOT_FOUND")));

    let (is_error, v) = mcp.tool_json("collab_get_task", json!({"owner": "o", "repo": "r", "task_id": "html"}));
    assert!(is_error);
    assert_eq!(v["error"]["status"], 502);
    assert_eq!(v["error"]["code"], "HTTP_ERROR");

    // Even a server that echoes the secret cannot get it through.
    let (is_error, text) = mcp.tool("collab_get_task", json!({"owner": "o", "repo": "r", "task_id": "echo"}));
    assert!(is_error);
    assert!(!text.contains(SECRET), "{text}");
    assert!(text.contains("[redacted]"));
    mcp.stop();
}

#[test]
fn t11_unreachable_server_is_a_transport_error_not_a_success() {
    // A port with nothing behind it.
    let dead = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", dead.local_addr().unwrap());
    drop(dead);
    let mut mcp = Mcp::start(&url, None);
    let (is_error, v) = mcp.tool_json("collab_start_session", start_args());
    assert!(is_error);
    assert_eq!(v["error"]["code"], "TRANSPORT_ERROR");
    mcp.stop();
}

#[test]
fn t11_invalid_arguments_are_rejected_before_any_request() {
    let stub = Stub::spawn(happy_handler());
    let mut mcp = Mcp::start(&stub.url, None);
    mcp.tool("collab_start_session", start_args());
    let before = stub.requests().len();

    let cases = [
        ("collab_get_task", json!({"owner": "../x", "repo": "r", "task_id": "t"})),
        ("collab_get_task", json!({"owner": "o", "repo": "r", "task_id": "../../etc"})),
        ("collab_get_task", json!({"owner": "o", "repo": "r"})),
        ("collab_get_task", json!({"repo": "r", "task_id": "t"})),
        ("collab_send_message", json!({"owner": "o", "repo": "r", "task_id": "t", "kind": "note"})),
        ("collab_update_task", json!({"owner": "o", "repo": "r", "task_id": "t", "title": "no version"})),
        ("collab_update_task", json!({"owner": "o", "repo": "r", "task_id": "t", "expected_version": "3"})),
        ("collab_mark_read", json!({"owner": "o", "repo": "r", "task_id": "t"})),
        ("collab_accept_handoff", json!({"owner": "o", "repo": "r", "handoff_id": "h"})),
        ("collab_create_handoff", json!({"owner": "o", "repo": "r", "task_id": "t", "summary": "s"})),
        ("collab_create_task", json!({"owner": "o", "repo": "r"})),
        ("collab_start_session", json!({"owner": "o", "repo": "r"})),
    ];
    for (tool, args) in cases {
        let (is_error, v) = mcp.tool_json(tool, args.clone());
        assert!(is_error, "{tool} {args}");
        assert_eq!(v["error"]["code"], "INVALID_ARGUMENTS", "{tool} {args}: {v}");
    }
    let (is_error, _) = mcp.tool("collab_get_task", json!("not an object"));
    assert!(is_error);
    assert_eq!(stub.requests().len(), before, "invalid calls must not reach the API");
    mcp.stop();
}

#[test]
fn t11_session_bound_tools_need_a_session_and_say_so() {
    let stub = Stub::spawn(happy_handler());
    let mut mcp = Mcp::start(&stub.url, None);
    for (tool, args) in [
        ("collab_create_task", json!({"owner": "o", "repo": "r", "title": "t"})),
        ("collab_heartbeat", json!({"owner": "o", "repo": "r"})),
        ("collab_send_message", json!({"owner": "o", "repo": "r", "task_id": "t", "kind": "note", "body": "b"})),
    ] {
        let (is_error, v) = mcp.tool_json(tool, args);
        assert!(is_error);
        assert_eq!(v["error"]["code"], "NO_ACTIVE_SESSION", "{tool}");
    }
    // Read-only tools work without a session.
    let (is_error, _) = mcp.tool("collab_get_task", json!({"owner": "o", "repo": "r", "task_id": "t"}));
    assert!(!is_error);
    assert!(stub.requests().iter().all(|r| r.method == "GET"), "no write reached the API");
    mcp.stop();
}

#[test]
fn t11_payload_mapping_keeps_nulls_and_idempotency_keys() {
    let stub = Stub::spawn(happy_handler());
    let mut mcp = Mcp::start(&stub.url, None);
    mcp.tool("collab_start_session", start_args());

    mcp.tool("collab_update_task", json!({
        "owner": "o", "repo": "r", "task_id": "t1", "expected_version": 4,
        "branch": null, "status": "done", "owner_session_id": null
    }));
    mcp.tool("collab_send_message", json!({"owner": "o", "repo": "r", "task_id": "t1", "kind": "help", "body": "stuck", "client_message_id": "my-key-1"}));
    let (_, generated) = mcp.tool_json("collab_send_message", json!({"owner": "o", "repo": "r", "task_id": "t1", "kind": "note", "body": "x"}));
    mcp.tool("collab_list_messages", json!({"owner": "o", "repo": "r", "task_id": "t1", "after_seq": 7, "limit": 20}));
    mcp.tool("collab_accept_handoff", json!({"owner": "o", "repo": "r", "handoff_id": "h1", "expected_version": 1}));
    mcp.tool("collab_mark_read", json!({"owner": "o", "repo": "r", "task_id": "t1", "last_read_seq": 5}));

    let reqs = stub.requests();
    let find = |m: &str, suffix: &str| reqs.iter().find(|r| r.method == m && r.path.ends_with(suffix)).unwrap_or_else(|| panic!("{m} {suffix} in {reqs:?}"));
    let patch = find("PATCH", "/collaboration/tasks/t1");
    assert!(patch.body["branch"].is_null() && patch.body.as_object().unwrap().contains_key("branch"), "explicit null must be sent");
    assert!(patch.body["owner_session_id"].is_null());
    assert_eq!((patch.body["expected_version"].as_i64(), patch.body["status"].as_str()), (Some(4), Some("done")));
    assert!(!patch.body.as_object().unwrap().contains_key("title"), "omitted fields stay omitted");

    let sends: Vec<_> = reqs.iter().filter(|r| r.method == "POST" && r.path.ends_with("/tasks/t1/messages")).collect();
    assert_eq!(sends[0].body["client_message_id"], "my-key-1");
    let auto = sends[1].body["client_message_id"].as_str().unwrap();
    assert!(uuid_like(auto), "{auto}");
    assert_eq!(generated["client_message_id"], auto, "the key used is reported for retries");

    assert!(reqs.iter().any(|r| r.path.ends_with("/tasks/t1/messages?after_seq=7&limit=20")));
    assert!(reqs.iter().any(|r| r.method == "POST" && r.path.ends_with("/handoffs/h1/accept") && r.body == json!({"expected_version": 1})));
    assert!(reqs.iter().any(|r| r.method == "PUT" && r.path.ends_with("/tasks/t1/read") && r.body == json!({"last_read_seq": 5})));
    mcp.stop();
}

fn uuid_like(s: &str) -> bool {
    s.len() == 36 && s.matches('-').count() == 4
}

#[test]
fn t11_expired_session_is_forgotten_and_a_new_one_can_be_registered() {
    let calls = Arc::new(Mutex::new(0u32));
    let c = calls.clone();
    let stub = Stub::spawn(Box::new(move |req| {
        if req.method == "POST" && req.path.ends_with("/collaboration/sessions") {
            let n = { let mut g = c.lock().unwrap(); *g += 1; *g };
            let secret = format!("ifs_{}", format!("{n:02}").repeat(32));
            (201, json!({"session": {"id": format!("sess-{n}")}, "session_secret": secret}).to_string())
        } else if req.path.ends_with("/heartbeat") {
            (409, json!({"error": {"code": "SESSION_EXPIRED", "message": "session has ended or expired"}}).to_string())
        } else {
            (200, "{}".to_string())
        }
    }));
    let mut mcp = Mcp::start(&stub.url, None);
    assert!(!mcp.tool("collab_start_session", start_args()).0);
    let (is_error, v) = mcp.tool_json("collab_heartbeat", json!({"owner": "o", "repo": "r"}));
    assert!(is_error);
    assert_eq!(v["error"]["code"], "SESSION_EXPIRED");
    // The dead credentials are dropped, so the next write says "register" instead of retrying them.
    let (_, v) = mcp.tool_json("collab_create_task", json!({"owner": "o", "repo": "r", "title": "t"}));
    assert_eq!(v["error"]["code"], "NO_ACTIVE_SESSION");
    // A fresh registration yields a new session.
    let (is_error, v) = mcp.tool_json("collab_start_session", start_args());
    assert!(!is_error);
    assert_eq!(v["id"], "sess-2");
    mcp.stop();
}

/// T11: process restart. Without a credential file the new process must say it
/// has no session and re-register; with one the same session resumes.
#[test]
fn t11_restart_without_a_credential_file_requires_explicit_re_registration() {
    let registrations = Arc::new(Mutex::new(0u32));
    let r = registrations.clone();
    let stub = Stub::spawn(Box::new(move |req| {
        if req.method == "POST" && req.path.ends_with("/collaboration/sessions") {
            let n = { let mut g = r.lock().unwrap(); *g += 1; *g };
            (201, json!({"session": {"id": format!("sess-{n}")}, "session_secret": format!("ifs_{}", "ab".repeat(32))}).to_string())
        } else {
            (200, "{}".to_string())
        }
    }));
    let mut first = Mcp::start(&stub.url, None);
    first.tool("collab_start_session", start_args());
    first.stop();

    let mut second = Mcp::start(&stub.url, None);
    let (is_error, v) = second.tool_json("collab_send_message", json!({"owner": "o", "repo": "r", "task_id": "t", "kind": "note", "body": "b"}));
    assert!(is_error);
    assert_eq!(v["error"]["code"], "NO_ACTIVE_SESSION", "an old session cannot be silently continued");
    let (is_error, v) = second.tool_json("collab_start_session", start_args());
    assert!(!is_error);
    assert_eq!(v["id"], "sess-2", "re-registration creates a new session");
    // The read side never needed a session.
    assert!(!second.tool("collab_list_messages", json!({"owner": "o", "repo": "r", "task_id": "t"})).0);
    second.stop();
}

#[test]
fn t11_restart_with_a_credential_file_resumes_the_same_session() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("sessions.json");
    let stub = Stub::spawn(happy_handler());

    let mut first = Mcp::start(&stub.url, Some(&file));
    first.tool("collab_start_session", start_args());
    first.stop();

    // The host-provided file holds the credentials, readable by the owner only.
    let stored = std::fs::read_to_string(&file).unwrap();
    assert!(stored.contains(SECRET) && stored.contains(SESSION_ID));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&file).unwrap().permissions().mode() & 0o777, 0o600);
    }

    let mut second = Mcp::start(&stub.url, Some(&file));
    let (is_error, v) = second.tool_json("collab_start_session", start_args());
    assert!(!is_error, "{v}");
    assert_eq!((v["resumed"].as_bool(), v["id"].as_str()), (Some(true), Some(SESSION_ID)));
    assert!(!v.to_string().contains(SECRET));
    let registrations = stub.requests().iter().filter(|r| r.method == "POST" && r.path.ends_with("/collaboration/sessions")).count();
    assert_eq!(registrations, 1, "resuming must not register a duplicate session");

    // It can act as the same session straight away.
    let (is_error, _) = second.tool("collab_send_message", json!({"owner": "o", "repo": "r", "task_id": "t", "kind": "note", "body": "after restart"}));
    assert!(!is_error);
    let last = stub.requests().into_iter().rev().find(|r| r.path.ends_with("/messages")).unwrap();
    assert_eq!(last.headers.get("x-ironforge-session-secret").map(String::as_str), Some(SECRET));

    // Ending the session removes the credentials from the file.
    assert!(!second.tool("collab_end_session", json!({"owner": "o", "repo": "r"})).0);
    let after = std::fs::read_to_string(&file).unwrap();
    assert!(!after.contains(SECRET) && !after.contains(SESSION_ID), "{after}");
    let stderr = second.stop();
    assert!(!stderr.contains(SECRET));
}

// ── P0 tools (P14) ───────────────────────────────────────────────────

#[test]
fn p14_p0_tools_map_to_the_rest_api_with_session_credentials_and_idempotency_keys() {
    let stub = Stub::spawn(happy_handler());
    let mut mcp = Mcp::start(&stub.url, None);
    let (err, _) = mcp.tool_json("collab_start_session", start_args());
    assert!(!err);
    let base = json!({"owner": "o", "repo": "r"});
    let with = |extra: Value| {
        let mut args = base.clone();
        args.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        args
    };
    let tid = "22222222-2222-4222-8222-222222222222";
    let qid = "33333333-3333-4333-8333-333333333333";

    let calls: Vec<(&str, Value)> = vec![
        ("collab_get_rules", with(json!({"ref": "feature/x", "path": "web/src"}))),
        ("collab_get_inbox", with(json!({"since": "2026-11-04T00:00:00Z", "limit": 5}))),
        ("collab_claim_task", with(json!({"task_id": tid, "expected_task_version": 3, "expected_owner_session_id": null, "reason": "owner gone"}))),
        ("collab_list_takeovers", with(json!({"task_id": tid}))),
        ("collab_ask_question", with(json!({"task_id": tid, "recipient": {"kind": "session", "id": "self"}, "title": "Which?", "body": "A or B", "options": ["A", "B"]}))),
        ("collab_list_questions", with(json!({"task_id": tid, "status": "open"}))),
        ("collab_get_question", with(json!({"question_id": qid}))),
        ("collab_answer_question", with(json!({"question_id": qid, "expected_version": 1, "acknowledged_context_hash": "abc", "option_id": "o1"}))),
        ("collab_cancel_question", with(json!({"question_id": qid, "expected_version": 1}))),
        ("collab_create_work_package", with(json!({"task_id": tid, "goal": "g", "evidence_refs": [{"kind": "pipeline", "reference": "7"}]}))),
        ("collab_list_work_packages", with(json!({"task_id": tid}))),
        ("collab_get_work_package", with(json!({"package_id": qid}))),
        ("collab_get_completion_status", with(json!({"task_id": tid}))),
    ];
    for (tool, args) in &calls {
        let (err, out) = mcp.tool(tool, args.clone());
        assert!(!err, "{tool}: {out}");
        assert!(!out.contains(SECRET), "{tool} leaked the secret");
    }

    let reqs = stub.requests();
    let find = |method: &str, suffix: &str| -> Req {
        reqs.iter()
            .find(|r| r.method == method && r.path.contains(suffix))
            .unwrap_or_else(|| panic!("no {method} …{suffix} in {:?}", reqs.iter().map(|r| format!("{} {}", r.method, r.path)).collect::<Vec<_>>()))
            .clone()
    };
    let session = |r: &Req| {
        assert_eq!(r.headers.get("x-ironforge-session").map(String::as_str), Some(SESSION_ID), "{}", r.path);
        assert_eq!(r.headers.get("x-ironforge-session-secret").map(String::as_str), Some(SECRET), "{}", r.path);
    };

    let rules = find("GET", "/collaboration/rules");
    assert!(rules.path.ends_with("/rules?ref=feature%2Fx&path=web%2Fsrc"), "{}", rules.path);
    let inbox = find("GET", "/collaboration/inbox");
    assert!(inbox.path.contains("since=2026-11-04T00%3A00%3A00Z") && inbox.path.contains("limit=5"), "{}", inbox.path);
    session(&inbox); // the inbox is the session's own

    let claim = find("POST", &format!("/tasks/{tid}/takeovers"));
    session(&claim);
    assert_eq!(claim.body["expected_task_version"], 3);
    assert!(claim.body["expected_owner_session_id"].is_null(), "explicit null is kept");
    assert!(uuid_like(claim.body["client_takeover_id"].as_str().unwrap()), "idempotency key generated");

    let ask = find("POST", &format!("/tasks/{tid}/questions"));
    session(&ask);
    assert_eq!(ask.body["recipient"], json!({"kind": "session", "id": SESSION_ID}), "\"self\" resolved");
    assert_eq!(ask.body["options"], json!(["A", "B"]));
    assert!(uuid_like(ask.body["client_question_id"].as_str().unwrap()));

    let answer = find("POST", &format!("/questions/{qid}/answer"));
    session(&answer);
    assert_eq!(answer.body["acknowledged_context_hash"], "abc");
    assert!(uuid_like(answer.body["client_answer_id"].as_str().unwrap()));
    session(&find("POST", &format!("/questions/{qid}/cancel")));

    let package = find("POST", &format!("/tasks/{tid}/work-packages"));
    session(&package);
    assert_eq!(package.body["evidence_refs"][0], json!({"kind": "pipeline", "reference": "7"}));
    assert!(uuid_like(package.body["client_package_id"].as_str().unwrap()));

    assert!(find("GET", &format!("/tasks/{tid}/questions?status=open")).path.contains("status=open"));
    assert!(find("GET", &format!("/work-packages/{qid}")).path.ends_with(&format!("/work-packages/{qid}")));
    assert!(find("GET", &format!("/tasks/{tid}/completion-status")).path.ends_with("/completion-status"));

    // The secret never appears in any URL or body.
    for r in &reqs {
        assert!(!r.path.contains(SECRET) && !r.body.to_string().contains(SECRET));
    }
    let stderr = mcp.stop();
    assert!(!stderr.contains(SECRET));
}

#[test]
fn p14_p0_tools_reject_bad_arguments_before_any_request() {
    let stub = Stub::spawn(happy_handler());
    let mut mcp = Mcp::start(&stub.url, None);
    let (err, _) = mcp.tool_json("collab_start_session", start_args());
    assert!(!err);
    let before = stub.requests().len();
    let tid = "22222222-2222-4222-8222-222222222222";
    for (tool, args) in [
        ("collab_claim_task", json!({"owner": "o", "repo": "r", "task_id": tid, "reason": "x"})), // no version
        ("collab_claim_task", json!({"owner": "o", "repo": "r", "task_id": tid, "expected_task_version": 1})), // no reason
        ("collab_ask_question", json!({"owner": "o", "repo": "r", "task_id": tid, "title": "t", "body": "b"})), // no recipient
        ("collab_ask_question", json!({"owner": "o", "repo": "r", "task_id": tid, "recipient": "everyone", "title": "t", "body": "b"})),
        ("collab_answer_question", json!({"owner": "o", "repo": "r", "question_id": "q", "expected_version": 1})), // no hash
        ("collab_get_question", json!({"owner": "o", "repo": "r", "question_id": "../x"})),
        ("collab_get_work_package", json!({"owner": "o", "repo": "r", "package_id": "a/b"})),
    ] {
        let (err, out) = mcp.tool_json(tool, args);
        assert!(err, "{tool} should be rejected: {out}");
    }
    assert_eq!(stub.requests().len(), before, "no request left the process");
    mcp.stop();
}

#[test]
fn p14_coded_conflicts_reach_the_model_with_their_details() {
    let handler: Handler = Box::new(|req| {
        if req.method == "POST" && req.path.ends_with("/collaboration/sessions") {
            (201, json!({"session": {"id": SESSION_ID, "state": "online"}, "session_secret": SECRET}).to_string())
        } else {
            (
                409,
                json!({"error": {"code": "OWNER_MAY_BE_ACTIVE", "message": "owner not gone",
                    "details": {"owner_state": "offline"}}})
                .to_string(),
            )
        }
    });
    let stub = Stub::spawn(handler);
    let mut mcp = Mcp::start(&stub.url, None);
    mcp.tool_json("collab_start_session", start_args());
    let (err, out) = mcp.tool_json(
        "collab_claim_task",
        json!({"owner": "o", "repo": "r", "task_id": "22222222-2222-4222-8222-222222222222", "expected_task_version": 1, "reason": "r"}),
    );
    assert!(err);
    assert_eq!(out["error"]["status"], 409);
    assert_eq!(out["error"]["code"], "OWNER_MAY_BE_ACTIVE");
    assert_eq!(out["error"]["details"]["owner_state"], "offline");
    mcp.stop();
}

// ── decision tools (U02) ─────────────────────────────────────────────

#[test]
fn u02_decision_tools_map_to_the_rest_api_and_reject_bad_arguments_locally() {
    let stub = Stub::spawn(happy_handler());
    let mut mcp = Mcp::start(&stub.url, None);
    let (err, _) = mcp.tool_json("collab_start_session", start_args());
    assert!(!err);
    let tid = "22222222-2222-4222-8222-222222222222";
    let did = "44444444-4444-4444-8444-444444444444";
    let repl = "55555555-5555-4555-8555-555555555555";
    let args = |extra: Value| {
        let mut args = json!({"owner": "o", "repo": "r"});
        args.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        args
    };

    for (tool, a) in [
        ("collab_create_decision", args(json!({"task_id": tid, "title": "Use PG", "rationale": "writers", "question_id": did, "context": {"task_version": 2}}))),
        ("collab_list_decisions", args(json!({"task_id": tid, "limit": 5}))),
        ("collab_get_decision", args(json!({"decision_id": did}))),
        ("collab_change_decision", args(json!({"decision_id": did, "kind": "superseded", "replacement_decision_id": repl, "reason": "newer"}))),
    ] {
        let (err, out) = mcp.tool(tool, a);
        assert!(!err, "{tool}: {out}");
        assert!(!out.contains(SECRET), "{tool} leaked the secret");
    }
    let reqs = stub.requests();
    let find = |method: &str, suffix: &str| -> Req {
        reqs.iter()
            .find(|r| r.method == method && r.path.contains(suffix))
            .unwrap_or_else(|| panic!("no {method} …{suffix}"))
            .clone()
    };
    let session = |r: &Req| {
        assert_eq!(r.headers.get("x-ironforge-session").map(String::as_str), Some(SESSION_ID));
        assert_eq!(r.headers.get("x-ironforge-session-secret").map(String::as_str), Some(SECRET));
    };
    let create = find("POST", &format!("/tasks/{tid}/decisions"));
    session(&create);
    assert_eq!(create.body["question_id"], did);
    assert_eq!(create.body["context"]["task_version"], 2);
    assert!(uuid_like(create.body["client_decision_id"].as_str().unwrap()), "idempotency key generated");
    assert!(find("GET", &format!("/tasks/{tid}/decisions?limit=5")).path.contains("limit=5"));
    assert!(find("GET", &format!("/decisions/{did}")).path.ends_with(&format!("/decisions/{did}")));
    let changed = find("POST", &format!("/decisions/{did}/changes"));
    session(&changed);
    assert_eq!(changed.body["kind"], "superseded");
    assert_eq!(changed.body["replacement_decision_id"], repl);
    assert!(uuid_like(changed.body["client_change_id"].as_str().unwrap()));
    for r in &reqs {
        assert!(!r.path.contains(SECRET) && !r.body.to_string().contains(SECRET));
    }

    let before = stub.requests().len();
    for (tool, a) in [
        ("collab_create_decision", args(json!({"task_id": tid, "title": "t"}))), // no rationale
        ("collab_create_decision", args(json!({"task_id": "../x", "title": "t", "rationale": "r"}))),
        ("collab_change_decision", args(json!({"decision_id": did, "kind": "revoked"}))), // no reason
        ("collab_change_decision", args(json!({"decision_id": did, "reason": "r"}))), // no kind
        ("collab_get_decision", args(json!({"decision_id": "a/b"}))),
    ] {
        let (err, out) = mcp.tool_json(tool, a);
        assert!(err, "{tool} should be rejected: {out}");
    }
    assert_eq!(stub.requests().len(), before, "no request left the process");
    mcp.stop();
}
