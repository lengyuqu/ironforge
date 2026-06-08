//! Pipeline runner — executes CI jobs sequentially by stage.
//!
//! Supports two execution modes:
//! - **Local**: `sh -c` (default, when no `image` is specified)
//! - **Docker**: `docker run --rm <image> sh -c` (when `image` field is set)
//!
//! Security measures:
//! - All job executions are bounded by a configurable timeout (default 1 hour).
//! - When Docker is requested but unavailable, the job **fails** instead of
//!   silently falling back to local execution (prevents privilege escalation).
//! - Local execution sanitizes the environment to avoid leaking sensitive vars.
//! - On timeout the child process is killed (not just abandoned).

use anyhow::{Context, Result};
use sea_orm::DatabaseConnection;

use rg_db::ops::pipeline_ops;

/// Default maximum execution time per job: 1 hour.
const DEFAULT_JOB_TIMEOUT_SECS: u64 = 3600;

/// Default CI token scopes for jobs.
/// Grants read access to the triggering repo and packages.
const DEFAULT_CI_TOKEN_SCOPES: &str = "repo:read packages:read";

/// Pipeline runner that executes stages/jobs sequentially.
pub struct PipelineRunner {
    db: DatabaseConnection,
    repo_path: std::path::PathBuf,
    pipeline_id: i64,
    repo_id: i64,
    jwt_secret: Option<String>,
    docker_enabled: bool,
    /// Per-job timeout in seconds (0 = no timeout).
    job_timeout_secs: u64,
}

impl PipelineRunner {
    pub fn new(db: DatabaseConnection, repo_path: &std::path::Path, pipeline_id: i64) -> Self {
        Self {
            db,
            repo_path: repo_path.to_path_buf(),
            pipeline_id,
            repo_id: 0,
            jwt_secret: None,
            docker_enabled: true,
            job_timeout_secs: DEFAULT_JOB_TIMEOUT_SECS,
        }
    }

    /// Create a runner with Docker disabled (local-only mode).
    pub fn new_local_only(db: DatabaseConnection, repo_path: &std::path::Path, pipeline_id: i64) -> Self {
        Self {
            db,
            repo_path: repo_path.to_path_buf(),
            pipeline_id,
            repo_id: 0,
            jwt_secret: None,
            docker_enabled: false,
            job_timeout_secs: DEFAULT_JOB_TIMEOUT_SECS,
        }
    }

    /// Set the repository ID (for CI_JOB_TOKEN generation).
    pub fn set_repo_id(&mut self, repo_id: i64) {
        self.repo_id = repo_id;
    }

    /// Set the JWT secret (for CI_JOB_TOKEN generation).
    /// If not set, CI_JOB_TOKEN will not be provided.
    pub fn set_jwt_secret(&mut self, secret: String) {
        self.jwt_secret = Some(secret);
    }

    /// Set the per-job timeout in seconds. Pass 0 to disable the timeout.
    pub fn set_job_timeout(&mut self, secs: u64) {
        self.job_timeout_secs = secs;
    }

