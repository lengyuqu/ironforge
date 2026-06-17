//! Gitea Actions / GitHub Actions workflow compatibility layer.
//!
//! Parses `.gitea/workflows/*.yml` (GitHub Actions-compatible format) and
//! translates them into IronForge's internal `CiConfig` model.
//!
//! Supported features:
//! - `on: push`, `on: pull_request` triggers with branch filtering
//! - `jobs.<id>.runs-on` → runner tags
//! - `jobs.<id>.steps[].run` → script commands
//! - `jobs.<id>.steps[].uses` → `actions/checkout` is implicit, others ignored
//! - `jobs.<id>.container.image` → Docker image
//! - `jobs.<id>.env` → environment variables
//! - `jobs.<id>.needs` → stage ordering (implicit via dependency graph)
//! - Basic `${{ }}` expression substitution

use anyhow::Result;
use serde::Deserialize;
use std::collections::HashMap;

use crate::config::{CiConfig, ConcurrencyConfig, JobConfig};

/// A parsed Gitea Actions workflow file.
#[derive(Debug, Deserialize)]
pub struct GiteaWorkflow {
    /// Workflow name (optional, defaults to filename).
    pub name: Option<String>,

    /// Event triggers.
    pub on: WorkflowTriggers,

    /// Job definitions.
    pub jobs: HashMap<String, GiteaJob>,

    /// Concurrency control (optional).
    pub concurrency: Option<GiteaConcurrency>,

    /// Workflow-level environment variables.
    #[serde(default)]
    pub env: HashMap<String, String>,
}

/// Workflow trigger definitions.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum WorkflowTriggers {
    /// Simple trigger: `on: push`
    Simple(String),
    /// Single event with config: `on: { push: { branches: [main] } }`
    Single {
        push: Option<EventFilter>,
        pull_request: Option<EventFilter>,
        #[serde(rename = "pull_request_target")]
        pull_request_target: Option<EventFilter>,
        schedule: Option<Vec<ScheduleTrigger>>,
        workflow_dispatch: Option<serde_yaml::Value>,
    },
    /// Array of event names: `on: [push, pull_request]`
    Array(Vec<String>),
}

/// Event filter with optional branch/tag/path filtering.
#[derive(Debug, Deserialize)]
pub struct EventFilter {
    pub branches: Option<Vec<String>>,
    #[serde(rename = "branches-ignore")]
    pub branches_ignore: Option<Vec<String>>,
    pub tags: Option<Vec<String>>,
    #[serde(rename = "tags-ignore")]
    pub tags_ignore: Option<Vec<String>>,
    pub paths: Option<Vec<String>>,
    #[serde(rename = "paths-ignore")]
    pub paths_ignore: Option<Vec<String>>,
}

/// Schedule trigger with cron expression.
#[derive(Debug, Deserialize)]
pub struct ScheduleTrigger {
    pub cron: String,
}

/// A Gitea Actions job definition.
#[derive(Debug, Deserialize)]
pub struct GiteaJob {
    /// Runner label (e.g., `ubuntu-latest`, `self-hosted`).
    #[serde(rename = "runs-on")]
    pub runs_on: Option<serde_yaml::Value>,

    /// Job steps.
    #[serde(default)]
    pub steps: Vec<GiteaStep>,

    /// Container specification.
    pub container: Option<GiteaContainer>,

    /// Job-level environment variables.
    #[serde(default)]
    pub env: HashMap<String, String>,

    /// Dependencies (job names that must complete before this job).
    #[serde(default)]
    pub needs: Option<Vec<String>>,

    /// Job condition (if expression).
    #[serde(rename = "if")]
    pub condition: Option<String>,

    /// Job timeout in minutes.
    #[serde(rename = "timeout-minutes")]
    pub timeout_minutes: Option<u64>,
}

/// A step within a Gitea Actions job.
#[derive(Debug, Deserialize)]
pub struct GiteaStep {
    /// Step name (optional).
    pub name: Option<String>,

    /// The action to use (e.g., `actions/checkout@v4`).
    #[serde(rename = "uses")]
    pub uses: Option<String>,

