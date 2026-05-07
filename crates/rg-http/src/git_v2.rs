//! Git Smart Protocol V2 HTTP handling.
//!
//! Protocol V2 over HTTP uses the same endpoints as V1, but:
//! 1. Client sends `Git-Protocol: version=2` header
//! 2. Server responds with V2 capability advertisement
//! 3. Subsequent requests use V2 command format
//!
//! Reference: <https://git-scm.com/docs/protocol-v2>

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use tokio::io::AsyncWriteExt;

use crate::AppState;
use rg_git::protocol::v2::handle_v2;

/// Check if client wants Protocol V2 based on HTTP headers.
pub fn wants_protocol_v2(headers: &HeaderMap) -> bool {
    if let Some(git_protocol) = headers.get("Git-Protocol") {
        if let Ok(protocol) = git_protocol.to_str() {
            return protocol.contains("version=2");
        }
    }
    false
}

/// Build V2 capability advertisement synchronously.
/// Uses the same format as build_v2_capability_advertisement in lib.rs.
fn build_v2_capability_sync() -> String {
    use std::io::Write;

    let mut buf = Vec::new();

    let write_pkt = |buf: &mut Vec<u8>, text: &str| {
        let payload = text.as_bytes();
        let len = payload.len() + 4;
        writeln!(buf, "{:04x}{}", len, text)?;
        Ok::<(), std::io::Error>(())
    };

    let _ = write_pkt(&mut buf, "version 2");
    let _ = write_pkt(&mut buf, "agent=ironforge/0.1");
    let _ = write_pkt(&mut buf, "ls-refs");
    let _ = write_pkt(&mut buf, "fetch=shallow");
    let _ = write_pkt(&mut buf, "object-format=sha1");
    let _ = write_pkt(&mut buf, "server-option");
    buf.extend_from_slice(b"0000");

    String::from_utf8(buf).unwrap_or_default()
}

/// Handle GET /git/{owner}/{repo}/info/refs
/// Protocol V2 negotiation happens on first request with Git-Protocol header.
pub async fn handle_info_refs_v2(
    State(state): State<AppState>,
    Path((owner, repo)): Path<(String, String)>,
    headers: HeaderMap,
) -> impl IntoResponse {
    // Check if client wants V2
    if !wants_protocol_v2(&headers) {
        // Fall back to V1 - this should be handled by the regular handler
        // In practice, the router should check the header and route accordingly
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": "Protocol V2 not requested",
                "hint": "Send Git-Protocol: version=2 header"
            })),
        )
            .into_response();
    }

    // Build repo path
    let repo_path = state.repo_root.join(&owner).join(format!("{}.git", &repo));

    if !repo_path.exists() {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": "Repository not found"
            })),
        )
            .into_response();
    }

    // For info/refs, we send the capability advertisement
    // The actual refs will be sent when client sends ls-refs command
    let response_body = build_v2_capability_sync();

    (
        StatusCode::OK,
        [
            ("Content-Type", "application/x-git-upload-pack-advertisement"),
            ("Cache-Control", "no-cache"),
        ],
        response_body,
    )
        .into_response()
}

/// Handle POST /git/{owner}/{repo}/git-upload-pack
/// For Protocol V2, the command processing happens here.
pub async fn handle_git_upload_pack_v2(
    State(state): State<AppState>,
    Path((owner, repo)): Path<(String, String)>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> impl IntoResponse {
    // Check if client wants V2
    if !wants_protocol_v2(&headers) {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": "Protocol V2 not requested"
            })),
        )
            .into_response();
    }

    // Build repo path
    let repo_path = state.repo_root.join(&owner).join(format!("{}.git", &repo));

    if !repo_path.exists() {
        return (StatusCode::NOT_FOUND, Json(serde_json::json!({
            "error": "Repository not found"
        }))).into_response();
    }

    let (reader, mut writer) = tokio::io::duplex(body.len() + 4096);

    // Write request body to the reader side
    if let Err(e) = writer.write_all(&body).await {
        tracing::error!(error = %e, "Failed to write request body");
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "Bad request"})),
        )
            .into_response();
    }
    drop(writer); // Close writer so reader gets EOF

    // Process V2 protocol
    let mut response_buf = Vec::new();

    match handle_v2(&repo_path, reader, &mut response_buf).await {
        Ok(()) => {
            let body = String::from_utf8(response_buf).unwrap_or_default();
            (
                StatusCode::OK,
                [("Content-Type", "application/x-git-upload-pack-result")],
                body,
            )
                .into_response()
        }
        Err(e) => {
            tracing::error!(error = %e, "V2 upload-pack failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
                .into_response()
        }
    }
}

/// Handle POST /git/{owner}/{repo}/git-receive-pack
/// For Protocol V2 push operations.
pub async fn handle_git_receive_pack_v2(
    State(state): State<AppState>,
    Path((owner, repo)): Path<(String, String)>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> impl IntoResponse {
    // Check if client wants V2
    if !wants_protocol_v2(&headers) {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": "Protocol V2 not requested"
            })),
        )
            .into_response();
    }

    // Build repo path
    let repo_path = state.repo_root.join(&owner).join(format!("{}.git", &repo));

    if !repo_path.exists() {
        return (StatusCode::NOT_FOUND, Json(serde_json::json!({
            "error": "Repository not found"
        }))).into_response();
    }

    // For receive-pack over V2, we still use the V1 receive-pack logic
    // because V2's fetch command is primarily for clone/fetch, not push
    // The push negotiation in V2 still uses similar mechanisms
    let (reader, mut writer) = tokio::io::duplex(body.len() + 4096);

    if let Err(e) = writer.write_all(&body).await {
        tracing::error!(error = %e, "Failed to write request body");
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "Bad request"})),
        )
            .into_response();
    }
    drop(writer);

    let mut response_buf = Vec::new();

    match rg_git::protocol::receive_pack::handle_receive_pack_http(
        &repo_path,
        reader,
        &mut response_buf,
    )
    .await
    {
        Ok(_ref_updates) => {
            let body = String::from_utf8(response_buf).unwrap_or_default();
            (
                StatusCode::OK,
                [("Content-Type", "application/x-git-receive-pack-result")],
                body,
            )
                .into_response()
        }
        Err(e) => {
            tracing::error!(error = %e, "V2 receive-pack failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
                .into_response()
        }
    }
}
