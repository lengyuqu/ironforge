//! IronForge HTTP server implementation using Axum.
//!
//! Provides:
//!  - Git Smart HTTP protocol endpoints (`/git/...`)
//!  - REST API (`/api/v1/...`)
//!  - Health check (`/health`)

pub mod api;

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use axum::body::Body;
use axum::extract::{Query, State};
use axum::http::header;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use sea_orm::DatabaseConnection;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

/// Shared application state injected into every Axum handler via `State<AppState>`.
#[derive(Clone)]
pub struct AppState {
    pub repo_root: Arc<PathBuf>,
    pub db: DatabaseConnection,
    pub jwt_secret: Arc<String>,
}

/// HTTP server configuration.
pub struct HttpServerConfig {
    /// Address to listen on (e.g., "0.0.0.0:8080").
    pub listen_addr: String,
    /// Root directory for git repositories.
    pub repo_root: PathBuf,
    /// Database connection.
    pub db: DatabaseConnection,
    /// JWT secret key.
    pub jwt_secret: String,
}

/// Start the HTTP server and run forever.
pub async fn run(config: HttpServerConfig) -> Result<()> {
    let state = AppState {
        repo_root: Arc::new(config.repo_root),
        db: config.db,
        jwt_secret: Arc::new(config.jwt_secret),
    };

    let app = create_router(state);

    let listener = tokio::net::TcpListener::bind(&config.listen_addr)
        .await
        .with_context(|| format!("failed to bind to {}", config.listen_addr))?;

    tracing::info!(addr = %config.listen_addr, "HTTP server listening");

    axum::serve(listener, app)
        .await
        .context("HTTP server error")?;

    Ok(())
}

/// Create the Axum router (Git + REST API + health).
fn create_router(state: AppState) -> Router {
    // ── Git Smart HTTP routes ──────────────────────────────────────────────
    let git_routes = Router::new()
        .route(
            "/{owner}/{repo}/info/refs",
            get(handle_info_refs),
        )
        .route(
            "/{owner}/{repo}/git-upload-pack",
            post(handle_git_upload_pack),
        )
        .route(
            "/{owner}/{repo}/git-receive-pack",
            post(handle_git_receive_pack),
        );

    // ── REST API routes ───────────────────────────────────────────────────
    let api_v1 = Router::new()
        // Users
        .route("/users/register", post(api::users::register))
        .route("/users/login", post(api::users::login))
        .route("/users/me", get(api::users::me))
        // Repos
        .route("/repos", post(api::repos::create_repo))
        .route("/repos/:owner", get(api::repos::list_repos))
        .route("/repos/:owner/:name", get(api::repos::get_repo))
        // Issues
        .route("/repos/:owner/:name/issues", get(api::issues::list_issues).post(api::issues::create_issue))
        .route("/repos/:owner/:name/issues/:number", get(api::issues::get_issue).patch(api::issues::update_issue))
        .route("/repos/:owner/:name/issues/:number/comments", get(api::issues::list_comments).post(api::issues::add_comment))
        // Pull Requests
        .route("/repos/:owner/:name/pulls", get(api::pulls::list_prs).post(api::pulls::create_pr))
        .route("/repos/:owner/:name/pulls/:number", get(api::pulls::get_pr).patch(api::pulls::update_pr))
        .route("/repos/:owner/:name/pulls/:number/diff", get(api::pulls::get_diff))
        .route("/repos/:owner/:name/pulls/:number/merge", post(api::pulls::merge_pr));

    Router::new()
        .nest("/git", git_routes)
        .nest("/api/v1", api_v1)
        .route("/health", get(health))
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        .with_state(state)
}

async fn health() -> impl IntoResponse {
    axum::Json(serde_json::json!({
        "status": "ok",
        "version": "0.1.0",
        "phase": "3",
    }))
}

async fn handle_info_refs(
    State(state): State<AppState>,
    axum::extract::Path((owner, repo)): axum::extract::Path<(String, String)>,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    let service = params.get("service").map(|s| s.as_str()).unwrap_or("");
    let repo_path = state.repo_root.join(format!("{}/{}.git", owner, repo));

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
        let line = format!(
            "{:04x}{} {}\0{}\n",
            sha.len() + refname.len() + caps.len() + 6,
            sha,
            refname,
            caps
        );
        buf.push_str(&line);
    } else {
        let null_sha = "0000000000000000000000000000000000000000";
        let line = format!(
            "{:04x}{} capabilities^{}\0{}\n",
            null_sha.len() + 15 + caps.len() + 1,
            null_sha,
            service,
            caps
        );
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
    State(state): State<AppState>,
    axum::extract::Path((owner, repo)): axum::extract::Path<(String, String)>,
    body: axum::body::Bytes,
) -> impl IntoResponse {
    let repo_path = state.repo_root.join(format!("{}/{}.git", owner, repo));

    if !repo_path.exists() {
        return (
            StatusCode::NOT_FOUND,
            [(header::CONTENT_TYPE, "application/x-git-upload-pack-result")],
            Body::from("repository not found"),
        );
    }

    let (mut pipe_read, mut pipe_write) = tokio::io::duplex(body.len() + 1024);
    tokio::spawn(async move {
        let _ = pipe_write.write_all(&body).await;
    });

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
            drop(buf_writer);
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
    State(state): State<AppState>,
    axum::extract::Path((owner, repo)): axum::extract::Path<(String, String)>,
    body: axum::body::Bytes,
) -> impl IntoResponse {
    let repo_path = state.repo_root.join(format!("{}/{}.git", owner, repo));

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
