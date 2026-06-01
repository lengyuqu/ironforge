//! IronForge Runner Agent — polls jobs from the server and executes them.
//!
//! ## Usage
//!
//! ```bash
//! # Register and start running
//! ironforge-runner run --server http://127.0.0.1:8080 --name my-runner
//!
//! # Using a config file
//! ironforge-runner run --config ~/.ironforge/runner.toml
//!
//! # Register only (get token for later use)
//! ironforge-runner register --server http://127.0.0.1:8080 --name my-runner
//! ```

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "ironforge-runner", about = "IronForge CI Runner Agent")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Register a new runner and get a token
    Register {
        /// IronForge server URL
        #[arg(long, default_value = "http://127.0.0.1:8080")]
        server: String,

        /// Runner name
        #[arg(long)]
        name: String,

        /// Runner labels (comma-separated, e.g. "docker,linux,amd64")
        #[arg(long)]
        labels: Option<String>,

        /// Save token to config file
        #[arg(long)]
        save: bool,
    },

    /// Start the runner (register if needed, then poll and execute jobs)
    Run {
        /// IronForge server URL
        #[arg(long, default_value = "http://127.0.0.1:8080")]
        server: String,

        /// Runner name
        #[arg(long)]
        name: Option<String>,

        /// Runner labels (comma-separated)
        #[arg(long)]
        labels: Option<String>,

        /// Existing runner token (skip registration)
        #[arg(long)]
        token: Option<String>,

        /// Existing runner ID (used with --token)
        #[arg(long)]
        runner_id: Option<i64>,

        /// Path to config file
        #[arg(long, default_value = "~/.ironforge/runner.toml")]
        config: String,
    },
}

/// Runner configuration file.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize, Default)]
struct RunnerConfig {
    server: Option<String>,
    token: Option<String>,
    runner_id: Option<i64>,
    name: Option<String>,
    labels: Option<Vec<String>>,
}

fn config_path(path: &str) -> PathBuf {
    let expanded = if path.starts_with("~") {
        match home::home_dir() {
            Some(home) => {
                let remainder = &path[1..];
                let mut result = home;
                // Remove leading slash if present (Unix-style ~/) 
                let trimmed = remainder.trim_start_matches('/');
                if !trimmed.is_empty() {
                    result.push(trimmed);
                }
                result.to_string_lossy().to_string()
            }
            None => path.to_string(),
        }
    } else {
        path.to_string()
    };
    
    PathBuf::from(expanded)
}

fn load_config(path: &str) -> Option<RunnerConfig> {
    let p = config_path(path);
    if p.exists() {
        let content = std::fs::read_to_string(&p).ok()?;
        toml::from_str(&content).ok()
    } else {
        None
    }
}

fn save_config(path: &str, config: &RunnerConfig) -> Result<()> {
    let p = config_path(path);
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let content = toml::to_string_pretty(config)?;
    std::fs::write(&p, content)?;
    Ok(())
}

/// Register a runner with the server.
async fn register_runner(client: &reqwest::Client, server: &str, name: &str, labels: &[String]) -> Result<(i64, String)> {
    let resp = client
        .post(format!("{}/api/v1/runners/register", server))
        .json(&serde_json::json!({
            "name": name,
            "labels": labels,
            "version": env!("CARGO_PKG_VERSION"),
            "os": std::env::consts::OS,
            "arch": std::env::consts::ARCH,
        }))
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("Registration failed ({}): {}", status, body);
    }

    let data: serde_json::Value = resp.json().await?;
    let runner_id = data["id"].as_i64().context("missing id in response")?;
    let token = data["token"].as_str().context("missing token in response")?.to_string();

    Ok((runner_id, token))
}

/// Poll for a pending job (long-polling with 30s timeout).
async fn poll_job(client: &reqwest::Client, server: &str, runner_id: i64, token: &str) -> Result<Option<PollJobResponse>> {
    let resp = client
        .get(format!("{}/api/v1/runners/{}/jobs/poll?timeout=30", server, runner_id))
        .header("Authorization", format!("Bearer {}", token))
        .send()
        .await?;

    match resp.status() {
        s if s == reqwest::StatusCode::NO_CONTENT => Ok(None),
        s if s.is_success() => {
            let job: PollJobResponse = resp.json().await?;
            Ok(Some(job))
        }
        s => {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Poll failed ({}): {}", s, body);
        }
    }
}

