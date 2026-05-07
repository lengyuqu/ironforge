//! CI configuration types for `.ironforge-ci.yml`.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Top-level CI configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CiConfig {
    /// Ordered list of stage names.
    #[serde(default)]
    pub stages: Option<Vec<String>>,

    /// Map of job name → job config.
    /// Jobs not listed in `stages` will be placed in a "default" stage.
    #[serde(flatten)]
    pub jobs: HashMap<String, JobConfig>,
}

/// A single CI job configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobConfig {
    /// Which stage this job belongs to.
    pub stage: Option<String>,

    /// Shell commands to execute (in order).
    pub script: Vec<String>,

    /// Container image to run in (future: Docker runner).
    pub image: Option<String>,

    /// Only run this job on these branch names.
    #[serde(default)]
    pub only: Option<Vec<String>>,

    /// Environment variables.
    #[serde(default)]
    pub variables: Option<HashMap<String, String>>,

    /// Whether this job can be manually triggered.
    #[serde(default)]
    pub when: Option<String>,

    /// Allow failure without marking the pipeline as failed.
    #[serde(default)]
    pub allow_failure: Option<bool>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_config() {
        let yml = r#"
stages:
  - build
  - test

build_app:
  stage: build
  script:
    - echo "Building..."
    - make build

test_unit:
  stage: test
  script:
    - make test
"#;
        let config: CiConfig = serde_yaml::from_str(yml).unwrap();
        assert_eq!(config.stages.as_ref().unwrap().len(), 2);
        assert_eq!(config.jobs.len(), 2);

        let build = config.jobs.get("build_app").unwrap();
        assert_eq!(build.stage.as_deref(), Some("build"));
        assert_eq!(build.script.len(), 2);

        let test = config.jobs.get("test_unit").unwrap();
        assert_eq!(test.stage.as_deref(), Some("test"));
    }

    #[test]
    fn test_parse_with_only() {
        let yml = r#"
stages:
  - deploy

deploy_prod:
  stage: deploy
  script:
    - make deploy
  only:
    - main
"#;
        let config: CiConfig = serde_yaml::from_str(yml).unwrap();
        let deploy = config.jobs.get("deploy_prod").unwrap();
        assert_eq!(deploy.only.as_ref().unwrap().len(), 1);
        assert_eq!(deploy.only.as_ref().unwrap()[0], "main");
    }

    #[test]
    fn test_parse_minimal_config() {
        // A job with only script (no stage, no image, etc.)
        let yml = r#"
hello:
  script:
    - echo "hello"
"#;
        let config: CiConfig = serde_yaml::from_str(yml).unwrap();
        assert!(config.stages.is_none());
        assert_eq!(config.jobs.len(), 1);
        let job = config.jobs.get("hello").unwrap();
        assert!(job.stage.is_none());
        assert_eq!(job.script, vec!["echo \"hello\""]);
        assert!(job.image.is_none());
        assert!(job.only.is_none());
        assert!(job.allow_failure.is_none());
    }

    #[test]
    fn test_parse_with_all_fields() {
        let yml = r#"
stages:
  - build

full_job:
  stage: build
  script:
    - cargo build
  image: rust:1.75
  only:
    - main
    - develop
  variables:
    RUST_BACKTRACE: "1"
    CARGO_HOME: /cargo
  when: manual
  allow_failure: true
"#;
        let config: CiConfig = serde_yaml::from_str(yml).unwrap();
        let job = config.jobs.get("full_job").unwrap();
        assert_eq!(job.image.as_deref(), Some("rust:1.75"));
        assert_eq!(job.only.as_ref().unwrap().len(), 2);
        assert_eq!(job.variables.as_ref().unwrap().get("RUST_BACKTRACE").unwrap(), "1");
        assert_eq!(job.when.as_deref(), Some("manual"));
        assert_eq!(job.allow_failure, Some(true));
    }

    #[test]
    fn test_parse_empty_jobs() {
        let yml = r#"
stages: []
"#;
        let config: CiConfig = serde_yaml::from_str(yml).unwrap();
        assert!(config.stages.as_ref().unwrap().is_empty());
        assert!(config.jobs.is_empty());
    }

    #[test]
    fn test_parse_multiple_jobs_same_stage() {
        let yml = r#"
stages:
  - test

unit_tests:
  stage: test
  script:
    - cargo test --lib

integration_tests:
  stage: test
  script:
    - cargo test --test integration
"#;
        let config: CiConfig = serde_yaml::from_str(yml).unwrap();
        assert_eq!(config.jobs.len(), 2);
        for (_, job) in &config.jobs {
            assert_eq!(job.stage.as_deref(), Some("test"));
        }
    }

    #[test]
    fn test_config_serialization_roundtrip() {
        let yml = r#"
stages:
  - build

build:
  stage: build
  script:
    - make
"#;
        let config: CiConfig = serde_yaml::from_str(yml).unwrap();
        let serialized = serde_yaml::to_string(&config).unwrap();
        let deserialized: CiConfig = serde_yaml::from_str(&serialized).unwrap();
        assert_eq!(config.stages, deserialized.stages);
        assert_eq!(config.jobs.len(), deserialized.jobs.len());
    }
}