    /// Run the pipeline: iterate stages in order, run jobs in each stage.
    ///
    /// If any job in a stage fails, subsequent stages are skipped
    /// (unless `allow_failure` is set — but that's a future feature).
    pub async fn run(&self) -> Result<()> {
        let now = chrono::Utc::now().naive_utc();

        // Mark pipeline as running
        pipeline_ops::update_pipeline_status(&self.db, self.pipeline_id, "running", Some(now), None)
            .await?;

        // Get stages in order
        let stages = pipeline_ops::list_stages_by_pipeline(&self.db, self.pipeline_id).await?;

        let mut pipeline_failed = false;

        for stage in &stages {
            if pipeline_failed {
                // Skip remaining stages
                pipeline_ops::update_stage_status(
                    &self.db,
                    stage.id,
                    "skipped",
                    None,
                    None,
                )
                .await?;

                // Mark all jobs in this stage as skipped
                let jobs = pipeline_ops::list_jobs_by_stage(&self.db, stage.id).await?;
                for job in jobs {
                    pipeline_ops::update_job_result(
                        &self.db,
                        job.id,
                        "skipped",
                        None,
                        None,
                        None,
                        None,
                    )
                    .await?;
                }
                continue;
            }

            // Mark stage as running
            let stage_start = chrono::Utc::now().naive_utc();
            pipeline_ops::update_stage_status(
                &self.db,
                stage.id,
                "running",
                Some(stage_start),
                None,
            )
            .await?;

            let mut stage_failed = false;
            let jobs = pipeline_ops::list_jobs_by_stage(&self.db, stage.id).await?;

            for job in &jobs {
                let job_result = self.run_job(job.id, &job.script, job.image.as_deref()).await;

                match job_result {
                    Ok((exit_code, log)) => {
                        let status = if exit_code == 0 { "success" } else { "failed" };
                        if exit_code != 0 {
                            stage_failed = true;
                        }
                        if let Err(e) = pipeline_ops::update_job_result(
                            &self.db,
                            job.id,
                            status,
                            Some(exit_code),
                            Some(&log),
                            None,
                            None,
                        )
                        .await
                        {
                            tracing::error!(job_id = job.id, error = %e, "Failed to update job result");
                        }
                    }
                    Err(e) => {
                        tracing::error!(job_id = job.id, "Job execution error: {:#}", e);
                        stage_failed = true;
                        if let Err(e) = pipeline_ops::update_job_result(
                            &self.db,
                            job.id,
                            "failed",
                            Some(-1),
                            Some(&format!("Runner error: {}", e)),
                            None,
                            None,
                        )
                        .await
                        {
                            tracing::error!(job_id = job.id, error = %e, "Failed to update job result");
                        }
                    }
                }
            }

            let stage_end = chrono::Utc::now().naive_utc();
            let stage_status = if stage_failed { "failed" } else { "success" };
            pipeline_ops::update_stage_status(
                &self.db,
                stage.id,
                stage_status,
                None,
                Some(stage_end),
            )
            .await?;

            if stage_failed {
                pipeline_failed = true;
            }
        }

        // Mark pipeline as completed
        let pipeline_end = chrono::Utc::now().naive_utc();
        let pipeline_status = if pipeline_failed { "failed" } else { "success" };
        pipeline_ops::update_pipeline_status(
            &self.db,
            self.pipeline_id,
            pipeline_status,
            None,
            Some(pipeline_end),
        )
        .await?;

        tracing::info!(
            pipeline_id = self.pipeline_id,
            status = pipeline_status,
            "Pipeline completed"
        );

        Ok(())
    }

    /// Run a single job.
    ///
    /// - If `image` is provided and Docker is available: `docker run --rm <image> sh -c <script>`
    /// - If `image` is provided but Docker is NOT available: **fail** (no silent fallback).
    /// - Otherwise: `sh -c <script>` (with timeout).
    ///
    /// Returns (exit_code, stdout+stderr output).
    async fn run_job(&self, job_id: i64, script: &str, image: Option<&str>) -> Result<(i32, String)> {
        let job_start = chrono::Utc::now().naive_utc();

        // Mark job as running
        if let Err(e) = pipeline_ops::update_job_result(
            &self.db,
            job_id,
            "running",
            None,
            None,
            Some(job_start),
            None,
        )
        .await
        {
            tracing::error!(job_id, error = %e, "Failed to update job status to running");
        }

        tracing::info!(job_id, "Running job");

        // Generate CI_JOB_TOKEN if we have the secret and repo_id
        let ci_job_token = if let Some(ref secret) = self.jwt_secret {
            if self.repo_id > 0 {
                rg_core::auth::ci_token::generate_ci_job_token(
                    self.repo_id,
                    self.pipeline_id,
                    job_id,
                    DEFAULT_CI_TOKEN_SCOPES,
                    secret,
                )
                .ok()
            } else {
                None
            }
        } else {
            None
        };

        // When an image is requested but Docker is disabled, fail immediately.
        // Silent fallback to local execution is a security risk: CI scripts
        // written for a sandboxed container would run with the server's full
        // permissions.
        if let Some(img) = image {
            if !self.docker_enabled {
                let msg = format!(
                    "Job requires Docker image '{}' but Docker is disabled on this runner. \
                     Refusing to fall back to local execution for security reasons.",
                    img
                );
                tracing::warn!(job_id, "{}", msg);
                return Err(anyhow::anyhow!("{}", msg));
            }
        }

        let timeout_secs = self.job_timeout_secs;
        let exec_future = async {
            if let Some(img) = image {
                self.run_job_docker(job_id, script, img).await
            } else {
                self.run_job_local(script, &ci_job_token).await
            }
        };

        // Apply timeout if configured
        if timeout_secs > 0 {
            match tokio::time::timeout(std::time::Duration::from_secs(timeout_secs), exec_future).await {
                Ok(result) => result,
                Err(_elapsed) => {
                    let msg = format!("Job timed out after {} seconds", timeout_secs);
                    tracing::warn!(job_id, "{}", msg);
                    Ok((-1, msg))
                }
            }
        } else {
            exec_future.await
        }
    }

