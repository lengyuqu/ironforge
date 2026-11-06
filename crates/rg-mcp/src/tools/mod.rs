//! MCP **Tools** – dispatched from `main.rs::dispatch`.
//!
//! Each tool:
//! 1. parses `req.params` → arguments JSON
//! 2. calls IronForge REST API
//! 3. returns `JsonRpcResponse` with `ToolCallResult`

use super::protocol::*;
use super::AppState;
use serde_json::Value;

// ── public: list tools ─────────────────────────────────

pub fn list_tools(_state: &AppState, req: &JsonRpcRequest) -> JsonRpcResponse {
    let tools = serde_json::json!([
        {
            "name": "list_repos",
            "description": "List repositories. With owner: that owner's repositories visible to the caller. Without owner: public repositories (explore).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "owner": { "type": "string", "description": "Owner (user or organization). Omit to list public repositories." }
                },
                "required": []
            }
        },
        {
            "name": "read_file",
            "description": "Read a file's content from a repository (UTF-8 text files only; a binary file is reported as an error).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "owner": { "type": "string", "description": "Repository owner" },
                    "repo":  { "type": "string", "description": "Repository name" },
                    "path":  { "type": "string", "description": "File path, e.g. 'src/main.rs'" },
                    "ref":   { "type": "string", "description": "Git ref (branch/tag/commit). Defaults to the default branch (HEAD)." }
                },
                "required": ["owner", "repo", "path"]
            }
        },
        {
            "name": "read_dir",
            "description": "List files and directories at a given path in a repository.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "owner": { "type": "string", "description": "Repository owner" },
                    "repo":  { "type": "string", "description": "Repository name" },
                    "path":  { "type": "string", "description": "Directory path ('' for repo root)" },
                    "ref":   { "type": "string", "description": "Git ref (branch/tag/commit). Defaults to the default branch (HEAD)." }
                },
                "required": ["owner", "repo"]
            }
        },
        {
            "name": "get_issue",
            "description": "Get a single issue by number.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "owner": { "type": "string", "description": "Repository owner" },
                    "repo":  { "type": "string", "description": "Repository name" },
                    "number":{ "type": "number", "description": "Issue number" }
                },
                "required": ["owner", "repo", "number"]
            }
        },
        {
            "name": "get_pr",
            "description": "Get a single pull request by number (includes diff when available).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "owner": { "type": "string", "description": "Repository owner" },
                    "repo":  { "type": "string", "description": "Repository name" },
                    "number":{ "type": "number", "description": "Pull request number" }
                },
                "required": ["owner", "repo", "number"]
            }
        }
    ]);

    let mut tools = tools;
    if let Some(list) = tools.as_array_mut() {
        list.extend(crate::collab::tool_definitions());
    }
    make_success(req.id.clone(), serde_json::json!({ "tools": tools }))
}

// ── public: call tool ─────────────────────────────────────

/// Dispatch a JSON-RPC tool call to the appropriate tool handler.
pub fn call_tool(state: &AppState, req: &JsonRpcRequest) -> JsonRpcResponse {
    let params = match &req.params {
        Some(v) => v.clone(),
        None => {
            return make_error(req.id.clone(), -32602, "missing params");
        }
    };

    let name = match params.get("name").and_then(|v| v.as_str()) {
        Some(n) => n,
        None => {
            return make_error(req.id.clone(), -32602, "missing tool name");
        }
    };

    let args = params
        .get("arguments")
        .cloned()
        .unwrap_or(serde_json::json!({}));

    if crate::collab::is_collab_tool(name) {
        let out = crate::collab::call(state, name, &args);
        return make_success(
            req.id.clone(),
            serde_json::json!({
                "content": [ { "type": "text", "text": out.text } ],
                "isError": out.is_error
            }),
        );
    }

    let result = match name {
        "list_repos" => tool_list_repos(state, &args),
        "read_file" => tool_read_file(state, &args),
        "read_dir" => tool_read_dir(state, &args),
        "get_issue" => tool_get_issue(state, &args),
        "get_pr" => tool_get_pr(state, &args),
        _ => {
            return make_error(req.id.clone(), -32601, &format!("unknown tool: {}", name));
        }
    };

    // A failure is a tool *error* (`isError: true`), not a normal result whose
    // text happens to start with "Error:": a caller that only looks at the flag
    // would otherwise take a refusal or a missing file for success.
    let (text, is_error) = match result {
        Ok(text) => (text, false),
        Err(message) => (format!("Error: {message}"), true),
    };
    let content = serde_json::json!({
        "content": [ { "type": "text", "text": text } ],
        "isError": is_error
    });
    make_success(req.id.clone(), content)
}

