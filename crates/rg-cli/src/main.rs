//! IronForge CLI — main entry point.
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use reqwest::header;
use serde_json;
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(name = "ironforge", about = "A Git hosting platform written in Rust")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
#[allow(clippy::large_enum_variant)]
enum Commands {
    /// Start the IronForge server
    Serve {
        /// Root directory for git repositories
        #[arg(long, default_value = "./repos")]
        repo_root: String,

        /// HTTP listen address
        #[arg(long, default_value = "0.0.0.0:8080")]
        http_addr: String,

        /// SSH listen address
        #[arg(long, default_value = "0.0.0.0:2222")]
        ssh_addr: String,

        /// Path to SSH host key
        #[arg(long)]
        host_key: Option<String>,

        /// SQLite database URL (e.g. sqlite:///tmp/ironforge/db.sqlite?mode=rwc)
        #[arg(long, default_value = "sqlite://./ironforge.db?mode=rwc")]
        db_url: String,

        /// JWT secret key (use a long random string in production)
        #[arg(long, default_value = "change-me-in-production")]
        jwt_secret: String,

        /// Enable Docker runner for CI jobs with `image` field
        #[arg(long, default_value_t = false)]
        docker: bool,

        /// Use external runners instead of embedded runner for CI jobs
        #[arg(long, default_value_t = false)]
        external_runners: bool,

        /// Rate limit: max requests per window per IP (0 = disabled)
        #[arg(long, default_value_t = 0)]
        rate_limit_max: u32,

        /// Rate limit: window duration in seconds
        #[arg(long, default_value_t = 60)]
        rate_limit_window: u64,

        /// SMTP server host (enables email notifications)
        #[arg(long)]
        smtp_host: Option<String>,

        /// SMTP server port
        #[arg(long, default_value_t = 587)]
        smtp_port: u16,

        /// SMTP username
        #[arg(long)]
        smtp_user: Option<String>,

        /// SMTP password
        #[arg(long)]
        smtp_pass: Option<String>,

        /// SMTP from email address
        #[arg(long)]
        smtp_from: Option<String>,

        /// Path to TLS certificate file (PEM format, enables HTTPS)
        #[arg(long)]
        tls_cert: Option<String>,

        /// Path to TLS private key file (PEM format)
        #[arg(long)]
        tls_key: Option<String>,

        /// Path to TOML configuration file (overrides CLI defaults)
        #[arg(long)]
        config: Option<String>,

        /// Log file path (enables file logging with rotation). If not set, logs to stderr only.
        #[arg(long)]
        log_file: Option<String>,

        /// Log rotation: max log file size in MB before rotation (default: 10)
        #[arg(long, default_value_t = 10)]
        log_max_size_mb: u64,

        /// Log rotation: max number of old log files to keep (default: 5)
        #[arg(long, default_value_t = 5)]
        log_max_files: usize,
    },

    /// Run database migrations and exit
    Migrate {
        /// SQLite database URL (e.g. sqlite://./ironforge.db?mode=rwc)
        #[arg(long, default_value = "sqlite://./ironforge.db?mode=rwc")]
        db_url: String,
    },

    /// Create a new bare repository (no DB record — for quick testing)
    CreateRepo {
        /// Owner username
        owner: String,

        /// Repository name (without .git suffix)
        name: String,

        /// Root directory for repositories
        #[arg(long, default_value = "./repos")]
        repo_root: String,
    },

    /// Run as a CI Runner — polls jobs and executes them
    Runner {
        /// IronForge server URL (e.g. http://127.0.0.1:8080)
        #[arg(long, default_value = "http://127.0.0.1:8080")]
        server: String,

        /// Runner name (used for registration if not already registered)
        #[arg(long)]
        name: Option<String>,

        /// Existing runner ID (skip registration)
        #[arg(long)]
        runner_id: Option<i64>,

        /// Existing runner token (skip registration)
        #[arg(long)]
        token: Option<String>,
    },
}

/// TOML configuration file structure.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
struct ConfigFile {
    #[serde(default)]
    server: ServerConfig,
    #[serde(default)]
    database: DatabaseConfig,
    #[serde(default)]
    auth: AuthConfig,
    #[serde(default)]
    ci: CiConfig,
    #[serde(default)]
    rate_limit: RateLimitConfig,
    #[serde(default)]
    smtp: SmtpConfig,
    #[serde(default)]
    tls: TlsConfig,
    #[serde(default)]
    logging: LoggingConfig,
}