    /// Execute script locally via platform-appropriate shell.
    ///
    /// The process runs with a sanitized environment (standard CI vars + PATH/LANG)
    /// plus CI_JOB_TOKEN for authenticated API access.
    async fn run_job_local(&self, script: &str, ci_job_token: &Option<String>) -> Result<(i32, String)> {
        // Build a safe environment with CI standard variables
        let safe_env: Vec<(String, String)> = {
            let mut env = Vec::new();

            // Standard CI marker variables
            env.push(("CI".to_string(), "true".to_string()));
            env.push(("IRONFORGE".to_string(), "true".to_string()));
            env.push(("CI_PIPELINE_ID".to_string(), self.pipeline_id.to_string()));

            // CI_JOB_TOKEN for authenticated API access
            if let Some(ref token) = ci_job_token {
                env.push(("CI_JOB_TOKEN".to_string(), token.clone()));
            }

            // Preserve PATH so that standard tools are available
            if let Ok(path) = std::env::var("PATH") {
                env.push(("PATH".to_string(), path));
            }
            // Preserve LANG for UTF-8 support
            if let Ok(lang) = std::env::var("LANG") {
                env.push(("LANG".to_string(), lang));
            }
            // Add HOME pointing to a temp directory (prevents access to real home)
            env.push(("HOME".to_string(), self.repo_path.to_string_lossy().into_owned()));
            env
        };

        #[cfg(unix)]
        let mut cmd = {
            let mut c = tokio::process::Command::new("sh");
            c.arg("-c")
                .arg(script)
                .current_dir(&self.repo_path)
                .env_clear()
                .envs(safe_env.iter().map(|(k, v)| (k.as_str(), v.as_str())))
                .kill_on_drop(true); // Kill child process when the handle is dropped (timeout)
            c
        };
        
        #[cfg(windows)]
        let mut cmd = {
            let mut c = tokio::process::Command::new("powershell.exe");
            c.args(&["-NoProfile", "-NonInteractive", "-Command", script])
                .current_dir(&self.repo_path)
                .env_clear()
                .envs(safe_env.iter().map(|(k, v)| (k.as_str(), v.as_str())))
                .kill_on_drop(true);
            c
        };

        let output = cmd
            .output()
            .await
            .context("failed to spawn job process")?;

        let exit_code = output.status.code().unwrap_or(-1);
        let mut log = String::new();
        if !output.stdout.is_empty() {
            log.push_str(&String::from_utf8_lossy(&output.stdout));
        }
        if !output.stderr.is_empty() {
            if !log.is_empty() {
                log.push('\n');
            }
            log.push_str(&String::from_utf8_lossy(&output.stderr));
        }

        Ok((exit_code, log))
    }

    /// Execute script inside a Docker container.
    ///
    /// Uses `docker run --rm` with the specified image.
    /// The repo working directory is mounted as a volume.
    /// If Docker is unavailable, the job **fails** — no silent fallback to local.
    async fn run_job_docker(&self, job_id: i64, script: &str, image: &str) -> Result<(i32, String)> {
        let repo_path_str = self.repo_path.to_str()
            .ok_or_else(|| anyhow::anyhow!("repo path is not valid UTF-8"))?;

        // Check if Docker is available
        let docker_check = tokio::process::Command::new("docker")
            .arg("info")
            .output()
            .await
            .context("Docker not found — is docker installed and running?")?;

        if !docker_check.status.success() {
            // SECURITY: Do NOT fall back to local execution.
            // The CI script was written expecting a sandboxed container;
            // running it locally with server permissions is a privilege escalation.
            return Err(anyhow::anyhow!(
                "Docker daemon not available. Job requires image '{}' but cannot run in container. \
                 Refusing to fall back to local execution.",
                image
            ));
        }

        // Generate a unique container name
        let container_name = format!("ironforge-job-{}", job_id);

        // Run: docker run --rm --name <name> -v <repo_path>:/workspace -w /workspace <image> sh -c <script>
        let output = tokio::process::Command::new("docker")
            .args([
                "run",
                "--rm",
                "--name", &container_name,
                "-v", &format!("{}:/workspace", repo_path_str),
                "-w", "/workspace",
                image,
                "sh",
                "-c",
                script,
            ])
            .output()
            .await
            .context("failed to spawn docker run")?;

        let exit_code = output.status.code().unwrap_or(-1);
        let mut log = String::new();
        if !output.stdout.is_empty() {
            log.push_str(&String::from_utf8_lossy(&output.stdout));
        }
        if !output.stderr.is_empty() {
            if !log.is_empty() {
                log.push('\n');
            }
            log.push_str(&String::from_utf8_lossy(&output.stderr));
        }

        // If docker run itself failed (e.g. image not found), provide a clear message
        if exit_code != 0 && log.is_empty() {
            log = format!("Docker container exited with code {} (no output)", exit_code);
        }

        Ok((exit_code, log))
    }
}
