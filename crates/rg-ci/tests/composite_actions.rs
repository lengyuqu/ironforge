//! T34 acceptance: composite Action expansion and the fail-closed paths.
//!
//! Every fixture lives under `tests/fixtures/actions/`, so nothing here touches
//! the network. The one test that does is `#[ignore]`d and needs
//! `IRONFORGE_ACTION_REMOTE_BASE` to be set.

use std::path::{Path, PathBuf};

use rg_ci::action_fetch::{ActionError, ActionErrorKind, ActionFetcher, ActionRuns};
use rg_ci::gitea_actions::{GiteaWorkflow, WorkflowContext};

fn fixtures_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/actions")
}

/// A fetcher whose `./path` references are relative to the fixture tree.
fn fetcher(cache: &tempfile::TempDir) -> ActionFetcher {
    ActionFetcher::for_directory(fixtures_root(), cache.path().join("actions"))
}

fn workflow(yaml: &str) -> GiteaWorkflow {
    GiteaWorkflow::parse(yaml).expect("workflow parses")
}

fn context() -> WorkflowContext {
    WorkflowContext {
        ref_name: "refs/heads/main".into(),
        sha: "0123456789abcdef".into(),
        event: "push".into(),
        repo_owner: "owner".into(),
        repo_name: "repo".into(),
    }
}

fn runs(steps: &[rg_ci::gitea_actions::GiteaStep]) -> Vec<String> {
    steps
        .iter()
        .map(|step| {
            step.run
                .clone()
                .or_else(|| step.uses.clone())
                .unwrap_or_default()
        })
        .collect()
}

/// 1. One composite Action with two steps becomes two steps, in order, at the
///    position where it was invoked.
#[test]
fn single_layer_composite_expands_to_its_steps_in_order() {
    let cache = tempfile::tempdir().unwrap();
    let mut wf = workflow(
        r#"
on: push
jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: ./single
      - run: echo after
"#,
    );

    wf.expand_composite_actions(&fetcher(&cache)).unwrap();

    let steps = &wf.jobs["build"].steps;
    assert_eq!(steps.len(), 4, "checkout + 2 expanded + trailing run");
    assert_eq!(steps[0].uses.as_deref(), Some("actions/checkout@v4"));
    assert_eq!(
        runs(steps),
        vec![
            "actions/checkout@v4".to_string(),
            "echo first".to_string(),
            "echo second".to_string(),
            "echo after".to_string(),
        ]
    );
    // checkout stays built-in, so the expanded steps are ordinary run steps.
    assert!(steps[1].uses.is_none());
    assert!(steps[2].uses.is_none());

    // The flattened script the current executor runs keeps that order.
    wf.validate_supported_actions().unwrap();
    let ci = wf.to_ci_config(&context());
    let script = ci.jobs["build"].script.join("\n");
    let first = script.find("echo first").expect("first step");
    let second = script.find("echo second").expect("second step");
    let after = script.find("echo after").expect("trailing step");
    assert!(first < second && second < after, "script:\n{script}");
}

/// 2. Composite Actions nest; the inner steps land before the outer tail.
#[test]
fn nested_composites_expand_depth_first() {
    let cache = tempfile::tempdir().unwrap();
    let mut wf = workflow(
        r#"
on: push
jobs:
  build:
    steps:
      - uses: ./outer
"#,
    );

    wf.expand_composite_actions(&fetcher(&cache)).unwrap();

    assert_eq!(
        runs(&wf.jobs["build"].steps),
        vec![
            "echo inner-one".to_string(),
            "echo inner-two".to_string(),
            "echo outer-done".to_string(),
        ]
    );
}

/// 3. A → B → A is an error, not a hang and not a panic.
#[test]
fn a_composite_cycle_fails_closed() {
    let cache = tempfile::tempdir().unwrap();
    let mut wf = workflow(
        r#"
on: push
jobs:
  build:
    steps:
      - uses: ./cycle-a
"#,
    );

    let error = wf
        .expand_composite_actions(&fetcher(&cache))
        .expect_err("a cycle must be rejected");
    let message = format!("{error:#}");
    assert!(message.contains("cycle"), "{message}");
    assert!(message.contains("./cycle-a"), "{message}");
    assert_eq!(
        error.downcast_ref::<ActionError>().map(ActionError::kind),
        Some(ActionErrorKind::Cycle)
    );
    // The workflow was not half-expanded into a silently empty job.
    assert_eq!(wf.jobs["build"].steps.len(), 1);
}

