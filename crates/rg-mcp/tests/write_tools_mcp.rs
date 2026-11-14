//! The write and CI tools (`commit_file`, `delete_file`, `create_pr`,
//! `list_pipelines`, `wait_pipeline`): the real `ironforge-mcp` binary over
//! stdio, against a recording stub of the IronForge REST API.
//!
//! The stub shows exactly which route and body each tool sends. The behaviour of
//! the routes themselves (dry run, `expected_sha`, branch limits) is covered by
//! the `rg-http` tests; the end-to-end run against a real server is recorded in
//! the verification note.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};

/// The full commit id the stub branch is at, and one it has moved past.
const TIP: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const STALE: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

#[derive(Clone, Debug)]
struct Req {
    method: String,
    path: String,
    body: String,
    idempotency_key: Option<String>,
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

    fn last(&self) -> Req {
        self.requests.lock().unwrap().last().cloned().unwrap()
    }

    fn count(&self) -> usize {
        self.requests.lock().unwrap().len()
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
        body: String::from_utf8_lossy(&raw).into_owned(),
        idempotency_key: headers.get("idempotency-key").cloned(),
    };
    let (status, text) = handler(&req);
    log.lock().unwrap().push(req);
    let mut stream = stream;
    let _ = write!(
        stream,
        "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}",
        text.len()
    );
}

fn api() -> Handler {
    Box::new(|req| {
        let (path, _query) = req.path.split_once('?').unwrap_or((req.path.as_str(), ""));
        match (req.method.as_str(), path) {
            ("POST", "/api/v1/repos/alice/demo/contents/src/main.rs") => {
                let body: Value = serde_json::from_str(&req.body).unwrap_or(Value::Null);
                if body["expected_sha"] == "stale" {
                    (
                        409,
                        json!({"error": {"code": "CONFLICT", "reason": "REF_ADVANCED",
                            "details": {"current_sha": "abc123", "expected_sha": "stale"}}})
                        .to_string(),
                    )
                } else if body["dry_run"] == true {
                    (200, json!({"dry_run": true, "would": []}).to_string())
                } else {
                    (
                        200,
                        json!({"success": true, "commit_sha": "c0ffee"}).to_string(),
                    )
                }
            }
            ("POST", "/api/v1/repos/alice/demo/commits") => {
                let body: Value = serde_json::from_str(&req.body).unwrap_or(Value::Null);
                if body["expected_sha"] == "stale" {
                    (
                        409,
                        json!({"error": {"code": "CONFLICT", "reason": "REF_ADVANCED",
                            "details": {"current_sha": "abc123", "expected_sha": "stale"}}})
                        .to_string(),
                    )
                } else if body["dry_run"] == true {
                    (200, json!({"dry_run": true, "would": []}).to_string())
                } else {
                    (
                        200,
                        json!({"dry_run": false, "commit_sha": "b47c4", "files": []}).to_string(),
                    )
                }
            }
            ("DELETE", "/api/v1/repos/alice/demo/contents/old.txt") => {
                (200, json!({"success": true}).to_string())
            }
            ("DELETE", "/api/v1/repos/alice/demo/branches/agent/x") => {
                (200, json!({"dry_run": true}).to_string())
            }
            ("POST", "/api/v1/repos/alice/demo/pulls") => {
                (201, json!({"number": 4, "state": "open"}).to_string())
            }
            ("GET", "/api/v1/repos/alice/demo/pipelines") => (
                200,
                json!({"data": [{"id": 9, "status": "running"}]}).to_string(),
            ),
            ("GET", "/api/v1/repos/alice/demo/pulls/3/export") => (
                200,
                json!({"format": "ironforge.pull_request_export", "format_version": 1}).to_string(),
            ),
            ("GET", "/api/v1/ai/repos/alice/demo/pulls/3/context") => (
                200,
                json!({"pull_request": {"number": 3}, "truncated": []}).to_string(),
            ),
            ("GET", "/api/v1/repos/alice/demo/events") => (
                200,
                json!({"events": [{"id": 8, "type": "issue.opened"}], "next_cursor": 8,
                       "timed_out": false})
                .to_string(),
            ),
            ("GET", "/api/v1/repos/alice/demo/pipelines/9/wait") => (
                200,
                json!({"terminal": true, "timed_out": false, "status": "failed"}).to_string(),
            ),
            ("POST", "/api/v1/repos/alice/demo/commits/abc1234/cherry-pick") => {
                pick_answer(req, "cherry_pick")
            }
            ("POST", "/api/v1/repos/alice/demo/commits/abc1234/revert") => {
                pick_answer(req, "revert")
            }
            ("POST", "/api/v1/repos/alice/demo/commits/deadbeef/cherry-pick") => {
                merge_conflict(req, "cherry_pick")
            }
            ("POST", "/api/v1/repos/alice/demo/commits/deadbeef/revert") => {
                merge_conflict(req, "revert")
            }
            _ => (404, json!({"error": {"code": "NOT_FOUND"}}).to_string()),
        }
    })
}

