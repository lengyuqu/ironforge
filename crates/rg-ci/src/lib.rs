//! IronForge CI/CD Engine.
//!
//! Parses `.ironforge-ci.yml` or `.gitea/workflows/*.yml` (Gitea Actions format)
//! from the repository and executes pipelines.
//!
//! ## Native format (`.ironforge-ci.yml`)
//!
//! ```yaml
//! stages:
//!   - build
//!   - test
//!
//! build_app:
//!   stage: build
//!   script:
//!     - cargo build
//! ```
//!
//! ## Gitea Actions format (`.gitea/workflows/*.yml`)
//!
//! ```yaml
//! name: CI
//! on: push
//! jobs:
//!   build:
//!     runs-on: ubuntu-latest
//!     steps:
//!       - uses: actions/checkout@v4
//!       - run: cargo build
//! ```

pub mod config;
pub mod gitea_actions;
pub mod runner;

use anyhow::{Context, Result};
use gix::bstr::ByteSlice;

use config::CiConfig;
use runner::PipelineRunner;

// M-14: TriggerPipelineParams and has_ci_config are now defined in rg-core.
// Re-export for backward compatibility with any code that still imports from rg_ci.
pub use rg_core::ci::{has_ci_config, TriggerPipelineParams};

/// CI engine implementation. Implements `rg_core::ci::CiTrigger` so that
/// `rg-http` can trigger pipelines without a direct dependency on `rg-ci`.
///
/// M-14: This struct decouples the HTTP layer from the CI engine crate.
pub struct CiEngine;

impl rg_core::ci::CiTrigger for CiEngine {
    fn has_ci_config(&self, repo_path: &std::path::Path, commit_sha: &str) -> bool {
        rg_core::ci::has_ci_config(repo_path, commit_sha)
    }

    fn trigger_pipeline<'a>(
        &'a self,
        params: rg_core::ci::TriggerPipelineParams<'a>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<i64>> + Send + 'a>> {
        Box::pin(trigger_pipeline(params))
    }
}