// ── tool implementations ─────────────────────────────────

/// What a tool returns: the text for the model, or an error message.
type ToolResult = Result<String, String>;

/// An owner or repository name: 1–100 characters of `[A-Za-z0-9._-]`, not `.`
/// or `..`. The values come from the model and are placed in a URL path, so
/// anything else (a `/`, `?`, `%` or `..`) could steer the request to a route
/// the tool was never meant to reach.
fn name_arg<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    let value = args.get(key).and_then(|v| v.as_str()).unwrap_or("");
    if value.is_empty() {
        return Err(format!("{key} is required"));
    }
    let ok = value.len() <= 100
        && value != "."
        && value != ".."
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'));
    if ok {
        Ok(value)
    } else {
        Err(format!("{key} must be 1-100 characters of [A-Za-z0-9._-]"))
    }
}

/// A positive issue or pull request number.
fn number_arg(args: &Value) -> Result<i64, String> {
    match args.get("number").and_then(|v| v.as_i64()) {
        Some(n) if n > 0 => Ok(n),
        Some(_) => Err("number must be a positive integer".into()),
        None => Err("number is required".into()),
    }
}

/// A repository path (`src/main.rs`) for a URL path: each segment is
/// percent-encoded and the separators are kept. Empty, `.` and `..` segments are
/// refused, as they never name a file and could climb out of the route.
fn repo_path_arg(args: &Value, key: &str, required: bool) -> Result<String, String> {
    let raw = args.get(key).and_then(|v| v.as_str()).unwrap_or("");
    let trimmed = raw.trim_matches('/');
    if trimmed.is_empty() {
        return if required {
            Err(format!("{key} is required"))
        } else {
            Ok(String::new())
        };
    }
    let segments: Vec<&str> = trimmed.split('/').collect();
    if segments
        .iter()
        .any(|s| s.is_empty() || *s == "." || *s == "..")
    {
        return Err(format!(
            "{key} must not contain empty, '.' or '..' path segments"
        ));
    }
    Ok(segments
        .iter()
        .map(|s| urlencoding::encode(s).into_owned())
        .collect::<Vec<_>>()
        .join("/"))
}

/// A query string of the given `name=value` pairs, skipping empty values.
fn query(pairs: &[(&str, &str)]) -> String {
    let parts: Vec<String> = pairs
        .iter()
        .filter(|(_, v)| !v.is_empty())
        .map(|(k, v)| format!("{k}={}", urlencoding::encode(v)))
        .collect();
    if parts.is_empty() {
        String::new()
    } else {
        format!("?{}", parts.join("&"))
    }
}

/// GET `api_path` and return the body, which must be JSON. An unknown route
/// answers 200 with the web app's HTML page, and that must not pass for data.
fn get_json_text(state: &AppState, api_path: &str) -> ToolResult {
    let client = crate::client::ApiClient::new(state);
    let text = tokio::runtime::Handle::current()
        .block_on(client.get_raw(api_path))
        .map_err(|e| e.to_string())?;
    if serde_json::from_str::<Value>(&text).is_err() {
        return Err(
            "the API answered with something that is not JSON (is IRONFORGE_URL the IronForge API server?)"
                .into(),
        );
    }
    Ok(text)
}

fn tool_list_repos(state: &AppState, args: &Value) -> ToolResult {
    let api_path = match args.get("owner").and_then(|v| v.as_str()) {
        Some(owner) if !owner.is_empty() => {
            format!("/repos/{}", name_arg(args, "owner")?)
        }
        _ => "/repos/explore".to_string(),
    };
    let text = get_json_text(state, &api_path)?;
    // Pretty-printed for the model; `get_json_text` already proved it parses.
    let value: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    serde_json::to_string_pretty(&value).map_err(|e| e.to_string())
}

fn tool_read_file(state: &AppState, args: &Value) -> ToolResult {
    let owner = name_arg(args, "owner")?;
    let repo = name_arg(args, "repo")?;
    let path = repo_path_arg(args, "path", true)?;
    let git_ref = args.get("ref").and_then(|v| v.as_str()).unwrap_or("");

    let api_path = format!(
        "/repos/{owner}/{repo}/blob/{path}{}",
        query(&[("ref", git_ref)])
    );
    let text = get_json_text(state, &api_path)?;
    let blob: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    if blob["is_binary"].as_bool().unwrap_or(false) {
        return Err(format!(
            "'{}' is a binary file ({} bytes); only UTF-8 text files can be read",
            blob["path"].as_str().unwrap_or(""),
            blob["size"].as_i64().unwrap_or(0)
        ));
    }
    match blob["content"].as_str() {
        Some(content) => Ok(content.to_string()),
        None => Err("the API answered without file content".into()),
    }
}

