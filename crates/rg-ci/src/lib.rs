//! IronForge CI/CD Engine.
//!
//! Parses `.ironforge-ci.yml` from the repository and executes pipelines.
//!
//! ## Configuration format (`.ironforge-ci.yml`)
//!
//! ```yaml
//! stages:
//!   - build
//!   - test
//!   - deploy
//!
//! build_app:
//!   stage: build
//!   script:
//!     - echo "Building..."
//!     - make build
//!
//! test_unit:
//!   stage: test
//!   script:
//!     - make test
//!
//! deploy_prod:
//!   stage: deploy
//!   script:
//!     - make deploy
//!   only:
//!     - main
//! ```

pub mod config;
pub mod runner;

use anyhow::{Context, Result};
use sea_orm::DatabaseConnection;

use config::CiConfig;
use runner::PipelineRunner;

/// Trigger a CI pipeline for a push event.
///
/// This function:
/// 1. Reads `.ironforge-ci.yml` from the repo at the given commit
/// 2. Parses the CI configuration
/// 3. Creates pipeline/stage/job records in the DB
/// 4. Spawns the pipeline runner in a background task
pub async fn trigger_pipeline(
    db: DatabaseConnection,
    repo_path: &std::path::Path,
    repo_id: i64,
    commit_sha: &str,
    ref_name: &str,
    trigger_type: &str,
    triggered_by: Option<i64>,
    docker_enabled: bool,
    external_runners: bool,
) -> Result<i64> {
    // 1. Read CI config from repo
    let ci_yml = read_ci_config(repo_path, commit_sha)?;

    // 2. Parse config
    let config: CiConfig = serde_yaml::from_str(&ci_yml)
        .with_context(|| format!("failed to parse .ironforge-ci.yml: {}", ci_yml))?;

    // 3. Create pipeline record
    let pipeline = rg_db::ops::pipeline_ops::create_pipeline(
        &db,
        repo_id,
        commit_sha,
        ref_name,
        trigger_type,
        triggered_by,
    )
    .await?;

    let pipeline_id = pipeline.id;

    // 4. Create stages
    let stage_names = config.stages.as_ref().cloned().unwrap_or_default();
    let mut stage_id_map: std::collections::HashMap<String, i64> = std::collections::HashMap::new();

    for (order, stage_name) in stage_names.iter().enumerate() {
        let stage = rg_db::ops::pipeline_ops::create_stage(
            &db,
            pipeline_id,
            stage_name,
            order as i32,
        )
        .await?;
        stage_id_map.insert(stage_name.clone(), stage.id);
    }

    // 5. Create jobs
    for (job_name, job_config) in &config.jobs {
        // Filter by `only` — if specified, skip jobs that don't match the ref
        if let Some(only) = &job_config.only {
            let ref_short = ref_name.strip_prefix("refs/heads/").unwrap_or(ref_name);
            if !only.iter().any(|pattern| {
                pattern == ref_short || pattern == ref_name
            }) {
                continue;
            }
        }

        let stage_name = job_config.stage.as_deref().unwrap_or("default");
        let stage_id = stage_id_map.get(stage_name).copied().unwrap_or_else(|| {
            // If stage not in the stages list, create it
            // This shouldn't normally happen with valid config
            -1i64
        });

        if stage_id < 0 {
            tracing::warn!(job = %job_name, stage = %stage_name, "Job references unknown stage, skipping");
            continue;
        }

        let script = job_config.script.join("\n");

        rg_db::ops::pipeline_ops::create_job(
            &db,
            stage_id,
            job_name,
            &script,
            job_config.image.as_deref(),
        )
        .await?;
    }

    // 6. Spawn pipeline runner in background (only if not using external runners)
    if !external_runners {
        let db_clone = db.clone();
        let pipeline_id_owned = pipeline_id;
        let repo_path_owned = repo_path.to_path_buf();

        tokio::spawn(async move {
            let runner = if docker_enabled {
                PipelineRunner::new(db_clone, &repo_path_owned, pipeline_id_owned)
            } else {
                PipelineRunner::new_local_only(db_clone, &repo_path_owned, pipeline_id_owned)
            };
            if let Err(e) = runner.run().await {
                tracing::error!(pipeline_id = pipeline_id_owned, "Pipeline runner error: {:#}", e);
            }
        });
    } else {
        tracing::info!(
            pipeline_id = pipeline_id,
            "Pipeline created with external runner mode — jobs will be picked up by registered runners"
        );
    }

    Ok(pipeline_id)
}

/// Read `.ironforge-ci.yml` from the repo at the given commit using gix.
fn read_ci_config(repo_path: &std::path::Path, commit_sha: &str) -> Result<String> {
    let repo = gix::open(repo_path)
        .with_context(|| format!("failed to open repository: {:?}", repo_path))?;

    let revspec = format!("{}:.ironforge-ci.yml", commit_sha);
    let object_id = repo
        .rev_parse_single(revspec.as_str())
        .map_err(|_| anyhow::anyhow!("no .ironforge-ci.yml found at commit {}", commit_sha))?;

    let object_id = object_id
        .object()
        .context("failed to resolve object")?;
    let blob = object_id
        .try_into_blob()
        .context("expected a blob object for .ironforge-ci.yml")?;

    String::from_utf8(blob.data.to_vec())
        .context(".ironforge-ci.yml is not valid UTF-8")
}

/// Check if a repo has a CI config file at the given commit using gix.
pub fn has_ci_config(repo_path: &std::path::Path, commit_sha: &str) -> bool {
    let repo = match gix::open(repo_path) {
        Ok(r) => r,
        Err(_) => return false,
    };

    let revspec = format!("{}:.ironforge-ci.yml", commit_sha);
    repo.rev_parse_single(revspec.as_str()).is_ok()
}