/// 4. Six nested composite Actions exceed the limit of five.
#[test]
fn a_sixth_composite_level_fails_closed() {
    let cache = tempfile::tempdir().unwrap();
    let mut wf = workflow(
        r#"
on: push
jobs:
  build:
    steps:
      - uses: ./depth1
"#,
    );

    let error = wf
        .expand_composite_actions(&fetcher(&cache))
        .expect_err("depth 6 must be rejected");
    let message = format!("{error:#}");
    assert!(message.contains("maximum depth"), "{message}");
    assert_eq!(
        error.downcast_ref::<ActionError>().map(ActionError::kind),
        Some(ActionErrorKind::DepthLimit)
    );
}

/// 5. A node20 Action is refused with a message naming the missing capability.
#[test]
fn a_node_action_is_refused_and_names_actions_v1() {
    let cache = tempfile::tempdir().unwrap();
    let mut wf = workflow(
        r#"
on: push
jobs:
  build:
    steps:
      - uses: ./node20
"#,
    );

    let error = wf
        .expand_composite_actions(&fetcher(&cache))
        .expect_err("node actions must fail closed");
    let message = format!("{error:#}");
    assert!(message.contains("actions-v1"), "{message}");
    assert!(message.contains("node20"), "{message}");
    // No server-side path leaks into the user-facing message.
    assert!(
        !message.contains(&*fixtures_root().display().to_string()),
        "{message}"
    );
    assert_eq!(
        error.downcast_ref::<ActionError>().map(ActionError::kind),
        Some(ActionErrorKind::Unsupported)
    );

    // The same rejection at Pipeline creation: a workflow with a real node
    // Action never passes validation.
    let direct = workflow(
        r#"
on: push
jobs:
  build:
    steps:
      - uses: actions/setup-node@v4
"#,
    );
    let message = direct.validate_supported_actions().unwrap_err().to_string();
    assert!(message.contains("actions-v1"), "{message}");
}

/// 6. `./local/path` with a subdirectory resolves without any cache or clone.
#[test]
fn a_local_path_composite_resolves_from_the_workflow_source() {
    let cache = tempfile::tempdir().unwrap();
    let mut wf = workflow(
        r#"
on: push
jobs:
  build:
    steps:
      - uses: ./grouped/local
"#,
    );

    wf.expand_composite_actions(&fetcher(&cache)).unwrap();

    assert_eq!(
        runs(&wf.jobs["build"].steps),
        vec!["echo grouped-local".to_string()]
    );
    // Nothing was written to the remote-action cache.
    assert!(!cache.path().join("actions").exists());
}

/// 7. Inputs: a manifest default is used when the caller says nothing, and the
///    caller's `with` wins when it does.
#[test]
fn inputs_use_defaults_unless_the_caller_overrides_them() {
    let cache = tempfile::tempdir().unwrap();
    let mut wf = workflow(
        r#"
on: push
jobs:
  build:
    steps:
      - uses: ./inputs
        with:
          greeting: hi
      - uses: ./inputs
        with:
          greeting: hello
          who: ironforge
      - uses: ./inputs
        with:
          greeting: hey
          who: iron forge
"#,
    );

    wf.expand_composite_actions(&fetcher(&cache)).unwrap();

    assert_eq!(
        runs(&wf.jobs["build"].steps),
        vec![
            "echo \"hi world\"".to_string(),
            "echo \"hello ironforge\"".to_string(),
            "echo \"hey iron forge\"".to_string(),
        ]
    );
    let env = &wf.jobs["build"].steps[0].env;
    assert_eq!(env["IRONFORGE_ACTION_PATH"], "/workspace/inputs");
    assert_eq!(env["INPUT_GREETING"], "hi");
    assert_eq!(env["INPUT_WHO"], "world");
    // A value with a space would break the converter's unquoted `export K=V`,
    // so it is only substituted into the script, not exported.
    assert!(
        !wf.jobs["build"].steps[2].env.contains_key("INPUT_WHO"),
        "unsafe env value must not be exported: {:?}",
        wf.jobs["build"].steps[2].env
    );
}

