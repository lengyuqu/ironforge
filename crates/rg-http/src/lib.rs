//! IronForge HTTP server implementation using Axum.
//!
//! Provides:
//!  - Git Smart HTTP protocol endpoints (`/git/...`)
//!  - REST API (`/api/v1/...`)
//!  - Health check (`/health`)
//!  - TLS/HTTPS support (rustls)
//!  - API pagination

pub mod api;
pub mod git_v2;
pub mod openapi;
pub mod pagination;
pub mod rate_limit;
pub mod ws;

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use axum::body::Body;
use axum::extract::{Query, State};
use axum::http::header;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{delete, get, patch, post, put};
use axum::Router;
use sea_orm::DatabaseConnection;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use tower_http::services::ServeDir;

/// Shared application state injected into every Axum handler via `State<AppState>`.
#[derive(Clone)]
pub struct AppState {
    pub repo_root: Arc<PathBuf>,
    pub db: DatabaseConnection,
    pub jwt_secret: Arc<String>,
    pub docker_enabled: bool,
    pub rate_limiter: rate_limit::RateLimiter,
    pub notification_hub: ws::NotificationHub,
    pub smtp_config: Option<rg_core::email::SmtpConfig>,
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
    /// Whether Docker runner is enabled for CI jobs.
    pub docker_enabled: bool,
    /// Rate limit: max requests per window (0 = disabled).
    pub rate_limit_max: u32,
    /// Rate limit: window duration in seconds.
    pub rate_limit_window_secs: u64,
    /// SMTP configuration for email notifications (None = disabled).
    pub smtp_config: Option<rg_core::email::SmtpConfig>,
    /// TLS configuration: (cert_path, key_path). None = HTTP only.
    pub tls_config: Option<(PathBuf, PathBuf)>,
}

/// Start the HTTP server and run forever.
pub async fn run(config: HttpServerConfig) -> Result<()> {
    let rate_limiter = rate_limit::RateLimiter::new(
        config.rate_limit_max.max(1),
        config.rate_limit_window_secs.max(1),
    );

    let notification_hub = ws::NotificationHub::new();

    let state = AppState {
        repo_root: Arc::new(config.repo_root),
        db: config.db,
        jwt_secret: Arc::new(config.jwt_secret),
        docker_enabled: config.docker_enabled,
        rate_limiter: rate_limiter.clone(),
        notification_hub: notification_hub.clone(),
        smtp_config: config.smtp_config,
    };

    let app = create_router(state.clone(), rate_limiter.clone());

    if let Some((cert_path, key_path)) = &config.tls_config {
        // ── HTTPS mode (axum-server + rustls) ──────────────────────────
        let tls_config = load_tls_config(cert_path, key_path).await?;
        let config_clone = config.listen_addr.clone();

        tracing::info!(addr = %config.listen_addr, "HTTPS server listening (TLS)");

        let app = app;
        let rustls_config = axum_server::tls_rustls::RustlsConfig::from_config(tls_config);
        axum_server::bind_rustls(config_clone.parse().unwrap(), rustls_config)
            .serve(app.into_make_service())
            .await
            .context("HTTPS server error")?;
    } else {
        // ── HTTP mode ───────────────────────────────────────────────────
        let listener = tokio::net::TcpListener::bind(&config.listen_addr)
            .await
            .with_context(|| format!("failed to bind to {}", config.listen_addr))?;

        tracing::info!(addr = %config.listen_addr, "HTTP server listening");

        axum::serve(listener, app)
            .await
            .context("HTTP server error")?;
    }

    Ok(())
}