#[derive(Debug, serde::Deserialize, Default)]
#[allow(dead_code)]
struct ServerConfig {
    repo_root: Option<String>,
    http_addr: Option<String>,
    ssh_addr: Option<String>,
    host_key: Option<String>,
}

#[derive(Debug, serde::Deserialize, Default)]
#[allow(dead_code)]
struct DatabaseConfig {
    url: Option<String>,
}

#[derive(Debug, serde::Deserialize, Default)]
#[allow(dead_code)]
struct AuthConfig {
    jwt_secret: Option<String>,
}

#[derive(Debug, serde::Deserialize, Default)]
struct CiConfig {
    #[serde(default)]
    docker: Option<bool>,
    #[serde(default)]
    external_runners: Option<bool>,
}

#[derive(Debug, serde::Deserialize, Default)]
struct RateLimitConfig {
    max: Option<u32>,
    window_secs: Option<u64>,
}

#[derive(Debug, serde::Deserialize, Default)]
struct SmtpConfig {
    host: Option<String>,
    port: Option<u16>,
    user: Option<String>,
    pass: Option<String>,
    from: Option<String>,
}

#[derive(Debug, serde::Deserialize, Default)]
struct TlsConfig {
    cert: Option<String>,
    key: Option<String>,
}

#[derive(Debug, serde::Deserialize, Default)]
struct LoggingConfig {
    file: Option<String>,
    max_size_mb: Option<u64>,
    max_files: Option<usize>,
}

fn load_config_file(path: &str) -> anyhow::Result<ConfigFile> {
    let content = std::fs::read_to_string(path)?;
    let config: ConfigFile = toml::from_str(&content)?;
    tracing::info!(path = %path, "Loaded configuration file");
    Ok(config)
}

