//! IronForge HTTP server implementation using Axum.
//!
//! Provides:
//!  - Git Smart HTTP protocol endpoints (`/git/...`)
//!  - REST API (`/api/v1/...`)
//!  - Health check (`/health`)
//!  - TLS/HTTPS support (rustls)
//!  - API pagination

pub mod api;
pub mod error;
pub mod git_v2;
pub mod middleware;
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

// gix is used via `gix::open()` etc. — crate available at crate root
use tower_http::trace::TraceLayer;
use tower_http::services::ServeDir;

/// Shared application state injected into every Axum handler via `State<AppState>`.
#[derive(Clone)]
pub struct AppState {
    pub repo_root: Arc<PathBuf>,
    pub db: DatabaseConnection,
    pub jwt_secret: Arc<String>,
    pub docker_enabled: bool,
    pub external_runners: bool,
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
    /// Whether to use external runners instead of embedded runner for CI.
    pub external_runners: bool,
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
    rate_limiter.spawn_cleanup_task();

    let notification_hub = ws::NotificationHub::new();

    let state = AppState {
        repo_root: Arc::new(config.repo_root),
        db: config.db,
        jwt_secret: Arc::new(config.jwt_secret),
        docker_enabled: config.docker_enabled,
        external_runners: config.external_runners,
        rate_limiter: rate_limiter.clone(),
        notification_hub: notification_hub.clone(),
        smtp_config: config.smtp_config,
    };

    let app = create_router(state.clone(), rate_limiter.clone());

    // ── HTTPS mode (axum-server + rustls) ──────────────────
        //
        //
        // To use TLS, you MUST use `axum_server::bind_rustls()` instead.
        //
        // Correct pattern (used below):
        //   let rustls_config = RustlsConfig::from_config(tls_config);
        //   axum_server::bind_rustls(addr, rustls_config).serve(app).await?;
        //
        // Wrong pattern (no TLS support):
        //   let listener = TcpListener::bind(addr).await?;
        //   axum::serve(listener, app).await?;  // ERROR: no TLS!
    // ── HTTPS mode (axum-server + rustls) ──────────────────
    //
    // CRITICAL: Axum TLS requires `axum-server`, NOT `axum::serve()` (踩坑经验 #2)
    //
    // `axum::serve()` only supports plain TCP (no TLS).
    // To use TLS, you MUST use `axum_server::bind_rustls()` instead.
    //
    // Correct pattern (used below):
    //   let rustls_config = RustlsConfig::from_config(tls_config);
    //   axum_server::bind_rustls(addr, rustls_config).serve(app).await?;
    //
    // Wrong pattern (no TLS support):
    //   let listener = TcpListener::bind(addr).await?;
    //   axum::serve(listener, app).await?;  // ERROR: no TLS!
    if let Some((cert_path, key_path)) = &config.tls_config {
        // ── HTTPS mode (axum-server + rustls) ──────────────────────────
        let tls_config = load_tls_config(cert_path, key_path).await?;
        let config_clone = config.listen_addr.clone();

        tracing::info!(addr = %config.listen_addr, "HTTPS server listening (TLS)");

        let app = app;
        let rustls_config = axum_server::tls_rustls::RustlsConfig::from_config(tls_config);
        axum_server::bind_rustls(config_clone.parse().with_context(|| {
            format!("invalid TLS listen address: {}", config_clone)
        })?, rustls_config)
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

/// Create the Axum router for testing (no rate limiter, no static file serving).
pub fn create_router_for_test(state: AppState) -> Router {
    build_test_router(state)
}

/// Create the Axum router (Git + REST API + health).
fn create_router(state: AppState, rate_limiter: rate_limit::RateLimiter) -> Router {
    build_router(state, rate_limiter)
}

/// Shared router builder used by both production and test routers.
///
/// CRITICAL: Axum `nest()` State requirement (踩坑经验 #2)
///
/// All nested routers MUST share the same `State<AppState>` type.
/// If `git_routes` or `api_v1` use a different State type,
/// Axum will reject the route with a compile-time error.
///
/// Correct pattern (used here):
///   let git_routes = Router::new()...with_state(state.clone());
///   let api_v1 = Router::new()...with_state(state.clone());
///   Router::new().nest("/git", git_routes).nest("/api/v1", api_v1)
///
/// Wrong pattern (will not compile):
///   let git_routes = Router::new()...with_state(git_state);  // different type
///   let api_v1 = Router::new()...with_state(api_state);     // different type
///   Router::new().nest("/git", git_routes).nest("/api/v1", api_v1)  // ERROR
fn build_router(state: AppState, rate_limiter: rate_limit::RateLimiter) -> Router {
    let (api_v1, git_routes) = build_routes(&state);

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
        // ── Middleware layers (order: bottom-up, last .layer() runs first) ──
        .layer(axum::middleware::from_fn(middleware::request_id_middleware))
        .layer(
            TraceLayer::new_for_http().make_span_with(|request: &axum::http::Request<axum::body::Body>| {
                let request_id = request
                    .headers()
                    .get("x-request-id")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("-");
                tracing::info_span!(
                    "http_request",
                    method = %request.method(),
                    uri = %request.uri(),
                    status = tracing::field::Empty,
                    request_id = %request_id,
                )
            }),
        )
        .layer(CorsLayer::permissive())
        .layer(axum::middleware::from_fn_with_state(
            rate_limiter.clone(),
            rate_limit::rate_limit_middleware,
        ))
        .with_state(state)
}

/// Build route definitions (shared between production and test routers).
fn build_routes(state: &AppState) -> (Router<AppState>, Router<AppState>) {
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

    // Runner routes that require authentication (single middleware layer)
    let runners_auth = Router::new()
        .route("/runners/{id}/heartbeat", post(api::runners::heartbeat))
        .route("/runners/{id}/jobs/poll", get(api::runners::poll_job))
        .route("/runners/{id}/jobs/{job_id}/start", post(api::runners::start_job))
        .route("/runners/{id}/jobs/{job_id}/log", post(api::runners::upload_log))
        .route("/runners/{id}/jobs/{job_id}/finish", post(api::runners::finish_job))
        .route("/runners/{id}/jobs/{job_id}/artifacts", post(api::artifacts::upload_artifact))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            api::runners::authenticate_runner,
        ))
        .with_state(state.clone());