/// Load TLS certificate and private key, return a rustls ServerConfig.
async fn load_tls_config(
    cert_path: &std::path::Path,
    key_path: &std::path::Path,
) -> Result<Arc<tokio_rustls::rustls::ServerConfig>> {
    use std::io::BufReader;
    use tokio_rustls::rustls::pki_types::CertificateDer;
    use tokio_rustls::rustls::ServerConfig;

    let cert_file = std::fs::File::open(cert_path)
        .with_context(|| format!("failed to open TLS cert: {}", cert_path.display()))?;
    let mut cert_reader = BufReader::new(cert_file);
    let certs: Vec<CertificateDer<'_>> = rustls_pemfile::certs(&mut cert_reader)
        .collect::<Result<Vec<_>, _>>()
        .context("failed to parse TLS certificates")?;

    let key_file = std::fs::File::open(key_path)
        .with_context(|| format!("failed to open TLS key: {}", key_path.display()))?;
    let mut key_reader = BufReader::new(key_file);

    let key = rustls_pemfile::private_key(&mut key_reader)
        .context("failed to parse TLS private key")?
        .ok_or_else(|| anyhow::anyhow!("no private key found in {}", key_path.display()))?;

    let server_config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .context("failed to build TLS server config")?;

    Ok(Arc::new(server_config))
}