/// Validate critical configuration before starting servers.
/// Refuses to start with dangerous defaults or invalid settings.
fn validate_config(
    jwt_secret: &str,
    repo_root: &std::path::Path,
    tls_config: &Option<(PathBuf, PathBuf)>,
) -> anyhow::Result<()> {
    // 1. Refuse default JWT secret
    if jwt_secret == "change-me-in-production" {
        tracing::error!(
            "FATAL: jwt_secret is set to the default value. \
             Set a strong secret via --jwt-secret or config file [auth].jwt_secret"
        );
        anyhow::bail!("refusing to start with default jwt_secret");
    }
    if jwt_secret.len() < 16 {
        tracing::warn!(
            jwt_len = jwt_secret.len(),
            "jwt_secret is shorter than 16 characters — consider using a stronger secret"
        );
    }

    // 2. Verify repo_root is writable
    let test_file = repo_root.join(".write_test");
    std::fs::write(&test_file, "test")
        .with_context(|| format!("repo_root is not writable: {:?}", repo_root))?;
    std::fs::remove_file(&test_file)?;

    // 3. Verify TLS files exist if configured
    if let Some((ref cert, ref key)) = tls_config {
        if !cert.exists() {
            anyhow::bail!("TLS certificate not found: {:?}", cert);
        }
        if !key.exists() {
            anyhow::bail!("TLS private key not found: {:?}", key);
        }
    }

    tracing::info!("Configuration validation passed");
    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Parse CLI args first (without initializing logging, to avoid early output)
    let cli = Cli::parse();

    match cli.command {
        Commands::Serve {
            repo_root,
            http_addr,
            ssh_addr,
            host_key,
            db_url,
            jwt_secret,
            docker,
            external_runners,
            rate_limit_max,
            rate_limit_window,
            smtp_host,
            smtp_port,
            smtp_user,
            smtp_pass,
            smtp_from,
            tls_cert,
            tls_key,
            config,
            log_file,
            log_max_size_mb,
            log_max_files,
        } => {
            // ── Load config file (if specified) ────────────────────────
            let cfg = if let Some(config_path) = &config {
                Some(load_config_file(config_path)?)
            } else {
                None
            };

            // Resolve values: CLI args > config file > defaults
            let resolved_repo_root = repo_root;
            let resolved_http_addr = http_addr;
            let resolved_ssh_addr = ssh_addr;
            let resolved_host_key = host_key.or_else(|| cfg.as_ref().and_then(|c| c.server.host_key.clone()));
            let resolved_db_url = db_url;
            let resolved_jwt_secret = jwt_secret;
            let resolved_docker = docker || cfg.as_ref().and_then(|c| c.ci.docker).unwrap_or(false);
            let resolved_external_runners = external_runners || cfg.as_ref().and_then(|c| c.ci.external_runners).unwrap_or(false);
            let resolved_rate_limit_max = if rate_limit_max > 0 { rate_limit_max } else { cfg.as_ref().and_then(|c| c.rate_limit.max).unwrap_or(0) };
            let resolved_rate_limit_window = if rate_limit_window != 60 { rate_limit_window } else { cfg.as_ref().and_then(|c| c.rate_limit.window_secs).unwrap_or(60) };

            // SMTP: CLI takes precedence, fallback to config
            let (resolved_smtp_host, resolved_smtp_port, resolved_smtp_user, resolved_smtp_pass, resolved_smtp_from) = {
                let h = smtp_host.or_else(|| cfg.as_ref().and_then(|c| c.smtp.host.clone()));
                let p = cfg.as_ref().and_then(|c| c.smtp.port).unwrap_or(smtp_port);
                let u = smtp_user.or_else(|| cfg.as_ref().and_then(|c| c.smtp.user.clone()));
                let pw = smtp_pass.or_else(|| cfg.as_ref().and_then(|c| c.smtp.pass.clone()));
                let f = smtp_from.or_else(|| cfg.as_ref().and_then(|c| c.smtp.from.clone()));
                (h, p, u, pw, f)
            };

            // TLS: CLI takes precedence, fallback to config
            let resolved_tls_cert = tls_cert.or_else(|| cfg.as_ref().and_then(|c| c.tls.cert.clone()));
            let resolved_tls_key = tls_key.or_else(|| cfg.as_ref().and_then(|c| c.tls.key.clone()));

            // Logging: CLI takes precedence, fallback to config
            let resolved_log_file = log_file.or_else(|| cfg.as_ref().and_then(|c| c.logging.file.clone()));
            let _resolved_log_max_size = if log_max_size_mb != 10 { log_max_size_mb } else { cfg.as_ref().and_then(|c| c.logging.max_size_mb).unwrap_or(10) };
            let resolved_log_max_files = if log_max_files != 5 { log_max_files } else { cfg.as_ref().and_then(|c| c.logging.max_files).unwrap_or(5) };

            // ── Initialize logging ─────────────────────────────────────
            if let Some(ref log_path) = resolved_log_file {
                // File logging with rotation
                let log_dir = std::path::Path::new(log_path)
                    .parent()
                    .unwrap_or(std::path::Path::new("."));
                let log_prefix = std::path::Path::new(log_path)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("ironforge");
                let log_suffix = std::path::Path::new(log_path)
                    .extension()
                    .and_then(|s| s.to_str())
                    .unwrap_or("log");

                let file_appender = tracing_appender::rolling::RollingFileAppender::builder()
                    .rotation(tracing_appender::rolling::Rotation::DAILY)
                    .filename_prefix(log_prefix)
                    .filename_suffix(log_suffix)
                    .max_log_files(resolved_log_max_files)
                    .build(log_dir)
                    .map_err(|e| anyhow::anyhow!("failed to create log appender: {}", e))?;

                let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

                tracing_subscriber::fmt()
                    .with_env_filter(
                        EnvFilter::try_from_default_env()
                            .unwrap_or_else(|_| EnvFilter::new("info")),
                    )
                    .with_target(false)
                    .with_writer(non_blocking)
                    .init();

                // Keep the guard alive for the process lifetime
                // (it will be dropped when main exits, which is fine)
                std::mem::forget(_guard);

                tracing::info!(file = %log_path, "Logging to file with rotation");
            } else {
                // Stderr logging (default)
                tracing_subscriber::fmt()
                    .with_env_filter(
                        EnvFilter::try_from_default_env()
                            .unwrap_or_else(|_| EnvFilter::new("info")),
                    )
                    .with_target(false)
                    .init();
            }

            let repo_root = PathBuf::from(&resolved_repo_root);
            std::fs::create_dir_all(&repo_root)?;

            // ── Database ──────────────────────────────────────────────────
            tracing::info!("Connecting to database: {}", resolved_db_url);
            let db = rg_db::connect(&resolved_db_url).await?;
            rg_db::run_migrations(&db).await?;
            tracing::info!("Database ready");

            // ── HTTP server ───────────────────────────────────────────────
            let smtp_config = match (resolved_smtp_host, resolved_smtp_user, resolved_smtp_pass, resolved_smtp_from) {
                (Some(host), Some(user), Some(pass), Some(from)) => {
                    Some(rg_core::email::SmtpConfig::new(&host, resolved_smtp_port, &user, &pass, &from))
                }
                _ => None,
            };

            let tls_config = match (resolved_tls_cert, resolved_tls_key) {
                (Some(cert), Some(key)) => {
                    tracing::info!("TLS enabled: cert={}, key={}", cert, key);
                    Some((PathBuf::from(cert), PathBuf::from(key)))
                }
                (Some(_), None) => {
                    tracing::warn!("TLS cert specified but no key — running HTTP only");
                    None
                }
                (None, Some(_)) => {
                    tracing::warn!("TLS key specified but no cert — running HTTP only");
                    None
                }
                _ => None,
            };

            // ── Configuration validation ─────────────────────────────────
            validate_config(&resolved_jwt_secret, &repo_root, &tls_config)?;

            let http_config = rg_http::HttpServerConfig {
                listen_addr: resolved_http_addr,
                repo_root: repo_root.clone(),
                db: db.clone(),
                jwt_secret: resolved_jwt_secret.clone(),
                docker_enabled: resolved_docker,
                external_runners: resolved_external_runners,
                rate_limit_max: resolved_rate_limit_max,
                rate_limit_window_secs: resolved_rate_limit_window,
                smtp_config,
                tls_config,
            };

            // ── SSH server ────────────────────────────────────────────────
            let host_key_path = resolved_host_key.unwrap_or_else(|| {
                let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
                format!("{}/.ssh/id_ed25519", home)
            });

            let ssh_config = rg_ssh::SshServerConfig {
                host_key_path: PathBuf::from(&host_key_path),
                listen_addr: resolved_ssh_addr,
                repo_root: repo_root.clone(),
                db: Some(db.clone()),
            };

            let http_handle = tokio::spawn(async move {
                if let Err(e) = rg_http::run(http_config).await {
                    tracing::error!("HTTP server error: {:#}", e);
                }
            });

            let ssh_handle = tokio::spawn(async move {
                if let Err(e) = rg_ssh::start_ssh_server(ssh_config).await {
                    tracing::error!("SSH server error: {:#}", e);
                }
            });

            tracing::info!("IronForge server started (Phase 20)");

            tokio::select! {
                _ = http_handle => {},
                _ = ssh_handle => {},
            }
        }

        Commands::Migrate { db_url } => {
            tracing_subscriber::fmt()
                .with_env_filter(
                    EnvFilter::try_from_default_env()
                        .unwrap_or_else(|_| EnvFilter::new("info")),
                )
                .with_target(false)
                .init();

            tracing::info!("Connecting to database: {}", db_url);
            let db = rg_db::connect(&db_url).await?;
            tracing::info!("Running database migrations...");
            rg_db::run_migrations(&db).await?;
            tracing::info!("Migrations complete ✅");
        }

        Commands::CreateRepo {
            owner,
            name,
            repo_root,
        } => {
            // Simple logging for create-repo command
            tracing_subscriber::fmt()
                .with_env_filter(
                    EnvFilter::try_from_default_env()
                        .unwrap_or_else(|_| EnvFilter::new("info")),
                )
                .with_target(false)
                .init();

            let repo_root = PathBuf::from(&repo_root);
            let repo_dir = repo_root.join(format!("{}/{}.git", owner, name));
            std::fs::create_dir_all(&repo_dir)?;
            
            // Replace git init --bare with gix API
            gix::create::into(&repo_dir, gix::create::Kind::Bare, gix::create::Options::default())
                .with_context(|| "failed to create bare repository")?;
            
            println!("Created repository: {}/{}.git", owner, name);
        }

        Commands::Runner { server, name, runner_id, token } => {
            use reqwest::header;

            let client = reqwest::Client::new();

            // ── Register or use existing credentials ─────────
            let (runner_id, token) = match (runner_id, token) {
                (Some(rid), Some(tok)) => (rid, tok),
                _ => {
                    // Register new runner
                    let name = name.as_deref().unwrap_or("default-runner");
                    let resp: serde_json::Value = client
                        .post(&format!("{}/api/v1/runners/register", server))
                        .json(&serde_json::json!({"name": name}))
                        .send()
                        .await
                        .context("failed to register runner")?
                        .json()
                        .await?;
                    let rid = resp["id"].as_i64()
                        .ok_or_else(|| anyhow::anyhow!("invalid register response: missing 'id'"))?;
                    let tok = resp["token"].as_str()
                        .ok_or_else(|| anyhow::anyhow!("invalid register response: missing 'token'"))?
                        .to_string();
                    eprintln!("Runner registered: id={}, token={}", rid, tok);
                    eprintln!("Save these credentials for future runs!");
                    (rid, tok)
                }
            };

            eprintln!("Runner started: server={}, id={}", server, runner_id);

            // ── Main poll loop ─────────────────────────────
            let auth_header = format!("Bearer {}", token);
            loop {
                // 1. Poll for job
                let poll_resp = client
                    .get(&format!("{}/api/v1/runners/{}/jobs/poll?timeout=30", server, runner_id))
                    .header(header::AUTHORIZATION, &auth_header)
                    .send()
                    .await;

                let job: serde_json::Value = match poll_resp {
                    Ok(r) if r.status() == reqwest::StatusCode::NO_CONTENT => {
                        continue;
                    }
                    Ok(r) => r.json().await?,
                    Err(e) => {
                        eprintln!("Poll error: {}, retrying in 5s", e);
                        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                        continue;
                    }
                };

                let job_id = job["job_id"].as_i64()
                    .ok_or_else(|| anyhow::anyhow!("invalid poll response"))?;
                let script: Vec<&str> = job["script"]
                    .as_array()
                    .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
                    .unwrap_or_default();
                let image = job["image"].as_str();

                eprintln!("Got job {}: {}", job_id, job["name"].as_str().unwrap_or(""));

                // 2. Start job
                let _ = client
                    .post(&format!("{}/api/v1/runners/{}/jobs/{}/start", server, runner_id, job_id))
                    .header(header::AUTHORIZATION, &auth_header)
                    .send()
                    .await;

                // 3. Execute job
                let script_str = script.join("\n");
                let (exit_code, log) = if let Some(img) = image {
                    run_job_docker(img, &script_str).await
                } else {
                    run_job_local(&script_str).await
                };

                // 4. Upload log
                let _ = client
                    .post(&format!("{}/api/v1/runners/{}/jobs/{}/log", server, runner_id, job_id))
                    .header(header::AUTHORIZATION, &auth_header)
                    .body(log.clone())
                    .send()
                    .await;

                // 5. Finish job
                let status = if exit_code == 0 { "success" } else { "failure" };
                let _ = client
                    .post(&format!("{}/api/v1/runners/{}/jobs/{}/finish", server, runner_id, job_id))
                    .header(header::AUTHORIZATION, &auth_header)
                    .json(&serde_json::json!({"status": status, "exit_code": exit_code}))
                    .send()
                    .await;

                eprintln!("Job {} finished: status={}, exit_code={}", job_id, status, exit_code);
            }
        }
    }

    Ok(())
}