    /// Shell command to run.
    pub run: Option<String>,

    /// Shell to use (default: bash).
    pub shell: Option<String>,

    /// Step-level environment variables.
    #[serde(default)]
    pub env: HashMap<String, String>,

    /// Step condition.
    #[serde(rename = "if")]
    pub condition: Option<String>,

    /// Input parameters for the action.
    #[serde(default)]
    pub with: HashMap<String, String>,
}

/// Container specification for a job.
#[derive(Debug, Deserialize)]
pub struct GiteaContainer {
    /// Docker image.
    pub image: String,

    /// Container options.
    pub options: Option<String>,

    /// Environment variables for the container.
    #[serde(default)]
    pub env: HashMap<String, String>,
}

/// Concurrency configuration (Gitea Actions format).
#[derive(Debug, Deserialize)]
pub struct GiteaConcurrency {
    pub group: String,
    #[serde(rename = "cancel-in-progress")]
    pub cancel_in_progress: Option<bool>,
}

/// Context for workflow expression evaluation.
pub struct WorkflowContext {
    pub ref_name: String,
    pub sha: String,
    pub event: String,
    pub repo_owner: String,
    pub repo_name: String,
}

impl GiteaWorkflow {
    /// Parse a Gitea Actions workflow YAML string.
    pub fn parse(yaml: &str) -> Result<Self> {
        let wf: GiteaWorkflow = serde_yaml::from_str(yaml)?;
        Ok(wf)
    }

    /// Check if this workflow should be triggered for the given event and ref.
    pub fn matches_event(&self, event: &str, ref_name: &str, default_branch: &str) -> bool {
        match &self.on {
            WorkflowTriggers::Simple(name) => name.as_str() == event,
            WorkflowTriggers::Array(names) => names.iter().any(|n| n.as_str() == event),
            WorkflowTriggers::Single { push, pull_request, .. } => match event {
                "push" => {
                    if let Some(filter) = push {
                        ref_matches_filter(ref_name, filter, default_branch)
                    } else {
                        false
                    }
                }
                "pull_request" => {
                    if let Some(filter) = pull_request {
                        ref_matches_filter(ref_name, filter, default_branch)
                    } else {
                        false
                    }
                }
                _ => false,
            },
        }
    }

