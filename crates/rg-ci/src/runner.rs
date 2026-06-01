//! Pipeline runner — executes CI jobs sequentially by stage.
//!
//! Supports two execution modes:
//! - **Local**: `sh -c` (default, when no `image` is specified)
//! - **Docker**: `docker run --rm <image> sh -c` (when `image` field is set)

use anyhow::{Context, Result};
use sea_orm::DatabaseConnection;

use rg_db::ops::pipeline_ops;

/// Pipeline runner that executes stages/jobs sequentially.
pub struct PipelineRunner {
    db: DatabaseConnection,
    repo_path: std::path::PathBuf,
    pipeline_id: i64,
    docker_enabled: bool,
}

impl PipelineRunner {
    pub fn new(db: DatabaseConnection, repo_path: &std::path::Path, pipeline_id: i64) -> Self {
        Self {
            db,
            repo_path: repo_path.to_path_buf(),
            pipeline_id,
            docker_enabled: true,
        }
    }

    /// Create a runner with Docker disabled (local-only mode).
    pub fn new_local_only(db: DatabaseConnection, repo_path: &std::path::Path, pipeline_id: i64) -> Self {
        Self {
            db,
            repo_path: repo_path.to_path_buf(),
            pipeline_id,
            docker_enabled: false,
        }
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
                        let _ = pipeline_ops::update_job_result(
                            &self.db,
                            job.id,
                            status,
                            Some(exit_code),
                            Some(&log),
                            None,
                            None,
                        )
                        .await;
                    }
                    Err(e) => {
                        tracing::error!(job_id = job.id, "Job execution error: {:#}", e);
                        stage_failed = true;
                        let _ = pipeline_ops::update_job_result(
                            &self.db,
                            job.id,
                            "failed",
                            Some(-1),
                            Some(&format!("Runner error: {}", e)),
                            None,
                            None,
                        )
                        .await;
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
    /// - Otherwise: `sh -c <script>`
    ///
    /// Returns (exit_code, stdout+stderr output).
    async fn run_job(&self, job_id: i64, script: &str, image: Option<&str>) -> Result<(i32, String)> {
        let job_start = chrono::Utc::now().naive_utc();

        // Mark job as running
        let _ = pipeline_ops::update_job_result(
            &self.db,
            job_id,
            "running",
            None,
            None,
            Some(job_start),
            None,
        )
        .await;

        tracing::info!(job_id, "Running job");

        let result = if let Some(img) = image {
            if self.docker_enabled {
                self.run_job_docker(job_id, script, img).await
            } else {
                tracing::warn!(job_id, image = %img, "Docker disabled, falling back to local execution");
                self.run_job_local(script).await
            }
        } else {
            self.run_job_local(script).await
        };

        result
    }

    /// Execute script locally via platform-appropriate shell.
    async fn run_job_local(&self, script: &str) -> Result<(i32, String)> {
        #[cfg(unix)]
        let output = tokio::process::Command::new("sh")
            .arg("-c")
            .arg(script)
            .current_dir(&self.repo_path)
            .output()
            .await
            .context("failed to spawn job process")?;
        
        #[cfg(windows)]
        let output = tokio::process::Command::new("powershell.exe")
            .args(&["-NoProfile", "-NonInteractive", "-Command", script])
            .current_dir(&self.repo_path)
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
            tracing::warn!(job_id, "Docker daemon not running, falling back to local execution");
            return self.run_job_local(script).await;
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