/// The answer a cherry-pick or revert route gives: a stale `expected_sha` is a
/// 409 `REF_ADVANCED`, `dry_run` writes nothing, anything else lands.
fn pick_answer(req: &Req, operation: &str) -> (u16, String) {
    let body: Value = serde_json::from_str(&req.body).unwrap_or(Value::Null);
    if body["expected_sha"] == STALE {
        return (
            409,
            json!({"error": {"code": "CONFLICT", "reason": "REF_ADVANCED",
                "details": {"current_sha": TIP, "expected_sha": STALE}}})
            .to_string(),
        );
    }
    if body["dry_run"] == true {
        return (
            200,
            json!({"dry_run": true, "operation": operation, "would": []}).to_string(),
        );
    }
    (
        200,
        json!({"dry_run": false, "operation": operation, "branch": body["branch"],
               "tip_before": TIP, "commit_sha": "c0ffee", "source_sha": "abc1234"})
        .to_string(),
    )
}

fn merge_conflict(req: &Req, operation: &str) -> (u16, String) {
    let body: Value = serde_json::from_str(&req.body).unwrap_or(Value::Null);
    (
        409,
        json!({"error": {"code": "CONFLICT", "reason": "MERGE_CONFLICT",
            "message": "the change does not apply cleanly",
            "details": {"operation": operation, "source_sha": "deadbeef",
                        "tip_sha": TIP, "paths": ["src/main.rs"],
                        "expected_sha": body["expected_sha"]}}})
        .to_string(),
    )
}

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
        serde_json::from_str(&out).unwrap_or_else(|_| panic!("bad response {out:?}"))
    }

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