fn tool_read_dir(state: &AppState, args: &Value) -> ToolResult {
    let owner = name_arg(args, "owner")?;
    let repo = name_arg(args, "repo")?;
    // The tree route takes `path` and `ref` as query parameters, and a repository
    // path may be empty (the root). It defaults to HEAD, so `ref` is only sent
    // when given: the default branch is not always called `main`.
    let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("");
    let git_ref = args.get("ref").and_then(|v| v.as_str()).unwrap_or("");
    let path = path.trim_matches('/');
    if path.split('/').any(|s| s == "." || s == "..") {
        return Err("path must not contain '.' or '..' path segments".into());
    }
    let api_path = format!(
        "/repos/{owner}/{repo}/tree{}",
        query(&[("path", path), ("ref", git_ref)])
    );
    get_json_text(state, &api_path)
}

fn tool_get_issue(state: &AppState, args: &Value) -> ToolResult {
    let owner = name_arg(args, "owner")?;
    let repo = name_arg(args, "repo")?;
    let number = number_arg(args)?;
    get_json_text(state, &format!("/repos/{owner}/{repo}/issues/{number}"))
}

fn tool_get_pr(state: &AppState, args: &Value) -> ToolResult {
    let owner = name_arg(args, "owner")?;
    let repo = name_arg(args, "repo")?;
    let number = number_arg(args)?;
    get_json_text(state, &format!("/repos/{owner}/{repo}/pulls/{number}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn names_are_restricted_to_what_can_sit_in_a_url_path() {
        assert_eq!(
            name_arg(&json!({"owner": "alice"}), "owner").unwrap(),
            "alice"
        );
        assert_eq!(
            name_arg(&json!({"repo": "my.repo_1-x"}), "repo").unwrap(),
            "my.repo_1-x"
        );
        for bad in [
            "",
            ".",
            "..",
            "a/b",
            "a?b",
            "a#b",
            "a%2Fb",
            "a b",
            "a\\b",
            "../x",
            &"x".repeat(101),
        ] {
            assert!(
                name_arg(&json!({"owner": bad}), "owner").is_err(),
                "{bad:?}"
            );
        }
        assert!(name_arg(&json!({}), "owner").is_err());
        assert!(name_arg(&json!({"owner": 5}), "owner").is_err());
    }

    #[test]
    fn numbers_must_be_positive_integers() {
        assert_eq!(number_arg(&json!({"number": 7})).unwrap(), 7);
        for bad in [
            json!({}),
            json!({"number": 0}),
            json!({"number": -3}),
            json!({"number": "7"}),
            json!({"number": 1.5}),
        ] {
            assert!(number_arg(&bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn repository_paths_are_encoded_per_segment_and_cannot_climb() {
        let enc = |p: &str| repo_path_arg(&json!({"path": p}), "path", true);
        assert_eq!(enc("src/main.rs").unwrap(), "src/main.rs");
        assert_eq!(enc("/src/main.rs/").unwrap(), "src/main.rs");
        assert_eq!(enc("docs/a b#c?d.md").unwrap(), "docs/a%20b%23c%3Fd.md");
        assert_eq!(
            enc("日本/語.txt").unwrap(),
            "%E6%97%A5%E6%9C%AC/%E8%AA%9E.txt"
        );
        for bad in ["..", "a/../b", "./a", "a//b", "a/./b"] {
            assert!(enc(bad).is_err(), "{bad:?}");
        }
        assert!(enc("").is_err(), "required");
        assert_eq!(repo_path_arg(&json!({}), "path", false).unwrap(), "");
    }

    #[test]
    fn query_strings_skip_empty_values_and_encode_the_rest() {
        assert_eq!(query(&[("ref", ""), ("path", "")]), "");
        assert_eq!(query(&[("path", "a b/c"), ("ref", "")]), "?path=a%20b%2Fc");
        assert_eq!(
            query(&[("path", "src"), ("ref", "feat/x")]),
            "?path=src&ref=feat%2Fx"
        );
    }
}