    /// Convert this workflow into an IronForge `CiConfig`.
    ///
    /// The conversion:
    /// - Groups jobs by their dependency order (needs) into stages
    /// - Extracts `run` commands from steps into `script`
    /// - Maps `runs-on` labels to `tags`
    /// - Translates `container.image` to `image`
    pub fn to_ci_config(&self, _ctx: &WorkflowContext) -> CiConfig {
        let mut job_configs: HashMap<String, JobConfig> = HashMap::new();
        let mut stages: Vec<String> = Vec::new();

        // Build dependency graph to determine stage ordering
        let mut job_deps: HashMap<&str, Vec<&str>> = HashMap::new();
        let mut job_order: Vec<&str> = Vec::new();

        for (name, job) in &self.jobs {
            job_order.push(name.as_str());
            if let Some(ref needs) = job.needs {
                job_deps.insert(name.as_str(), needs.iter().map(|s| s.as_str()).collect());
            } else {
                job_deps.insert(name.as_str(), vec![]);
            }
        }

        // Topological sort: simple BFS (each job goes to stage = max(deps_stage) + 1)
        let mut job_stage: HashMap<String, usize> = HashMap::new();
        let mut max_stage = 0usize;

        // Iterate until all jobs have a stage assigned
        let mut remaining = job_order.clone();
        let mut changed = true;
        while changed && !remaining.is_empty() {
            changed = false;
            let mut next_remaining: Vec<&str> = Vec::new();
            for &name in &remaining {
                let deps = job_deps.get(name).map(|v| v.as_slice()).unwrap_or(&[]);
                if deps.is_empty() || deps.iter().all(|d| job_stage.contains_key(*d)) {
                    let s = if deps.is_empty() {
                        0
                    } else {
                        deps.iter().map(|d| job_stage.get(*d).copied().unwrap_or(0)).max().unwrap_or(0) + 1
                    };
                    job_stage.insert(name.to_string(), s);
                    max_stage = max_stage.max(s);
                    changed = true;
                } else {
                    next_remaining.push(name);
                }
            }
            remaining = next_remaining;
        }

        // Assign any remaining jobs to stage 0 (circular deps or self-references)
        for name in remaining {
            job_stage.entry(name.to_string()).or_insert(0);
        }

        // Generate stage names
        for i in 0..=max_stage {
            stages.push(format!("stage-{}", i));
        }

        // Convert each job
        for (name, job) in &self.jobs {
            let mut script: Vec<String> = Vec::new();
            let mut job_vars: HashMap<String, String> = HashMap::new();

            // Copy workflow-level env
            for (k, v) in &self.env {
                job_vars.insert(k.clone(), v.clone());
            }
            // Copy job-level env
            for (k, v) in &job.env {
                job_vars.insert(k.clone(), v.clone());
            }

            // Process steps
            let mut has_checkout = false;
            for step in &job.steps {
                // Handle `uses: actions/checkout@vX` — implicit in IronForge, skip
                if let Some(ref uses) = step.uses {
                    if uses.starts_with("actions/checkout") {
                        has_checkout = true;
                        continue;
                    }
                    // For other actions, emit a comment
                    script.push(format!("# [IronForge] action '{}' is not natively supported; skipping", uses));
                    continue;
                }

                // Handle `run:` commands
                if let Some(ref run_cmd) = step.run {
                    // Copy step-level env
                    for (k, v) in &step.env {
                        let expanded = substitute_expr(v, name, &self.env, &job_vars);
                        script.push(format!("export {}={}", k, expanded));
                    }

                    // Substitute expressions in the command
                    let expanded_cmd = substitute_expr(run_cmd, name, &self.env, &job_vars);
                    script.push(expanded_cmd);
                }
            }

            // If no checkout step was found, add a comment
            if !has_checkout && !script.is_empty() {
                script.insert(0, "# [IronForge] Repository is already checked out at /workspace".to_string());
            }

            // Determine tags from runs-on
            let tags = match &job.runs_on {
                Some(serde_yaml::Value::String(s)) => Some(vec![s.clone()]),
                Some(serde_yaml::Value::Sequence(arr)) => {
                    let tags: Vec<String> = arr
                        .iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect();
                    if tags.is_empty() { None } else { Some(tags) }
                }
                _ => None,
            };

            // Determine stage
            let s = job_stage.get(name).copied().unwrap_or(0);
            let stage_name = format!("stage-{}", s);

            job_configs.insert(
                name.clone(),
                JobConfig {
                    stage: Some(stage_name),
                    script,
                    image: job.container.as_ref().map(|c| c.image.clone()),
                    only: None, // filtering is done at trigger time
                    variables: if job_vars.is_empty() { None } else { Some(job_vars) },
                    when: None,
                    allow_failure: None,
                    tags,
                },
            );
        }

        CiConfig {
            stages: Some(stages),
            concurrency: self.concurrency.as_ref().map(|c| ConcurrencyConfig {
                group: c.group.clone(),
                cancel_in_progress: c.cancel_in_progress.unwrap_or(false),
            }),
            jobs: job_configs,
        }
    }
}

/// Check if a ref matches an event filter.
fn ref_matches_filter(ref_name: &str, filter: &EventFilter, default_branch: &str) -> bool {
    // Extract branch name from ref (e.g., "refs/heads/main" → "main")
    let branch = ref_name.strip_prefix("refs/heads/").unwrap_or(ref_name);

    // Check branches filter
    if let Some(ref branches) = filter.branches {
        if !branches.iter().any(|pattern| match_branch_pattern(branch, pattern, default_branch)) {
            return false;
        }
    }

    // Check branches-ignore filter
    if let Some(ref ignored) = filter.branches_ignore {
        if ignored.iter().any(|pattern| match_branch_pattern(branch, pattern, default_branch)) {
            return false;
        }
    }

    // Check tags filter
    let is_tag = ref_name.starts_with("refs/tags/");
    let tag = ref_name.strip_prefix("refs/tags/").unwrap_or(ref_name);
    if let Some(ref tags) = filter.tags {
        if !is_tag || !tags.iter().any(|p| match_glob(tag, p)) {
            return false;
        }
    }

    true
}