#[derive(serde::Deserialize)]
struct PollJobResponse {
    job_id: i64,
    name: String,
    script: Vec<String>,
    image: Option<String>,
    #[allow(dead_code)]
    variables: Option<serde_json::Value>,
    #[allow(dead_code)]
    timeout: i64,
}

/// Send a heartbeat to keep the runner marked as online.
async fn send_heartbeat(client: &reqwest::Client, server: &str, runner_id: i64, token: &str) {
    let _ = client
        .post(format!("{}/api/v1/runners/{}/heartbeat", server, runner_id))
        .header("Authorization", format!("Bearer {}", token))
        .send()
        .await;
}

/// Notify the server that job execution has started.
async fn start_job(client: &reqwest::Client, server: &str, runner_id: i64, job_id: i64, token: &str) {
    let _ = client
        .post(format!("{}/api/v1/runners/{}/jobs/{}/start", server, runner_id, job_id))
        .header("Authorization", format!("Bearer {}", token))
        .send()
        .await;
}

/// Upload job log output.
async fn upload_log(client: &reqwest::Client, server: &str, runner_id: i64, job_id: i64, token: &str, log: &str) {
    let _ = client
        .post(format!("{}/api/v1/runners/{}/jobs/{}/log", server, runner_id, job_id))
        .header("Authorization", format!("Bearer {}", token))
        .body(log.to_string())
        .send()
        .await;
}

/// Report job completion.
async fn finish_job(client: &reqwest::Client, server: &str, runner_id: i64, job_id: i64, token: &str, status: &str, exit_code: i32) {
    let _ = client
        .post(format!("{}/api/v1/runners/{}/jobs/{}/finish", server, runner_id, job_id))
        .header("Authorization", format!("Bearer {}", token))
        .json(&serde_json::json!({"status": status, "exit_code": exit_code}))
        .send()
        .await;
}

/// Execute a job script locally via platform-appropriate shell.
async fn run_job_local(script: &str) -> (i32, String) {
    #[cfg(unix)]
    let output = tokio::process::Command::new("sh")
        .arg("-c")
        .arg(script)
        .output()
        .await;
    
    #[cfg(windows)]
    let output = tokio::process::Command::new("powershell.exe")
        .args(&["-NoProfile", "-NonInteractive", "-Command", script])
        .output()
        .await;
    
    match output {
        Ok(o) => {
            let code = o.status.code().unwrap_or(-1);
            let mut log = String::from_utf8_lossy(&o.stdout).to_string();
            let stderr = String::from_utf8_lossy(&o.stderr).to_string();
            if !stderr.is_empty() {
                if !log.is_empty() { log.push('\n'); }
                log.push_str(&stderr);
            }
            (code, log)
        }
        Err(e) => (-1, format!("Failed to spawn job: {}", e)),
    }
}

