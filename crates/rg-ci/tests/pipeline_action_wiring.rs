//! T34 acceptance through the real entry point: `trigger_pipeline` must expand
//! composite Actions before the job rows are written, and must surface the
//! `actions-v1` rejection instead of falling back to "no CI config found".
//!
//! No network: the remote Action is served from the cache directory the
//! production code uses (`<repo parent>/.ironforge/actions`).

use std::path::{Path, PathBuf};

use rg_ci::{trigger_pipeline, TriggerPipelineParams};
use rg_db::sea_orm::{NotSet, Set};

fn git(dir: &Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .expect("git is installed");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

/// A bare repository with one commit, laid out like the server's data
/// directory (`<root>/owner/<name>.git`). Returns (bare path, sha).
fn seed_repository(root: &Path, files: &[(&str, &str)]) -> (PathBuf, String) {
    let worktree = root.join("worktree");
    std::fs::create_dir_all(&worktree).unwrap();
    git(&worktree, &["init", "-q", "-b", "main"]);
    git(&worktree, &["config", "user.email", "ci@example.invalid"]);
    git(&worktree, &["config", "user.name", "IronForge CI"]);
    for (relative, contents) in files {
        let path = worktree.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
    }
    git(&worktree, &["add", "-A"]);
    git(&worktree, &["commit", "-qm", "ci"]);
    let sha = git(&worktree, &["rev-parse", "HEAD"]);

    let owner = root.join("owner");
    std::fs::create_dir_all(&owner).unwrap();
    let bare = owner.join("ci.git");
    let status = std::process::Command::new("git")
        .args(["clone", "--bare", "-q"])
        .arg(&worktree)
        .arg(&bare)
        .status()
        .expect("git is installed");
    assert!(status.success(), "bare clone failed");
    (bare, sha)
}

/// Prime the cache entry for `owner/repo@ref` exactly where the production
/// reader looks for it: next to the bare repository directory.
fn prime_remote_action(bare: &Path, owner: &str, name: &str, git_ref: &str, manifest: &str) {
    let dir = bare
        .parent()
        .unwrap()
        .join(".ironforge/actions")
        .join(owner)
        .join(name)
        .join(git_ref);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("action.yml"), manifest).unwrap();
}

async fn database(root: &Path) -> rg_db::DatabaseConnection {
    let db = rg_db::connect(&format!(
        "sqlite://{}?mode=rwc",
        root.join("ci.db").display()
    ))
    .await
    .unwrap();
    rg_db::run_migrations(&db).await.unwrap();
    db
}

async fn create_repo_row(db: &rg_db::DatabaseConnection, root: &Path) -> i64 {
    let user = rg_db::ops::user_ops::create_user(
        db,
        "t34-owner",
        "t34@example.invalid",
        "unused",
        "T34 Owner",
    )
    .await
    .unwrap();
    let now = chrono::Utc::now();
    let repo = rg_db::ops::repo_ops::create(
        db,
        rg_db::entities::repository::ActiveModel {
            id: NotSet,
            owner_id: Set(user.id),
            name: Set(format!(
                "t34-{}",
                root.file_name().unwrap().to_string_lossy()
            )),
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
            is_template: Set(false),
        },
    )
    .await
    .unwrap();
    repo.id
}

async fn trigger(
    db: &rg_db::DatabaseConnection,
    repo_id: i64,
    repo_path: &Path,
    sha: &str,
) -> anyhow::Result<i64> {
    trigger_pipeline(TriggerPipelineParams {
        db,
        repo_path,
        repo_id,
        commit_sha: sha,
        ref_name: "refs/heads/main",
        trigger_type: "push",
        triggered_by: None,
        docker_enabled: false,
        external_runners: true,
        allow_host_execution: true,
        jwt_secret: Some("secret"),
        external_url: None,
    })
    .await
}

/// A workflow with a repository-local composite Action and a cached remote
/// composite Action creates a Pipeline whose job script contains both Actions'
/// commands and no `uses:`.
#[tokio::test]
async fn creating_a_pipeline_expands_local_and_cached_remote_composites() {
    let root = tempfile::tempdir().unwrap();
    let (repo, sha) = seed_repository(
        root.path(),
        &[
            (
                ".gitea/actions/greet/action.yml",
                "name: greet\ninputs:\n  who:\n    default: world\nruns:\n  using: composite\n  steps:\n    - run: echo \"hi ${{ inputs.who }}\"\n      shell: bash\n",
            ),
            (
                ".gitea/workflows/ci.yml",
                "on: push\njobs:\n  build:\n    steps:\n      - uses: ./.gitea/actions/greet\n        with:\n          who: pipeline\n      - uses: some-owner/some-composite@v1\n",
            ),
        ],
    );
    prime_remote_action(
        &repo,
        "some-owner",
        "some-composite",
        "v1",
        "name: remote\nruns:\n  using: composite\n  steps:\n    - run: echo remote-composite\n      shell: bash\n",
    );

    let db = database(root.path()).await;
    let repo_id = create_repo_row(&db, root.path()).await;

    let pipeline_id = trigger(&db, repo_id, &repo, &sha)
        .await
        .expect("pipeline is created");
    let jobs = rg_db::ops::pipeline_ops::list_jobs_by_pipeline(&db, pipeline_id)
        .await
        .unwrap();
    let job = jobs
        .iter()
        .find(|job| job.name == "ci/build")
        .expect("converted job");
    println!("pipeline job script:\n{}", job.script);
    assert!(
        job.script.contains("echo \"hi pipeline\""),
        "{}",
        job.script
    );
    assert!(
        job.script.contains("echo remote-composite"),
        "{}",
        job.script
    );
    assert!(
        !job.script.contains("uses:"),
        "no action step may survive conversion: {}",
        job.script
    );
}

/// A workflow with a Node Action is refused while the Pipeline is created, and
/// the message names the missing capability rather than leaking a server path.
#[tokio::test]
async fn creating_a_pipeline_refuses_a_node_action_with_actions_v1() {
    let root = tempfile::tempdir().unwrap();
    let (repo, sha) = seed_repository(
        root.path(),
        &[(
            ".gitea/workflows/ci.yml",
            "on: push\njobs:\n  build:\n    steps:\n      - uses: actions/setup-node@v4\n",
        )],
    );
    let db = database(root.path()).await;
    let repo_id = create_repo_row(&db, root.path()).await;

    let error = trigger(&db, repo_id, &repo, &sha)
        .await
        .expect_err("a node action must be refused");
    let message = format!("{error:#}");
    assert!(message.contains("actions-v1"), "{message}");
    assert!(message.contains("actions/setup-node@v4"), "{message}");
    assert!(
        !message.contains(&*root.path().display().to_string()),
        "the message must not leak the repository path: {message}"
    );
}
