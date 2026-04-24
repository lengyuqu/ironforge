//! IronForge HTTP server implementation using Axum.
//!
//! Provides the Git Smart HTTP protocol endpoints for clone/push.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use axum::body::Body;
use axum::extract::Query;
use axum::http::header;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use tracing;

/// HTTP server configuration.
pub struct HttpServerConfig {
    /// Address to listen on (e.g., "0.0.0.0:8080").
    pub listen_addr: String,
    /// Root directory for git repositories.
    pub repo_root: PathBuf,
}

/// Start the HTTP server and run forever.
pub async fn run(config: HttpServerConfig) -> Result<()> {
    let repo_root = Arc::new(config.repo_root);
    let app = create_router(repo_root);

    let listener = tokio::net::TcpListener::bind(&config.listen_addr)
        .await
        .with_context(|| format!("failed to bind to {}", config.listen_addr))?;

    tracing::info!(addr = %config.listen_addr, "HTTP server listening");

    axum::serve(listener, app)
        .await
        .context("HTTP server error")?;

    Ok(())
}

/// Create the Axum router.
fn create_router(repo_root: Arc<PathBuf>) -> Router {
    let info_refs_repo = repo_root.clone();
    let info_refs = move |axum::extract::Path((owner, repo)): axum::extract::Path<(String, String)>,
                         Query(params): Query<std::collections::HashMap<String, String>>| {
        let repo_root = info_refs_repo.clone();
        async move { handle_info_refs(repo_root, owner, repo, params).await }
    };

    let upload_repo = repo_root.clone();
    let git_upload_pack = move |axum::extract::Path((owner, repo)): axum::extract::Path<(String, String)>,
                                body: axum::body::Bytes| {
        let repo_root = upload_repo.clone();
        async move { handle_git_upload_pack(repo_root, owner, repo, body).await }
    };

    let receive_repo = repo_root.clone();
    let git_receive_pack = move |axum::extract::Path((owner, repo)): axum::extract::Path<(String, String)>,
                                 body: axum::body::Bytes| {
        let repo_root = receive_repo.clone();
        async move { handle_git_receive_pack(repo_root, owner, repo, body).await }
    };

    let git_routes = Router::new()
        .route("/{owner}/{repo}/info/refs", get(info_refs))
        .route("/{owner}/{repo}/git-upload-pack", axum::routing::post(git_upload_pack))
        .route("/{owner}/{repo}/git-receive-pack", axum::routing::post(git_receive_pack));

    Router::new()
        .nest("/git", git_routes)
        .route("/health", get(health))
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
}

async fn health() -> impl IntoResponse {
    axum::Json(serde_json::json!({
        "status": "ok",
        "version": "0.1.0",
    }))
}

async fn handle_info_refs(
    repo_root: Arc<PathBuf>,
    owner: String,
    repo: String,
    params: std::collections::HashMap<String, String>,
) -> impl IntoResponse {
    let service = params.get("service").map(|s| s.as_str()).unwrap_or("");
    let repo_path = repo_root.join(format!("{}/{}.git", owner, repo));

    if !repo_path.exists() {
        return (
            StatusCode::NOT_FOUND,
            [(header::CONTENT_TYPE, "text/plain")],
            "repository not found".to_string(),
        );
    }

    match service {
        "git-upload-pack" | "git-receive-pack" => {
            let content_type = if service == "git-upload-pack" {
                "application/x-git-upload-pack-advertisement"
            } else {
                "application/x-git-receive-pack-advertisement"
            };

            match build_info_refs(&repo_path, service) {
                Ok(data) => (StatusCode::OK, [(header::CONTENT_TYPE, content_type)], data),
                Err(e) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    [(header::CONTENT_TYPE, "text/plain")],
                    format!("error: {:#}", e),
                ),
            }
        }
        _ => (
            StatusCode::BAD_REQUEST,
            [(header::CONTENT_TYPE, "text/plain")],
            "invalid or missing service parameter".to_string(),
        ),
    }
}

