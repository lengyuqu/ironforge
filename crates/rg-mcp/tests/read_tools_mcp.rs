//! The five read tools (`list_repos`, `read_file`, `read_dir`, `get_issue`,
//! `get_pr`): the real `ironforge-mcp` binary over stdio, against a recording
//! stub of the IronForge REST API.
//!
//! The stub shows exactly which route each tool calls. Two of them used to ask
//! for routes that do not exist (the server answered with its web page and a
//! 200, or with 405), and every failure came back as a normal result whose text
//! began with "Error:". These tests pin the routes, the argument checks and the
//! `isError` flag. Behaviour against the real server is covered by the manual
//! end-to-end run recorded in the verification note.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};

// ── stub REST API ─────────────────────────────────────────────────────

#[derive(Clone, Debug)]
struct Req {
    method: String,
    path: String,
    headers: HashMap<String, String>,
}

type Handler = Box<dyn Fn(&Req) -> (u16, String, &'static str) + Send + Sync>;

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
        // One thread per connection: a connection that never sends a request
        // (a client that connected and went quiet) must not stop the stub from
        // answering everyone else.
        let handler = Arc::new(handler);
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let handler = handler.clone();
                let log = log.clone();
                std::thread::spawn(move || serve(stream, &handler, &log));
            }
        });
        Stub { url, requests }
    }

    fn paths(&self) -> Vec<String> {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .map(|r| format!("{} {}", r.method, r.path))
            .collect()
    }

    fn requests(&self) -> Vec<Req> {
        self.requests.lock().unwrap().clone()
    }
}

fn serve(stream: TcpStream, handler: &Handler, log: &Arc<Mutex<Vec<Req>>>) {
    // A peer that stops talking is dropped after a while instead of holding the
    // connection (and, before, the whole stub) forever.
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(20)));
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut line = String::new();
    if reader.read_line(&mut line).unwrap_or(0) == 0 {
        return;
    }
    let mut parts = line.split_whitespace();
    let (method, path) = (
        parts.next().unwrap_or("").to_string(),
        parts.next().unwrap_or("").to_string(),
    );
    let mut headers = HashMap::new();
    loop {
        let mut h = String::new();
        if reader.read_line(&mut h).unwrap_or(0) == 0 {
            return;
        }
        let h = h.trim_end();
        if h.is_empty() {
            break;
        }
        if let Some((k, v)) = h.split_once(':') {
            headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
        }
    }
    let len: usize = headers
        .get("content-length")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let mut raw = vec![0u8; len];
    if reader.read_exact(&mut raw).is_err() {
        return;
    }
    let req = Req {
        method,
        path,
        headers,
    };
    let (status, text, content_type) = handler(&req);
    log.lock().unwrap().push(req);
    let mut stream = stream;
    let _ = write!(
        stream,
        "HTTP/1.1 {status} X\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}",
        text.len()
    );
}

fn json_ok(value: Value) -> (u16, String, &'static str) {
    (200, value.to_string(), "application/json")
}