/// 8. Regression: checkout and cache keep their native adapters, and expansion
///    leaves those steps untouched.
#[test]
fn checkout_and_cache_behaviour_is_unchanged() {
    let cache = tempfile::tempdir().unwrap();
    let mut wf = workflow(
        r#"
on: push
jobs:
  test:
    steps:
      - uses: actions/checkout@v4
      - uses: actions/cache@v4
        with:
          path: |
            target
            .cargo/registry
          key: build-${{ github.sha }}
      - run: cargo test
"#,
    );

    wf.expand_composite_actions(&fetcher(&cache)).unwrap();
    assert_eq!(wf.jobs["test"].steps.len(), 3, "nothing was expanded");
    wf.validate_supported_actions().unwrap();

    let ci = wf.to_ci_config(&context());
    let config = &ci.jobs["test"];
    let cache_config = config.cache.as_ref().expect("cache is mapped natively");
    assert_eq!(cache_config.key, "build-${CI_SHA}");
    assert_eq!(cache_config.paths, vec!["target", ".cargo/registry"]);
    let script = config.script.join("\n");
    assert!(script.contains("cargo test"), "{script}");
    assert!(
        !script.contains("does not support action"),
        "built-ins must not be refused: {script}"
    );
}

/// Declared composite outputs are published on the calling step's `id`, with a
/// generated step that reads the inner step's output file.
#[test]
fn composite_outputs_are_published_on_the_calling_step() {
    let cache = tempfile::tempdir().unwrap();
    let mut wf = workflow(
        r#"
on: push
jobs:
  build:
    steps:
      - id: released
        uses: ./outputs
      - run: echo "${{ steps.released.outputs.version }}"
"#,
    );

    wf.expand_composite_actions(&fetcher(&cache)).unwrap();

    let steps = &wf.jobs["build"].steps;
    assert_eq!(steps.len(), 3, "inner step + publisher + trailing run");
    assert_eq!(steps[0].id.as_deref(), Some("released.detect"));
    assert_eq!(steps[1].id.as_deref(), Some("released"));
    let publisher = steps[1].run.as_deref().expect("publisher has a script");
    assert!(
        publisher.contains("version<<IRONFORGE_COMPOSITE_OUTPUT"),
        "{publisher}"
    );
    assert!(
        publisher.contains(".ironforge-outputs/step-0.outputs"),
        "the inner step is at index 0: {publisher}"
    );

    let ci = wf.to_ci_config(&context());
    // The publisher keeps the caller's id, so it receives the output file.
    let script = ci.jobs["build"].script.join("\n");
    assert!(
        script.contains("step-1.outputs"),
        "the publishing step is at index 1: {script}"
    );
}

/// A Docker Action is refused for the same reason as a Node Action.
#[test]
fn a_docker_action_is_refused_and_names_actions_v1() {
    let cache = tempfile::tempdir().unwrap();
    let mut wf = workflow(
        r#"
on: push
jobs:
  build:
    steps:
      - uses: ./dockerish
"#,
    );
    let message = format!(
        "{:#}",
        wf.expand_composite_actions(&fetcher(&cache))
            .expect_err("docker actions must fail closed")
    );
    assert!(message.contains("actions-v1"), "{message}");
    assert!(message.contains("Dockerfile"), "{message}");
}

/// A remote Action with no configured source is refused instead of guessing a
/// host. The real fetch path is covered by the ignored test below.
#[test]
fn an_uncached_remote_action_fails_closed_without_a_remote_source() {
    let cache = tempfile::tempdir().unwrap();
    let mut wf = workflow(
        r#"
on: push
jobs:
  build:
    steps:
      - uses: some-owner/some-composite@v1
"#,
    );
    let error = wf
        .expand_composite_actions(&fetcher(&cache))
        .expect_err("an uncached remote action must not be fetched implicitly");
    let message = format!("{error:#}");
    assert!(message.contains("not cached"), "{message}");
    assert!(message.contains("actions-v1"), "{message}");
}

/// A pre-populated cache entry resolves without any network: `owner/repo/sub@v1`
/// is looked up under `{cache}/{owner}/{repo}/{ref}/{sub}`.
#[test]
fn a_cached_remote_action_resolves_without_network() {
    let cache = tempfile::tempdir().unwrap();
    let action_dir = cache.path().join("owner/repo/v1/sub");
    std::fs::create_dir_all(&action_dir).unwrap();
    std::fs::write(
        action_dir.join("action.yml"),
        "name: cached\nruns:\n  using: composite\n  steps:\n    - run: echo cached\n      shell: bash\n",
    )
    .unwrap();

    let fetcher = ActionFetcher::for_directory(fixtures_root(), cache.path());
    let mut wf = workflow(
        r#"
on: push
jobs:
  build:
    steps:
      - uses: owner/repo/sub@v1
"#,
    );
    wf.expand_composite_actions(&fetcher).unwrap();
    assert_eq!(
        runs(&wf.jobs["build"].steps),
        vec!["echo cached".to_string()]
    );
}