/// Execute a job script inside a Docker container.
async fn run_job_docker(image: &str, script: &str) -> (i32, String) {
    // Check if Docker daemon is running
    let docker_ok = tokio::process::Command::new("docker")
        .arg("info")
        .output()
        .await
        .map(|o| o.status.success())
        .unwrap_or(false);

    if !docker_ok {
        tracing::warn!("Docker not available, falling back to local execution");
        return run_job_local(script).await;
    }

    match tokio::process::Command::new("docker")
        .args(["run", "--rm", image, "sh", "-c", script])
        .output()
        .await
    {
        Ok(o) => {
            let code = o.status.code().unwrap_or(-1);
            let mut log = String::from_utf8_lossy(&o.stdout).to_string();
            let stderr = String::from_utf8_lossy(&o.stderr).to_string();
            if !stderr.is_empty() {
                if !log.is_empty() { log.push('\n'); }
                log.push_str(&stderr);
            }
            if code != 0 && log.is_empty() {
                log = format!("Docker exited with code {}", code);
            }
            (code, log)
        }
        Err(e) => (-1, format!("Failed to run docker: {}", e)),
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"))
        )
        .with_target(false)
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Register { server, name, labels, save } => {
            let client = reqwest::Client::new();
            let labels_vec: Vec<String> = labels
                .map(|s| s.split(',').map(|s| s.trim().to_string()).collect())
                .unwrap_or_default();

            println!("Registering runner '{}' with {}...", name, server);
            let (runner_id, token) = register_runner(&client, &server, &name, &labels_vec).await?;
            println!("Runner registered successfully!");
            println!("  ID:    {}", runner_id);
            println!("  Token: {}", token);

            if save {
                let config = RunnerConfig {
                    server: Some(server),
                    runner_id: Some(runner_id),
                    token: Some(token.clone()),
                    name: Some(name),
                    labels: Some(labels_vec),
                };
                let config_path = "~/.ironforge/runner.toml";
                save_config(config_path, &config)?;
                println!("  Config saved to {}", config_path);
            }
        }

        Commands::Run { server, name, labels, token, runner_id, config } => {
            let client = reqwest::Client::new();

            // Resolve config: CLI args > config file > defaults
            let cfg = load_config(&config);
            let resolved_server = server.as_str();
            let (resolved_id, resolved_token, resolved_name) = match (runner_id, token, name) {
                (Some(id), Some(tok), _) => (id, tok, cfg.as_ref().and_then(|c| c.name.clone()).unwrap_or_default()),
                (Some(id), Some(tok), Some(n)) => (id, tok, n),
                _ => {
                    // Need to register
                    let cfg_name = cfg.as_ref().and_then(|c| c.name.clone()).unwrap_or_else(|| {
                        hostname::get().unwrap_or_else(|_| "unnamed-runner".to_string())
                    });
                    let cfg_labels = cfg.as_ref().and_then(|c| c.labels.clone()).unwrap_or_default();
                    let resolved_labels = labels
                        .map(|s| s.split(',').map(|s| s.trim().to_string()).collect::<Vec<_>>())
                        .unwrap_or(cfg_labels);

                    println!("Registering runner '{}' with {}...", cfg_name, resolved_server);
                    let (id, tok) = register_runner(&client, resolved_server, &cfg_name, &resolved_labels).await?;
                    println!("Registered! ID={}, Token={}", id, tok);

                    // Save for future runs
                    let mut updated_cfg = cfg.clone().unwrap_or_default();
                    updated_cfg.server = Some(resolved_server.to_string());
                    updated_cfg.runner_id = Some(id);
                    updated_cfg.token = Some(tok.clone());
                    updated_cfg.name = Some(cfg_name.clone());
                    updated_cfg.labels = Some(resolved_labels);
                    if save_config(&config, &updated_cfg).is_ok() {
                        println!("Config saved to {}", config);
                    }

                    (id, tok, cfg_name)
                }
            };

            println!("Runner {} started (server={})", resolved_name, resolved_server);

            // Spawn heartbeat task (every 30s)
            let hb_client = client.clone();
            let hb_server = resolved_server.to_string();
            let hb_token = resolved_token.clone();
            let hb_id = resolved_id;
            tokio::spawn(async move {
                loop {
                    tokio::time::sleep(std::time::Duration::from_secs(30)).await;
                    send_heartbeat(&hb_client, &hb_server, hb_id, &hb_token).await;
                }
            });

            // Main job polling loop
            loop {
                match poll_job(&client, resolved_server, resolved_id, &resolved_token).await {
                    Ok(Some(job)) => {
                        println!("→ Job #{}: {} (image={})", job.job_id, job.name, job.image.as_deref().unwrap_or("local"));

                        // Start
                        start_job(&client, resolved_server, resolved_id, job.job_id, &resolved_token).await;

                        // Execute
                        let script_str = job.script.join("\n");
                        let (exit_code, log) = if let Some(img) = &job.image {
                            run_job_docker(img, &script_str).await
                        } else {
                            run_job_local(&script_str).await
                        };

                        // Upload log
                        upload_log(&client, resolved_server, resolved_id, job.job_id, &resolved_token, &log).await;

                        // Finish
                        let status = if exit_code == 0 { "success" } else { "failure" };
                        finish_job(&client, resolved_server, resolved_id, job.job_id, &resolved_token, status, exit_code).await;

                        println!("  ✓ {} (exit={})", status, exit_code);
                    }
                    Ok(None) => {
                        // No job available (timeout) — continue polling
                        continue;
                    }
                    Err(e) => {
                        tracing::error!("Poll error: {}", e);
                        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                        continue;
                    }
                }
            }
        }
    }

    Ok(())
}

/// Get the system hostname via `hostname` command.
mod hostname {
    pub fn get() -> std::io::Result<String> {
        std::process::Command::new("hostname")
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))
    }
}