/// A server with the real routes for the read tools, and, like the real server,
/// the web page with a 200 for anything it does not know.
fn api() -> Handler {
    Box::new(|req| {
        if req.method != "GET" {
            return (405, String::new(), "text/plain");
        }
        let (path, _query) = req.path.split_once('?').unwrap_or((req.path.as_str(), ""));
        match path {
            "/api/v1/repos/explore" => json_ok(json!({"data": [{"name": "public-one"}]})),
            "/api/v1/repos/alice" => json_ok(json!({"data": [{"name": "demo"}]})),
            "/api/v1/repos/alice/demo/tree" => json_ok(json!({"entries": [{"name": "src"}]})),
            "/api/v1/repos/alice/demo/blob/src/main.rs" => json_ok(json!({
                "path": "src/main.rs", "sha": "abc", "size": 12,
                "content": "fn main() {}\n", "encoding": "utf-8", "is_binary": false
            })),
            "/api/v1/repos/alice/demo/blob/big.txt" => {
                let content: String = (1..=2500).map(|n| format!("line {n}\n")).collect();
                json_ok(json!({
                    "path": "big.txt", "sha": "bbb", "size": content.len(),
                    "content": content, "encoding": "utf-8", "is_binary": false
                }))
            }
            "/api/v1/repos/alice/demo/blob/logo.png" => json_ok(json!({
                "path": "logo.png", "sha": "def", "size": 2048,
                "content": "AAAA", "encoding": "base64", "is_binary": true
            })),
            "/api/v1/repos/alice/demo/blob/docs/a%20b%23c.md" => json_ok(json!({
                "path": "docs/a b#c.md", "sha": "aaa", "size": 3,
                "content": "hey", "encoding": "utf-8", "is_binary": false
            })),
            "/api/v1/repos/alice/demo/issues/7" => json_ok(json!({"number": 7, "title": "bug"})),
            "/api/v1/repos/alice/demo/pulls/3" => json_ok(json!({"number": 3, "title": "fix"})),
            "/api/v1/repos/alice/private/tree" => (
                403,
                json!({"error": {"code": "FORBIDDEN", "message": "personal access token scope denied"}})
                    .to_string(),
                "application/json",
            ),
            "/api/v1/repos/alice/demo/blob/missing.txt" => (
                404,
                json!({"error": {"code": "NOT_FOUND", "message": "path 'missing.txt' not found"}})
                    .to_string(),
                "application/json",
            ),
            _ => (200, "<!doctype html><html></html>".to_string(), "text/html"),
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
    fn start(api: &str) -> Mcp {
        let mut child = Command::new(env!("CARGO_BIN_EXE_ironforge-mcp"))
            .env("IRONFORGE_URL", api)
            .env("IRONFORGE_PAT", "pat-for-tests")
            .env_remove("IRONFORGE_COLLAB_SESSION_FILE")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let reader = BufReader::new(child.stdout.take().unwrap());
        Mcp {
            child,
            reader,
            next_id: 1,
        }
    }

    fn rpc(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        let line = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        let stdin = self.child.stdin.as_mut().unwrap();
        writeln!(stdin, "{line}").unwrap();
        stdin.flush().unwrap();
        // If the server never answers, fail with what it said instead of
        // hanging until the CI job's own deadline.
        let (done, watch) = std::sync::mpsc::channel::<()>();
        let pid = self.child.id();
        let watchdog = std::thread::spawn(move || {
            if matches!(
                watch.recv_timeout(std::time::Duration::from_secs(60)),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout)
            ) {
                let _ = Command::new("kill").arg("-9").arg(pid.to_string()).status();
                true
            } else {
                false
            }
        });
        let mut out = String::new();
        let read = self.reader.read_line(&mut out);
        drop(done);
        let killed = watchdog.join().unwrap_or(false);
        if killed || read.map(|n| n == 0).unwrap_or(true) {
            let mut stderr = String::new();
            if let Some(mut err) = self.child.stderr.take() {
                let _ = err.read_to_string(&mut stderr);
            }
            let tail: String = stderr
                .chars()
                .rev()
                .take(4000)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            panic!(
                "ironforge-mcp gave no answer to `{method}` (request id {id}, {}); its stderr ends with:\n{tail}",
                if killed { "no reply within 60s, killed" } else { "stdout closed" },
            );
        }
        let resp: Value =
            serde_json::from_str(&out).unwrap_or_else(|_| panic!("bad response {out:?}"));
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
}

impl Drop for Mcp {
    fn drop(&mut self) {
        drop(self.child.stdin.take());
        let _ = self.child.wait();
    }
}

// ── tests ───────────────────────────────────────────────────────────

#[test]
fn read_dir_asks_the_tree_route_with_query_parameters() {
    let stub = Stub::spawn(api());
    let mut mcp = Mcp::start(&stub.url);

    let (err, text) = mcp.tool("read_dir", json!({"owner": "alice", "repo": "demo"}));
    assert!(!err, "{text}");
    assert_eq!(
        serde_json::from_str::<Value>(&text).unwrap()["entries"][0]["name"],
        "src"
    );
    // The root listing sends no query at all, so the server's default branch
    // applies (it is not always `main`).
    assert_eq!(stub.paths(), ["GET /api/v1/repos/alice/demo/tree"]);

    mcp.tool(
        "read_dir",
        json!({"owner": "alice", "repo": "demo", "path": "/src/sub dir/", "ref": "feat/x"}),
    );
    assert_eq!(
        stub.paths().last().unwrap(),
        "GET /api/v1/repos/alice/demo/tree?path=src%2Fsub%20dir&ref=feat%2Fx"
    );
}

#[test]
fn read_file_asks_the_blob_route_and_returns_just_the_text() {
    let stub = Stub::spawn(api());
    let mut mcp = Mcp::start(&stub.url);

    let (err, text) = mcp.tool(
        "read_file",
        json!({"owner": "alice", "repo": "demo", "path": "src/main.rs"}),
    );
    assert!(!err, "{text}");
    assert_eq!(text, "fn main() {}\n");
    assert_eq!(
        stub.paths(),
        ["GET /api/v1/repos/alice/demo/blob/src/main.rs"]
    );

    // Each path segment is percent-encoded; a ref is only sent when given.
    let (err, text) = mcp.tool(
        "read_file",
        json!({"owner": "alice", "repo": "demo", "path": "docs/a b#c.md"}),
    );
    assert!(!err && text == "hey", "{text}");
    mcp.tool(
        "read_file",
        json!({"owner": "alice", "repo": "demo", "path": "src/main.rs", "ref": "v1.0"}),
    );
    assert_eq!(
        stub.paths().last().unwrap(),
        "GET /api/v1/repos/alice/demo/blob/src/main.rs?ref=v1.0"
    );
}

#[test]
fn a_long_file_is_returned_in_a_window_that_says_where_to_continue() {
    let stub = Stub::spawn(api());
    let mut mcp = Mcp::start(&stub.url);
    let (err, text) = mcp.tool(
        "read_file",
        json!({"owner": "alice", "repo": "demo", "path": "big.txt"}),
    );
    assert!(!err, "{text}");
    assert!(text.starts_with("line 1\nline 2\n"));
    assert!(text.contains("line 2000\n"));
    assert!(!text.contains("line 2001\n"));
    assert!(
        text.ends_with(
            "[lines 1-2000 of 2500; call read_file again with start_line=2001 to continue]"
        ),
        "{text}"
    );

    // Continuing reaches the end and says so; max_lines cannot exceed the cap.
    let (err, text) = mcp.tool(
        "read_file",
        json!({"owner": "alice", "repo": "demo", "path": "big.txt", "start_line": 2001, "max_lines": 99999}),
    );
    assert!(!err, "{text}");
    assert!(
        text.starts_with("line 2001\n") && text.ends_with("[lines 2001-2500 of 2500; end of file]"),
        "{text}"
    );

    // Bad window arguments are reported, not ignored.
    let (err, text) = mcp.tool(
        "read_file",
        json!({"owner": "alice", "repo": "demo", "path": "big.txt", "start_line": 0}),
    );
    assert!(
        err && text.contains("start_line must be a positive integer"),
        "{text}"
    );
}

#[test]
fn a_binary_file_is_reported_as_an_error_not_dumped() {
    let stub = Stub::spawn(api());
    let mut mcp = Mcp::start(&stub.url);
    let (err, text) = mcp.tool(
        "read_file",
        json!({"owner": "alice", "repo": "demo", "path": "logo.png"}),
    );
    assert!(err);
    assert!(text.starts_with("Error:") && text.contains("binary") && text.contains("2048"));
    assert!(!text.contains("AAAA"));
}

#[test]
fn list_repos_uses_the_owner_route_or_explore() {
    let stub = Stub::spawn(api());
    let mut mcp = Mcp::start(&stub.url);

    let (err, text) = mcp.tool("list_repos", json!({"owner": "alice"}));
    assert!(!err, "{text}");
    assert!(text.contains("demo"));
    let (err, text) = mcp.tool("list_repos", json!({}));
    assert!(!err, "{text}");
    assert!(text.contains("public-one"));
    assert_eq!(
        stub.paths(),
        ["GET /api/v1/repos/alice", "GET /api/v1/repos/explore"]
    );
}

#[test]
fn issues_and_pull_requests_use_their_routes() {
    let stub = Stub::spawn(api());
    let mut mcp = Mcp::start(&stub.url);
    let (err, text) = mcp.tool(
        "get_issue",
        json!({"owner": "alice", "repo": "demo", "number": 7}),
    );
    assert!(!err && text.contains("\"bug\""), "{text}");
    let (err, text) = mcp.tool(
        "get_pr",
        json!({"owner": "alice", "repo": "demo", "number": 3}),
    );
    assert!(!err && text.contains("\"fix\""), "{text}");
    assert_eq!(
        stub.paths(),
        [
            "GET /api/v1/repos/alice/demo/issues/7",
            "GET /api/v1/repos/alice/demo/pulls/3"
        ]
    );
}

#[test]
fn a_refusal_or_a_missing_file_is_a_tool_error_with_the_status() {
    let stub = Stub::spawn(api());
    let mut mcp = Mcp::start(&stub.url);

    let (err, text) = mcp.tool("read_dir", json!({"owner": "alice", "repo": "private"}));
    assert!(err, "a 403 must set isError: {text}");
    assert!(text.starts_with("Error:") && text.contains("403"), "{text}");

    let (err, text) = mcp.tool(
        "read_file",
        json!({"owner": "alice", "repo": "demo", "path": "missing.txt"}),
    );
    assert!(err && text.contains("404"), "{text}");
}

#[test]
fn an_answer_that_is_not_json_is_an_error_not_data() {
    // The real server answers an unknown route with its web page and a 200.
    let stub = Stub::spawn(api());
    let mut mcp = Mcp::start(&stub.url);
    let (err, text) = mcp.tool(
        "get_issue",
        json!({"owner": "alice", "repo": "demo", "number": 999}),
    );
    assert!(err, "{text}");
    assert!(text.contains("not JSON"), "{text}");
    assert!(!text.contains("<!doctype"));
}

#[test]
fn bad_arguments_are_refused_before_any_request_is_made() {
    let stub = Stub::spawn(api());
    let mut mcp = Mcp::start(&stub.url);
    for (tool, args) in [
        (
            "read_dir",
            json!({"owner": "alice/demo/issues?x=", "repo": "demo"}),
        ),
        ("read_dir", json!({"owner": "..", "repo": "demo"})),
        ("read_dir", json!({"owner": "alice", "repo": "de mo"})),
        (
            "read_dir",
            json!({"owner": "alice", "repo": "demo", "path": "../etc"}),
        ),
        (
            "read_file",
            json!({"owner": "alice", "repo": "demo", "path": "a/../b"}),
        ),
        ("read_file", json!({"owner": "alice", "repo": "demo"})),
        (
            "read_file",
            json!({"owner": "", "repo": "demo", "path": "x"}),
        ),
        (
            "get_issue",
            json!({"owner": "alice", "repo": "demo", "number": 0}),
        ),
        ("get_issue", json!({"owner": "alice", "repo": "demo"})),
        (
            "get_pr",
            json!({"owner": "alice", "repo": "demo", "number": -1}),
        ),
        ("list_repos", json!({"owner": "a/b"})),
    ] {
        let (err, text) = mcp.tool(tool, args.clone());
        assert!(err && text.starts_with("Error:"), "{tool} {args}: {text}");
    }
    assert!(
        stub.paths().is_empty(),
        "no request may be sent: {:?}",
        stub.paths()
    );
}

#[test]
fn the_token_is_sent_as_a_bearer_credential() {
    let stub = Stub::spawn(api());
    let mut mcp = Mcp::start(&stub.url);
    mcp.tool("read_dir", json!({"owner": "alice", "repo": "demo"}));
    let auth = stub.requests()[0]
        .headers
        .get("authorization")
        .cloned()
        .unwrap_or_default();
    assert_eq!(auth, "Bearer pat-for-tests");
}