/// Create the Axum router (Git + REST API + health).
fn create_router(state: AppState, rate_limiter: rate_limit::RateLimiter) -> Router {
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
        // PAT
        .route("/users/tokens", get(api::users::list_tokens).post(api::users::create_token))
        .route("/users/tokens/:id", delete(api::users::delete_token))
        // Repos
        .route("/repos", post(api::repos::create_repo))
        .route("/repos/:owner", get(api::repos::list_repos))
        .route("/repos/:owner/:name", get(api::repos::get_repo))
        // Milestones (before issues to avoid routing conflicts)
        .route("/repos/:owner/:name/milestones", get(api::issues::list_milestones).post(api::issues::create_milestone))
        .route("/repos/:owner/:name/milestones/:id", get(api::issues::get_milestone).patch(api::issues::update_milestone).delete(api::issues::delete_milestone))
        // Labels
        .route("/repos/:owner/:name/labels", get(api::labels::list_labels).post(api::labels::create_label))
        .route("/repos/:owner/:name/labels/:id", get(api::labels::get_label).patch(api::labels::update_label).delete(api::labels::delete_label))
        // Issues
        .route("/repos/:owner/:name/issues", get(api::issues::list_issues).post(api::issues::create_issue))
        .route("/repos/:owner/:name/issues/:number", get(api::issues::get_issue).patch(api::issues::update_issue))
        .route("/repos/:owner/:name/issues/:number/comments", get(api::issues::list_comments).post(api::issues::add_comment))
        // Pull Requests
        .route("/repos/:owner/:name/pulls", get(api::pulls::list_prs).post(api::pulls::create_pr))
        .route("/repos/:owner/:name/pulls/:number", get(api::pulls::get_pr).patch(api::pulls::update_pr))
        .route("/repos/:owner/:name/pulls/:number/diff", get(api::pulls::get_diff))
        .route("/repos/:owner/:name/pulls/:number/merge", post(api::pulls::merge_pr))
        // PR Reviews
        .route("/repos/:owner/:name/pulls/:number/reviews", get(api::reviews::list_reviews).post(api::reviews::submit_review))
        .route("/repos/:owner/:name/pulls/:number/reviews/:id", get(api::reviews::get_review))
        .route("/repos/:owner/:name/pulls/:number/reviews/:id/dismiss", post(api::reviews::dismiss_review))
        .route("/repos/:owner/:name/pulls/:number/comments", get(api::reviews::list_review_comments).post(api::reviews::create_review_comment))
        // Wiki
        .route("/repos/:owner/:name/wiki", get(api::wiki::list_pages).post(api::wiki::create_page))
        .route("/repos/:owner/:name/wiki/:title", get(api::wiki::get_page).patch(api::wiki::update_page).delete(api::wiki::delete_page))
        // LFS
        .route("/repos/:owner/:name/lfs/objects/batch", post(api::lfs::batch))
        .route("/repos/:owner/:name/lfs/objects/:oid", get(api::lfs::download_object).put(api::lfs::upload_object))
        // Webhooks
        .route("/repos/:owner/:name/hooks", get(api::webhooks::list_webhooks).post(api::webhooks::create_webhook))
        .route("/repos/:owner/:name/hooks/:id", get(api::webhooks::get_webhook).patch(api::webhooks::update_webhook).delete(api::webhooks::delete_webhook))
        .route("/repos/:owner/:name/hooks/:id/deliveries", get(api::webhooks::list_deliveries))
        .route("/repos/:owner/:name/hooks/:id/deliveries/:delivery_id/redeliver", post(api::webhooks::redeliver))
        // CI/CD Pipelines
        .route("/repos/:owner/:name/pipelines", get(api::ci::list_pipelines).post(api::ci::trigger_pipeline))
        .route("/repos/:owner/:name/pipelines/:id", get(api::ci::get_pipeline))
        .route("/repos/:owner/:name/pipelines/:id/retry", post(api::ci::retry_pipeline))
        .route("/repos/:owner/:name/pipelines/:id/cancel", post(api::ci::cancel_pipeline))
        .route("/repos/:owner/:name/pipelines/:id/jobs/:job_id", get(api::ci::get_job))
        // Branch Protection
        .route("/repos/:owner/:name/branches/protection", get(api::branch_protection::list_protections).post(api::branch_protection::create_protection))
        .route("/repos/:owner/:name/branches/protection/:id", get(api::branch_protection::get_protection).patch(api::branch_protection::update_protection).delete(api::branch_protection::delete_protection))
        // Collaborators
        .route("/repos/:owner/:name/collaborators", get(api::collaborators::list_collaborators).post(api::collaborators::add_collaborator))
        .route("/repos/:owner/:name/collaborators/:id", patch(api::collaborators::update_permission))
        .route("/repos/:owner/:name/collaborators/:user_id/remove", post(api::collaborators::remove_collaborator))
        // Repo Content Browsing
        .route("/repos/:owner/:name/tree", get(api::repo_content::list_tree))
        .route("/repos/:owner/:name/blob/*path", get(api::repo_content::get_blob))
        .route("/repos/:owner/:name/log", get(api::repo_content::get_log))
        .route("/repos/:owner/:name/branches", get(api::repo_content::list_branches))
        .route("/repos/:owner/:name/tags", get(api::repo_content::list_tags))
        // GPG Signatures
        .route("/repos/:owner/:name/commits/:sha/signature", get(api::repo_content::get_commit_signature))
        // Organizations
        .route("/orgs", get(api::orgs::list_orgs).post(api::orgs::create_org))
        .route("/orgs/:name", get(api::orgs::get_org).patch(api::orgs::update_org).delete(api::orgs::delete_org))
        .route("/orgs/:name/members", get(api::orgs::list_org_members).post(api::orgs::add_org_member))
        .route("/orgs/:name/members/:user_id", delete(api::orgs::remove_org_member))
        .route("/orgs/:name/teams", get(api::orgs::list_org_teams).post(api::orgs::create_team))
        .route("/orgs/:name/teams/:team_id", get(api::orgs::get_team).delete(api::orgs::delete_team))
        .route("/orgs/:name/teams/:team_id/members", get(api::orgs::list_team_members).post(api::orgs::add_team_member))
        .route("/orgs/:name/teams/:team_id/members/:user_id", delete(api::orgs::remove_team_member))
        // Notifications
        .route("/notifications", get(api::notifications::list_notifications))
        .route("/notifications/unread-count", get(api::notifications::unread_count))
        .route("/notifications/mark-all-read", post(api::notifications::mark_all_read))
        .route("/notifications/:id/read", post(api::notifications::mark_read))
        .route("/notifications/:id", delete(api::notifications::delete_notification))
        // Star/Watch
        .route("/repos/:owner/:name/star", put(api::repos::star_repo))
        .route("/repos/:owner/:name/stargazers", get(api::repos::get_stargazers))
        .route("/repos/:owner/:name/watch", put(api::repos::watch_repo).delete(api::repos::unwatch_repo))
        // Repo Delete (combined with GET)
        .route("/repos/:owner/:name", delete(api::repos::delete_repo_handler))
        // Releases
        .route("/repos/:owner/:name/releases", get(api::releases::list_releases).post(api::releases::create_release))
        .route("/repos/:owner/:name/releases/:id", get(api::releases::get_release).patch(api::releases::update_release).delete(api::releases::delete_release))
        // Fork
        .route("/repos/:owner/:name/fork", post(api::repos::fork_repo_handler))
        .route("/repos/:owner/:name/forks", get(api::repos::list_forks_handler))
        // Transfer
        .route("/repos/:owner/:name/transfer", post(api::repos::transfer_repo_handler))
        // Admin
        .route("/admin/users", get(api::admin::list_users))
        .route("/admin/users/:id", get(api::admin::get_user))
        .route("/admin/users/:id", patch(api::admin::update_user))
        .route("/admin/users/:id", delete(api::admin::delete_user))
        .route("/admin/orgs", get(api::admin::list_orgs))
        .route("/admin/orgs/:name", get(api::admin::get_org))
        .route("/admin/orgs/:name", delete(api::admin::delete_org))
        // WebSocket
        .route("/ws/notifications", get(ws::ws_notifications_handler));

    Router::new()
        .nest("/git", git_routes)
        .nest("/api/v1", api_v1)
        .route("/health", get(health))
        // ── OpenAPI spec endpoint ──────────────────────────────────────────
        .route("/api-docs/openapi.json", get(openapi_handler))
        // Swagger UI — serve embedded Swagger UI static files
        .route("/api-docs/{*tail}", get(swagger_ui_handler))
        // Serve SvelteKit static assets if the build directory exists
        .fallback_service(
            ServeDir::new("web/build").fallback(ServeDir::new("web/build/index.html"))
        )
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        .layer(axum::Extension(rate_limiter))
        .with_state(state)
}

