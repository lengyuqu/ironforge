//! Transport-independent request dispatch.
//!
//! Both transports (stdio in `main.rs`, streamable HTTP in [`crate::remote`])
//! hand one parsed JSON-RPC request to [`dispatch`], so the tool set, the
//! argument checks and every call into the REST API are the same code.

use crate::protocol::*;
use crate::AppState;

/// Protocol version the stdio transport answers `initialize` with.
pub const STDIO_PROTOCOL_VERSION: &str = "2024-11-05";

pub fn dispatch(state: &AppState, req: &JsonRpcRequest) -> JsonRpcResponse {
    match req.method.as_str() {
        "initialize" => handle_initialize(req, STDIO_PROTOCOL_VERSION),
        "notifications/initialized" => make_success(req.id.clone(), serde_json::json!({})),
        "ping" => make_success(req.id.clone(), serde_json::json!({})),
        "tools/list" => crate::tools::list_tools(state, req),
        "tools/call" => crate::tools::call_tool(state, req),
        "resources/list" => crate::resources::list_resources(state, req),
        "resources/read" => crate::resources::read_resource(state, req),
        "notifications/cancelled" => make_success(req.id.clone(), serde_json::json!({})),
        _ => make_error(
            req.id.clone(),
            -32601,
            &format!("method not found: {}", req.method),
        ),
    }
}

/// The `initialize` result for `protocol_version`.
pub fn handle_initialize(req: &JsonRpcRequest, protocol_version: &str) -> JsonRpcResponse {
    let result = serde_json::json!({
        "protocolVersion": protocol_version,
        "serverInfo": {
            "name": "ironforge-mcp",
            "version": "0.1.0"
        },
        "capabilities": {
            "tools": { "listChanged": true },
            "resources": { "subscribe": false, "listChanged": true }
        }
    });
    make_success(req.id.clone(), result)
}