#[test]
fn the_write_tools_are_advertised_with_their_required_arguments() {
    let stub = Stub::spawn(api());
    let mut mcp = Mcp::start(&stub.url);
    let resp = mcp.rpc("tools/list", json!({}));
    let tools = resp["result"]["tools"].as_array().unwrap();
    let find = |name: &str| tools.iter().find(|t| t["name"] == name).unwrap();
    for (name, required) in [
        (
            "commit_file",
            vec!["owner", "repo", "path", "content", "message", "branch"],
        ),
        (
            "commit_files",
            vec![
                "owner",
                "repo",
                "branch",
                "expected_sha",
                "message",
                "files",
            ],
        ),
        (
            "delete_file",
            vec!["owner", "repo", "path", "message", "branch", "sha"],
        ),
        (
            "cherry_pick_commit",
            vec!["owner", "repo", "sha", "branch", "expected_sha"],
        ),
        (
            "revert_commit",
            vec!["owner", "repo", "sha", "branch", "expected_sha"],
        ),
        ("delete_branch", vec!["owner", "repo", "branch"]),
        ("create_pr", vec!["owner", "repo", "title", "head", "base"]),
        ("list_pipelines", vec!["owner", "repo"]),
        ("wait_pipeline", vec!["owner", "repo", "pipeline_id"]),
        ("wait_repo_events", vec!["owner", "repo"]),
        ("get_pr_context", vec!["owner", "repo", "number"]),
    ] {
        let schema = &find(name)["inputSchema"];
        let got: Vec<&str> = schema["required"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(got, required, "{name}");
    }
    // The file tools offer the safety arguments.
    for name in [
        "commit_file",
        "commit_files",
        "delete_file",
        "delete_branch",
        "cherry_pick_commit",
        "revert_commit",
    ] {
        let props = &find(name)["inputSchema"]["properties"];
        assert!(
            props.get("expected_sha").is_some() && props.get("dry_run").is_some(),
            "{name}"
        );
    }
    // A pick takes a mainline for merge commits and is replayable.
    for name in ["cherry_pick_commit", "revert_commit"] {
        let props = &find(name)["inputSchema"]["properties"];
        assert_eq!(props["mainline"]["minimum"], 1, "{name}");
        assert!(props.get("idempotency_key").is_some(), "{name}");
    }
}

#[test]
fn commit_file_sends_the_branch_and_the_safety_arguments() {
    let stub = Stub::spawn(api());
    let mut mcp = Mcp::start(&stub.url);

    let (err, text) = mcp.tool(
        "commit_file",
        json!({"owner": "alice", "repo": "demo", "path": "src/main.rs",
               "content": "fn main() {}\n", "message": "add main", "branch": "agent/x",
               "sha": "blob1", "expected_sha": "tip1"}),
    );
    assert!(!err, "{text}");
    assert_eq!(
        serde_json::from_str::<Value>(&text).unwrap()["commit_sha"],
        "c0ffee"
    );
    let sent = stub.last();
    assert_eq!(sent.method, "POST");
    assert_eq!(sent.path, "/api/v1/repos/alice/demo/contents/src/main.rs");
    let body: Value = serde_json::from_str(&sent.body).unwrap();
    assert_eq!(body["branch"], "agent/x");
    assert_eq!(body["sha"], "blob1");
    assert_eq!(body["expected_sha"], "tip1");
    assert!(body.get("dry_run").is_none());

    // dry_run is passed through; an empty file is allowed.
    let (err, text) = mcp.tool(
        "commit_file",
        json!({"owner": "alice", "repo": "demo", "path": "src/main.rs",
               "content": "", "message": "m", "branch": "agent/x", "dry_run": true}),
    );
    assert!(!err, "{text}");
    assert_eq!(
        serde_json::from_str::<Value>(&text).unwrap()["dry_run"],
        true
    );
    let body: Value = serde_json::from_str(&stub.last().body).unwrap();
    assert_eq!(body["dry_run"], true);
    assert_eq!(body["content"], "");
}

#[test]
fn commit_files_sends_one_request_with_every_change() {
    let stub = Stub::spawn(api());
    let mut mcp = Mcp::start(&stub.url);
    let files = json!([
        {"operation": "create", "path": "a.txt", "content": "a\n"},
        {"operation": "update", "path": "b.txt", "content": "", "sha": "blobB"},
        {"operation": "delete", "path": "c.txt", "sha": "blobC"},
        {"operation": "move", "from": "d.txt", "to": "e/d.txt", "sha": "blobD"},
    ]);
    let before = stub.count();
    let (err, text) = mcp.tool(
        "commit_files",
        json!({"owner": "alice", "repo": "demo", "branch": "agent/x", "expected_sha": "tip1",
               "message": "four changes", "files": files, "idempotency_key": "k-1"}),
    );
    assert!(!err, "{text}");
    assert_eq!(
        serde_json::from_str::<Value>(&text).unwrap()["commit_sha"],
        "b47c4"
    );
    // Four changes, one request: one commit, one CI run on the server.
    assert_eq!(stub.count() - before, 1);
    let sent = stub.last();
    assert_eq!(sent.method, "POST");
    assert_eq!(sent.path, "/api/v1/repos/alice/demo/commits");
    assert_eq!(sent.idempotency_key.as_deref(), Some("k-1"));
    let body: Value = serde_json::from_str(&sent.body).unwrap();
    assert_eq!(body["branch"], "agent/x");
    assert_eq!(body["expected_sha"], "tip1");
    assert_eq!(body["message"], "four changes");
    assert_eq!(body["files"], files);
    assert!(body.get("dry_run").is_none());

    let (err, text) = mcp.tool(
        "commit_files",
        json!({"owner": "alice", "repo": "demo", "branch": "agent/x", "expected_sha": "tip1",
               "message": "m", "dry_run": true,
               "files": [{"operation": "create", "path": "a.txt", "content": "a"}]}),
    );
    assert!(!err, "{text}");
    assert_eq!(
        serde_json::from_str::<Value>(&text).unwrap()["dry_run"],
        true
    );
    let body: Value = serde_json::from_str(&stub.last().body).unwrap();
    assert_eq!(body["dry_run"], true);
}

#[test]
fn commit_files_refusals_keep_the_status_and_reason() {
    let stub = Stub::spawn(api());
    let mut mcp = Mcp::start(&stub.url);
    let (err, text) = mcp.tool(
        "commit_files",
        json!({"owner": "alice", "repo": "demo", "branch": "main", "expected_sha": "stale",
               "message": "m", "files": [{"operation": "create", "path": "a", "content": "x"}]}),
    );
    assert!(err);
    assert!(text.starts_with("Error: HTTP 409"), "{text}");
    assert!(
        text.contains("REF_ADVANCED") && text.contains("abc123"),
        "{text}"
    );
}

#[test]
fn commit_files_checks_its_arguments_before_any_request() {
    let stub = Stub::spawn(api());
    let mut mcp = Mcp::start(&stub.url);
    let one = json!([{"operation": "create", "path": "a", "content": "x"}]);
    for (args, wants) in [
        // branch and expected_sha are both required: no fallback to HEAD.
        (
            json!({"owner": "alice", "repo": "demo", "expected_sha": "t", "message": "m", "files": one}),
            "branch is required",
        ),
        (
            json!({"owner": "alice", "repo": "demo", "branch": "b", "message": "m", "files": one}),
            "expected_sha is required",
        ),
        (
            json!({"owner": "alice", "repo": "demo", "branch": "b", "expected_sha": "t", "files": one}),
            "message is required",
        ),
        (
            json!({"owner": "alice", "repo": "demo", "branch": "b", "expected_sha": "t", "message": "m"}),
            "files must hold at least one entry",
        ),
        (
            json!({"owner": "alice", "repo": "demo", "branch": "b", "expected_sha": "t", "message": "m", "files": []}),
            "files must hold at least one entry",
        ),
        (
            json!({"owner": "alice", "repo": "demo", "branch": "b", "expected_sha": "t", "message": "m", "files": "a"}),
            "files must be an array",
        ),
        (
            json!({"owner": "alice", "repo": "demo", "branch": "b", "expected_sha": "t", "message": "m",
                   "files": [{"operation": "copy", "path": "a"}]}),
            "files[0].operation must be one of",
        ),
        (
            json!({"owner": "alice", "repo": "demo", "branch": "b", "expected_sha": "t", "message": "m",
                   "files": [{"path": "a", "content": "x"}]}),
            "files[0].operation is required",
        ),
        // An update without the blob it replaces would be unguarded.
        (
            json!({"owner": "alice", "repo": "demo", "branch": "b", "expected_sha": "t", "message": "m",
                   "files": [{"operation": "create", "path": "a", "content": "x"},
                             {"operation": "update", "path": "b", "content": "x"}]}),
            "files[1].sha is required for update",
        ),
        (
            json!({"owner": "alice", "repo": "demo", "branch": "b", "expected_sha": "t", "message": "m",
                   "files": [{"operation": "delete", "path": "a", "sha": ""}]}),
            "files[0].sha is required for delete",
        ),
        (
            json!({"owner": "alice", "repo": "demo", "branch": "b", "expected_sha": "t", "message": "m",
                   "files": [{"operation": "move", "from": "a", "sha": "s"}]}),
            "files[0].to is required for move",
        ),
        // A misspelt or foreign field is refused instead of dropped.
        (
            json!({"owner": "alice", "repo": "demo", "branch": "b", "expected_sha": "t", "message": "m",
                   "files": [{"operation": "update", "path": "a", "content": "x", "blob_sha": "s"}]}),
            "'blob_sha' is not a field of a update entry",
        ),
        (
            json!({"owner": "alice", "repo": "demo", "branch": "b", "expected_sha": "t", "message": "m",
                   "files": [{"operation": "delete", "path": "a", "sha": "s", "content": "x"}]}),
            "'content' is not a field of a delete entry",
        ),
        (
            json!({"owner": "alice", "repo": "demo", "branch": "b", "expected_sha": "t", "message": "m",
                   "files": [{"operation": "create", "path": "a", "content": 5}]}),
            "files[0].content must be a string",
        ),
        (
            json!({"owner": "al/ice", "repo": "demo", "branch": "b", "expected_sha": "t", "message": "m", "files": one}),
            "owner must be",
        ),
        (
            json!({"owner": "alice", "repo": "demo", "branch": "b", "expected_sha": "t", "message": "m",
                   "files": one, "dry_run": "yes"}),
            "dry_run must be true or false",
        ),
    ] {
        let (err, text) = mcp.tool("commit_files", args.clone());
        assert!(err, "{args}");
        assert!(text.contains(wants), "{args} -> {text}");
    }
    assert_eq!(stub.count(), 0, "nothing left the process");
}

#[test]
fn a_refusal_comes_back_as_an_error_with_the_status_and_the_reason() {
    let stub = Stub::spawn(api());
    let mut mcp = Mcp::start(&stub.url);
    let (err, text) = mcp.tool(
        "commit_file",
        json!({"owner": "alice", "repo": "demo", "path": "src/main.rs",
               "content": "x", "message": "m", "branch": "main", "expected_sha": "stale"}),
    );
    assert!(err);
    assert!(text.starts_with("Error: HTTP 409"), "{text}");
    assert!(
        text.contains("REF_ADVANCED") && text.contains("abc123"),
        "{text}"
    );
}

#[test]
fn arguments_are_checked_before_any_request_is_made() {
    let stub = Stub::spawn(api());
    let mut mcp = Mcp::start(&stub.url);
    for (name, args, wants) in [
        // The branch is required: an agent must say where it writes.
        (
            "commit_file",
            json!({"owner": "alice", "repo": "demo", "path": "a.txt", "content": "x", "message": "m"}),
            "branch is required",
        ),
        (
            "commit_file",
            json!({"owner": "alice", "repo": "demo", "path": "../etc/passwd", "content": "x",
                   "message": "m", "branch": "b"}),
            "path must not contain",
        ),
        (
            "commit_file",
            json!({"owner": "al/ice", "repo": "demo", "path": "a", "content": "x",
                   "message": "m", "branch": "b"}),
            "owner must be",
        ),
        (
            "commit_file",
            json!({"owner": "alice", "repo": "demo", "path": "a", "content": "x",
                   "message": "m", "branch": "b", "dry_run": "yes"}),
            "dry_run must be true or false",
        ),
        (
            "delete_file",
            json!({"owner": "alice", "repo": "demo", "path": "a", "message": "m", "branch": "b"}),
            "sha is required",
        ),
        (
            "delete_branch",
            json!({"owner": "alice", "repo": "demo"}),
            "branch is required",
        ),
        (
            "delete_branch",
            json!({"owner": "alice", "repo": "demo", "branch": "a/../b"}),
            "must not contain",
        ),
        (
            "create_pr",
            json!({"owner": "alice", "repo": "demo", "title": " ", "head": "h", "base": "b"}),
            "title is required",
        ),
        (
            "wait_pipeline",
            json!({"owner": "alice", "repo": "demo", "pipeline_id": 9, "timeout": 120}),
            "timeout must be between 1 and 55",
        ),
        (
            "wait_pipeline",
            json!({"owner": "alice", "repo": "demo"}),
            "pipeline_id is required",
        ),
    ] {
        let (err, text) = mcp.tool(name, args);
        assert!(err && text.contains(wants), "{name}: {text}");
    }
    assert_eq!(stub.count(), 0, "no request should have been sent");
}

#[test]
fn delete_pr_and_pipeline_tools_use_their_routes() {
    let stub = Stub::spawn(api());
    let mut mcp = Mcp::start(&stub.url);

    let (err, text) = mcp.tool(
        "delete_file",
        json!({"owner": "alice", "repo": "demo", "path": "old.txt", "message": "rm it",
               "branch": "agent/x", "sha": "blob9", "expected_sha": "tip9", "dry_run": true}),
    );
    assert!(!err, "{text}");
    let sent = stub.last();
    assert_eq!(sent.method, "DELETE");
    assert_eq!(
        sent.path,
        "/api/v1/repos/alice/demo/contents/old.txt?branch=agent%2Fx&message=rm%20it&sha=blob9&expected_sha=tip9&dry_run=true"
    );

    let (err, text) = mcp.tool(
        "delete_branch",
        json!({"owner": "alice", "repo": "demo", "branch": "agent/x", "expected_sha": "tip9", "dry_run": true}),
    );
    assert!(!err, "{text}");
    let sent = stub.last();
    assert_eq!(sent.method, "DELETE");
    assert_eq!(
        sent.path,
        "/api/v1/repos/alice/demo/branches/agent/x?expected_sha=tip9&dry_run=true"
    );

    let (err, text) = mcp.tool(
        "create_pr",
        json!({"owner": "alice", "repo": "demo", "title": "Fix", "head": "agent/x",
               "base": "main", "body": "details", "draft": true}),
    );
    assert!(!err, "{text}");
    assert_eq!(stub.last().path, "/api/v1/repos/alice/demo/pulls");
    let body: Value = serde_json::from_str(&stub.last().body).unwrap();
    assert_eq!(body["head"], "agent/x");
    assert_eq!(body["draft"], true);
    assert_eq!(body["body"], "details");

    let (err, text) = mcp.tool("list_pipelines", json!({"owner": "alice", "repo": "demo"}));
    assert!(!err && text.contains("running"), "{text}");
    assert_eq!(stub.last().path, "/api/v1/repos/alice/demo/pipelines");

    let (err, text) = mcp.tool(
        "wait_pipeline",
        json!({"owner": "alice", "repo": "demo", "pipeline_id": 9, "timeout": 20}),
    );
    assert!(!err && text.contains("failed"), "{text}");
    assert_eq!(
        stub.last().path,
        "/api/v1/repos/alice/demo/pipelines/9/wait?timeout=20&until=terminal"
    );
}

#[test]
fn wait_repo_events_starts_from_now_and_then_follows_the_cursor() {
    let stub = Stub::spawn(api());
    let mut mcp = Mcp::start(&stub.url);

    // No cursor: just ask where the log ends (no waiting parameters).
    let (err, text) = mcp.tool(
        "wait_repo_events",
        json!({"owner": "alice", "repo": "demo"}),
    );
    assert!(!err, "{text}");
    assert_eq!(stub.last().path, "/api/v1/repos/alice/demo/events");

    // With a cursor it waits, filters and limits.
    let (err, text) = mcp.tool(
        "wait_repo_events",
        json!({"owner": "alice", "repo": "demo", "since": 7, "types": "issue.opened,push",
               "limit": 50, "timeout": 20}),
    );
    assert!(!err && text.contains("issue.opened"), "{text}");
    assert_eq!(
        stub.last().path,
        "/api/v1/repos/alice/demo/events?since=7&types=issue.opened%2Cpush&limit=50&timeout=20"
    );
    let before = stub.count();
    for (args, wants) in [
        (
            json!({"owner": "alice", "repo": "demo", "since": -1}),
            "since must be",
        ),
        (
            json!({"owner": "alice", "repo": "demo", "since": 1, "timeout": 0}),
            "timeout must be",
        ),
        (
            json!({"owner": "alice", "repo": "demo", "since": 1, "timeout": 56}),
            "timeout must be",
        ),
        (
            json!({"owner": "alice", "repo": "demo", "limit": 500}),
            "limit must be",
        ),
    ] {
        let (err, text) = mcp.tool("wait_repo_events", args);
        assert!(err && text.contains(wants), "{text}");
    }
    assert_eq!(stub.count(), before, "bad arguments send no request");
}

#[test]
fn an_idempotency_key_is_sent_as_the_header_and_checked_first() {
    let stub = Stub::spawn(api());
    let mut mcp = Mcp::start(&stub.url);
    let (err, text) = mcp.tool(
        "create_pr",
        json!({"owner": "alice", "repo": "demo", "title": "Fix", "head": "agent/x",
               "base": "main", "idempotency_key": "agent-7:fix-pr"}),
    );
    assert!(!err, "{text}");
    assert_eq!(
        stub.last().idempotency_key.as_deref(),
        Some("agent-7:fix-pr")
    );
    // The key is not part of the body.
    let body: Value = serde_json::from_str(&stub.last().body).unwrap();
    assert!(body.get("idempotency_key").is_none());

    // Without one, no header is sent.
    mcp.tool(
        "create_pr",
        json!({"owner": "alice", "repo": "demo", "title": "Fix", "head": "h", "base": "main"}),
    );
    assert_eq!(stub.last().idempotency_key, None);

    let before = stub.count();
    for key in [
        json!("has space"),
        json!(""),
        json!(5),
        json!("k".repeat(256)),
    ] {
        let (err, text) = mcp.tool(
            "commit_file",
            json!({"owner": "alice", "repo": "demo", "path": "src/main.rs", "content": "x",
                   "message": "m", "branch": "b", "idempotency_key": key}),
        );
        assert!(err && text.contains("idempotency_key must be"), "{text}");
    }
    assert_eq!(stub.count(), before);
}

#[test]
fn get_pr_context_asks_the_ai_route_with_a_checked_budget() {
    let stub = Stub::spawn(api());
    let mut mcp = Mcp::start(&stub.url);
    let (err, text) = mcp.tool(
        "get_pr_context",
        json!({"owner": "alice", "repo": "demo", "number": 3}),
    );
    assert!(!err && text.contains("\"number\": 3"), "{text}");
    assert_eq!(
        stub.last().path,
        "/api/v1/ai/repos/alice/demo/pulls/3/context"
    );

    let (err, _) = mcp.tool(
        "get_pr_context",
        json!({"owner": "alice", "repo": "demo", "number": 3, "budget": 2000}),
    );
    assert!(!err);
    assert_eq!(
        stub.last().path,
        "/api/v1/ai/repos/alice/demo/pulls/3/context?budget=2000"
    );
    let before = stub.count();
    for args in [
        json!({"owner": "alice", "repo": "demo", "number": 3, "budget": 10}),
        json!({"owner": "alice", "repo": "demo", "number": 3, "budget": 60000}),
        json!({"owner": "alice", "repo": "demo", "number": 0}),
    ] {
        let (err, _) = mcp.tool("get_pr_context", args);
        assert!(err);
    }
    assert_eq!(stub.count(), before, "bad arguments send no request");
}

#[test]
fn create_pr_can_be_checked_first_and_a_pr_can_be_exported() {
    let stub = Stub::spawn(api());
    let mut mcp = Mcp::start(&stub.url);

    let (err, text) = mcp.tool(
        "create_pr",
        json!({"owner": "alice", "repo": "demo", "title": "Fix", "head": "agent/x",
               "base": "main", "dry_run": true}),
    );
    assert!(!err, "{text}");
    let body: Value = serde_json::from_str(&stub.last().body).unwrap();
    assert_eq!(body["dry_run"], true);

    // Without the flag the body carries none.
    mcp.tool(
        "create_pr",
        json!({"owner": "alice", "repo": "demo", "title": "Fix", "head": "h", "base": "main"}),
    );
    let body: Value = serde_json::from_str(&stub.last().body).unwrap();
    assert!(body.get("dry_run").is_none());

    let (err, text) = mcp.tool(
        "export_pr",
        json!({"owner": "alice", "repo": "demo", "number": 3}),
    );
    assert!(!err, "{text}");
    assert_eq!(
        serde_json::from_str::<Value>(&text).unwrap()["format_version"],
        1
    );
    assert_eq!(stub.last().path, "/api/v1/repos/alice/demo/pulls/3/export");
}

#[test]
fn cherry_pick_commit_sends_the_route_and_the_guarded_body() {
    let stub = Stub::spawn(api());
    let mut mcp = Mcp::start(&stub.url);

    let (err, text) = mcp.tool(
        "cherry_pick_commit",
        json!({"owner": "alice", "repo": "demo", "sha": "abc1234", "branch": "agent/x",
               "expected_sha": TIP, "message": "bring the fix", "idempotency_key": "pick-1"}),
    );
    assert!(!err, "{text}");
    assert_eq!(
        serde_json::from_str::<Value>(&text).unwrap()["operation"],
        "cherry_pick"
    );
    let sent = stub.last();
    assert_eq!(sent.method, "POST");
    assert_eq!(
        sent.path,
        "/api/v1/repos/alice/demo/commits/abc1234/cherry-pick"
    );
    assert_eq!(sent.idempotency_key.as_deref(), Some("pick-1"));
    let body: Value = serde_json::from_str(&sent.body).unwrap();
    assert_eq!(body["branch"], "agent/x");
    assert_eq!(body["expected_sha"], TIP);
    assert_eq!(body["message"], "bring the fix");
    // Not asked for: neither is sent, and the key stays in the header.
    assert!(body.get("mainline").is_none());
    assert!(body.get("dry_run").is_none());
    assert!(body.get("idempotency_key").is_none());
}

#[test]
fn revert_commit_uses_its_route_and_passes_mainline_and_dry_run() {
    let stub = Stub::spawn(api());
    let mut mcp = Mcp::start(&stub.url);

    let (err, text) = mcp.tool(
        "revert_commit",
        json!({"owner": "alice", "repo": "demo", "sha": "abc1234", "branch": "agent/x",
               "expected_sha": TIP, "mainline": 2, "dry_run": true}),
    );
    assert!(!err, "{text}");
    let answer: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(answer["operation"], "revert");
    assert_eq!(answer["dry_run"], true);
    let sent = stub.last();
    assert_eq!(sent.method, "POST");
    assert_eq!(sent.path, "/api/v1/repos/alice/demo/commits/abc1234/revert");
    let body: Value = serde_json::from_str(&sent.body).unwrap();
    assert_eq!(body["branch"], "agent/x");
    assert_eq!(body["expected_sha"], TIP);
    assert_eq!(body["mainline"], 2);
    assert_eq!(body["dry_run"], true);
    assert!(body.get("message").is_none());
}

#[test]
fn pick_refusals_keep_the_status_and_the_details() {
    let stub = Stub::spawn(api());
    let mut mcp = Mcp::start(&stub.url);

    let (err, text) = mcp.tool(
        "cherry_pick_commit",
        json!({"owner": "alice", "repo": "demo", "sha": "deadbeef", "branch": "agent/x",
               "expected_sha": TIP}),
    );
    assert!(err);
    assert!(text.starts_with("Error: HTTP 409"), "{text}");
    assert!(
        text.contains("MERGE_CONFLICT") && text.contains("src/main.rs"),
        "{text}"
    );

    // A branch that moved is refused with the commit it is at now.
    let (err, text) = mcp.tool(
        "revert_commit",
        json!({"owner": "alice", "repo": "demo", "sha": "abc1234", "branch": "agent/x",
               "expected_sha": STALE}),
    );
    assert!(err);
    assert!(text.starts_with("Error: HTTP 409"), "{text}");
    assert!(
        text.contains("REF_ADVANCED") && text.contains(TIP),
        "{text}"
    );
}

#[test]
fn pick_tools_check_their_arguments_before_any_request() {
    let stub = Stub::spawn(api());
    let mut mcp = Mcp::start(&stub.url);
    let base = json!({"owner": "alice", "repo": "demo", "sha": "abc1234", "branch": "agent/x",
                      "expected_sha": TIP});
    let cases = [
        // The commit to apply is required and is a plain hex id, never a revspec.
        (
            json!({"owner": "alice", "repo": "demo", "branch": "b", "expected_sha": TIP}),
            "sha is required",
        ),
        (
            json!({"owner": "alice", "repo": "demo", "sha": "HEAD~1", "branch": "b",
                "expected_sha": TIP}),
            "sha must be a hex commit id of 4-64 characters",
        ),
        (
            json!({"owner": "alice", "repo": "demo", "sha": "abc", "branch": "b",
                "expected_sha": TIP}),
            "sha must be a hex commit id of 4-64 characters",
        ),
        (
            json!({"owner": "alice", "repo": "demo", "sha": "../../x", "branch": "b",
                "expected_sha": TIP}),
            "sha must be a hex commit id of 4-64 characters",
        ),
        // branch has no default and is never created here.
        (
            json!({"owner": "alice", "repo": "demo", "sha": "abc1234", "expected_sha": TIP}),
            "branch is required",
        ),
        // expected_sha is required and must be the full id the branch is at.
        (
            json!({"owner": "alice", "repo": "demo", "sha": "abc1234", "branch": "b"}),
            "expected_sha is required",
        ),
        (
            json!({"owner": "alice", "repo": "demo", "sha": "abc1234", "branch": "b",
                "expected_sha": "abc1234"}),
            "expected_sha must be a full commit id",
        ),
        (
            json!({"owner": "alice", "repo": "demo", "sha": "abc1234", "branch": "b",
                "expected_sha": "zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz"}),
            "expected_sha must be a full commit id",
        ),
        // mainline is a 1-based parent number.
        (
            json!({"owner": "alice", "repo": "demo", "sha": "abc1234", "branch": "b",
                "expected_sha": TIP, "mainline": 0}),
            "mainline must be a positive integer",
        ),
        (
            json!({"owner": "alice", "repo": "demo", "sha": "abc1234", "branch": "b",
                "expected_sha": TIP, "mainline": "2"}),
            "mainline must be a positive integer",
        ),
        // A blank message is refused by the server; say so before sending.
        (
            json!({"owner": "alice", "repo": "demo", "sha": "abc1234", "branch": "b",
                "expected_sha": TIP, "message": "   "}),
            "message must not be blank",
        ),
        (
            json!({"owner": "alice", "repo": "demo", "sha": "abc1234", "branch": "b",
                "expected_sha": TIP, "dry_run": "yes"}),
            "dry_run must be true or false",
        ),
        (
            json!({"owner": "al/ice", "repo": "demo", "sha": "abc1234", "branch": "b",
                "expected_sha": TIP}),
            "owner must be",
        ),
        (
            json!({"owner": "alice", "repo": "demo", "sha": "abc1234", "branch": "b",
                "expected_sha": TIP, "idempotency_key": "has space"}),
            "idempotency_key must be",
        ),
    ];
    for name in ["cherry_pick_commit", "revert_commit"] {
        for (args, wants) in &cases {
            let (err, text) = mcp.tool(name, args.clone());
            assert!(err, "{name}: {args}");
            assert!(text.contains(wants), "{name}: {args} -> {text}");
        }
        // The valid shape is accepted: this is the only case that leaves the process.
        let (err, text) = mcp.tool(name, base.clone());
        assert!(!err, "{name}: {text}");
    }
    // Exactly the two valid calls reached the stub.
    assert_eq!(stub.count(), 2, "bad arguments send no request");
}