async fn health() -> impl IntoResponse {
    axum::Json(serde_json::json!({
        "status": "ok",
        "version": "0.1.0",
        "phase": "10",
    }))
}

/// GET /api-docs/openapi.json — serve the OpenAPI specification.
async fn openapi_handler() -> impl IntoResponse {
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/json")],
        openapi::openapi_spec(),
    )
}

/// GET /api-docs/{*tail} — serve Swagger UI static files.
async fn swagger_ui_handler(axum::extract::Path(tail): axum::extract::Path<String>) -> impl IntoResponse {
    let config = openapi::swagger_config();
    let path = if tail.is_empty() { "/" } else { &tail };

    match utoipa_swagger_ui::serve(path, config) {
        Ok(Some(file)) => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, file.content_type)],
            file.bytes.to_vec(),
        ),
        Ok(None) => (StatusCode::NOT_FOUND, [(header::CONTENT_TYPE, "text/plain".to_string())], "Not Found".as_bytes().to_vec()),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, [(header::CONTENT_TYPE, "text/plain".to_string())], format!("Swagger UI error: {e}").into_bytes()),
    }
}

/// Extract user ID from HTTP Basic Auth or Bearer token.
/// Returns None for anonymous access.
fn extract_actor_id(headers: &axum::http::HeaderMap, jwt_secret: &str) -> Option<i64> {
    // Try Bearer token first
    if let Some(auth) = headers.get(header::AUTHORIZATION) {
        let auth_str = auth.to_str().ok()?;

        if let Some(token) = auth_str.strip_prefix("Bearer ") {
            if let Some(claims) = rg_core::auth::jwt::validate_token(token, jwt_secret) {
                return claims.sub.parse().ok();
            }
        }

        // Try Basic auth
        if let Some(encoded) = auth_str.strip_prefix("Basic ") {
            if let Ok(decoded) = base64_decode(encoded) {
                if let Ok(credentials) = String::from_utf8(decoded) {
                    if let Some((username, _password)) = credentials.split_once(':') {
                        tracing::debug!(username = %username, "Basic auth in git protocol — use token auth instead");
                    }
                }
            }
        }
    }

    None
}

