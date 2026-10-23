//! Pipeline runner — executes CI jobs sequentially by stage.
//!
//! Supports two execution modes:
//! - **Docker**: `docker run --rm <image> sh -c` (when `image` field is set)
//! - **Local**: `sh -c` (when no `image` is specified) — **opt-in only**
//!
//! Security measures:
//! - All job executions are bounded by a configurable timeout (default 1 hour).
//! - When Docker is requested but unavailable, the job **fails** instead of
//!   silently falling back to local execution (prevents privilege escalation).
//! - A job with no `image` is **refused** unless the operator set
//!   `allow_host_execution` (config `[ci].allow_host_execution`, CLI
//!   `--allow-host-execution`): its script is repository content, so running it
//!   on the host is code execution as the server user.
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
    /// Whether jobs without an `image` may run on the host. See
    /// `TriggerPipelineParams::allow_host_execution`; defaults to `false`.
    allow_host_execution: bool,
    /// Resource/capability limits applied to job containers.
    docker_limits: rg_ci_isolation::DockerLimits,
    oidc_token_url: Option<String>,
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
            allow_host_execution: false,
            docker_limits: rg_ci_isolation::DockerLimits::default(),
            oidc_token_url: None,
            job_timeout_secs: DEFAULT_JOB_TIMEOUT_SECS,
        }
    }

    /// Create a runner with Docker disabled (local-only mode).
    pub fn new_local_only(
        db: DatabaseConnection,
        repo_path: &std::path::Path,
        pipeline_id: i64,
    ) -> Self {
        Self {
            db,
            repo_path: repo_path.to_path_buf(),
            pipeline_id,
            repo_id: 0,
            jwt_secret: None,
            docker_enabled: false,
            allow_host_execution: false,
            docker_limits: rg_ci_isolation::DockerLimits::default(),
            oidc_token_url: None,
            job_timeout_secs: DEFAULT_JOB_TIMEOUT_SECS,
        }
    }

    /// Opt in to running jobs without an `image` directly on the host.
    ///
    /// This is `false` by default: CI scripts come from repository content, so
    /// host execution grants their authors code execution as the server user.
    pub fn set_allow_host_execution(&mut self, allow: bool) {
        self.allow_host_execution = allow;
    }

    /// Override the resource/capability limits applied to job containers.
    pub fn set_docker_limits(&mut self, limits: rg_ci_isolation::DockerLimits) {
        self.docker_limits = limits;
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

    pub fn set_oidc_token_url(&mut self, url: String) {
        self.oidc_token_url = Some(url);
    }

    /// Set the per-job timeout in seconds. Pass 0 to disable the timeout.
    pub fn set_job_timeout(&mut self, secs: u64) {
        self.job_timeout_secs = secs;
    }

    /// Run the pipeline: iterate stages in order, run jobs in each stage.
    ///
    /// If a non-allowed job in a stage fails, subsequent stages are skipped.
    pub async fn run(&self) -> Result<()> {
        if let Err(error) = self.prepare_workspace().await {
            let now = chrono::Utc::now().naive_utc();
            if let Err(update_error) = pipeline_ops::update_pipeline_status(
                &self.db,
                self.pipeline_id,
                "failed",
                Some(now),
                Some(now),
            )
            .await
            {
                tracing::error!(pipeline_id = self.pipeline_id, %update_error, "failed to mark pipeline failed after workspace error");
            }
            return Err(error);
        }
        let result = self.run_pipeline().await;
        if let Err(error) = self.cleanup_workspace().await {
            tracing::warn!(pipeline_id = self.pipeline_id, %error, "failed to clean CI workspace");
        }
        result
    }

    async fn run_pipeline(&self) -> Result<()> {
        let now = chrono::Utc::now().naive_utc();

        // Mark pipeline as running
        let pipeline_started_at = pipeline_ops::get_pipeline(&self.db, self.pipeline_id)
            .await?
            .filter(|pipeline| pipeline.started_at.is_none())
            .map(|_| now);
        pipeline_ops::update_pipeline_status(
            &self.db,
            self.pipeline_id,
            "running",
            pipeline_started_at,
            None,
        )
        .await?;

        // Get stages in order
        let stages = pipeline_ops::list_stages_by_pipeline(&self.db, self.pipeline_id).await?;

        let mut pipeline_failed = false;

        for stage in &stages {
            if matches!(stage.status.as_str(), "success" | "skipped") {
                continue;
            }
            if matches!(
                stage.status.as_str(),
                "failed" | "failure" | "error" | "canceled"
            ) {
                pipeline_failed = true;
                continue;
            }
            if pipeline_failed {
                // Skip remaining stages
                pipeline_ops::update_stage_status(&self.db, stage.id, "skipped", None, None)
                    .await?;

                // Mark all jobs in this stage as skipped
                let jobs = pipeline_ops::list_jobs_by_stage(&self.db, stage.id).await?;
                for job in jobs {
                    pipeline_ops::update_job_result(
                        &self.db, job.id, "skipped", None, None, None, None,
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
                stage.started_at.is_none().then_some(stage_start),
                None,
            )
            .await?;

            let mut stage_failed = false;
            let jobs = pipeline_ops::list_jobs_by_stage(&self.db, stage.id).await?;

            for job in &jobs {
                if matches!(job.status.as_str(), "success" | "skipped" | "canceled") {
                    continue;
                }
                if matches!(job.status.as_str(), "failed" | "failure" | "error") {
                    if !job.allow_failure {
                        stage_failed = true;
                    }
                    continue;
                }
                if job.status == "manual" {
                    pipeline_ops::update_stage_status(&self.db, stage.id, "manual", None, None)
                        .await?;
                    pipeline_ops::update_pipeline_status(
                        &self.db,
                        self.pipeline_id,
                        "manual",
                        None,
                        None,
                    )
                    .await?;
                    tracing::info!(
                        pipeline_id = self.pipeline_id,
                        job_id = job.id,
                        "Pipeline paused at manual job"
                    );
                    return Ok(());
                }
                if job.status == "waiting_approval" {
                    pipeline_ops::update_stage_status(
                        &self.db,
                        stage.id,
                        "waiting_approval",
                        None,
                        None,
                    )
                    .await?;
                    pipeline_ops::update_pipeline_status(
                        &self.db,
                        self.pipeline_id,
                        "waiting_approval",
                        None,
                        None,
                    )
                    .await?;
                    tracing::info!(
                        pipeline_id = self.pipeline_id,
                        job_id = job.id,
                        "Pipeline paused for environment approval"
                    );
                    return Ok(());
                }
                if job.status != "pending" {
                    anyhow::bail!(
                        "job {} cannot be resumed from unexpected status '{}'",
                        job.id,
                        job.status
                    );
                }
                let job_result = self
                    .run_job(
                        job.id,
                        &job.script,
                        job.image.as_deref(),
                        job.variables.as_deref(),
                        job.cache_key.as_deref(),
                        job.cache_paths.as_deref(),
                        job.timeout_seconds,
                    )
                    .await;

                match job_result {
                    Ok((exit_code, log)) => {
                        let status = if exit_code == 0 { "success" } else { "failed" };
                        if exit_code != 0 && !job.allow_failure {
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
                        if !job.allow_failure {
                            stage_failed = true;
                        }
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

        if pipeline_status == "success" {
            if let (Some(repo_root), Some(pipeline)) = (
                self.repo_path.parent().and_then(std::path::Path::parent),
                pipeline_ops::get_pipeline(&self.db, self.pipeline_id).await?,
            ) {
                if let Err(error) = rg_core::pull_request::try_auto_merges_for_head_commit(
                    &self.db,
                    repo_root,
                    pipeline.repo_id,
                    &pipeline.commit_sha,
                )
                .await
                {
                    tracing::warn!(pipeline_id = self.pipeline_id, %error, "auto-merge evaluation after local CI failed");
                }
                let ci_engine = crate::CiEngine;
                if let Err(error) =
                    rg_core::pull_request::merge_queue::process_for_head_commit_with_ci(
                        &self.db,
                        repo_root,
                        pipeline.repo_id,
                        &pipeline.commit_sha,
                        &rg_core::pull_request::merge_queue::MergeQueueCi {
                            trigger: &ci_engine,
                            docker_enabled: self.docker_enabled,
                            external_runners: false,
                            allow_host_execution: self.allow_host_execution,
                            jwt_secret: self.jwt_secret.as_deref(),
                            external_url: self
                                .oidc_token_url
                                .as_deref()
                                .and_then(|url| url.strip_suffix("/api/v1/ci/oidc/token")),
                        },
                    )
                    .await
                {
                    tracing::warn!(pipeline_id = self.pipeline_id, %error, "merge queue evaluation after local CI failed");
                }
            }
        }

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
    #[allow(clippy::too_many_arguments)]
    async fn run_job(
        &self,
        job_id: i64,
        script: &str,
        image: Option<&str>,
        variables: Option<&str>,
        cache_key: Option<&str>,
        cache_paths: Option<&str>,
        timeout_seconds: Option<i64>,
    ) -> Result<(i32, String)> {
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
                rg_core::auth::ci_token::generate_ci_job_token_with_ttl(
                    self.repo_id,
                    self.pipeline_id,
                    job_id,
                    DEFAULT_CI_TOKEN_SCOPES,
                    secret,
                    timeout_seconds
                        .unwrap_or(self.job_timeout_secs as i64)
                        .clamp(60, 86_400)
                        + 300,
                )
                .ok()
            } else {
                None
            }
        } else {
            None
        };
        let (job_environment, secret_values) = self
            .job_environment(ci_job_token.as_deref(), variables)
            .await?;
        let cache = cache_spec(cache_key, cache_paths, &job_environment)?;
        if let Some((key, _)) = &cache {
            if let Err(error) = self.restore_cache(key).await {
                tracing::warn!(job_id, %error, "CI cache restore failed; continuing without cache");
            }
        }

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

        let timeout_secs = timeout_seconds
            .and_then(|seconds| u64::try_from(seconds).ok())
            .unwrap_or(self.job_timeout_secs)
            .min(86_400);
        // A job with no `image` would run `sh -c <script>` directly on the
        // server host. That script is repository content, so executing it is
        // remote code execution as the server user. It is refused unless the
        // operator explicitly opted in.
        if image.is_none() && !self.allow_host_execution {
            let msg = "Job has no `image` and host execution is disabled on this runner; set `ci.allow_host_execution = true` (or `--allow-host-execution`) to run jobs directly on the host, or give the job an `image`.";
            tracing::warn!(job_id, "{}", msg);
            return Err(anyhow::anyhow!("{}", msg));
        }

        let exec_future = async {
            if let Some(img) = image {
                self.run_job_docker(job_id, script, img, &job_environment)
                    .await
            } else {
                self.run_job_local(script, &job_environment).await
            }
        };

        // Apply timeout if configured
        let result = if timeout_secs > 0 {
            match tokio::time::timeout(std::time::Duration::from_secs(timeout_secs), exec_future)
                .await
            {
                Ok(result) => result,
                Err(_elapsed) => {
                    let msg = format!("Job timed out after {} seconds", timeout_secs);
                    tracing::warn!(job_id, "{}", msg);
                    Ok((-1, msg))
                }
            }
        } else {
            exec_future.await
        };
        if let (Ok((0, _)), Some((key, paths))) = (&result, &cache) {
            if let Err(error) = self.save_cache(key, paths).await {
                tracing::warn!(job_id, %error, "CI cache save failed; job remains successful");
            }
        }
        result
            .map(|(code, log)| {
                (
                    code,
                    rg_core::auth::encryption::mask_values(&log, &secret_values),
                )
            })
            .map_err(|e| {
                // Q3.3: Mask any error message before it reaches the job log,
                // since docker/spawn errors may echo env values or secret content.
                let raw = format!("{:#}", e);
                let masked = rg_core::auth::encryption::mask_values(&raw, &secret_values);
                anyhow::anyhow!(masked)
            })
    }

    /// Execute script locally via platform-appropriate shell.
    ///
    /// The process runs with a sanitized environment (standard CI vars + PATH/LANG)
    /// plus CI_JOB_TOKEN for authenticated API access.
    async fn run_job_local(
        &self,
        script: &str,
        job_environment: &[(String, String)],
    ) -> Result<(i32, String)> {
        #[cfg(unix)]
        let mut cmd = {
            let mut c = tokio::process::Command::new("sh");
            c.arg("-c")
                .arg(script)
                .current_dir(self.workspace_path())
                .env_clear()
                .envs(
                    job_environment
                        .iter()
                        .map(|(k, v)| (k.as_str(), v.as_str())),
                )
                .kill_on_drop(true); // Kill child process when the handle is dropped (timeout)
            c
        };

        #[cfg(windows)]
        let mut cmd = {
            let mut c = tokio::process::Command::new("powershell.exe");
            c.args(["-NoProfile", "-NonInteractive", "-Command", script])
                .current_dir(self.workspace_path())
                .env_clear()
                .envs(
                    job_environment
                        .iter()
                        .map(|(k, v)| (k.as_str(), v.as_str())),
                )
                .kill_on_drop(true);
            // PowerShell cannot boot without SystemRoot & friends — see
            // rg_core::platform::process::POWERSHELL_ENV_KEYS.
            for (key, value) in rg_core::platform::process::powershell_env_vars() {
                c.env(key, value);
            }
            c
        };

        let output = cmd.output().await.context("failed to spawn job process")?;

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
    async fn run_job_docker(
        &self,
        job_id: i64,
        script: &str,
        image: &str,
        job_environment: &[(String, String)],
    ) -> Result<(i32, String)> {
        let workspace_path = self.workspace_path();
        let repo_path_str = workspace_path
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("repo path is not valid UTF-8"))?;

        // `image` is repository content: validate it before it reaches the
        // `docker run` argument list, where a value like `--privileged` would
        // be consumed as an option.
        rg_ci_isolation::validate_container_image(image)
            .map_err(|error| anyhow::anyhow!("refusing container image {image:?}: {error}"))?;

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
            //
            // B-014 LIMITATION: The error is returned and eventually written to
            // the job log (see the Err branch in run_pipeline), but no proactive
            // notification (email, webhook, etc.) is sent to the repo owner.
            // Users must check the job log to discover why their pipeline failed.
            // Future improvement: send an email or in-app notification to the
            // repo owner when Docker unavailability causes a job failure.
            return Err(anyhow::anyhow!(
                "Docker daemon not available. Job requires image '{}' but cannot run in container. \
                 Refusing to fall back to local execution.",
                image
            ));
        }

        // Generate a unique container name
        let container_name = format!("ironforge-job-{}", job_id);

        // Run: docker run --rm --name <name> -v <repo_path>:/workspace -w /workspace <image> sh -c <script>
        let mut args = vec![
            "run".to_string(),
            "--rm".to_string(),
            "--name".to_string(),
            container_name,
            "-v".to_string(),
            format!("{}:/workspace", repo_path_str),
            "-w".to_string(),
            "/workspace".to_string(),
        ];
        // Bound resources and drop capabilities. Without these a job could
        // exhaust the host or escalate inside the container.
        args.extend(self.docker_limits.hardening_args());
        for (key, _) in job_environment {
            if key == "HOME" {
                continue;
            }
            args.push("-e".to_string());
            // Pass only the variable name on the command line. The value is
            // inherited from the Docker CLI environment so CI_JOB_TOKEN and
            // future secrets are not exposed in the host process arguments.
            args.push(key.clone());
        }
        args.extend([
            "-e".to_string(),
            "HOME=/tmp".to_string(),
        ]);
        // `--` ends option parsing so the validated image cannot be mistaken
        // for a flag even if a future Docker version accepts new options.
        args.push("--".to_string());
        args.extend([
            image.to_string(),
            "sh".to_string(),
            "-c".to_string(),
            script.to_string(),
        ]);
        let mut command = tokio::process::Command::new("docker");
        command.args(&args);
        for (key, value) in job_environment {
            if key != "HOME" {
                command.env(key, value);
            }
        }
        let output = command
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
            log = format!(
                "Docker container exited with code {} (no output)",
                exit_code
            );
        }

        Ok((exit_code, log))
    }

    async fn job_environment(
        &self,
        ci_job_token: Option<&str>,
        variables: Option<&str>,
    ) -> Result<(Vec<(String, String)>, Vec<String>)> {
        let pipeline = pipeline_ops::get_pipeline(&self.db, self.pipeline_id)
            .await?
            .context("pipeline not found while preparing job environment")?;
        let mut env = std::collections::BTreeMap::new();
        let mut secret_values = Vec::new();
        if let Some(json) = variables {
            let configured: std::collections::HashMap<String, String> =
                serde_json::from_str(json).context("invalid stored CI job variables")?;
            for (key, value) in configured {
                if valid_environment_name(&key) && !is_reserved_ci_variable(&key) {
                    env.insert(key, value);
                } else {
                    tracing::warn!(variable = %key, "ignoring invalid or reserved CI variable");
                }
            }
        }
        if self.repo_id > 0 {
            if let Some(jwt_secret) = &self.jwt_secret {
                let key = rg_core::auth::encryption::derive_key(jwt_secret);
                for secret in
                    rg_db::ops::ci_secret_ops::list_by_repo(&self.db, self.repo_id).await?
                {
                    if !valid_environment_name(&secret.name)
                        || is_reserved_ci_variable(&secret.name)
                    {
                        continue;
                    }
                    let value = rg_core::auth::encryption::decrypt(&secret.encrypted_value, &key)
                        .with_context(|| {
                        format!("failed to decrypt CI secret '{}'", secret.name)
                    })?;
                    secret_values.push(value.clone());
                    env.insert(secret.name, value);
                }
            }
        }
        env.insert("CI".into(), "true".into());
        env.insert("IRONFORGE".into(), "true".into());
        env.insert("CI_PIPELINE_ID".into(), self.pipeline_id.to_string());
        env.insert("CI_COMMIT_SHA".into(), pipeline.commit_sha.clone());
        env.insert("CI_SHA".into(), pipeline.commit_sha);
        env.insert("CI_REF".into(), pipeline.ref_name);
        env.insert("CI_EVENT".into(), pipeline.trigger_type);
        if let Some(token) = ci_job_token {
            env.insert("CI_JOB_TOKEN".into(), token.to_string());
        }
        if let Some(url) = &self.oidc_token_url {
            env.insert("CI_OIDC_TOKEN_URL".into(), url.clone());
        }
        if let Ok(path) = std::env::var("PATH") {
            env.insert("PATH".into(), path);
        }
        if let Ok(lang) = std::env::var("LANG") {
            env.insert("LANG".into(), lang);
        }
        env.insert(
            "HOME".into(),
            self.workspace_path().to_string_lossy().into_owned(),
        );
        Ok((env.into_iter().collect(), secret_values))
    }

    fn workspace_path(&self) -> std::path::PathBuf {
        let root = self
            .repo_path
            .parent()
            .and_then(std::path::Path::parent)
            .unwrap_or_else(|| self.repo_path.parent().unwrap_or(&self.repo_path));
        root.join("_ci_workspaces")
            .join(self.repo_id.to_string())
            .join(self.pipeline_id.to_string())
    }

    /// Materialise the pipeline commit as a detached linked worktree.
    ///
    /// This used to shell out to `git worktree add --detach`. gix 0.90 exposes
    /// native linked-worktree support (`Repository::add_worktree`), so the CI
    /// path stays inside the library. The repository is bare, which gix handles
    /// by writing the worktree's private Git directory below
    /// `<git-dir>/worktrees/<id>` just like Git does.
    async fn prepare_workspace(&self) -> Result<()> {
        let pipeline = pipeline_ops::get_pipeline(&self.db, self.pipeline_id)
            .await?
            .context("pipeline not found while preparing workspace")?;
        let repo_path = self.repo_path.clone();
        let workspace = self.workspace_path();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let repo = open_repo(&repo_path)?;
            // A crashed run can leave both the checkout and its registration
            // behind. Drop the stale registration first: `add_worktree` refuses
            // to reuse a destination that is still registered.
            let stale = registered_worktrees_at(&repo, &workspace);
            if workspace.exists() {
                std::fs::remove_dir_all(&workspace).context("remove stale CI workspace")?;
            }
            for git_dir in stale {
                if git_dir.exists() {
                    std::fs::remove_dir_all(&git_dir).with_context(|| {
                        format!("prune stale worktree registration {}", git_dir.display())
                    })?;
                }
            }
            if let Some(parent) = workspace.parent() {
                std::fs::create_dir_all(parent).context("create CI workspace parent")?;
            }
            let commit = gix::ObjectId::from_hex(pipeline.commit_sha.as_bytes()).map_err(|error| {
                anyhow::anyhow!(
                    "pipeline commit {:?} is not a valid object id: {error}",
                    pipeline.commit_sha
                )
            })?;
            let interrupt = std::sync::atomic::AtomicBool::new(false);
            let (_worktree, _outcome) = repo
                .add_worktree(
                    &workspace,
                    gix::worktree::add::Head::Detached(commit),
                    gix::progress::Discard,
                    &interrupt,
                )
                .with_context(|| {
                    format!(
                        "failed to create CI worktree at {} for commit {}",
                        workspace.display(),
                        pipeline.commit_sha
                    )
                })?;
            Ok(())
        })
        .await?
    }

    /// Discard the pipeline's worktree.
    ///
    /// `Force::DiscardChanges` is the exact equivalent of `git worktree remove
    /// --force`: it throws away local modifications and initialised submodules
    /// but still refuses to touch a worktree an operator locked. Anything the
    /// library refuses to remove falls back to deleting the checkout directory,
    /// and the now-dangling registration is pruned so the path can be reused.
    async fn cleanup_workspace(&self) -> Result<()> {
        let repo_path = self.repo_path.clone();
        let workspace = self.workspace_path();
        let pipeline_id = self.pipeline_id;
        tokio::task::spawn_blocking(move || -> Result<()> {
            let repo = open_repo(&repo_path)?;
            // Collect registrations before the checkout disappears — `base()`
            // reads the pointer file inside the worktree's Git directory.
            let registered = registered_worktrees_at(&repo, &workspace);
            if let Err(error) = repo.remove_worktree(
                &workspace,
                gix::worktree::remove::Force::DiscardChanges,
                gix::progress::Discard,
            ) {
                tracing::warn!(
                    pipeline_id,
                    workspace = %workspace.display(),
                    %error,
                    "gitoxide refused to remove the CI worktree; falling back to deleting the checkout"
                );
                if workspace.exists() {
                    std::fs::remove_dir_all(&workspace).context("remove CI workspace")?;
                }
                for git_dir in registered {
                    if git_dir.exists() {
                        std::fs::remove_dir_all(&git_dir).with_context(|| {
                            format!("prune worktree registration {}", git_dir.display())
                        })?;
                    }
                }
            }
            Ok(())
        })
        .await?
    }

    fn cache_archive_path(&self, key: &str) -> std::path::PathBuf {
        use sha2::Digest;
        let digest = sha2::Sha256::digest(key.as_bytes());
        let name = digest
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let root = self
            .repo_path
            .parent()
            .and_then(std::path::Path::parent)
            .unwrap_or_else(|| self.repo_path.parent().unwrap_or(&self.repo_path));
        root.join("_ci_cache")
            .join(self.repo_id.to_string())
            .join(format!("{name}.tar"))
    }

    async fn restore_cache(&self, key: &str) -> Result<()> {
        let archive = self.cache_archive_path(key);
        let key_hash = cache_key_hash(key);
        let policy = rg_db::ops::ci_retention_ops::get_policy(&self.db, self.repo_id).await?;
        if let Some(entry) =
            rg_db::ops::ci_retention_ops::find_cache_entry(&self.db, self.repo_id, &key_hash)
                .await?
        {
            if entry.expires_at <= chrono::Utc::now() {
                let _ = std::fs::remove_file(&archive);
                rg_db::ops::ci_retention_ops::delete_cache_entry(&self.db, entry.id).await?;
                return Ok(());
            }
        }
        if !archive.exists() {
            return Ok(());
        }
        let size = std::fs::metadata(&archive)?.len() as i64;
        tar::Archive::new(std::fs::File::open(&archive)?)
            .unpack(self.workspace_path())
            .context("unpack CI cache")?;
        rg_db::ops::ci_retention_ops::upsert_cache_entry(
            &self.db,
            self.repo_id,
            &key_hash,
            archive.to_string_lossy().as_ref(),
            size,
            policy.cache_retention_days,
        )
        .await?;
        Ok(())
    }

    async fn save_cache(&self, key: &str, paths: &[String]) -> Result<()> {
        let archive = self.cache_archive_path(key);
        if let Some(parent) = archive.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let temporary = archive.with_extension("tar.tmp");
        let file = std::fs::File::create(&temporary)?;
        let mut builder = tar::Builder::new(file);
        let workspace = self.workspace_path();
        for path in paths {
            let source = workspace.join(path);
            if source.is_dir() {
                builder.append_dir_all(path, &source)?;
            } else if source.is_file() {
                builder.append_path_with_name(&source, path)?;
            }
        }
        builder.finish()?;
        std::fs::rename(temporary, &archive)?;
        let size = std::fs::metadata(&archive)?.len() as i64;
        let policy = rg_db::ops::ci_retention_ops::get_policy(&self.db, self.repo_id).await?;
        if let Err(error) = rg_db::ops::ci_retention_ops::upsert_cache_entry(
            &self.db,
            self.repo_id,
            &cache_key_hash(key),
            archive.to_string_lossy().as_ref(),
            size,
            policy.cache_retention_days,
        )
        .await
        {
            let _ = std::fs::remove_file(&archive);
            return Err(error);
        }
        Ok(())
    }
}

fn cache_key_hash(key: &str) -> String {
    use sha2::Digest;
    hex::encode(sha2::Sha256::digest(key.as_bytes()))
}

/// Open a repository for worktree management.
///
/// IronForge stores bare repositories, so there is no working tree to discover:
/// `open` on the Git directory itself is the correct entry point.
fn open_repo(repo_path: &std::path::Path) -> Result<gix::Repository> {
    gix::open(repo_path).with_context(|| format!("open repository at {}", repo_path.display()))
}

/// Return the private Git directories of linked worktrees checked out at `target`.
///
/// A registration is only reported when its recorded checkout path matches, so
/// a lock held on some unrelated worktree is never considered for pruning.
fn registered_worktrees_at(repo: &gix::Repository, target: &std::path::Path) -> Vec<std::path::PathBuf> {
    let Ok(proxies) = repo.worktrees() else {
        return Vec::new();
    };
    proxies
        .iter()
        .filter(|proxy| !proxy.is_locked())
        .filter(|proxy| proxy.base().is_ok_and(|base| same_path(&base, target)))
        .map(|proxy| proxy.git_dir().to_path_buf())
        .collect()
}

/// Compare two paths, falling back to canonicalisation for symlinked layouts.
fn same_path(left: &std::path::Path, right: &std::path::Path) -> bool {
    if left == right {
        return true;
    }
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

fn cache_spec(
    key: Option<&str>,
    paths: Option<&str>,
    environment: &[(String, String)],
) -> Result<Option<(String, Vec<String>)>> {
    let (Some(key), Some(paths)) = (key, paths) else {
        return Ok(None);
    };
    let mut key = key.to_owned();
    for (name, value) in environment {
        key = key
            .replace(&format!("${{{name}}}"), value)
            .replace(&format!("${name}"), value);
    }
    if key.is_empty() || key.len() > 512 {
        anyhow::bail!("CI cache key must contain 1-512 bytes");
    }
    let paths: Vec<String> =
        serde_json::from_str(paths).context("invalid stored CI cache paths")?;
    if paths.is_empty() || paths.len() > 64 {
        anyhow::bail!("CI cache requires 1-64 paths");
    }
    for path in &paths {
        let path_value = std::path::Path::new(path);
        if path_value.is_absolute()
            || path_value.components().any(|component| {
                matches!(
                    component,
                    std::path::Component::ParentDir
                        | std::path::Component::RootDir
                        | std::path::Component::Prefix(_)
                )
            })
        {
            anyhow::bail!("CI cache path must stay within the workspace: {path}");
        }
    }
    Ok(Some((key, paths)))
}

fn valid_environment_name(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some('_' | 'A'..='Z' | 'a'..='z'))
        && chars.all(|ch| matches!(ch, '_' | 'A'..='Z' | 'a'..='z' | '0'..='9'))
}

fn is_reserved_ci_variable(name: &str) -> bool {
    matches!(
        name,
        "CI" | "IRONFORGE"
            | "CI_PIPELINE_ID"
            | "CI_COMMIT_SHA"
            | "CI_SHA"
            | "CI_REF"
            | "CI_EVENT"
            | "CI_JOB_TOKEN"
            | "CI_OIDC_TOKEN_URL"
            | "HOME"
            | "PATH"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::{NotSet, Set};

    #[test]
    fn validates_environment_names_and_protects_runner_variables() {
        assert!(valid_environment_name("DEPLOY_TARGET_2"));
        assert!(!valid_environment_name("2TARGET"));
        assert!(!valid_environment_name("BAD-NAME"));
        assert!(is_reserved_ci_variable("CI_JOB_TOKEN"));
        assert!(!is_reserved_ci_variable("PROJECT_MODE"));
    }

    #[test]
    fn masks_longest_secret_values_first() {
        assert_eq!(
            rg_core::auth::encryption::mask_values(
                "token=abcdef and abcd",
                &["abcd".into(), "abcdef".into()]
            ),
            "token=*** and ***"
        );
    }

    #[test]
    fn cache_config_resolves_variables_and_rejects_workspace_escape() {
        let env = vec![("CI_SHA".into(), "abc123".into())];
        let cache = cache_spec(Some("build-${CI_SHA}"), Some(r#"["target"]"#), &env)
            .unwrap()
            .unwrap();
        assert_eq!(cache.0, "build-abc123");
        assert!(cache_spec(Some("bad"), Some(r#"["../outside"]"#), &env).is_err());
    }

    #[tokio::test]
    async fn repository_scoped_cache_round_trips_workspace_paths() {
        let temp = tempfile::tempdir().unwrap();
        let db = rg_db::connect(&format!(
            "sqlite://{}?mode=rwc",
            temp.path().join("cache.db").display()
        ))
        .await
        .unwrap();
        rg_db::run_migrations(&db).await.unwrap();
        let user = rg_db::ops::user_ops::create_user(
            &db,
            "cache-owner",
            "cache-owner@example.com",
            "unused",
            "Cache Owner",
        )
        .await
        .unwrap();
        let now = chrono::Utc::now();
        let repository = rg_db::ops::repo_ops::create(
            &db,
            rg_db::entities::repository::ActiveModel {
                id: NotSet,
                owner_id: Set(user.id),
                name: Set("repo".into()),
                description: Set(None),
                is_private: Set(true),
                default_branch: Set("main".into()),
                fork_id: Set(None),
                stars_count: Set(0),
                forks_count: Set(0),
                org_id: Set(None),
                created_at: Set(now),
                updated_at: Set(now),
                deleted_at: Set(None),
                origin_repo_id: Set(None),
            },
        )
        .await
        .unwrap();
        let repo_path = temp.path().join("repos/owner/repo.git");
        std::fs::create_dir_all(&repo_path).unwrap();
        let mut runner = PipelineRunner::new_local_only(db, &repo_path, 77);
        runner.set_repo_id(repository.id);
        let workspace = runner.workspace_path();
        std::fs::create_dir_all(workspace.join("target")).unwrap();
        std::fs::write(workspace.join("target/cache.txt"), "cached").unwrap();
        runner
            .save_cache("build-main", &["target".into()])
            .await
            .unwrap();
        std::fs::remove_dir_all(&workspace).unwrap();
        std::fs::create_dir_all(&workspace).unwrap();
        runner.restore_cache("build-main").await.unwrap();
        assert_eq!(
            std::fs::read_to_string(workspace.join("target/cache.txt")).unwrap(),
            "cached"
        );
    }

    /// CI workspaces are linked worktrees of a *bare* repository. gix only grew
    /// native worktree support in 0.89/0.90, so this pins the two properties the
    /// runner depends on: the checkout is materialised outside the Git
    /// directory, and cleanup drops both the checkout and its registration even
    /// when a job left local modifications behind (`--force` semantics).
    #[tokio::test]
    async fn workspace_is_a_linked_worktree_of_the_bare_repository() {
        let temp = tempfile::tempdir().unwrap();
        let db = rg_db::connect(&format!(
            "sqlite://{}?mode=rwc",
            temp.path().join("ci-worktree.db").display()
        ))
        .await
        .unwrap();
        rg_db::run_migrations(&db).await.unwrap();
        let user = rg_db::ops::user_ops::create_user(
            &db,
            "worktree-owner",
            "worktree@example.com",
            "unused",
            "Worktree Owner",
        )
        .await
        .unwrap();
        let now = chrono::Utc::now();
        let repository = rg_db::ops::repo_ops::create(
            &db,
            rg_db::entities::repository::ActiveModel {
                id: NotSet,
                owner_id: Set(user.id),
                name: Set("repo".into()),
                description: Set(None),
                is_private: Set(true),
                default_branch: Set("main".into()),
                fork_id: Set(None),
                stars_count: Set(0),
                forks_count: Set(0),
                org_id: Set(None),
                created_at: Set(now),
                updated_at: Set(now),
                deleted_at: Set(None),
                origin_repo_id: Set(None),
            },
        )
        .await
        .unwrap();

        // Build a commit in a throw-away worktree and then take a bare clone of
        // it: that is the layout the server stores and the one the runner sees.
        let git = rg_git::cli_gateway::global_gateway().as_ref().unwrap();
        let source = temp.path().join("source");
        std::fs::create_dir_all(&source).unwrap();
        let run = |args: &[&str], cwd: Option<&std::path::Path>| {
            assert!(
                git.run(args, cwd).unwrap().success(),
                "git {args:?} failed while setting up the fixture"
            );
        };
        run(&["init", "-b", "main", &source.to_string_lossy()], None);
        run(&["config", "user.email", "ci@example.com"], Some(&source));
        run(&["config", "user.name", "CI"], Some(&source));
        std::fs::write(source.join("README.md"), "workspace\n").unwrap();
        run(&["add", "-A"], Some(&source));
        run(&["commit", "-m", "initial"], Some(&source));
        let commit_sha = git
            .run(&["rev-parse", "HEAD"], Some(&source))
            .unwrap()
            .stdout_str()
            .trim()
            .to_owned();

        let bare = temp.path().join("repos/owner/repo.git");
        std::fs::create_dir_all(bare.parent().unwrap()).unwrap();
        run(
            &["clone", "--bare", &source.to_string_lossy(), &bare.to_string_lossy()],
            None,
        );
        let registrations = || -> Vec<std::path::PathBuf> {
            match std::fs::read_dir(bare.join("worktrees")) {
                Ok(entries) => entries.filter_map(|entry| entry.ok()).map(|e| e.path()).collect(),
                Err(_) => Vec::new(),
            }
        };
        assert!(registrations().is_empty(), "a fresh bare clone has no worktrees");

        let pipeline = rg_db::ops::pipeline_ops::create_pipeline(
            &db,
            repository.id,
            &commit_sha,
            "refs/heads/main",
            "manual",
            Some(user.id),
        )
        .await
        .unwrap();
        let mut runner = PipelineRunner::new_local_only(db, &bare, pipeline.id);
        runner.set_repo_id(repository.id);
        let workspace = runner.workspace_path();

        runner.prepare_workspace().await.unwrap();
        assert_eq!(
            std::fs::read_to_string(workspace.join("README.md")).unwrap(),
            "workspace\n",
            "the pipeline commit must be checked out into the workspace"
        );
        assert_eq!(
            registrations().len(),
            1,
            "the checkout must be registered as a linked worktree of the bare repository"
        );

        // Re-preparing without a cleanup models a crashed run: the checkout and
        // its registration are still around, and the retry has to recover
        // instead of tripping over the orphaned registration.
        runner
            .prepare_workspace()
            .await
            .expect("preparing again must recover from a leftover worktree");
        assert_eq!(
            registrations().len(),
            1,
            "recovery must refresh the registration instead of adding a second one"
        );
        assert_eq!(
            std::fs::read_to_string(workspace.join("README.md")).unwrap(),
            "workspace\n"
        );

        // A job that leaves the tree dirty must not block cleanup.
        std::fs::write(workspace.join("README.md"), "modified by a job\n").unwrap();

        runner.cleanup_workspace().await.unwrap();
        assert!(!workspace.exists(), "cleanup must delete the checkout");
        assert!(
            registrations().is_empty(),
            "cleanup must also drop the worktree registration, otherwise the path cannot be reused"
        );
    }

    #[tokio::test]
    async fn persisted_job_variables_reach_the_local_runner() {
        let temp = tempfile::tempdir().unwrap();
        let db_path = temp.path().join("ci.db");
        let db = rg_db::connect(&format!("sqlite://{}?mode=rwc", db_path.display()))
            .await
            .unwrap();
        rg_db::run_migrations(&db).await.unwrap();
        let user = rg_db::ops::user_ops::create_user(
            &db,
            "ci-vars",
            "ci-vars@example.com",
            "unused",
            "CI Vars",
        )
        .await
        .unwrap();
        let now = chrono::Utc::now();
        let repo = rg_db::ops::repo_ops::create(
            &db,
            rg_db::entities::repository::ActiveModel {
                id: NotSet,
                owner_id: Set(user.id),
                name: Set("variables".into()),
                description: Set(None),
                is_private: Set(true),
                default_branch: Set("main".into()),
                fork_id: Set(None),
                stars_count: Set(0),
                forks_count: Set(0),
                org_id: Set(None),
                created_at: Set(now),
                updated_at: Set(now),
                deleted_at: Set(None),
                origin_repo_id: Set(None),
            },
        )
        .await
        .unwrap();
        let repo_path = temp.path().join("repos/ci-vars/variables.git");
        std::fs::create_dir_all(&repo_path).unwrap();
        let git = rg_git::cli_gateway::global_gateway().as_ref().unwrap();
        assert!(git.run(&["init"], Some(&repo_path)).unwrap().success());
        assert!(git
            .run(&["config", "user.name", "CI Test"], Some(&repo_path))
            .unwrap()
            .success());
        assert!(git
            .run(
                &["config", "user.email", "ci@example.com"],
                Some(&repo_path)
            )
            .unwrap()
            .success());
        std::fs::write(repo_path.join("README.md"), "workspace").unwrap();
        assert!(git
            .run(&["add", "README.md"], Some(&repo_path))
            .unwrap()
            .success());
        assert!(git
            .run(&["commit", "-m", "initial"], Some(&repo_path))
            .unwrap()
            .success());
        let commit_sha = git
            .run(&["rev-parse", "HEAD"], Some(&repo_path))
            .unwrap()
            .stdout_str()
            .trim()
            .to_owned();
        let pipeline = rg_db::ops::pipeline_ops::create_pipeline(
            &db,
            repo.id,
            &commit_sha,
            "refs/heads/main",
            "manual",
            Some(user.id),
        )
        .await
        .unwrap();
        let stage = rg_db::ops::pipeline_ops::create_stage(&db, pipeline.id, "test", 0)
            .await
            .unwrap();
        // Job scripts run through the platform shell (`sh` on unix,
        // `powershell.exe -Command` on Windows), so the assertion script has to
        // be written in that shell's syntax.
        #[cfg(unix)]
        let variables_script = format!(
            "test \"$MESSAGE\" = hello && test \"$CI_SHA\" = {commit_sha} \
             && test -f README.md && echo \"secret=$DEPLOY_SECRET\""
        );
        #[cfg(windows)]
        let variables_script = format!(
            "if ($env:MESSAGE -ne 'hello') {{ exit 1 }}; \
             if ($env:CI_SHA -ne '{commit_sha}') {{ exit 1 }}; \
             if (-not (Test-Path README.md)) {{ exit 1 }}; \
             \"secret=$env:DEPLOY_SECRET\""
        );
        let job = rg_db::ops::pipeline_ops::create_job(
            &db,
            stage.id,
            "variables",
            &variables_script,
            None,
            None,
            Some(r#"{"MESSAGE":"hello","CI_JOB_TOKEN":"must-not-override"}"#),
            None,
            None,
            false,
            None,
            None,
            None,
        )
        .await
        .unwrap();
        let allowed_failure = rg_db::ops::pipeline_ops::create_job(
            &db,
            stage.id,
            "allowed-failure",
            "exit 7",
            None,
            None,
            None,
            None,
            None,
            true,
            None,
            None,
            None,
        )
        .await
        .unwrap();
        let allowed_timeout = rg_db::ops::pipeline_ops::create_job(
            &db,
            stage.id,
            "allowed-timeout",
            "sleep 2",
            None,
            None,
            None,
            None,
            None,
            true,
            Some(1),
            None,
            None,
        )
        .await
        .unwrap();

        let jwt_secret = "runner-encryption-key";
        let encrypted = rg_core::auth::encryption::encrypt(
            "super-secret-value",
            &rg_core::auth::encryption::derive_key(jwt_secret),
        )
        .unwrap();
        rg_db::ops::ci_secret_ops::upsert(&db, repo.id, "DEPLOY_SECRET", &encrypted, user.id)
            .await
            .unwrap();

        let mut runner = PipelineRunner::new_local_only(db.clone(), &repo_path, pipeline.id);
        runner.set_repo_id(repo.id);
        runner.set_jwt_secret(jwt_secret.into());
        // This test exercises host execution deliberately.
        runner.set_allow_host_execution(true);
        runner.run().await.unwrap();

        let completed = rg_db::ops::pipeline_ops::get_job(&db, job.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(completed.status, "success", "{:?}", completed.log);
        let log = completed.log.unwrap_or_default();
        assert!(log.contains("secret=***"));
        assert!(!log.contains("super-secret-value"));
        let allowed_failure = rg_db::ops::pipeline_ops::get_job(&db, allowed_failure.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(allowed_failure.status, "failed");
        let allowed_timeout = rg_db::ops::pipeline_ops::get_job(&db, allowed_timeout.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(allowed_timeout.status, "failed");
        assert!(allowed_timeout
            .log
            .unwrap_or_default()
            .contains("timed out"));
        assert_eq!(
            rg_db::ops::pipeline_ops::get_pipeline(&db, pipeline.id)
                .await
                .unwrap()
                .unwrap()
                .status,
            "success"
        );

        let manual_pipeline = rg_db::ops::pipeline_ops::create_pipeline(
            &db,
            repo.id,
            &commit_sha,
            "refs/heads/main",
            "manual-resume-test",
            Some(user.id),
        )
        .await
        .unwrap();
        let manual_stage =
            rg_db::ops::pipeline_ops::create_stage(&db, manual_pipeline.id, "deploy", 0)
                .await
                .unwrap();
        let automatic_job = rg_db::ops::pipeline_ops::create_job(
            &db,
            manual_stage.id,
            "prepare",
            "echo prepared",
            None,
            None,
            None,
            None,
            None,
            false,
            None,
            None,
            None,
        )
        .await
        .unwrap();
        let manual_job = rg_db::ops::pipeline_ops::create_job(
            &db,
            manual_stage.id,
            "deploy",
            "echo deployed",
            None,
            None,
            None,
            None,
            None,
            false,
            None,
            Some("manual"),
            None,
        )
        .await
        .unwrap();

        let mut runner = PipelineRunner::new_local_only(db.clone(), &repo_path, manual_pipeline.id);
        runner.set_allow_host_execution(true);
        runner.run().await.unwrap();
        assert_eq!(
            rg_db::ops::pipeline_ops::get_pipeline(&db, manual_pipeline.id)
                .await
                .unwrap()
                .unwrap()
                .status,
            "manual"
        );
        assert_eq!(
            rg_db::ops::pipeline_ops::get_job(&db, manual_job.id)
                .await
                .unwrap()
                .unwrap()
                .status,
            "manual"
        );

        assert!(
            rg_db::ops::pipeline_ops::play_manual_job(&db, manual_job.id)
                .await
                .unwrap()
        );
        assert!(
            !rg_db::ops::pipeline_ops::play_manual_job(&db, manual_job.id)
                .await
                .unwrap()
        );
        rg_db::ops::pipeline_ops::resume_pipeline_chain(&db, manual_pipeline.id, manual_stage.id)
            .await
            .unwrap();
        let mut runner = PipelineRunner::new_local_only(db.clone(), &repo_path, manual_pipeline.id);
        runner.set_allow_host_execution(true);
        runner.run().await.unwrap();
        assert_eq!(
            rg_db::ops::pipeline_ops::get_pipeline(&db, manual_pipeline.id)
                .await
                .unwrap()
                .unwrap()
                .status,
            "success"
        );
        assert_eq!(
            rg_db::ops::pipeline_ops::get_job(&db, automatic_job.id)
                .await
                .unwrap()
                .unwrap()
                .log
                .unwrap_or_default()
                .matches("prepared")
                .count(),
            1
        );
    }

    /// Regression: a job without an `image` must not silently run on the host.
    /// Host execution is RCE-as-the-server-user and is now opt-in.
    #[tokio::test]
    async fn job_without_image_is_refused_unless_host_execution_is_enabled() {
        let temp = tempfile::tempdir().unwrap();
        let db_path = temp.path().join("ci-host-exec.db");
        let db = rg_db::connect(&format!("sqlite://{}?mode=rwc", db_path.display()))
            .await
            .unwrap();
        rg_db::run_migrations(&db).await.unwrap();
        let user = rg_db::ops::user_ops::create_user(
            &db,
            "host-exec-owner",
            "host-exec-owner@example.com",
            "unused",
            "Host Exec Owner",
        )
        .await
        .unwrap();
        let now = chrono::Utc::now();
        let repo = rg_db::ops::repo_ops::create(
            &db,
            rg_db::entities::repository::ActiveModel {
                id: NotSet,
                owner_id: Set(user.id),
                name: Set("host-exec".into()),
                description: Set(None),
                is_private: Set(true),
                default_branch: Set("main".into()),
                fork_id: Set(None),
                stars_count: Set(0),
                forks_count: Set(0),
                org_id: Set(None),
                created_at: Set(now),
                updated_at: Set(now),
                deleted_at: Set(None),
                origin_repo_id: Set(None),
            },
        )
        .await
        .unwrap();
        let repo_path = temp.path().join("repos/host-exec/host-exec.git");
        std::fs::create_dir_all(&repo_path).unwrap();
        // The runner materialises a worktree, so the target must be a real
        // (bare) repository with at least one commit.
        let git = rg_git::cli_gateway::global_gateway().as_ref().unwrap();
        assert!(git.run(&["init"], Some(&repo_path)).unwrap().success());
        assert!(git
            .run(&["config", "user.name", "Host Exec"], Some(&repo_path))
            .unwrap()
            .success());
        assert!(git
            .run(
                &["config", "user.email", "host-exec@example.com"],
                Some(&repo_path),
            )
            .unwrap()
            .success());
        std::fs::write(repo_path.join("README.md"), "host exec").unwrap();
        assert!(git
            .run(&["add", "README.md"], Some(&repo_path))
            .unwrap()
            .success());
        assert!(git
            .run(&["commit", "-m", "initial"], Some(&repo_path))
            .unwrap()
            .success());
        let commit_sha = git
            .run(&["rev-parse", "HEAD"], Some(&repo_path))
            .unwrap()
            .stdout_str()
            .trim()
            .to_owned();

        let pipeline = rg_db::ops::pipeline_ops::create_pipeline(
            &db,
            repo.id,
            commit_sha.as_str(),
            "refs/heads/main",
            "host-exec",
            Some(user.id),
        )
        .await
        .unwrap();
        let stage = rg_db::ops::pipeline_ops::create_stage(&db, pipeline.id, "test", 0)
            .await
            .unwrap();
        let job = rg_db::ops::pipeline_ops::create_job(
            &db,
            stage.id,
            "no-image",
            "echo should-not-run",
            None,
            None,
            None,
            None,
            None,
            false,
            None,
            None,
            None,
        )
        .await
        .unwrap();

        // Default (`allow_host_execution == false`): the job must fail and the
        // script must not run.
        let runner = PipelineRunner::new_local_only(db.clone(), &repo_path, pipeline.id);
        runner.run().await.unwrap();
        let refused = rg_db::ops::pipeline_ops::get_job(&db, job.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(refused.status, "failed");
        assert!(
            refused
                .log
                .unwrap_or_default()
                .contains("host execution is disabled"),
            "the refusal must explain why the job did not run"
        );

        // Opt in explicitly: the job now runs on the host.
        let pipeline = rg_db::ops::pipeline_ops::create_pipeline(
            &db,
            repo.id,
            commit_sha.as_str(),
            "refs/heads/main",
            "host-exec-optin",
            Some(user.id),
        )
        .await
        .unwrap();
        let stage = rg_db::ops::pipeline_ops::create_stage(&db, pipeline.id, "test", 0)
            .await
            .unwrap();
        let job = rg_db::ops::pipeline_ops::create_job(
            &db,
            stage.id,
            "no-image",
            "echo host-ran",
            None,
            None,
            None,
            None,
            None,
            false,
            None,
            None,
            None,
        )
        .await
        .unwrap();
        let mut runner = PipelineRunner::new_local_only(db.clone(), &repo_path, pipeline.id);
        runner.set_allow_host_execution(true);
        runner.run().await.unwrap();
        let allowed = rg_db::ops::pipeline_ops::get_job(&db, job.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(allowed.status, "success");
        assert!(allowed.log.unwrap_or_default().contains("host-ran"));
    }
}