fn build_info_refs(repo_path: &std::path::Path, service: &str) -> Result<String> {
    use std::process::Command;

    let mut buf = String::new();
    buf.push_str(&format!("# service={}\n", service));
    buf.push_str("0000");

    let refs_output = Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .args(["for-each-ref", "--format=%(objectname) %(refname)"])
        .output()?;

    let stdout = String::from_utf8(refs_output.stdout)?;
    let mut ref_list: Vec<(String, String)> = Vec::new();

    for line in stdout.lines() {
        let parts: Vec<&str> = line.splitn(2, ' ').collect();
        if parts.len() == 2 {
            ref_list.push((parts[0].to_string(), parts[1].to_string()));
        }
    }

    // Try to resolve HEAD
    let head_sha = Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .and_then(|o| {
            let s = String::from_utf8(o.stdout).ok()?.trim().to_string();
            if s.len() == 40 && s.chars().all(|c| c.is_ascii_hexdigit()) {
                Some(s)
            } else {
                None
            }
        });

    if let Some(sha) = &head_sha {
        ref_list.insert(0, (sha.clone(), "HEAD".to_string()));
    }

    let caps = if service == "git-upload-pack" {
        "multi_ack_detailed no-done side-band-64k thin-pack ofs-delta agent=ironforge/0.1"
    } else {
        "report-status report-status-v2 side-band-64k agent=ironforge/0.1"
    };

    if let Some((sha, refname)) = ref_list.first() {
        let line = format!("{:04x}{} {}\0{}\n", sha.len() + refname.len() + caps.len() + 6, sha, refname, caps);
        buf.push_str(&line);
    } else {
        let null_sha = "0000000000000000000000000000000000000000";
        let line = format!("{:04x}{} capabilities^{}\0{}\n", null_sha.len() + 15 + caps.len() + 1, null_sha, service, caps);
        buf.push_str(&line);
    }

    for (sha, refname) in ref_list.iter().skip(1) {
        let line = format!("{:04x}{} {}\n", sha.len() + refname.len() + 2, sha, refname);
        buf.push_str(&line);
    }

    buf.push_str("0000");
    Ok(buf)
}

async fn handle_git_upload_pack(
    repo_root: Arc<PathBuf>,
    owner: String,
    repo: String,
    body: axum::body::Bytes,
) -> impl IntoResponse {
    let repo_path = repo_root.join(format!("{}/{}.git", owner, repo));

    if !repo_path.exists() {
        return (
            StatusCode::NOT_FOUND,
            [(header::CONTENT_TYPE, "application/x-git-upload-pack-result")],
            Body::from("repository not found"),
        );
    }

    // Use in-memory pipe: write input bytes to a pipe, handle_upload_pack_http reads from it
    let (mut pipe_read, mut pipe_write) = tokio::io::duplex(body.len() + 1024);

    // Write input to pipe in background
    tokio::spawn(async move {
        let _ = pipe_write.write_all(&body).await;
    });

    // Collect output from handler into a buffer
    let (mut buf_reader, mut buf_writer) = tokio::io::duplex(64 * 1024);

    match rg_git::protocol::upload_pack::handle_upload_pack_http(
        &repo_path,
        pipe_read,
        &mut buf_writer,
    )
    .await
    {
        Ok(()) => {
            let _ = buf_writer.flush().await;
            drop(buf_writer); // Close the write end
            let mut output = Vec::new();
            let _ = buf_reader.read_to_end(&mut output).await;
            (
                StatusCode::OK,
                [(header::CONTENT_TYPE, "application/x-git-upload-pack-result")],
                Body::from(output),
            )
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            [(header::CONTENT_TYPE, "text/plain")],
            Body::from(format!("error: {:#}", e)),
        ),
    }
}

async fn handle_git_receive_pack(
    repo_root: Arc<PathBuf>,
    owner: String,
    repo: String,
    body: axum::body::Bytes,
) -> impl IntoResponse {
    let repo_path = repo_root.join(format!("{}/{}.git", owner, repo));

    if !repo_path.exists() {
        return (
            StatusCode::NOT_FOUND,
            [(header::CONTENT_TYPE, "application/x-git-receive-pack-result")],
            Body::from("repository not found"),
        );
    }

    let (mut pipe_read, mut pipe_write) = tokio::io::duplex(body.len() + 1024);
    tokio::spawn(async move {
        let _ = pipe_write.write_all(&body).await;
    });

    let (mut buf_reader, mut buf_writer) = tokio::io::duplex(64 * 1024);

    match rg_git::protocol::receive_pack::handle_receive_pack_http(
        &repo_path,
        pipe_read,
        &mut buf_writer,
    )
    .await
    {
        Ok(()) => {
            let _ = buf_writer.flush().await;
            drop(buf_writer);
            let mut output = Vec::new();
            let _ = buf_reader.read_to_end(&mut output).await;
            (
                StatusCode::OK,
                [(header::CONTENT_TYPE, "application/x-git-receive-pack-result")],
                Body::from(output),
            )
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            [(header::CONTENT_TYPE, "text/plain")],
            Body::from(format!("error: {:#}", e)),
        ),
    }
}