/// Real network fetch, opt-in. Set `IRONFORGE_ACTION_REMOTE_BASE` to a git host,
/// e.g. `https://github.com`, and run with `--ignored`.
#[test]
#[ignore = "requires network access to a git host"]
fn fetches_a_real_remote_action_from_the_configured_base() {
    let base = std::env::var("IRONFORGE_ACTION_REMOTE_BASE")
        .expect("set IRONFORGE_ACTION_REMOTE_BASE to a git host such as https://github.com");
    let cache = tempfile::tempdir().unwrap();
    let fetcher =
        ActionFetcher::for_directory(fixtures_root(), cache.path()).with_remote_base(base);

    let resolved = fetcher.resolve("actions/setup-node@v4").expect("fetch");
    assert!(matches!(resolved.manifest.runs, ActionRuns::Node { .. }));

    // A second resolve must be a cache hit (no network reachable check here;
    // the cache directory is what makes it deterministic).
    let cached = fetcher.resolve("actions/setup-node@v4").expect("cache hit");
    assert_eq!(cached.identity, resolved.identity);
}

/// The production shape of a local Action: the manifest lives in the pipeline's
/// commit, not in a checked-out working tree. This drives the commit-tree
/// reader end to end and prints the job script a Pipeline would receive.
#[test]
fn a_local_composite_is_expanded_from_the_workflow_commit() {
    let repo = tempfile::tempdir().unwrap();
    git(repo.path(), &["init", "-q", "-b", "main"]);
    git(repo.path(), &["config", "user.email", "ci@example.invalid"]);
    git(repo.path(), &["config", "user.name", "IronForge CI"]);
    std::fs::create_dir_all(repo.path().join(".gitea/actions/greet")).unwrap();
    std::fs::write(
        repo.path().join(".gitea/actions/greet/action.yml"),
        "name: greet\ninputs:\n  who:\n    default: world\nruns:\n  using: composite\n  steps:\n    - run: echo \"hi ${{ inputs.who }}\"\n      shell: bash\n",
    )
    .unwrap();
    std::fs::create_dir_all(repo.path().join(".gitea/workflows")).unwrap();
    std::fs::write(
        repo.path().join(".gitea/workflows/ci.yml"),
        "on: push\njobs:\n  build:\n    steps:\n      - uses: ./.gitea/actions/greet\n        with:\n          who: pipeline\n",
    )
    .unwrap();
    let workflow_text =
        std::fs::read_to_string(repo.path().join(".gitea/workflows/ci.yml")).unwrap();
    git(repo.path(), &["add", "-A"]);
    git(repo.path(), &["commit", "-qm", "workflow and action"]);
    let sha = git(repo.path(), &["rev-parse", "HEAD"]);

    let cache = tempfile::tempdir().unwrap();
    let fetcher = ActionFetcher::for_git_commit(repo.path(), sha, cache.path());

    let mut wf = GiteaWorkflow::parse(&workflow_text).unwrap();
    wf.expand_composite_actions(&fetcher).unwrap();

    let steps = &wf.jobs["build"].steps;
    assert_eq!(steps.len(), 1);
    assert_eq!(steps[0].run.as_deref(), Some("echo \"hi pipeline\""));

    let ci = wf.to_ci_config(&context());
    let script = ci.jobs["build"].script.join("\n");
    assert!(script.contains("echo \"hi pipeline\""), "{script}");
    // Printed so the report can quote a real converted job.
    println!("converted script:\n{script}");
}

fn git(dir: &Path, args: &[&str]) -> String {
    // Through the gateway: `test_no_raw_git_command_in_crates` guards the whole
    // workspace, tests included, so fixtures spawn git the same way production
    // does.
    let gateway = rg_git::cli_gateway::GitCommandGateway::new().expect("git gateway");
    let output = gateway.run(args, Some(dir)).expect("git is installed");
    assert!(
        output.success(),
        "git {args:?} failed: {}",
        output.stderr_str()
    );
    output.stdout_str().trim().to_string()
}