/// Trigger a CI pipeline for a push event.
///
/// This function:
/// 1. Reads `.ironforge-ci.yml` from the repo at the given commit
/// 2. Parses the CI configuration
/// 3. Checks concurrency control (if configured)
/// 4. Creates pipeline/stage/job records in the DB
/// 5. Spawns the pipeline runner in a background task, injecting CI_JOB_TOKEN
pub async fn trigger_pipeline(params: TriggerPipelineParams<'_>) -> Result<i64> {
    let TriggerPipelineParams {
        db,
        repo_path,
        repo_id,
        commit_sha,
        ref_name,
        trigger_type,
        triggered_by,
        docker_enabled,
        external_runners,
        jwt_secret,
    } = params;

    // 1. Read CI config from repo
    let config = read_ci_config(repo_path, commit_sha, ref_name, trigger_type)?;

    // 2. Concurrency control
    if let Some(ref concurrency) = config.concurrency {
        let group =
            rg_db::ops::pipeline_ops::resolve_concurrency_group(&concurrency.group, ref_name);
        let active =
            rg_db::ops::pipeline_ops::find_active_pipelines_by_ref(db, repo_id, ref_name).await?;

        if !active.is_empty() {
            if concurrency.cancel_in_progress {
                tracing::info!(
                    concurrency_group = %group,
                    "Cancelling {} in-progress pipeline(s) for concurrency group",
                    active.len()
                );
                for p in &active {
                    if let Err(e) = rg_db::ops::pipeline_ops::cancel_pipeline_chain(db, p.id).await
                    {
                        tracing::warn!(pipeline_id = p.id, "Failed to cancel pipeline: {:#}", e);
                    }
                }
            } else {
                return Err(anyhow::anyhow!(
                    "Concurrency group '{}' has {} active pipeline(s). \
                     Set cancel_in_progress: true to auto-cancel, or wait for them to finish.",
                    group,
                    active.len()
                ));
            }
        }
    }

    // 3. Create pipeline record
    let pipeline = rg_db::ops::pipeline_ops::create_pipeline(
        db,
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
        let stage =
            rg_db::ops::pipeline_ops::create_stage(db, pipeline_id, stage_name, order as i32)
                .await?;
        stage_id_map.insert(stage_name.clone(), stage.id);
    }

    // 5. Create jobs
    for (job_name, job_config) in &config.jobs {
        // Filter by `only` — if specified, skip jobs that don't match the ref
        if let Some(only) = &job_config.only {
            let ref_short = ref_name.strip_prefix("refs/heads/").unwrap_or(ref_name);
            if !only
                .iter()
                .any(|pattern| pattern == ref_short || pattern == ref_name)
            {
                continue;
            }
        }

        let stage_name = job_config.stage.as_deref().unwrap_or("default");
        let stage_id = stage_id_map.get(stage_name).copied().unwrap_or(-1);

        if stage_id < 0 {
            tracing::warn!(job = %job_name, stage = %stage_name, "Job references unknown stage, skipping");
            continue;
        }

        let script = job_config.script.join("\n");

        // Serialize tags to JSON for storage
        let tags_json = job_config
            .tags
            .as_ref()
            .map(|t| serde_json::to_string(t).unwrap_or_default());

        rg_db::ops::pipeline_ops::create_job(
            db,
            stage_id,
            job_name,
            &script,
            job_config.image.as_deref(),
            tags_json.as_deref(),
        )
        .await?;
    }

    // 6. Spawn pipeline runner in background (only if not using external runners)
    if !external_runners {
        let db_clone = db.clone();
        let pipeline_id_owned = pipeline_id;
        let repo_path_owned = repo_path.to_path_buf();
        let jwt_secret_owned = jwt_secret.map(|s| s.to_string());

        tokio::spawn(async move {
            let mut runner = if docker_enabled {
                PipelineRunner::new(db_clone, &repo_path_owned, pipeline_id_owned)
            } else {
                PipelineRunner::new_local_only(db_clone, &repo_path_owned, pipeline_id_owned)
            };
            runner.set_repo_id(repo_id);
            if let Some(ref secret) = jwt_secret_owned {
                runner.set_jwt_secret(secret.clone());
            }
            if let Err(e) = runner.run().await {
                tracing::error!(
                    pipeline_id = pipeline_id_owned,
                    "Pipeline runner error: {:#}",
                    e
                );
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

/// Read CI configuration from the repo at the given commit.
///
/// Tries formats in order:
/// 1. `.gitea/workflows/*.yml` (Gitea Actions format)
/// 2. `.ironforge-ci.yml` (native format)
///
/// For Gitea Actions workflows, multiple files are merged into a single `CiConfig`.
/// Jobs from different workflow files are placed in separate stages.
fn read_ci_config(
    repo_path: &std::path::Path,
    commit_sha: &str,
    ref_name: &str,
    event: &str,
) -> Result<CiConfig> {
    let repo = gix::open(repo_path)
        .with_context(|| format!("failed to open repository: {:?}", repo_path))?;

    // Try Gitea Actions format first
    if let Ok(config) = try_read_gitea_workflows(&repo, commit_sha, ref_name, event) {
        tracing::info!("Using Gitea Actions workflow from .gitea/workflows/");
        return Ok(config);
    }

    // Fall back to native .ironforge-ci.yml
    let revspec = format!("{}:.ironforge-ci.yml", commit_sha);
    let object_id = repo.rev_parse_single(revspec.as_str()).map_err(|_| {
        anyhow::anyhow!(
            "no CI config found (.gitea/workflows/*.yml or .ironforge-ci.yml) at commit {}",
            commit_sha
        )
    })?;

    let object_id = object_id.object().context("failed to resolve object")?;
    let blob = object_id
        .try_into_blob()
        .context("expected a blob object for .ironforge-ci.yml")?;

    let ci_yml =
        String::from_utf8(blob.data.to_vec()).context(".ironforge-ci.yml is not valid UTF-8")?;

    let config: CiConfig = serde_yaml::from_str(&ci_yml)
        .with_context(|| format!("failed to parse .ironforge-ci.yml: {}", ci_yml))?;

    Ok(config)
}

/// Try to find and parse Gitea Actions workflow files in `.gitea/workflows/`.
fn try_read_gitea_workflows(
    repo: &gix::Repository,
    commit_sha: &str,
    ref_name: &str,
    event: &str,
) -> Result<CiConfig> {
    let tree_revspec = format!("{}:.gitea/workflows", commit_sha);
    let object_id = repo
        .rev_parse_single(tree_revspec.as_str())
        .map_err(|_| anyhow::anyhow!("no .gitea/workflows directory"))?;

    let object = object_id.object()?;
    let tree = object
        .try_into_tree()
        .map_err(|_| anyhow::anyhow!(".gitea/workflows is not a directory"))?;

    let default_branch = get_default_branch(repo)?;

    let mut all_jobs: std::collections::HashMap<String, config::JobConfig> =
        std::collections::HashMap::new();
    let mut all_stages: Vec<String> = Vec::new();

    // Iterate through .gitea/workflows/*.yml entries
    for entry in tree.iter() {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let name = entry.filename().to_string();
        if !name.ends_with(".yml") && !name.ends_with(".yaml") {
            continue;
        }

        let entry_id = entry.oid();
        let entry_object = match repo.find_object(entry_id) {
            Ok(o) => o,
            Err(_) => continue,
        };
        let blob = match entry_object.try_into_blob() {
            Ok(b) => b,
            Err(_) => continue,
        };
        let yml = String::from_utf8(blob.data.to_vec()).unwrap_or_default();

        let workflow = match gitea_actions::GiteaWorkflow::parse(&yml) {
            Ok(w) => w,
            Err(e) => {
                tracing::debug!("Skipping .gitea/workflows/{} (parse error: {})", name, e);
                continue;
            }
        };

        // Check if this workflow should be triggered
        if !workflow.matches_event(event, ref_name, &default_branch) {
            continue;
        }

        tracing::info!("Triggering workflow from .gitea/workflows/{}", name);

        let ctx = gitea_actions::WorkflowContext {
            ref_name: ref_name.to_string(),
            sha: commit_sha.to_string(),
            event: event.to_string(),
            repo_owner: String::new(), // filled later
            repo_name: String::new(),
        };

        let mut wf_config = workflow.to_ci_config(&ctx);

        // Prefix job names with workflow filename to avoid collisions
        let wf_prefix = name.trim_end_matches(".yml").trim_end_matches(".yaml");
        let mut renamed_jobs = std::collections::HashMap::new();
        for (job_name, mut job) in wf_config.jobs {
            let new_name = format!("{}/{}", wf_prefix, job_name);
            // Prefix stage names too
            if let Some(ref stage) = job.stage {
                job.stage = Some(format!("{}/{}", wf_prefix, stage));
            }
            renamed_jobs.insert(new_name, job);
        }
        wf_config.jobs = renamed_jobs;

        // Add stages
        if let Some(ref stages) = wf_config.stages {
            for stage in stages {
                all_stages.push(format!("{}/{}", wf_prefix, stage));
            }
        }

        // Merge jobs
        all_jobs.extend(wf_config.jobs);
    }

    if all_jobs.is_empty() {
        anyhow::bail!("no matching workflows found in .gitea/workflows/");
    }

    Ok(CiConfig {
        stages: Some(all_stages),
        concurrency: None, // per-workflow concurrency not merged
        jobs: all_jobs,
    })
}

/// Get the default branch name of the repository.
fn get_default_branch(repo: &gix::Repository) -> Result<String> {
    // Try to read HEAD reference
    if let Ok(Some(head_ref)) = repo.head_ref() {
        if let Ok(name) = head_ref.name().shorten().to_str() {
            return Ok(name.to_string());
        }
    }
    Ok("main".to_string())
}

// M-14: has_ci_config moved to rg_core::ci::has_ci_config and re-exported above.