    // ── REST API routes ───────────────────────────────────────────────────
    let api_v1 = Router::new()
        // Users
        .route("/users/register", post(api::users::register))
        .route("/users/login", post(api::users::login))
        .route("/users/me", get(api::users::me))
        // PAT
        .route("/users/tokens", get(api::users::list_tokens).post(api::users::create_token))
        .route("/users/tokens/{id}", delete(api::users::delete_token))
        // Repos
        .route("/repos", post(api::repos::create_repo))
        .route("/repos/{owner}", get(api::repos::list_repos))
        .route("/repos/{owner}/{name}", get(api::repos::get_repo))
        // Milestones (before issues to avoid routing conflicts)
        .route("/repos/{owner}/{name}/milestones", get(api::issues::list_milestones).post(api::issues::create_milestone))
        .route("/repos/{owner}/{name}/milestones/{id}", get(api::issues::get_milestone).patch(api::issues::update_milestone).delete(api::issues::delete_milestone))
        // Labels
        .route("/repos/{owner}/{name}/labels", get(api::labels::list_labels).post(api::labels::create_label))
        .route("/repos/{owner}/{name}/labels/{id}", get(api::labels::get_label).patch(api::labels::update_label).delete(api::labels::delete_label))
        // Issues
        .route("/repos/{owner}/{name}/issues", get(api::issues::list_issues).post(api::issues::create_issue))
        .route("/repos/{owner}/{name}/issues/{number}", get(api::issues::get_issue).patch(api::issues::update_issue))
        .route("/repos/{owner}/{name}/issues/{number}/labels", get(api::issues::get_issue_labels))
        .route("/repos/{owner}/{name}/issues/{number}/comments", get(api::issues::list_comments).post(api::issues::add_comment))
        // Pull Requests
        .route("/repos/{owner}/{name}/pulls", get(api::pulls::list_prs).post(api::pulls::create_pr))
        .route("/repos/{owner}/{name}/pulls/{number}", get(api::pulls::get_pr).patch(api::pulls::update_pr))
        .route("/repos/{owner}/{name}/pulls/{number}/diff", get(api::pulls::get_diff))
        .route("/repos/{owner}/{name}/pulls/{number}/merge", post(api::pulls::merge_pr))
        // PR Reviews
        .route("/repos/{owner}/{name}/pulls/{number}/reviews", get(api::reviews::list_reviews).post(api::reviews::submit_review))
        .route("/repos/{owner}/{name}/pulls/{number}/reviews/{id}", get(api::reviews::get_review))
        .route("/repos/{owner}/{name}/pulls/{number}/reviews/{id}/dismiss", post(api::reviews::dismiss_review))
        .route("/repos/{owner}/{name}/pulls/{number}/comments", get(api::reviews::list_review_comments).post(api::reviews::create_review_comment))
        // Wiki
        .route("/repos/{owner}/{name}/wiki", get(api::wiki::list_pages).post(api::wiki::create_page))
        .route("/repos/{owner}/{name}/wiki/{title}", get(api::wiki::get_page).patch(api::wiki::update_page).delete(api::wiki::delete_page))
        // LFS
        .route("/repos/{owner}/{name}/lfs/objects/batch", post(api::lfs::batch))
        .route("/repos/{owner}/{name}/lfs/objects/{oid}", get(api::lfs::download_object).put(api::lfs::upload_object))
        // Webhooks
        .route("/repos/{owner}/{name}/hooks", get(api::webhooks::list_webhooks).post(api::webhooks::create_webhook))
        .route("/repos/{owner}/{name}/hooks/{id}", get(api::webhooks::get_webhook).patch(api::webhooks::update_webhook).delete(api::webhooks::delete_webhook))
        .route("/repos/{owner}/{name}/hooks/{id}/deliveries", get(api::webhooks::list_deliveries))
        .route("/repos/{owner}/{name}/hooks/{id}/deliveries/{delivery_id}/redeliver", post(api::webhooks::redeliver))
        // CI/CD Pipelines
        .route("/repos/{owner}/{name}/pipelines", get(api::ci::list_pipelines).post(api::ci::trigger_pipeline))
        .route("/repos/{owner}/{name}/pipelines/{id}", get(api::ci::get_pipeline))
        .route("/repos/{owner}/{name}/pipelines/{id}/retry", post(api::ci::retry_pipeline))
        .route("/repos/{owner}/{name}/pipelines/{id}/cancel", post(api::ci::cancel_pipeline))
        .route("/repos/{owner}/{name}/pipelines/{id}/jobs/{job_id}", get(api::ci::get_job))
        // Branch Protection
        .route("/repos/{owner}/{name}/branches/protection", get(api::branch_protection::list_protections).post(api::branch_protection::create_protection))
        .route("/repos/{owner}/{name}/branches/protection/{id}", get(api::branch_protection::get_protection).patch(api::branch_protection::update_protection).delete(api::branch_protection::delete_protection))
        // Collaborators
        .route("/repos/{owner}/{name}/collaborators", get(api::collaborators::list_collaborators).post(api::collaborators::add_collaborator))
        .route("/repos/{owner}/{name}/collaborators/{id}", patch(api::collaborators::update_permission))
        .route("/repos/{owner}/{name}/collaborators/{user_id}/remove", post(api::collaborators::remove_collaborator))
        // Repo Content Browsing
        .route("/repos/{owner}/{name}/tree", get(api::repo_content::list_tree))
        .route("/repos/{owner}/{name}/blob/{*path}", get(api::repo_content::get_blob))
        .route("/repos/{owner}/{name}/log", get(api::repo_content::get_log))
        .route("/repos/{owner}/{name}/branches", get(api::repo_content::list_branches))
        .route("/repos/{owner}/{name}/tags", get(api::repo_content::list_tags))
        // GPG Signatures
        .route("/repos/{owner}/{name}/commits/{sha}/signature", get(api::repo_content::get_commit_signature))
        // Commit Statuses
        .route("/repos/{owner}/{name}/statuses/{sha}", post(api::repos::create_commit_status))
        .route("/repos/{owner}/{name}/commits/{sha}/statuses", get(api::repos::list_commit_statuses))
        .route("/repos/{owner}/{name}/commits/{sha}/status", get(api::repos::get_combined_status))
        // Organizations
        .route("/orgs", get(api::orgs::list_orgs).post(api::orgs::create_org))
        .route("/orgs/{name}", get(api::orgs::get_org).patch(api::orgs::update_org).delete(api::orgs::delete_org))
        .route("/orgs/{name}/members", get(api::orgs::list_org_members).post(api::orgs::add_org_member))
        .route("/orgs/{name}/members/{user_id}", delete(api::orgs::remove_org_member))
        .route("/orgs/{name}/teams", get(api::orgs::list_org_teams).post(api::orgs::create_team))
        .route("/orgs/{name}/teams/{team_id}", get(api::orgs::get_team).delete(api::orgs::delete_team))
        .route("/orgs/{name}/teams/{team_id}/members", get(api::orgs::list_team_members).post(api::orgs::add_team_member))
        .route("/orgs/{name}/teams/{team_id}/members/{user_id}", delete(api::orgs::remove_team_member))
        // Notifications
        .route("/notifications", get(api::notifications::list_notifications))
        .route("/notifications/unread-count", get(api::notifications::unread_count))
        .route("/notifications/mark-all-read", post(api::notifications::mark_all_read))
        .route("/notifications/{id}/read", post(api::notifications::mark_read))
        .route("/notifications/{id}", delete(api::notifications::delete_notification))
        // Star/Watch
        .route("/repos/{owner}/{name}/star", put(api::repos::star_repo))
        .route("/repos/{owner}/{name}/stargazers", get(api::repos::get_stargazers))
        .route("/repos/{owner}/{name}/watch", put(api::repos::watch_repo).delete(api::repos::unwatch_repo))
        // Repo Delete (combined with GET)
        .route("/repos/{owner}/{name}", delete(api::repos::delete_repo_handler))
        // Releases
        .route("/repos/{owner}/{name}/releases", get(api::releases::list_releases).post(api::releases::create_release))
        .route("/repos/{owner}/{name}/releases/{id}", get(api::releases::get_release).patch(api::releases::update_release).delete(api::releases::delete_release))
        // Release Assets
        .route("/repos/{owner}/{name}/releases/{release_id}/assets", get(api::releases::list_assets).post(api::releases::upload_asset))
        .route("/repos/{owner}/{name}/releases/assets/{asset_id}", get(api::releases::get_asset).delete(api::releases::delete_asset))
        .route("/repos/{owner}/{name}/releases/assets/{asset_id}/download", get(api::releases::download_asset))
        // Fork
        .route("/repos/{owner}/{name}/fork", post(api::repos::fork_repo_handler))
        .route("/repos/{owner}/{name}/forks", get(api::repos::list_forks_handler))
        // Transfer
        .route("/repos/{owner}/{name}/transfer", post(api::repos::transfer_repo_handler))
        // CI/CD Runners
        .route("/runners/register", post(api::runners::register))
        .merge(runners_auth)
        .route("/repos/{owner}/{name}/pipelines/{id}/artifacts", get(api::artifacts::list_pipeline_artifacts))
        .route("/artifacts/{id}", get(api::artifacts::get_artifact))
        .route("/artifacts/{id}", delete(api::artifacts::delete_artifact))
        // Admin
        .route("/admin/runners", get(api::runners::list_runners_admin))
        .route("/admin/runners/{id}", delete(api::runners::delete_runner_admin))
        .route("/admin/users", get(api::admin::list_users))
        .route("/admin/users/{id}", get(api::admin::get_user))
        .route("/admin/users/{id}", patch(api::admin::update_user))
        .route("/admin/users/{id}", delete(api::admin::delete_user))
        .route("/admin/orgs", get(api::admin::list_orgs))
        .route("/admin/orgs/{name}", get(api::admin::get_org))
        .route("/admin/orgs/{name}", delete(api::admin::delete_org))
        // Global Search
        .route("/search", get(api::search::search))
        // ── AI Agent endpoints ─────────────────────────────
        .route("/ai/repos/{owner}/{name}/summary", get(api::ai::ai_repo_summary))
        .route("/ai/repos/{owner}/{name}/issues", get(api::ai::ai_list_issues))
        .route("/ai/repos/{owner}/{name}/prs", get(api::ai::ai_list_prs))
        .route("/ai/repos/{owner}/{name}/tree", get(api::ai::ai_repo_tree))
        .route("/ai/repos/{owner}/{name}/search/code", get(api::ai::ai_search_code))
        // WebSocket
        .route("/ws/notifications", get(ws::ws_notifications_handler))
        .route("/ws/job/{job_id}", get(ws::ws_job_log_handler));