/// Execute a job script locally via `sh -c`.
async fn run_job_local(script: &str) -> (i32, String) {
    let output = tokio::process::Command::new("sh")
        .arg("-c")
        .arg(script)
        .output()
        .await;

    match output {
        Ok(o) => {
            let code = o.status.code().unwrap_or(-1);
            let mut log = String::new();
            if !o.stdout.is_empty() {
                log.push_str(&String::from_utf8_lossy(&o.stdout));
            }
            if !o.stderr.is_empty() {
                if !log.is_empty() {
                    log.push('\n');
                }
                log.push_str(&String::from_utf8_lossy(&o.stderr));
            }
            (code, log)
        }
        Err(e) => (-1, format!("Failed to spawn job: {}", e)),
    }
}

/// Execute a job script inside a Docker container.
async fn run_job_docker(image: &str, script: &str) -> (i32, String) {
    // Check if Docker is available
    let docker_check = tokio::process::Command::new("docker")
        .arg("info")
        .output()
        .await;

    match docker_check {
        Ok(check) if !check.status.success() => {
            return run_job_local(script).await;
        }
        Err(_) => {
            return run_job_local(script).await;
        }
        _ => {}
    }

    let output = tokio::process::Command::new("docker")
        .args(["run", "--rm", image, "sh", "-c", script])
        .output()
        .await;

    match output {
        Ok(o) => {
            let code = o.status.code().unwrap_or(-1);
            let mut log = String::new();
            if !o.stdout.is_empty() {
                log.push_str(&String::from_utf8_lossy(&o.stdout));
            }
            if !o.stderr.is_empty() {
                if !log.is_empty() {
                    log.push('\n');
                }
                log.push_str(&String::from_utf8_lossy(&o.stderr));
            }
            if code != 0 && log.is_empty() {
                log = format!("Docker exited with code {}", code);
            }
            (code, log)
        }
        Err(e) => (-1, format!("Failed to run docker: {}", e)),
    }
}
