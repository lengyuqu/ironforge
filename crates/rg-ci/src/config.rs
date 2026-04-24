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
}