    (api_v1, git_routes)
}

/// Build the test router (no rate limiter, no static file serving).
fn build_test_router(state: AppState) -> Router {
    let (api_v1, git_routes) = build_routes(&state);

    Router::new()
        .nest("/git", git_routes)
        .nest("/api/v1", api_v1)
        .route("/health", get(health))
        // ── Middleware layers (no rate limiter for tests) ──────────────────
        .layer(axum::middleware::from_fn(middleware::request_id_middleware))
        .layer(
            TraceLayer::new_for_http().make_span_with(|request: &axum::http::Request<axum::body::Body>| {
                let request_id = request
                    .headers()
                    .get("x-request-id")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("-");
                tracing::info_span!(
                    "http_request",
                    method = %request.method(),
                    uri = %request.uri(),
                    status = tracing::field::Empty,
                    request_id = %request_id,
                )
            }),
        )
        .layer(CorsLayer::permissive())
        .with_state(state)
}

async fn health(State(state): State<AppState>) -> impl IntoResponse {
    use sea_orm::{ConnectionTrait, Statement};

    let mut checks = serde_json::Map::new();

    // DB ping
    let db_ok = state
        .db
        .execute(Statement::from_string(
            sea_orm::DatabaseBackend::Sqlite,
            "SELECT 1".to_string(),
        ))
        .await
        .is_ok();
    checks.insert(
        "database".to_string(),
        serde_json::json!(if db_ok { "ok" } else { "error" }),
    );

    // Filesystem check
    let fs_ok = state.repo_root.exists() && state.repo_root.read_dir().is_ok();
    checks.insert(
        "filesystem".to_string(),
        serde_json::json!(if fs_ok { "ok" } else { "error" }),
    );

    let overall = if db_ok && fs_ok { "ok" } else { "degraded" };
    let status_code = if db_ok && fs_ok {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };

    (
        status_code,
        axum::Json(serde_json::json!({
            "status": overall,
            "version": env!("CARGO_PKG_VERSION"),
            "phase": 20,
            "checks": checks,
        })),
    )
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

/// Git Smart HTTP `/info/refs` endpoint.
///
/// CRITICAL: Content-Type handling (踩坑经验 #6)
///
/// The Git Smart HTTP protocol is VERY sensitive to Content-Type headers.
/// Incorrect Content-Type will cause `git` client to silently fail or
/// report "fatal: protocol error: bad line length character".
///
/// Correct Content-Types:
/// - info/refs response:
///   - upload-pack: `application/x-git-upload-pack-advertisement`
///   - receive-pack: `application/x-git-receive-pack-advertisement`
/// - request body (POST):
///   - upload-pack: `application/x-git-upload-pack-request`
///   - receive-pack: `application/x-git-receive-pack-request`
/// - response body (POST):
///   - upload-pack: `application/x-git-upload-pack-result`
///   - receive-pack: `application/x-git-receive-pack-result`
///
/// Common mistake: Using `text/plain` or wrong subtype will break git clients.
/// Always verify Content-Type matches the Git Smart HTTP spec exactly.
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
    let mut buf = String::new();
    buf.push_str(&format!("# service={}\n", service));
    buf.push_str("0000");

    let repo = gix::open(repo_path)
        .with_context(|| format!("failed to open repository: {:?}", repo_path))?;
    
    // Get all references (like git for-each-ref)
    let references = repo.references()?;
    let mut ref_list: Vec<(String, String)> = references.all()?
        .filter_map(|r| r.ok())
        .filter_map(|r| {
            let oid = r.target().try_id()?.to_owned();
            let name = String::from_utf8_lossy(r.name().as_bstr()).to_string();
            Some((oid.to_string(), name))
        })
        .collect();

    // Get HEAD SHA
    let head_sha = if let Some(head) = repo.head().ok() {
        head.try_into_referent()  // Returns Option<Reference>
            .and_then(|r| r.target().try_id().map(|id| id.to_string()))
    } else {
        None
    };

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
            let external_runners = state.external_runners;
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
                        external_runners,
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
    external_runners: bool,
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
        external_runners,
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
                *external_runners,
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

        // 3. Trigger branch/tag-specific webhooks
        if let Some(branch_name) = update.refname.strip_prefix("refs/heads/") {
            if update.old_sha.is_empty() || update.old_sha == "0000000000000000000000000000000000000000" {
                // New branch created
                let _ = rg_core::webhook::service::trigger_branch_created(db, repo_id, branch_name).await;
            } else if update.new_sha.is_empty() || update.new_sha == "0000000000000000000000000000000000000000" {
                // Branch deleted
                let _ = rg_core::webhook::service::trigger_branch_deleted(db, repo_id, branch_name).await;
            }
        } else if let Some(tag_name) = update.refname.strip_prefix("refs/tags/") {
            if update.old_sha.is_empty() || update.old_sha == "0000000000000000000000000000000000000000" {
                // New tag created
                let _ = rg_core::webhook::service::trigger_tag_created(db, repo_id, tag_name).await;
            } else if update.new_sha.is_empty() || update.new_sha == "0000000000000000000000000000000000000000" {
                // Tag deleted
                let _ = rg_core::webhook::service::trigger_tag_deleted(db, repo_id, tag_name).await;
            }
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