/// Match a branch name against a pattern (supports `*` wildcard and `**`).
fn match_branch_pattern(branch: &str, pattern: &str, _default_branch: &str) -> bool {
    match pattern {
        // Special case: pattern is empty (shouldn't happen but guard)
        "" => branch.is_empty(),
        // Exact match
        p if !p.contains('*') => branch == p,
        // Glob match
        p => match_glob(branch, p),
    }
}

/// Simple glob matching supporting `*` (single segment) and `**` (multi segment).
fn match_glob(s: &str, pattern: &str) -> bool {
    // Simple cases
    if pattern == "*" {
        return true;
    }
    if pattern == "**" {
        return true;
    }
    // `**/*` matches anything with at least one path segment
    if pattern == "**/*" {
        return s.contains('/') || !s.is_empty();
    }
    // Simple prefix/suffix glob
    if let Some(prefix) = pattern.strip_suffix('*') {
        return s.starts_with(prefix);
    }
    if let Some(suffix) = pattern.strip_prefix('*') {
        return s.ends_with(suffix);
    }
    // `*middle*` case
    if pattern.starts_with('*') && pattern.ends_with('*') && pattern.len() > 2 {
        let middle = &pattern[1..pattern.len() - 1];
        return s.contains(middle);
    }
    s == pattern
}