/// Simple base64 decoder (no external crate needed).
fn base64_decode(input: &str) -> Result<Vec<u8>, ()> {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = Vec::new();
    let bytes = input.as_bytes();
    let mut i = 0;

    while i + 3 < bytes.len() + 3 {
        let mut accum: u32 = 0;
        let mut bits = 0;

        for j in 0..4 {
            if i + j < bytes.len() {
                if let Some(pos) = ALPHABET.iter().position(|&c| c == bytes[i + j]) {
                    accum = (accum << 6) | pos as u32;
                    bits += 6;
                }
            }
        }

        for shift in (0..bits - 6).rev().step_by(8) {
            if shift >= 8 {
                result.push(((accum >> (shift - 8)) & 0xFF) as u8);
            }
        }

        i += 4;
    }

    Ok(result)
}

/// Check repository access for git protocol.
///
/// - upload-pack (clone/fetch): can_read
/// - receive-pack (push): can_write
///
/// Returns Ok(()) if access is granted, or an error response.
async fn check_git_access(
    db: &DatabaseConnection,
    owner: &str,
    repo_name: &str,
    actor_id: Option<i64>,
    require_write: bool,
) -> Result<(), (StatusCode, [(header::HeaderName, &'static str); 1], String)> {
    let access = if require_write {
        rg_core::repo::service::can_write(db, owner, repo_name, actor_id).await
    } else {
        rg_core::repo::service::can_read(db, owner, repo_name, actor_id).await
    };

    match access {
        Ok(true) => Ok(()),
        Ok(false) => Err((
            StatusCode::FORBIDDEN,
            [(header::CONTENT_TYPE, "text/plain")],
            "access denied".to_string(),
        )),
        Err(e) => Err((
            StatusCode::NOT_FOUND,
            [(header::CONTENT_TYPE, "text/plain")],
            format!("repository not found: {}", e),
        )),
    }
}

async fn handle_info_refs(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
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

    // Extract actor from auth header
    let actor_id = extract_actor_id(&headers, &state.jwt_secret);
    let require_write = service == "git-receive-pack";

    // Check access
    if let Err(resp) = check_git_access(&state.db, &owner, &repo, actor_id, require_write).await {
        return resp;
    }

    // Check if client wants Protocol V2
    let wants_v2 = git_v2::wants_protocol_v2(&headers);

    match service {
        "git-upload-pack" | "git-receive-pack" => {
            if wants_v2 {
                // Protocol V2: send capability advertisement (refs sent via ls-refs command)
                return match build_v2_capability_advertisement() {
                    Ok(data) => {
                        let content_type = if service == "git-upload-pack" {
                            "application/x-git-upload-pack-advertisement"
                        } else {
                            "application/x-git-receive-pack-advertisement"
                        };
                        (StatusCode::OK, [(header::CONTENT_TYPE, content_type)], data)
                    }
                    Err(e) => (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        [(header::CONTENT_TYPE, "text/plain")],
                        format!("error: {:#}", e),
                    ),
                };
            }

            // Protocol V1: send full ref advertisement
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

/// Build Protocol V2 capability advertisement.
/// Format: version 2 + capabilities + flush
/// Manual pkt-line construction to avoid async in sync context.
fn build_v2_capability_advertisement() -> Result<String> {
    use std::io::Write;

    let mut buf = Vec::new();

    // Helper to write pkt-line data
    let write_pkt = |buf: &mut Vec<u8>, text: &str| {
        let payload = text.as_bytes();
        let len = payload.len() + 4; // +4 for header
        writeln!(buf, "{:04x}{}", len, text)?;
        Ok::<(), std::io::Error>(())
    };

    // Protocol version line
    write_pkt(&mut buf, "version 2")?;
    write_pkt(&mut buf, "agent=ironforge/0.1")?;
    write_pkt(&mut buf, "ls-refs")?;
    write_pkt(&mut buf, "fetch=shallow")?;
    write_pkt(&mut buf, "object-format=sha1")?;
    write_pkt(&mut buf, "server-option")?;

    // Flush packet
    buf.extend_from_slice(b"0000");

    Ok(String::from_utf8(buf)?)
}

async fn handle_git_upload_pack(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
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

    // Check read access
    let actor_id = extract_actor_id(&headers, &state.jwt_secret);
    if let Err(resp) = check_git_access(&state.db, &owner, &repo, actor_id, false).await {
        return (resp.0, resp.1, Body::from(resp.2));
    }

    // Check if client wants Protocol V2
    let wants_v2 = git_v2::wants_protocol_v2(&headers);

    if wants_v2 {
        // Protocol V2: use V2 handler
        let (pipe_read, mut pipe_write) = tokio::io::duplex(body.len() + 1024);
        tokio::spawn(async move {
            let _ = pipe_write.write_all(&body).await;
        });

        let (mut buf_reader, mut buf_writer) = tokio::io::duplex(64 * 1024);

        match rg_git::protocol::v2::handle_v2(&repo_path, pipe_read, &mut buf_writer).await {
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
    } else {
        // Protocol V1: use V1 handler
        let (pipe_read, mut pipe_write) = tokio::io::duplex(body.len() + 1024);
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
}

async fn handle_git_receive_pack(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
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

    // Check write access
    let actor_id = extract_actor_id(&headers, &state.jwt_secret);
    if let Err(resp) = check_git_access(&state.db, &owner, &repo, actor_id, true).await {
        return (resp.0, resp.1, Body::from(resp.2));
    }

    let (pipe_read, mut pipe_write) = tokio::io::duplex(body.len() + 1024);
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
        Ok(ref_updates) => {
            let _ = buf_writer.flush().await;
            drop(buf_writer);
            let mut output = Vec::new();
            let _ = buf_reader.read_to_end(&mut output).await;

            // ── Post-push hooks: trigger CI + Webhook ───────────────
            let db = state.db.clone();
            let repo_path_clone = repo_path.clone();
            let owner_clone = owner.clone();
            let repo_clone = repo.clone();
            let docker_enabled = state.docker_enabled;
            let hub = state.notification_hub.clone();
            let smtp = state.smtp_config.clone();

            tokio::spawn(async move {
                post_push_hooks(
                    &PostPushParams {
                        db: &db,
                        repo_path: &repo_path_clone,
                        owner: &owner_clone,
                        repo_name: &repo_clone,
                        docker_enabled,
                        notification_hub: &hub,
                        smtp_config: &smtp,
                    },
                    &ref_updates,
                ).await;
            });

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

/// Parameters for post-push hooks.
struct PostPushParams<'a> {
    db: &'a DatabaseConnection,
    repo_path: &'a std::path::Path,
    owner: &'a str,
    repo_name: &'a str,
    docker_enabled: bool,
    notification_hub: &'a ws::NotificationHub,
    smtp_config: &'a Option<rg_core::email::SmtpConfig>,
}

/// Post-push hook: trigger CI pipeline and webhook for push events.
async fn post_push_hooks(
    params: &PostPushParams<'_>,
    ref_updates: &[rg_git::protocol::receive_pack::RefUpdate],
) {
    let PostPushParams {
        db,
        repo_path,
        owner,
        repo_name,
        docker_enabled,
        notification_hub,
        smtp_config,
    } = params;

    // Find repo_id from DB
    let repo_model = find_repo_by_name(db, owner, repo_name).await;

    let (repo_id, repo_owner_id) = match repo_model {
        Ok(Some(r)) => (r.id, r.owner_id),
        _ => {
            tracing::warn!(owner = %owner, repo = %repo_name, "Post-push: repo not found in DB, skipping hooks");
            return;
        }
    };

    for update in ref_updates {
        if update.status != "ok" {
            continue;
        }

        tracing::info!(
            refname = %update.refname,
            new_sha = %update.new_sha,
            "Post-push: triggering hooks"
        );

        // 0. Branch protection audit: log if push targets a protected branch
        if let Some(branch_name) = update.refname.strip_prefix("refs/heads/") {
            match rg_db::ops::protected_branch_ops::find_by_repo_and_branch(db, repo_id, branch_name).await {
                Ok(Some(_protection)) => {
                    tracing::warn!(
                        branch = %branch_name,
                        "Post-push: push to protected branch detected (should be enforced by pre-receive hook)"
                    );
                }
                Err(e) => {
                    tracing::warn!(error = %e, "Failed to check branch protection");
                }
                _ => {}
            }
        }

        // 1. Trigger CI pipeline if .ironforge-ci.yml exists
        if rg_ci::has_ci_config(repo_path, &update.new_sha) {
            match rg_ci::trigger_pipeline(
                (*db).clone(),
                repo_path,
                repo_id,
                &update.new_sha,
                &update.refname,
                "push",
                None,
                *docker_enabled,
            )
            .await
            {
                Ok(pipeline_id) => {
                    tracing::info!(pipeline_id, "CI pipeline triggered");

                    // Push real-time notification to repo owner
                    ws::push_notification(
                        notification_hub,
                        repo_owner_id,
                        "ci_triggered",
                        serde_json::json!({
                            "pipeline_id": pipeline_id,
                            "repo": format!("{}/{}", owner, repo_name),
                            "ref": update.refname,
                            "commit": update.new_sha,
                        }),
                    );

                    // Send email notification if SMTP is configured
                    if let Some(smtp) = smtp_config {
                        if let Ok(Some(owner_user)) = rg_db::ops::user_ops::find_by_id(db, repo_owner_id).await {
                            let subject = format!("[IronForge] CI pipeline #{} triggered for {}/{}", pipeline_id, owner, repo_name);
                            let body = format!(
                                "A CI pipeline has been triggered for repository {}/{} on branch {}.<br/><br/>Commit: {}<br/>Pipeline ID: {}",
                                owner, repo_name, update.refname, update.new_sha, pipeline_id
                            );
                            if let Err(e) = rg_core::email::send_html_notification(
                                smtp,
                                &owner_user.email,
                                &subject,
                                &body,
                                None,
                            ).await {
                                tracing::warn!(error = %e, "Failed to send CI notification email");
                            }
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!(error = %e, "Failed to trigger CI pipeline");
                }
            }
        }

        // 2. Trigger push webhook
        let payload = serde_json::json!({
            "ref": update.refname,
            "before": update.old_sha,
            "after": update.new_sha,
            "repository": {
                "owner": owner,
                "name": repo_name,
            },
        });

        if let Err(e) = rg_core::webhook::service::trigger_event(db, repo_id, "push", &payload).await {
            tracing::warn!(error = %e, "Failed to trigger push webhook");
        }

        // Push real-time notification for push event
        ws::push_notification(
            notification_hub,
            repo_owner_id,
            "push",
            serde_json::json!({
                "repo": format!("{}/{}", owner, repo_name),
                "ref": update.refname,
                "commit": update.new_sha,
            }),
        );
    }
}

/// Find a repository by owner name (user or org) and repo name (DB lookup).
async fn find_repo_by_name(
    db: &DatabaseConnection,
    owner: &str,
    name: &str,
) -> anyhow::Result<Option<rg_db::entities::repository::Model>> {
    rg_core::repo::service::find_repo_by_owner_name(db, owner, name).await
}