/// Basic `${{ expression }}` substitution.
fn substitute_expr(
    input: &str,
    _job_name: &str,
    workflow_env: &HashMap<String, String>,
    job_env: &HashMap<String, String>,
) -> String {
    let mut result = input.to_string();

    // Substitute ${{ env.VAR }} and ${{ vars.VAR }}
    for (key, value) in workflow_env.iter().chain(job_env.iter()) {
        let pattern = format!("${{{{ env.{} }}}}", key);
        result = result.replace(&pattern, value);
        let pattern2 = format!("${{{{ vars.{} }}}}", key);
        result = result.replace(&pattern2, value);
    }

    // Handle common built-in expressions
    result = result.replace("${{ github.ref }}", "${CI_REF}");
    result = result.replace("${{ github.sha }}", "${CI_SHA}");
    result = result.replace("${{ github.event_name }}", "${CI_EVENT}");

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_workflow() {
        let yml = r#"
name: CI
on: push
jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Build
        run: cargo build --release
      - name: Test
        run: cargo test
"#;
        let wf = GiteaWorkflow::parse(yml).unwrap();
        assert_eq!(wf.name.as_deref(), Some("CI"));
        assert!(wf.matches_event("push", "refs/heads/main", "main"));

        let ctx = WorkflowContext {
            ref_name: "refs/heads/main".into(),
            sha: "abc123".into(),
            event: "push".into(),
            repo_owner: "owner".into(),
            repo_name: "repo".into(),
        };
        let ci_config = wf.to_ci_config(&ctx);
        assert_eq!(ci_config.jobs.len(), 1);

        let build = ci_config.jobs.get("build").unwrap();
        assert_eq!(build.image, None);
        assert!(build.script.len() > 1);
        // checkout should be skipped, build and test run commands present
        let script_str = build.script.join("\n");
        assert!(script_str.contains("cargo build --release"));
        assert!(script_str.contains("cargo test"));
    }

    #[test]
    fn test_parse_push_with_branches() {
        let yml = r#"
on:
  push:
    branches: [main, develop]
  pull_request:
    branches: [main]
jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - run: echo test
"#;
        let wf = GiteaWorkflow::parse(yml).unwrap();
        assert!(wf.matches_event("push", "refs/heads/main", "main"));
        assert!(wf.matches_event("push", "refs/heads/develop", "main"));
        assert!(!wf.matches_event("push", "refs/heads/feature", "main"));
        assert!(wf.matches_event("pull_request", "refs/heads/feature", "main"));
    }

    #[test]
    fn test_parse_on_array() {
        let yml = r#"
on: [push, pull_request]
jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - run: echo hi
"#;
        let wf = GiteaWorkflow::parse(yml).unwrap();
        assert!(wf.matches_event("push", "refs/heads/main", "main"));
        assert!(wf.matches_event("pull_request", "refs/heads/main", "main"));
        assert!(!wf.matches_event("schedule", "", "main"));
    }

    #[test]
    fn test_parse_with_container() {
        let yml = r#"
on: push
jobs:
  build:
    runs-on: ubuntu-latest
    container:
      image: rust:1.75
    steps:
      - run: cargo build
"#;
        let wf = GiteaWorkflow::parse(yml).unwrap();
        let ctx = WorkflowContext {
            ref_name: "refs/heads/main".into(),
            sha: "abc".into(),
            event: "push".into(),
            repo_owner: "o".into(),
            repo_name: "r".into(),
        };
        let ci = wf.to_ci_config(&ctx);
        let job = ci.jobs.get("build").unwrap();
        assert_eq!(job.image.as_deref(), Some("rust:1.75"));
    }

    #[test]
    fn test_parse_with_env() {
        let yml = r#"
on: push
env:
  GLOBAL_VAR: global
jobs:
  build:
    runs-on: ubuntu-latest
    env:
      JOB_VAR: job-level
    steps:
      - run: echo $GLOBAL_VAR
      - run: echo $JOB_VAR
"#;
        let wf = GiteaWorkflow::parse(yml).unwrap();
        let ctx = WorkflowContext {
            ref_name: "refs/heads/main".into(),
            sha: "abc".into(),
            event: "push".into(),
            repo_owner: "o".into(),
            repo_name: "r".into(),
        };
        let ci = wf.to_ci_config(&ctx);
        let job = ci.jobs.get("build").unwrap();
        let vars = job.variables.as_ref().unwrap();
        assert_eq!(vars.get("GLOBAL_VAR").unwrap(), "global");
        assert_eq!(vars.get("JOB_VAR").unwrap(), "job-level");
    }

    #[test]
    fn test_parse_with_needs() {
        let yml = r#"
on: push
jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - run: cargo build
  test:
    runs-on: ubuntu-latest
    needs: [build]
    steps:
      - run: cargo test
  deploy:
    runs-on: ubuntu-latest
    needs: [build, test]
    steps:
      - run: deploy.sh
"#;
        let wf = GiteaWorkflow::parse(yml).unwrap();
        let ctx = WorkflowContext {
            ref_name: "refs/heads/main".into(),
            sha: "abc".into(),
            event: "push".into(),
            repo_owner: "o".into(),
            repo_name: "r".into(),
        };
        let ci = wf.to_ci_config(&ctx);
        assert_eq!(ci.jobs.len(), 3);

        // build should be stage-0, test stage-1, deploy stage-2
        let build_stage = ci.jobs.get("build").unwrap().stage.as_deref();
        let test_stage = ci.jobs.get("test").unwrap().stage.as_deref();
        let deploy_stage = ci.jobs.get("deploy").unwrap().stage.as_deref();
        assert_eq!(build_stage, Some("stage-0"));
        assert_eq!(test_stage, Some("stage-1"));
        assert_eq!(deploy_stage, Some("stage-2"));
    }

    #[test]
    fn test_parse_concurrency() {
        let yml = r#"
on: push
concurrency:
  group: deploy-group
  cancel-in-progress: true
jobs:
  deploy:
    runs-on: ubuntu-latest
    steps:
      - run: deploy.sh
"#;
        let wf = GiteaWorkflow::parse(yml).unwrap();
        let ctx = WorkflowContext {
            ref_name: "refs/heads/main".into(),
            sha: "abc".into(),
            event: "push".into(),
            repo_owner: "o".into(),
            repo_name: "r".into(),
        };
        let ci = wf.to_ci_config(&ctx);
        let cc = ci.concurrency.as_ref().unwrap();
        assert_eq!(cc.group, "deploy-group");
        assert!(cc.cancel_in_progress);
    }
}
