//! Import pipeline service — orchestrates full repository migration.
//!
//! Supports importing from GitHub and GitLab, including:
//! - Repository cloning (git clone --bare) + IronForge DB registration
//! - Labels and milestones
//! - Issues with comments
//! - Pull/Merge requests with reviews/comments
//! - Releases
//!
//! Generic Git and Gitea imports currently clone the repository only.
//!
//! The import runs asynchronously and updates progress in the
//! import_tasks database table.

use anyhow::Result;
use chrono::Utc;
use sea_orm::{ActiveValue::Set, DatabaseConnection};
use std::collections::HashMap;
use std::path::Path;

use rg_db::entities::import_task::{self, Model as ImportTask};
use rg_db::entities::{label, milestone};
use rg_db::ops::{
    import_task_ops, issue_comment_ops, issue_ops, label_ops, milestone_ops, org_ops,
    pr_review_ops, pull_request_ops, user_ops,
};

use crate::error::{CoreError, CoreResult};
use crate::import::cnb_client::{
    parse_cnb_url, CnbClient, CnbComment, CnbIssue, CnbPull, CnbPullRef, CnbUser,
};
use crate::import::github_client::{
    GitHubClient, GitHubComment, GitHubIssue, GitHubLabel, GitHubMilestone, GitHubPR, GitHubRef,
    GitHubRelease, GitHubReview, GitHubUser,
};
use crate::import::gitlab_client::{
    GitLabClient, GitLabIssue, GitLabLabel, GitLabMR, GitLabMilestone, GitLabNote, GitLabRelease,
};

/// Statistics collected during import.
#[derive(Debug, Default, serde::Serialize)]
pub struct ImportStats {
    pub repo_cloned: bool,
    pub labels_imported: usize,
    pub milestones_imported: usize,
    pub issues_imported: usize,
    pub issue_comments_imported: usize,
    pub prs_imported: usize,
    pub pr_reviews_imported: usize,
    pub releases_imported: usize,
    pub wiki_pages_imported: usize,
}

/// Credentials for the source repository, resolved once per import run.
///
/// Deliberately no `Debug`: the token is plaintext and an accidental
/// `tracing::debug!(?auth)` must not be able to print it.
pub(crate) struct SourceAuth {
    pub username: String,
    pub token: String,
}

impl std::fmt::Debug for SourceAuth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SourceAuth")
            .field("username", &self.username)
            .field("token", &"***")
            .finish()
    }
}

/// Read the task's stored token, tolerating rows written before the column was
/// actually encrypted.
///
/// Same shape as `mirror::service::decode_mirror_password`; the duplication is
/// deliberate until that one lands, at which point both should call a single
/// helper in `auth::encryption`.
fn decode_task_token(stored: &str, enc_key: &[u8; 32]) -> CoreResult<String> {
    match crate::auth::encryption::decrypt(stored, enc_key) {
        Ok(plain) => Ok(plain),
        Err(e) => {
            if looks_like_ciphertext(stored) {
                return Err(CoreError::internal(format!(
                    "stored import token cannot be decrypted (instance secret changed?): {e}"
                )));
            }
            // A pre-encryption row: the column held whatever the caller sent.
            Ok(stored.to_string())
        }
    }
}

/// Is this value shaped like `encrypt` output (base64url of nonce + ciphertext)?
fn looks_like_ciphertext(value: &str) -> bool {
    use base64::Engine as _;
    match base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(value) {
        Ok(bytes) => bytes.len() >= 28,
        Err(_) => false,
    }
}

/// Which credentials the clone (and the platform API) should use.
///
/// Order: the task's own token first — a user who pasted one into the import
/// dialog meant *that* token. Failing that, the importing user's host
/// dictionary, which is the "configure `github.com` once" path.
async fn resolve_source_auth(
    db: &DatabaseConnection,
    task: &ImportTask,
    enc_key: &[u8; 32],
) -> CoreResult<Option<SourceAuth>> {
    if let Some(stored) = task.auth_token_encrypted.as_deref() {
        let token = decode_task_token(stored, enc_key)?;
        if !token.is_empty() {
            return Ok(Some(SourceAuth {
                username: rg_db::entities::remote_credential::DEFAULT_BASIC_USERNAME.to_string(),
                token,
            }));
        }
    }

    let scope = crate::remote_credential::service::Scope::User(task.user_id);
    match crate::remote_credential::service::resolve(db, scope, &task.source_url, enc_key).await {
        Ok(Some(credential)) => Ok(Some(SourceAuth {
            username: credential.username,
            token: credential.token,
        })),
        Ok(None) => Ok(None),
        // An unparsable URL means no credential can exist for it — not that the
        // import failed. The platform importer reports the bad URL where the
        // error is actionable; failing here would replace a precise message
        // ("invalid GitHub URL") with a generic one about hosts.
        Err(CoreError::InvalidInput(_)) => Ok(None),
        Err(error) => Err(error),
    }
}

/// Run a full import pipeline and update the import task as it progresses.
pub async fn run_import(
    db: &DatabaseConnection,
    task: &ImportTask,
    repo_root: &Path,
    enc_key: &[u8; 32],
) -> Result<ImportStats> {
    let mut stats = ImportStats::default();
    let auth = resolve_source_auth(db, task, enc_key).await?;

    match task.platform.as_str() {
        "github" => run_github_import(db, task, repo_root, auth.as_ref(), &mut stats, None).await?,
        "gitlab" => run_gitlab_import(db, task, repo_root, auth.as_ref(), &mut stats).await?,
        // Gitea's API deliberately mirrors GitHub's paths
        // (`/repos/{owner}/{repo}/issues`, `/labels`, ...), so the GitHub client
        // does the work once it is pointed at `<host>/api/v1`. Falling back to a
        // plain git clone threw away issues, labels, milestones and releases
        // that the source was perfectly willing to hand over.
        "gitea" => {
            let api_base = gitea_api_base(&task.source_url)?;
            run_github_import(
                db,
                task,
                repo_root,
                auth.as_ref(),
                &mut stats,
                Some(api_base),
            )
            .await?
        }
        "cnb" => run_cnb_import(db, task, repo_root, auth.as_ref(), &mut stats).await?,
        "git" => run_git_import(db, task, repo_root, auth.as_ref(), &mut stats).await?,
        other => {
            return Err(CoreError::invalid_input(format!("unsupported platform: {other}")).into())
        }
    }

    Ok(stats)
}

// ═══════════════════════════════════════════════════════════════════════
// Generic Git / Gitea import
// ═══════════════════════════════════════════════════════════════════════

async fn run_git_import(
    db: &DatabaseConnection,
    task: &ImportTask,
    repo_root: &Path,
    auth: Option<&SourceAuth>,
    stats: &mut ImportStats,
) -> Result<()> {
    let repo_id =
        resolve_or_create_target_repo(db, &task.target_owner, &task.target_name, repo_root).await?;
    let _ = import_task_ops::set_repo_id(db, task.id, repo_id).await;

    if task.import_repo {
        update_stage(db, task.id, "cloning", 0, "Cloning repository...").await?;
        clone_repo(
            &task.source_url,
            repo_root,
            &task.target_owner,
            &task.target_name,
            auth,
        )?;
        stats.repo_cloned = true;
        update_stage(db, task.id, "importing", 90, "Repository cloned").await?;
    }

    Ok(())
}

async fn load_milestone_map(db: &DatabaseConnection, repo_id: i64) -> Result<HashMap<String, i64>> {
    let existing = milestone_ops::list_by_repo(db, repo_id, None).await?;
    let mut map = HashMap::with_capacity(existing.len());
    for ms in existing {
        map.insert(ms.title.clone(), ms.id);
    }
    Ok(map)
}

// ═══════════════════════════════════════════════════════════════════════
// GitHub import
// ═══════════════════════════════════════════════════════════════════════

async fn run_github_import(
    db: &DatabaseConnection,
    task: &ImportTask,
    repo_root: &Path,
    auth: Option<&SourceAuth>,
    stats: &mut ImportStats,
    api_base: Option<String>,
) -> Result<()> {
    let token = auth.map(|auth| auth.token.as_str()).unwrap_or("");
    let client = GitHubClient::new(token.to_string(), api_base);

    // Parse owner/repo from source URL (https://github.com/owner/repo)
    let (gh_owner, gh_repo) = parse_github_url(&task.source_url)?;

    // Resolve (or create) the target repo in IronForge DB
    let repo_id =
        resolve_or_create_target_repo(db, &task.target_owner, &task.target_name, repo_root).await?;

    // Update task with repo_id
    let _ = import_task_ops::set_repo_id(db, task.id, repo_id).await;

    // Step 1: Clone repository
    if task.import_repo {
        update_stage(db, task.id, "cloning", 0, "Cloning repository...").await?;
        clone_repo(
            &task.source_url,
            repo_root,
            &task.target_owner,
            &task.target_name,
            auth,
        )?;
        stats.repo_cloned = true;
        update_stage(db, task.id, "importing", 10, "Repository cloned").await?;
    }

    // Build user mapping from all referenced users
    let user_map = build_github_user_map(&client, &gh_owner, &gh_repo, task).await?;

    // Build milestone cache (existing in target repo + newly imported ones when enabled)
    let mut milestone_map = load_milestone_map(db, repo_id).await?;

    // Step 2: Labels

    if task.import_labels {
        update_stage(db, task.id, "importing", 15, "Importing labels...").await?;
        let labels = client.list_labels(&gh_owner, &gh_repo).await?;
        stats.labels_imported = import_github_labels(db, repo_id, &labels).await?;
        update_stage(
            db,
            task.id,
            "importing",
            20,
            &format!("Imported {} labels", stats.labels_imported),
        )
        .await?;
    }

    // Step 3: Milestones
    if task.import_milestones {
        update_stage(db, task.id, "importing", 25, "Importing milestones...").await?;
        let milestones = client.list_milestones(&gh_owner, &gh_repo).await?;
        stats.milestones_imported =
            import_github_milestones(db, repo_id, &milestones, &mut milestone_map).await?;
        update_stage(
            db,
            task.id,
            "importing",
            30,
            &format!("Imported {} milestones", stats.milestones_imported),
        )
        .await?;
    }

    // Step 4: Issues
    if task.import_issues {
        update_stage(db, task.id, "importing", 35, "Importing issues...").await?;
        let issues = client.list_issues(&gh_owner, &gh_repo).await?;
        let total = issues.len();

        for (i, issue) in issues.iter().enumerate() {
            let comments = client
                .list_issue_comments(&gh_owner, &gh_repo, issue.number)
                .await?;
            import_github_issue(
                db,
                repo_id,
                &task.target_owner,
                &task.target_name,
                issue,
                &comments,
                &user_map,
                &milestone_map,
            )
            .await?;
            stats.issues_imported += 1;
            stats.issue_comments_imported += comments.len();

            let pct = 35 + (i as f64 / total.max(1) as f64 * 20.0) as i32;
            update_stage(
                db,
                task.id,
                "importing",
                pct,
                &format!("Importing issues ({}/{})", i + 1, total),
            )
            .await?;
        }
    }

    // Step 5: Pull Requests
    if task.import_pull_requests {
        update_stage(db, task.id, "importing", 60, "Importing pull requests...").await?;
        let prs = client.list_pull_requests(&gh_owner, &gh_repo).await?;
        let total = prs.len();

        for (i, pr) in prs.iter().enumerate() {
            let comments = client
                .list_issue_comments(&gh_owner, &gh_repo, pr.number)
                .await?;
            let reviews = client
                .list_pr_reviews(&gh_owner, &gh_repo, pr.number)
                .await?;
            import_github_pr(
                db,
                repo_id,
                pr,
                &comments,
                &reviews,
                &user_map,
                &milestone_map,
            )
            .await?;
            stats.prs_imported += 1;
            stats.pr_reviews_imported += reviews.len();
            stats.issue_comments_imported += comments.len();

            let pct = 60 + (i as f64 / total.max(1) as f64 * 15.0) as i32;
            update_stage(
                db,
                task.id,
                "importing",
                pct,
                &format!("Importing PRs ({}/{})", i + 1, total),
            )
            .await?;
        }
    }

    // Step 6: Releases
    if task.import_releases {
        update_stage(db, task.id, "importing", 80, "Importing releases...").await?;
        let releases = client.list_releases(&gh_owner, &gh_repo).await?;
        stats.releases_imported =
            import_github_releases(db, repo_id, &releases, &user_map, repo_root).await?;
        update_stage(
            db,
            task.id,
            "importing",
            90,
            &format!("Imported {} releases", stats.releases_imported),
        )
        .await?;
    }

    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════
// GitLab import
// ═══════════════════════════════════════════════════════════════════════

async fn run_gitlab_import(
    db: &DatabaseConnection,
    task: &ImportTask,
    repo_root: &Path,
    auth: Option<&SourceAuth>,
    stats: &mut ImportStats,
) -> Result<()> {
    let token = auth.map(|auth| auth.token.as_str()).unwrap_or("");
    // Without a base URL the client talks to gitlab.com, so a self-hosted
    // instance is queried at the wrong host entirely — the migration then fails
    // with "project not found" for a project that plainly exists.
    let client = GitLabClient::new(token.to_string(), Some(gitlab_api_base(&task.source_url)?));

    // Extract project path from source URL (https://gitlab.com/group/project)
    let project_path = parse_gitlab_url(&task.source_url)?;

    // Resolve (or create) the target repo in IronForge DB
    let repo_id =
        resolve_or_create_target_repo(db, &task.target_owner, &task.target_name, repo_root).await?;

    let _ = import_task_ops::set_repo_id(db, task.id, repo_id).await;

    // Step 1: Clone repository
    if task.import_repo {
        update_stage(db, task.id, "cloning", 0, "Cloning repository...").await?;
        let project = client.get_project(&project_path).await?;
        // The token is *not* spliced into the URL any more: gix persists the
        // clone URL as `remote.origin.url`, so `oauth2:<token>@host` would stay
        // readable in the repository config forever. It goes in the header.
        //
        // The header's username matters: GitLab only accepts `oauth2` (or a real
        // account name) with an OAuth token, and rejects the `x-access-token`
        // default that the GitHub path uses with a bare 401 that says nothing
        // about the cause.
        let gitlab_auth = auth.map(|auth| SourceAuth {
            username: "oauth2".to_string(),
            token: auth.token.clone(),
        });
        clone_repo(
            &project.http_url_to_repo,
            repo_root,
            &task.target_owner,
            &task.target_name,
            gitlab_auth.as_ref(),
        )?;
        stats.repo_cloned = true;
        update_stage(db, task.id, "importing", 10, "Repository cloned").await?;
    }

    // Build milestone cache (existing in target repo + newly imported ones when enabled)
    let mut milestone_map = load_milestone_map(db, repo_id).await?;

    // Step 2: Labels

    if task.import_labels {
        update_stage(db, task.id, "importing", 15, "Importing labels...").await?;
        let labels = client.list_labels(&project_path).await?;
        stats.labels_imported = import_gitlab_labels(db, repo_id, &labels).await?;
        update_stage(
            db,
            task.id,
            "importing",
            20,
            &format!("Imported {} labels", stats.labels_imported),
        )
        .await?;
    }

    // Step 3: Milestones
    if task.import_milestones {
        update_stage(db, task.id, "importing", 25, "Importing milestones...").await?;
        let milestones = client.list_milestones(&project_path).await?;
        stats.milestones_imported =
            import_gitlab_milestones(db, repo_id, &milestones, &mut milestone_map).await?;
        update_stage(
            db,
            task.id,
            "importing",
            30,
            &format!("Imported {} milestones", stats.milestones_imported),
        )
        .await?;
    }

    // Build user map
    let user_map = build_gitlab_user_map(&client, &project_path, task).await?;

    // Step 4: Issues
    if task.import_issues {
        update_stage(db, task.id, "importing", 35, "Importing issues...").await?;
        let issues = client.list_issues(&project_path).await?;
        let total = issues.len();

        for (i, issue) in issues.iter().enumerate() {
            let notes = client.list_issue_notes(&project_path, issue.iid).await?;
            import_gitlab_issue(
                db,
                repo_id,
                &task.target_owner,
                &task.target_name,
                issue,
                &notes,
                &user_map,
                &milestone_map,
            )
            .await?;
            stats.issues_imported += 1;
            stats.issue_comments_imported += notes.len();

            let pct = 35 + (i as f64 / total.max(1) as f64 * 20.0) as i32;
            update_stage(
                db,
                task.id,
                "importing",
                pct,
                &format!("Importing issues ({}/{})", i + 1, total),
            )
            .await?;
        }
    }

    // Step 5: Merge Requests
    if task.import_pull_requests {
        update_stage(db, task.id, "importing", 60, "Importing merge requests...").await?;
        let mrs = client.list_merge_requests(&project_path).await?;
        let total = mrs.len();

        for (i, mr) in mrs.iter().enumerate() {
            let notes = client.list_mr_notes(&project_path, mr.iid).await?;
            import_gitlab_mr(db, repo_id, mr, &notes, &user_map, &milestone_map).await?;
            stats.prs_imported += 1;
            stats.issue_comments_imported += notes.len();

            let pct = 60 + (i as f64 / total.max(1) as f64 * 15.0) as i32;
            update_stage(
                db,
                task.id,
                "importing",
                pct,
                &format!("Importing MRs ({}/{})", i + 1, total),
            )
            .await?;
        }
    }

    // Step 6: Releases
    if task.import_releases {
        update_stage(db, task.id, "importing", 80, "Importing releases...").await?;
        let releases = client.list_releases(&project_path).await?;
        stats.releases_imported =
            import_gitlab_releases(db, repo_id, &releases, &user_map, repo_root).await?;
        update_stage(
            db,
            task.id,
            "importing",
            90,
            &format!("Imported {} releases", stats.releases_imported),
        )
        .await?;
    }

    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════
// Target repo resolution
// ═══════════════════════════════════════════════════════════════════════

/// Find the target repo in IronForge DB, or create it if it doesn't exist.
/// Returns the repo_id for use in all subsequent import operations.
async fn resolve_or_create_target_repo(
    db: &DatabaseConnection,
    target_owner: &str,
    target_name: &str,
    repo_root: &Path,
) -> Result<i64> {
    // Try to find existing repo via the repo service (handles user+org lookup)
    if let Some(repo) = crate::repo::service::find_repo_by_owner_name(db, target_owner, target_name)
        .await
        .unwrap_or(None)
    {
        tracing::info!(repo_id = repo.id, "Found existing target repo");
        return Ok(repo.id);
    }

    // Resolve owner: try user first, then org
    let (owner_id, org_id) =
        if let Some(user) = user_ops::find_by_username(db, target_owner).await? {
            (user.id, None)
        } else if let Some(org) = org_ops::get_org_by_name(db, target_owner).await? {
            (org.owner_id, Some(org.id))
        } else {
            return Err(CoreError::not_found(format!(
                "target owner '{}' not found (must be an existing IronForge user or organization)",
                target_owner
            ))
            .into());
        };

    // Create the repo via the repo service
    let repo = crate::repo::service::create_repo(
        db,
        owner_id,
        target_name,
        None,  // no description — imported repo
        false, // public by default
        repo_root,
        org_id,
    )
    .await?;

    tracing::info!(repo_id = repo.id, "Created target repo for import");
    Ok(repo.id)
}

// ═══════════════════════════════════════════════════════════════════════
// Git helpers
// ═══════════════════════════════════════════════════════════════════════

/// `http.extraHeader=Authorization: Basic <base64(user:token)>`
fn http_auth_override(username: &str, token: &str) -> String {
    use base64::Engine as _;
    let raw = format!("{username}:{token}");
    format!(
        "http.extraHeader=Authorization: Basic {}",
        base64::engine::general_purpose::STANDARD.encode(raw)
    )
}

fn is_http_url(url: &str) -> bool {
    url.starts_with("http://") || url.starts_with("https://")
}

/// Open options for the clone, optionally carrying a credential.
///
/// Mirrors what `gix::prepare_clone_bare` builds internally (full trust, config
/// from the git installation so credential helpers keep working) and adds the
/// token as a configuration override — the gix equivalent of `git -c`. Nothing
/// is written to disk and the token never appears in the URL.
fn source_open_options(auth: Option<&SourceAuth>, source_url: &str) -> gix::open::Options {
    use gix_sec::trust::DefaultForLevel;
    let mut opts = gix::open::Options::default_for_level(gix_sec::Trust::Full);
    opts.permissions.config.git_binary = true;
    if let Some(auth) = auth {
        // `git://` is anonymous and ssh:// carries its own key: an HTTP header
        // would be silently ignored on both, so only attach it where it applies.
        if is_http_url(source_url) {
            opts = opts.config_overrides([http_auth_override(&auth.username, &auth.token)]);
        }
    }
    opts
}

/// Clone a repository (bare) into the IronForge repo root.
///
/// `auth` reaches the remote as an `http.extraHeader` config override, never in
/// the URL — the URL is persisted as `remote.origin.url` in the new repository's
/// config, where a token would remain readable.
/// Does this bare repository contain at least one ref?
///
/// Written against the filesystem because the question is asked about a
/// directory that may be a freshly created empty shell: opening it as a
/// repository succeeds either way, so it answers nothing.
fn repository_has_refs(target_dir: &Path) -> bool {
    std::fs::read_dir(target_dir.join("refs").join("heads"))
        .map(|mut entries| entries.next().is_some())
        .unwrap_or(false)
        || target_dir.join("packed-refs").exists()
}

fn clone_repo(
    source_url: &str,
    repo_root: &Path,
    owner: &str,
    name: &str,
    auth: Option<&SourceAuth>,
) -> Result<()> {
    let target_dir = repo_root.join(format!("{}/{}.git", owner, name));
    // "Has a HEAD file" is not the same as "has been cloned": creating the
    // target repository writes HEAD before anything is fetched, so using its
    // presence to skip the clone meant a second attempt never fetched at all —
    // and the first attempt reported success with an empty repository.
    if repository_has_refs(&target_dir) {
        tracing::info!(
            path = %target_dir.display(),
            "Repository already exists, skipping clone"
        );
        return Ok(());
    }

    let parent = target_dir
        .parent()
        .ok_or_else(|| anyhow::anyhow!("invalid path: no parent directory"))?;
    std::fs::create_dir_all(parent)?;

    // The target repository is created (with a `HEAD`) before this runs, so the
    // directory is non-empty by now and `gix` refuses to initialise into it —
    // "Refusing to initialize the non-empty directory". Clearing the shell is
    // what makes the clone possible at all; refs are checked above, so anything
    // still here is a leftover from a failed attempt, never imported content.
    if target_dir.exists() {
        std::fs::remove_dir_all(&target_dir).map_err(|error| {
            anyhow::anyhow!(
                "clearing the empty target repository {}: {error}",
                target_dir.display()
            )
        })?;
    }

    // `prepare_clone_bare` is the library form of `git clone --bare`: branches
    // land in `refs/heads/` and no default fetch refspec is written. The
    // `blocking-network-client` feature (implied by the curl transport) also
    // enables the credential helpers, so private sources keep working.
    let interrupt = std::sync::atomic::AtomicBool::new(false);
    let cloned = gix::clone::PrepareFetch::new(
        source_url,
        &target_dir,
        gix::create::Kind::Bare,
        gix::create::Options::default(),
        source_open_options(auth, source_url),
    )
    .and_then(|mut prepare| {
        prepare
            .fetch_only(gix::progress::Discard, &interrupt)
            .map(|_| ())
    });
    // The CLI's stderr named the destination path and echoed the source URL
    // (which may embed a token); the task's `error` field is shown to the
    // user, so keep the detail in the operator log only.
    if let Err(error) = cloned {
        tracing::warn!(error = %format!("{error:#}"), "import: bare clone failed");
        return Err(CoreError::invalid_input(
            "failed to clone the source repository; check that the URL is reachable and any credentials are valid",
        )
        .into());
    }

    // A fetch that reached nothing still leaves a valid *empty* bare repository,
    // so `fetch_only` returning Ok proves nothing. A private source without
    // credentials answers "Repository Not Found", which lands here as an empty
    // success — the task would report `repo_cloned: true` and the target would
    // have no commits. That is the one failure shape a migration must never
    // have, so the result is checked rather than assumed.
    if !repository_has_refs(&target_dir) {
        // Leave nothing behind: an empty repository would make the next attempt
        // take the "already exists" branch above and skip the clone entirely.
        let _ = std::fs::remove_dir_all(&target_dir);
        return Err(CoreError::invalid_input(
            "the source repository answered but contained no commits; check the URL and that the credentials can read it",
        )
        .into());
    }

    tracing::info!(path = %target_dir.display(), "Repository cloned");
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════
// URL parsing
// ═══════════════════════════════════════════════════════════════════════

fn parse_github_url(url: &str) -> Result<(String, String)> {
    let url = url.trim_end_matches('/').trim_end_matches(".git");
    let parts: Vec<&str> = url.split('/').collect();
    if parts.len() < 2 {
        return Err(CoreError::invalid_input(format!("invalid GitHub URL: {url}")).into());
    }
    let repo = parts[parts.len() - 1].to_string();
    let owner = parts[parts.len() - 2].to_string();
    Ok((owner, repo))
}

/// Gitea serves its API on a path of the same host as the web UI.
///
/// Derived from the repository URL rather than asked for separately: the URL is
/// what the user pastes, and asking for the API base as well would be a second
/// chance to get it wrong.
fn gitea_api_base(source_url: &str) -> Result<String> {
    let url = source_url.trim_end_matches('/');
    let (scheme, rest) = url
        .split_once("://")
        .ok_or_else(|| CoreError::invalid_input(format!("invalid Gitea URL: {url}")))?;
    let host = rest.split('/').next().unwrap_or("");
    if host.is_empty() {
        return Err(CoreError::invalid_input(format!("invalid Gitea URL: {url}")).into());
    }
    Ok(format!("{scheme}://{host}/api/v1"))
}

/// GitLab serves its API under `/api/v4` on the same host as the web UI.
///
/// Same reasoning as [`gitea_api_base`]: derive it from the URL the user pasted
/// rather than asking for it again.
fn gitlab_api_base(source_url: &str) -> Result<String> {
    let url = source_url.trim_end_matches('/');
    let (scheme, rest) = url
        .split_once("://")
        .ok_or_else(|| CoreError::invalid_input(format!("invalid GitLab URL: {url}")))?;
    let host = rest.split('/').next().unwrap_or("");
    if host.is_empty() {
        return Err(CoreError::invalid_input(format!("invalid GitLab URL: {url}")).into());
    }
    Ok(format!("{scheme}://{host}/api/v4"))
}

fn parse_gitlab_url(url: &str) -> Result<String> {
    let url = url.trim_end_matches('/').trim_end_matches(".git");
    if let Some(pos) = url.find("://") {
        let after_protocol = &url[pos + 3..];
        if let Some(slash_pos) = after_protocol.find('/') {
            let path = &after_protocol[slash_pos + 1..];
            return Ok(path.to_string());
        }
    }
    Err(CoreError::invalid_input(format!("invalid GitLab URL: {url}")).into())
}

// ═══════════════════════════════════════════════════════════════════════
// User mapping
// ═══════════════════════════════════════════════════════════════════════

async fn build_github_user_map(
    client: &GitHubClient,
    owner: &str,
    repo: &str,
    task: &ImportTask,
) -> Result<HashMap<String, i64>> {
    let mut logins: std::collections::HashSet<String> = std::collections::HashSet::new();

    if task.import_issues {
        if let Ok(issues) = client.list_issues(owner, repo).await {
            for issue in &issues {
                if let Some(ref user) = issue.user {
                    logins.insert(user.login.clone());
                }
                for a in &issue.assignees {
                    logins.insert(a.login.clone());
                }
            }
        }
    }

    if task.import_pull_requests {
        if let Ok(prs) = client.list_pull_requests(owner, repo).await {
            for pr in &prs {
                if let Some(ref user) = pr.user {
                    logins.insert(user.login.clone());
                }
            }
        }
    }

    map_users(task, &logins)
}

async fn build_gitlab_user_map(
    client: &GitLabClient,
    project_id: &str,
    task: &ImportTask,
) -> Result<HashMap<String, i64>> {
    let mut usernames: std::collections::HashSet<String> = std::collections::HashSet::new();

    if let Ok(issues) = client.list_issues(project_id).await {
        for issue in &issues {
            if let Some(ref author) = issue.author {
                usernames.insert(author.username.clone());
            }
        }
    }

    if let Ok(mrs) = client.list_merge_requests(project_id).await {
        for mr in &mrs {
            if let Some(ref author) = mr.author {
                usernames.insert(author.username.clone());
            }
        }
    }

    map_users(task, &usernames)
}

/// Map external user logins/usernames to local IronForge user IDs.
/// Falls back to the importing user if no match is found.
fn map_users(
    task: &ImportTask,
    external_users: &std::collections::HashSet<String>,
) -> Result<HashMap<String, i64>> {
    let mut mapping = HashMap::new();
    for user in external_users {
        mapping.entry(user.clone()).or_insert(task.user_id);
    }
    Ok(mapping)
}

// ═══════════════════════════════════════════════════════════════════════
// Date/time helpers
// ═══════════════════════════════════════════════════════════════════════

fn parse_opt_datetime(s: &Option<String>) -> Option<chrono::DateTime<Utc>> {
    s.as_ref().and_then(|v| {
        chrono::DateTime::parse_from_rfc3339(v)
            .ok()
            .map(|d| d.with_timezone(&Utc))
    })
}

fn parse_datetime_or_now(s: &str) -> chrono::DateTime<Utc> {
    chrono::DateTime::parse_from_rfc3339(s)
        .map(|d| d.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now())
}

// ═══════════════════════════════════════════════════════════════════════
// Import helpers — GitHub
// ═══════════════════════════════════════════════════════════════════════

/// Create labels from bare names, for sources that have no label objects.
async fn import_label_names(
    db: &DatabaseConnection,
    repo_id: i64,
    names: &[String],
) -> Result<usize> {
    let existing = label_ops::list_by_repo(db, repo_id).await?;
    let now = Utc::now();
    let mut count = 0;
    for name in names {
        if existing.iter().any(|label| &label.name == name) {
            continue;
        }
        let model = rg_db::entities::label::ActiveModel {
            id: sea_orm::NotSet,
            repo_id: Set(repo_id),
            name: Set(name.clone()),
            color: Set("ededed".to_string()),
            description: Set(None),
            created_at: Set(now),
            updated_at: Set(now),
        };
        match label_ops::create(db, model).await {
            Ok(_) => count += 1,
            Err(error) => tracing::warn!(%error, name = %name, "failed to create label"),
        }
    }
    Ok(count)
}

async fn import_github_labels(
    db: &DatabaseConnection,
    repo_id: i64,
    labels: &[GitHubLabel],
) -> Result<usize> {
    let existing = label_ops::list_by_repo(db, repo_id).await?;
    let now = Utc::now();
    let mut count = 0;

    for gl in labels {
        // Skip if label with same name already exists
        if existing.iter().any(|l| l.name == gl.name) {
            continue;
        }
        // GitHub colors are "ff0000" without # prefix
        let color = if gl.color.starts_with('#') {
            gl.color.clone()
        } else {
            format!("#{}", gl.color)
        };

        let model = label::ActiveModel {
            id: sea_orm::NotSet, // sea_orm NotSet via Default
            repo_id: Set(repo_id),
            name: Set(gl.name.clone()),
            color: Set(color),
            description: Set(gl.description.clone()),
            created_at: Set(now),
            updated_at: Set(now),
        };

        if let Err(e) = label_ops::create(db, model).await {
            tracing::warn!("Failed to create label '{}': {e}", gl.name);
        } else {
            count += 1;
        }
    }

    Ok(count)
}

async fn import_github_milestones(
    db: &DatabaseConnection,
    repo_id: i64,
    milestones: &[GitHubMilestone],
    milestone_map: &mut HashMap<String, i64>,
) -> Result<usize> {
    let existing = milestone_ops::list_by_repo(db, repo_id, None).await?;
    let now = Utc::now();
    let mut count = 0;

    for gm in milestones {
        // Skip if milestone with same title already exists
        if existing.iter().any(|m| m.title == gm.title) {
            continue;
        }

        let state = match gm.state.as_str() {
            "open" => "open",
            "closed" => "closed",
            _ => "open",
        };

        let due_date = parse_opt_datetime(&gm.due_on);

        let model = milestone::ActiveModel {
            id: sea_orm::NotSet,
            repo_id: Set(repo_id),
            title: Set(gm.title.clone()),
            description: Set(gm.description.clone()),
            state: Set(state.to_string()),
            due_date: Set(due_date),
            created_at: Set(now),
            updated_at: Set(now),
        };

        match milestone_ops::create(db, model).await {
            Ok(ms) => {
                milestone_map.insert(gm.title.clone(), ms.id);
                count += 1;
            }
            Err(e) => {
                tracing::warn!("Failed to create milestone '{}': {e}", gm.title);
            }
        }
    }

    Ok(count)
}

#[allow(clippy::too_many_arguments)]
async fn import_github_issue(
    db: &DatabaseConnection,
    repo_id: i64,
    _target_owner: &str,
    _target_name: &str,
    issue: &GitHubIssue,
    comments: &[GitHubComment],
    user_map: &HashMap<String, i64>,
    milestone_map: &HashMap<String, i64>,
) -> Result<()> {
    // Resolve author
    let author_id = issue
        .user
        .as_ref()
        .and_then(|u| user_map.get(&u.login))
        .copied()
        .unwrap_or(1); // fallback to admin

    // Collect label names
    let label_names: Vec<String> = issue.labels.iter().map(|l| l.name.clone()).collect();
    let labels_json = if label_names.is_empty() {
        None
    } else {
        Some(serde_json::to_string(&label_names).unwrap_or_else(|_| "[]".into()))
    };

    // Resolve milestone
    let milestone_id = issue
        .milestone
        .as_ref()
        .and_then(|m| milestone_map.get(&m.title))
        .copied();

    // Preserve the source number. Renumbering breaks every cross-reference in
    // bodies and commit messages ("fixes #3"), and a migration whose numbers do
    // not match the source looks correct while being wrong — the worst shape a
    // migration bug can take. The target repository is new, so the number
    // cannot already be taken.
    let number = issue.number;
    let created_at = parse_datetime_or_now(&issue.created_at);
    let closed_at = parse_opt_datetime(&issue.closed_at);
    let state = if issue.state == "closed" {
        "closed"
    } else {
        "open"
    };

    let model = rg_db::entities::issue::ActiveModel {
        id: sea_orm::NotSet,
        repo_id: Set(repo_id),
        number: Set(number),
        title: Set(issue.title.clone()),
        body: Set(issue.body.clone()),
        state: Set(state.to_string()),
        author_id: Set(author_id),
        assignee_id: Set(None),
        milestone_id: Set(milestone_id),
        labels: Set(labels_json),
        created_at: Set(created_at),
        updated_at: Set(parse_datetime_or_now(&issue.updated_at)),
        closed_at: Set(closed_at),
        deleted_at: Set(None),
    };

    let saved = issue_ops::create(db, model).await?;

    // Import comments
    for comment in comments {
        let comment_author = comment
            .user
            .as_ref()
            .and_then(|u| user_map.get(&u.login))
            .copied()
            .unwrap_or(author_id);

        let cm = rg_db::entities::issue_comment::ActiveModel {
            id: sea_orm::NotSet,
            issue_id: Set(saved.id),
            author_id: Set(comment_author),
            body: Set(comment.body.clone().unwrap_or_default()),
            created_at: Set(parse_datetime_or_now(&comment.created_at)),
            updated_at: Set(parse_datetime_or_now(&comment.updated_at)),
        };

        if let Err(e) = issue_comment_ops::create(db, cm).await {
            tracing::warn!("Failed to import comment on issue #{}: {e}", issue.number);
        }
    }

    Ok(())
}

/// Whether the importer mapped a review's author onto an agent account.
/// Failures are logged and answered with `false`: an unreadable user row must
/// not abort an import.
async fn reviewer_is_agent(db: &DatabaseConnection, user_id: i64) -> bool {
    match rg_db::ops::user_ops::find_by_id(db, user_id).await {
        Ok(Some(user)) => crate::agent::is_agent(&user),
        Ok(None) => false,
        Err(e) => {
            tracing::warn!(user_id, "could not read importing reviewer: {e}");
            false
        }
    }
}

async fn import_github_pr(
    db: &DatabaseConnection,
    repo_id: i64,
    pr: &GitHubPR,
    comments: &[GitHubComment],
    reviews: &[GitHubReview],
    user_map: &HashMap<String, i64>,
    milestone_map: &HashMap<String, i64>,
) -> Result<()> {
    let author_id = pr
        .user
        .as_ref()
        .and_then(|u| user_map.get(&u.login))
        .copied()
        .unwrap_or(1);

    let label_names: Vec<String> = pr.labels.iter().map(|l| l.name.clone()).collect();
    let labels_json = if label_names.is_empty() {
        None
    } else {
        Some(serde_json::to_string(&label_names).unwrap_or_else(|_| "[]".into()))
    };

    let milestone_id = pr
        .milestone
        .as_ref()
        .and_then(|m| milestone_map.get(&m.title))
        .copied();

    // Same reasoning as issues: a pull request's number is referenced from
    // commit messages and issue bodies.
    let number = pr.number;
    let state = if pr.merged.unwrap_or(false) {
        "merged"
    } else if pr.state == "closed" {
        "closed"
    } else {
        "open"
    };

    let model = rg_db::entities::pull_request::ActiveModel {
        id: sea_orm::NotSet,
        repo_id: Set(repo_id),
        number: Set(number),
        title: Set(pr.title.clone()),
        body: Set(pr.body.clone()),
        state: Set(state.to_string()),
        is_draft: Set(pr.draft),
        auto_merge_enabled: Set(false),
        auto_merge_strategy: Set(None),
        auto_merge_enabled_by_id: Set(None),
        auto_merge_enabled_at: Set(None),
        author_id: Set(author_id),
        reviewer_id: Set(None),
        head_branch: Set(pr.head.ref_name.clone()),
        base_branch: Set(pr.base.ref_name.clone()),
        head_sha: Set(Some(pr.head.sha.clone())),
        merge_strategy: Set(None),
        merge_commit_sha: Set(None),
        head_repo_id: Set(None),
        milestone_id: Set(milestone_id),
        labels: Set(labels_json),
        created_at: Set(parse_datetime_or_now(&pr.created_at)),
        updated_at: Set(parse_datetime_or_now(&pr.updated_at)),
        closed_at: Set(parse_opt_datetime(&pr.closed_at)),
        merged_at: Set(parse_opt_datetime(&pr.merged_at)),
    };

    let saved = pull_request_ops::create(db, model).await?;

    // Import PR comments (general discussion)
    for comment in comments {
        let comment_author = comment
            .user
            .as_ref()
            .and_then(|u| user_map.get(&u.login))
            .copied()
            .unwrap_or(author_id);

        let cm = rg_db::entities::issue_comment::ActiveModel {
            id: sea_orm::NotSet,
            issue_id: Set(saved.id), // issue_id is PR id in this context
            author_id: Set(comment_author),
            body: Set(comment.body.clone().unwrap_or_default()),
            created_at: Set(parse_datetime_or_now(&comment.created_at)),
            updated_at: Set(parse_datetime_or_now(&comment.updated_at)),
        };

        if let Err(e) = issue_comment_ops::create(db, cm).await {
            tracing::warn!("Failed to import PR comment on #{}: {e}", pr.number);
        }
    }

    // Import reviews
    for review in reviews {
        let reviewer_id = review
            .user
            .as_ref()
            .and_then(|u| user_map.get(&u.login))
            .copied()
            .unwrap_or(author_id);

        let action = match review.state.as_str() {
            "APPROVED" => "approve",
            "CHANGES_REQUESTED" => "request_changes",
            "COMMENTED" => "comment",
            "DISMISSED" => "dismiss",
            _ => "comment",
        };
        if action == "approve" && reviewer_is_agent(db, reviewer_id).await {
            // The approval count only counts people, so this row would sit in
            // the timeline and change nothing. Say so instead of letting it
            // look like it counted.
            tracing::warn!(
                pr = pr.number,
                reviewer_id,
                "imported approval from an agent; it will not count towards the required approvals"
            );
        }

        let rv = rg_db::entities::pr_review::ActiveModel {
            id: sea_orm::NotSet,
            pr_id: Set(saved.id),
            repo_id: Set(repo_id),
            reviewer_id: Set(reviewer_id),
            action: Set(action.to_string()),
            body: Set(review.body.clone()),
            commit_id: Set(None),
            created_at: Set(parse_datetime_or_now(
                review.submitted_at.as_deref().unwrap_or(""),
            )),
        };

        if let Err(e) = pr_review_ops::create(db, rv).await {
            tracing::warn!("Failed to import review on PR #{}: {e}", pr.number);
        }
    }

    Ok(())
}

async fn import_github_releases(
    db: &DatabaseConnection,
    repo_id: i64,
    releases: &[GitHubRelease],
    _user_map: &HashMap<String, i64>,
    repo_root: &Path,
) -> Result<usize> {
    let mut count = 0;

    for release in releases {
        let author_id = 1; // GitHub releases API doesn't expose author in the list endpoint
                           // In a full implementation, we'd fetch release details

        let title = release
            .name
            .clone()
            .unwrap_or_else(|| release.tag_name.clone());
        let is_draft = release.draft;
        let is_prerelease = release.prerelease;

        match crate::release::service::create_release(
            db,
            repo_id,
            author_id,
            &release.tag_name,
            &title,
            release.body.as_deref(),
            &release.tag_name, // target_commitish defaults to tag
            is_draft,
            is_prerelease,
            repo_root,
        )
        .await
        {
            Ok(_) => count += 1,
            Err(e) => {
                tracing::warn!("Failed to import release '{}': {e}", release.tag_name);
            }
        }
    }

    Ok(count)
}

// ═══════════════════════════════════════════════════════════════════════
// CNB import
// ═══════════════════════════════════════════════════════════════════════

/// Import from CNB (cnb.cool).
///
/// CNB's shapes are converted into the GitHub ones and handed to the shared
/// importers, so number preservation, label handling and comment import are the
/// same code that the Gitea and GitHub paths use — there is one place where a
/// migration can get those wrong, not three.
///
/// CNB pulls carry merge state outside `state` (`state: closed` plus
/// `is_merged`), call a draft "wip", and send full refs — all translated in
/// [`cnb_pull_to_github`]. Reviews are not imported: CNB exposes a
/// `review_count` but has no review-list endpoint, so a review would be a
/// guess.
async fn run_cnb_import(
    db: &DatabaseConnection,
    task: &ImportTask,
    repo_root: &Path,
    auth: Option<&SourceAuth>,
    stats: &mut ImportStats,
) -> Result<()> {
    let token = auth.map(|auth| auth.token.as_str()).unwrap_or("");
    if token.is_empty() {
        return Err(CoreError::invalid_input(
            "CNB requires an access token; pass one as auth_token",
        )
        .into());
    }
    let client = CnbClient::new(token.to_string());
    let source = parse_cnb_url(&task.source_url)?;

    let repo_id =
        resolve_or_create_target_repo(db, &task.target_owner, &task.target_name, repo_root).await?;
    let _ = import_task_ops::set_repo_id(db, task.id, repo_id).await;

    // Step 1: clone
    if task.import_repo {
        update_stage(db, task.id, "cloning", 0, "Cloning repository...").await?;
        // CNB serves git from the web host, not the API host.
        let clone_url = format!("https://cnb.cool/{}/{}.git", source.owner, source.name);
        clone_repo(
            &clone_url,
            repo_root,
            &task.target_owner,
            &task.target_name,
            auth,
        )?;
        stats.repo_cloned = true;
        update_stage(db, task.id, "importing", 20, "Repository cloned").await?;
    }

    // Step 2: issues (and the labels they carry — CNB has no repository-level
    // label list endpoint; its `/issues/labels` answers 404)
    if task.import_issues || task.import_labels {
        update_stage(db, task.id, "importing", 30, "Fetching issues...").await?;
        let issues = client.list_issues(&source).await?;

        if task.import_labels {
            let mut names: Vec<String> = issues
                .iter()
                .flat_map(|issue| issue.label_names())
                .collect();
            names.sort();
            names.dedup();
            stats.labels_imported = import_label_names(db, repo_id, &names).await?;
        }

        if task.import_issues {
            let total = issues.len();
            for (index, issue) in issues.iter().enumerate() {
                let percent = 30 + ((index + 1) * 50 / total.max(1)) as i32;
                update_stage(
                    db,
                    task.id,
                    "importing",
                    percent.min(80),
                    &format!("Importing issues ({}/{})", index + 1, total),
                )
                .await?;

                // The list endpoint omits `body`; the text only exists on the
                // single-issue endpoint.
                let detail = match issue.body {
                    Some(_) => issue.clone(),
                    None => match client.get_issue(&source, &issue.number).await {
                        Ok(detail) => detail,
                        Err(error) => {
                            tracing::warn!(number = %issue.number, %error, "CNB: failed to fetch issue body");
                            issue.clone()
                        }
                    },
                };
                let comments = client
                    .list_issue_comments(&source, &issue.number)
                    .await
                    .unwrap_or_default();

                let converted = cnb_issue_to_github(&detail);
                let converted_comments: Vec<GitHubComment> =
                    comments.iter().map(cnb_comment_to_github).collect();
                let empty_user_map = HashMap::new();
                let empty_milestone_map = HashMap::new();
                if let Err(error) = import_github_issue(
                    db,
                    repo_id,
                    &task.target_owner,
                    &task.target_name,
                    &converted,
                    &converted_comments,
                    &empty_user_map,
                    &empty_milestone_map,
                )
                .await
                {
                    tracing::warn!(number = %issue.number, %error, "CNB: failed to import issue");
                } else {
                    stats.issues_imported += 1;
                    stats.issue_comments_imported += converted_comments.len();
                }
            }
        }
    }

    // Step 3: pull requests
    if task.import_pull_requests {
        update_stage(db, task.id, "importing", 82, "Fetching pull requests...").await?;
        let pulls = client.list_pulls(&source).await?;
        let total = pulls.len();
        for (index, pull) in pulls.iter().enumerate() {
            update_stage(
                db,
                task.id,
                "importing",
                (82 + ((index + 1) * 15 / total.max(1)) as i32).min(97),
                &format!("Importing PRs ({}/{})", index + 1, total),
            )
            .await?;

            let comments = client
                .list_pull_comments(&source, &pull.number)
                .await
                .unwrap_or_default();
            let converted_comments: Vec<GitHubComment> =
                comments.iter().map(cnb_comment_to_github).collect();
            let converted = cnb_pull_to_github(pull);
            let empty_user_map = HashMap::new();
            let empty_milestone_map = HashMap::new();
            if let Err(error) = import_github_pr(
                db,
                repo_id,
                &converted,
                &converted_comments,
                &[],
                &empty_user_map,
                &empty_milestone_map,
            )
            .await
            {
                tracing::warn!(number = %pull.number, %error, "CNB: failed to import pull request");
            } else {
                stats.prs_imported += 1;
            }
        }
    }

    Ok(())
}

fn cnb_user(user: Option<&CnbUser>) -> Option<GitHubUser> {
    user.map(|user| GitHubUser {
        id: 0,
        login: user.username.clone(),
        email: None,
        avatar_url: None,
        user_type: None,
    })
}

/// Map a CNB issue onto the shared issue shape.
///
/// CNB has no milestone, so `milestone` stays `None`; labels are synthesised
/// because only their names are available.
fn cnb_issue_to_github(issue: &CnbIssue) -> GitHubIssue {
    GitHubIssue {
        number: issue.number_as_i64(),
        title: issue.title.clone(),
        body: issue.body.clone(),
        state: issue.state.clone(),
        labels: issue
            .label_names()
            .into_iter()
            .map(|name| GitHubLabel {
                id: 0,
                name,
                color: "ededed".to_string(),
                description: None,
            })
            .collect(),
        milestone: None,
        user: cnb_user(issue.author.as_ref()),
        assignees: Vec::new(),
        comments: issue.comment_count,
        created_at: issue.created_at.clone(),
        updated_at: issue.updated_at.clone(),
        // CNB sends an empty string for an open issue's closed_at.
        closed_at: (!issue.closed_at.is_empty()).then(|| issue.closed_at.clone()),
        pull_request: None,
    }
}

/// Map a CNB pull request onto the shared shape.
///
/// CNB keeps merging out of `state` (`state: closed` + `is_merged: true`), calls
/// a draft "wip", and sends full refs. Each of those is translated here so the
/// shared importer sees what it expects.
fn cnb_pull_to_github(pull: &CnbPull) -> GitHubPR {
    GitHubPR {
        number: pull.number_as_i64(),
        title: pull.title.clone(),
        body: pull.body.clone().filter(|body| !body.is_empty()),
        state: pull.state.clone(),
        merged: Some(pull.is_merged),
        merged_at: None,
        draft: pull.is_wip,
        user: cnb_user(pull.author.as_ref()),
        head: cnb_ref(pull.head.as_ref()),
        base: cnb_ref(pull.base.as_ref()),
        labels: pull
            .label_names()
            .into_iter()
            .map(|name| GitHubLabel {
                id: 0,
                name,
                color: "ededed".to_string(),
                description: None,
            })
            .collect(),
        milestone: None,
        created_at: pull.created_at.clone(),
        updated_at: pull.updated_at.clone(),
        closed_at: (!pull.closed_at.is_empty()).then(|| pull.closed_at.clone()),
    }
}

/// CNB sends `refs/heads/main`; the importer stores and displays short names.
fn cnb_ref(reference: Option<&CnbPullRef>) -> GitHubRef {
    let empty = CnbPullRef {
        r#ref: String::new(),
        sha: String::new(),
    };
    let reference = reference.unwrap_or(&empty);
    GitHubRef {
        ref_name: reference
            .r#ref
            .trim_start_matches("refs/heads/")
            .to_string(),
        sha: reference.sha.clone(),
        label: None,
        repo: None,
    }
}

fn cnb_comment_to_github(comment: &CnbComment) -> GitHubComment {
    GitHubComment {
        id: comment
            .id
            .as_deref()
            .and_then(|id| id.parse().ok())
            .unwrap_or(0),
        body: comment.body.clone(),
        user: cnb_user(comment.author.as_ref()),
        created_at: comment.created_at.clone(),
        updated_at: comment.created_at.clone(),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Import helpers — GitLab
// ═══════════════════════════════════════════════════════════════════════

async fn import_gitlab_labels(
    db: &DatabaseConnection,
    repo_id: i64,
    labels: &[GitLabLabel],
) -> Result<usize> {
    let existing = label_ops::list_by_repo(db, repo_id).await?;
    let now = Utc::now();
    let mut count = 0;

    for gl in labels {
        if existing.iter().any(|l| l.name == gl.name) {
            continue;
        }
        // GitLab colors are "#FF0000" with # prefix already
        let color = if gl.color.starts_with('#') {
            gl.color.clone()
        } else {
            format!("#{}", gl.color)
        };

        let model = label::ActiveModel {
            id: sea_orm::NotSet,
            repo_id: Set(repo_id),
            name: Set(gl.name.clone()),
            color: Set(color),
            description: Set(gl.description.clone()),
            created_at: Set(now),
            updated_at: Set(now),
        };

        if let Err(e) = label_ops::create(db, model).await {
            tracing::warn!("Failed to create label '{}': {e}", gl.name);
        } else {
            count += 1;
        }
    }

    Ok(count)
}

async fn import_gitlab_milestones(
    db: &DatabaseConnection,
    repo_id: i64,
    milestones: &[GitLabMilestone],
    milestone_map: &mut HashMap<String, i64>,
) -> Result<usize> {
    let existing = milestone_ops::list_by_repo(db, repo_id, None).await?;
    let now = Utc::now();
    let mut count = 0;

    for gm in milestones {
        if existing.iter().any(|m| m.title == gm.title) {
            continue;
        }

        // GitLab uses "active" instead of "open"
        let state = match gm.state.as_str() {
            "active" => "open",
            "closed" => "closed",
            _ => "open",
        };

        let due_date = parse_opt_datetime(&gm.due_date);

        let model = milestone::ActiveModel {
            id: sea_orm::NotSet,
            repo_id: Set(repo_id),
            title: Set(gm.title.clone()),
            description: Set(gm.description.clone()),
            state: Set(state.to_string()),
            due_date: Set(due_date),
            created_at: Set(now),
            updated_at: Set(now),
        };

        match milestone_ops::create(db, model).await {
            Ok(ms) => {
                milestone_map.insert(gm.title.clone(), ms.id);
                count += 1;
            }
            Err(e) => {
                tracing::warn!("Failed to create milestone '{}': {e}", gm.title);
            }
        }
    }

    Ok(count)
}

#[allow(clippy::too_many_arguments)]
async fn import_gitlab_issue(
    db: &DatabaseConnection,
    repo_id: i64,
    _target_owner: &str,
    _target_name: &str,
    issue: &GitLabIssue,
    notes: &[GitLabNote],
    user_map: &HashMap<String, i64>,
    milestone_map: &HashMap<String, i64>,
) -> Result<()> {
    let author_id = issue
        .author
        .as_ref()
        .and_then(|a| user_map.get(&a.username))
        .copied()
        .unwrap_or(1);

    // GitLab labels are plain strings
    let labels_json = if issue.labels.is_empty() {
        None
    } else {
        Some(serde_json::to_string(&issue.labels).unwrap_or_else(|_| "[]".into()))
    };

    let milestone_id = issue
        .milestone
        .as_ref()
        .and_then(|m| milestone_map.get(&m.title))
        .copied();

    // GitLab's `iid` is the per-project number a body refers to
    // as `#12`; `id` is global and means nothing here.
    let number = issue.iid;
    let state = if issue.state == "closed" {
        "closed"
    } else {
        "open"
    };

    let model = rg_db::entities::issue::ActiveModel {
        id: sea_orm::NotSet,
        repo_id: Set(repo_id),
        number: Set(number),
        title: Set(issue.title.clone()),
        body: Set(issue.description.clone()),
        state: Set(state.to_string()),
        author_id: Set(author_id),
        assignee_id: Set(None),
        milestone_id: Set(milestone_id),
        labels: Set(labels_json),
        created_at: Set(parse_datetime_or_now(&issue.created_at)),
        updated_at: Set(parse_datetime_or_now(&issue.updated_at)),
        closed_at: Set(parse_opt_datetime(&issue.closed_at)),
        deleted_at: Set(None),
    };

    let saved = issue_ops::create(db, model).await?;

    // Import notes (skip system notes)
    for note in notes {
        if note.system {
            continue;
        }
        let note_author = note
            .author
            .as_ref()
            .and_then(|a| user_map.get(&a.username))
            .copied()
            .unwrap_or(author_id);

        let cm = rg_db::entities::issue_comment::ActiveModel {
            id: sea_orm::NotSet,
            issue_id: Set(saved.id),
            author_id: Set(note_author),
            body: Set(note.body.clone().unwrap_or_default()),
            created_at: Set(parse_datetime_or_now(&note.created_at)),
            updated_at: Set(parse_datetime_or_now(&note.updated_at)),
        };

        if let Err(e) = issue_comment_ops::create(db, cm).await {
            tracing::warn!("Failed to import note on issue !{}: {e}", issue.iid);
        }
    }

    Ok(())
}

async fn import_gitlab_mr(
    db: &DatabaseConnection,
    repo_id: i64,
    mr: &GitLabMR,
    notes: &[GitLabNote],
    user_map: &HashMap<String, i64>,
    milestone_map: &HashMap<String, i64>,
) -> Result<()> {
    let author_id = mr
        .author
        .as_ref()
        .and_then(|a| user_map.get(&a.username))
        .copied()
        .unwrap_or(1);

    let labels_json = if mr.labels.is_empty() {
        None
    } else {
        Some(serde_json::to_string(&mr.labels).unwrap_or_else(|_| "[]".into()))
    };

    let milestone_id = mr
        .milestone
        .as_ref()
        .and_then(|m| milestone_map.get(&m.title))
        .copied();

    // Same reasoning as issues: `iid` is what references point at.
    let number = mr.iid;
    let state = if mr.merged_at.is_some() {
        "merged"
    } else if mr.state == "closed" {
        "closed"
    } else {
        "open"
    };

    let model = rg_db::entities::pull_request::ActiveModel {
        id: sea_orm::NotSet,
        repo_id: Set(repo_id),
        number: Set(number),
        title: Set(mr.title.clone()),
        body: Set(mr.description.clone()),
        state: Set(state.to_string()),
        is_draft: Set(mr.draft),
        auto_merge_enabled: Set(false),
        auto_merge_strategy: Set(None),
        auto_merge_enabled_by_id: Set(None),
        auto_merge_enabled_at: Set(None),
        author_id: Set(author_id),
        reviewer_id: Set(None),
        head_branch: Set(mr.source_branch.clone()),
        base_branch: Set(mr.target_branch.clone()),
        head_sha: Set(None),
        merge_strategy: Set(None),
        merge_commit_sha: Set(None),
        head_repo_id: Set(mr.source_project_id),
        milestone_id: Set(milestone_id),
        labels: Set(labels_json),
        created_at: Set(parse_datetime_or_now(&mr.created_at)),
        updated_at: Set(parse_datetime_or_now(&mr.updated_at)),
        closed_at: Set(parse_opt_datetime(&mr.closed_at)),
        merged_at: Set(parse_opt_datetime(&mr.merged_at)),
    };

    let saved = pull_request_ops::create(db, model).await?;

    // Import MR notes (skip system notes)
    for note in notes {
        if note.system {
            continue;
        }
        let note_author = note
            .author
            .as_ref()
            .and_then(|a| user_map.get(&a.username))
            .copied()
            .unwrap_or(author_id);

        // A note on a merge request belongs to the merge request. Writing it
        // into `issue_comments` with the pull-request id as `issue_id` produced
        // rows that no query could reach — nothing in `issues` has that id — so
        // the import counted them and the review showed none. Found by reading
        // the migrated merge request back and finding its note missing.
        let cm = rg_db::entities::pr_review::ActiveModel {
            id: sea_orm::NotSet,
            pr_id: Set(saved.id),
            repo_id: Set(repo_id),
            reviewer_id: Set(note_author),
            action: Set("comment".to_string()),
            body: Set(note.body.clone()),
            commit_id: Set(None),
            created_at: Set(parse_datetime_or_now(&note.created_at)),
        };

        if let Err(e) = rg_db::ops::pr_review_ops::create(db, cm).await {
            tracing::warn!("Failed to import MR note on !{}: {e}", mr.iid);
        }
    }

    Ok(())
}

async fn import_gitlab_releases(
    db: &DatabaseConnection,
    repo_id: i64,
    releases: &[GitLabRelease],
    _user_map: &HashMap<String, i64>,
    repo_root: &Path,
) -> Result<usize> {
    let mut count = 0;

    for release in releases {
        let title = release
            .name
            .clone()
            .unwrap_or_else(|| release.tag_name.clone());

        match crate::release::service::create_release(
            db,
            repo_id,
            1, // GitLab releases don't expose author in list; default to admin
            &release.tag_name,
            &title,
            release.description.as_deref(),
            &release.tag_name,
            false, // is_draft — GitLab doesn't have drafts
            false, // is_prerelease — could parse from tag name, simplified
            repo_root,
        )
        .await
        {
            Ok(_) => count += 1,
            Err(e) => {
                tracing::warn!("Failed to import release '{}': {e}", release.tag_name);
            }
        }
    }

    Ok(count)
}

// ═══════════════════════════════════════════════════════════════════════
// Progress helpers
// ═══════════════════════════════════════════════════════════════════════

async fn update_stage(
    db: &DatabaseConnection,
    task_id: i64,
    status: &str,
    progress: i32,
    stage: &str,
) -> Result<()> {
    import_task_ops::update_progress(db, task_id, status, progress, Some(stage)).await?;
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════
// Public API: start import task
// ═══════════════════════════════════════════════════════════════════════

/// Create a new import task and start the background import process.
#[allow(clippy::too_many_arguments)]
pub async fn start_import(
    db: &DatabaseConnection,
    user_id: i64,
    platform: String,
    source_url: String,
    target_owner: String,
    target_name: String,
    auth_token: Option<String>,
    import_repo: bool,
    import_issues: bool,
    import_pull_requests: bool,
    import_wiki: bool,
    import_releases: bool,
    import_labels: bool,
    import_milestones: bool,
    repo_root: &Path,
    enc_key: &[u8; 32],
) -> CoreResult<ImportTask> {
    let now = Utc::now();
    // Gitea goes through the GitHub importer, so it supports metadata too.
    // CNB has issues and labels but no milestone concept, and its pull requests
    // have not been verified against a live instance — so it is listed for the
    // metadata it can actually deliver today.
    let supports_metadata = matches!(platform.as_str(), "github" | "gitlab" | "gitea" | "cnb");
    // The column has always been called `auth_token_encrypted`; until now the
    // caller's string went straight in.
    let auth_token = match auth_token {
        Some(token) if !token.trim().is_empty() => {
            Some(crate::auth::encryption::encrypt(token.trim(), enc_key)?)
        }
        _ => None,
    };

    let model = import_task::ActiveModel {
        user_id: Set(user_id),
        repo_id: Set(None),
        platform: Set(platform),
        source_url: Set(source_url),
        target_owner: Set(target_owner),
        target_name: Set(target_name),
        auth_token_encrypted: Set(auth_token),
        status: Set("pending".to_string()),
        progress: Set(0),
        stage: Set(None),
        error: Set(None),
        user_mapping: Set(None),
        import_repo: Set(import_repo),
        import_issues: Set(supports_metadata && import_issues),
        import_pull_requests: Set(supports_metadata && import_pull_requests),
        import_wiki: Set(supports_metadata && import_wiki),
        import_releases: Set(supports_metadata && import_releases),
        import_labels: Set(supports_metadata && import_labels),
        import_milestones: Set(supports_metadata && import_milestones),
        stats: Set(None),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    };

    let task = import_task_ops::create(db, model).await?;
    let task_clone = task.clone();

    // Spawn background task
    let db_clone = db.clone();
    let repo_root_clone = repo_root.to_path_buf();
    let key = *enc_key;
    tokio::spawn(async move {
        match run_import(&db_clone, &task_clone, &repo_root_clone, &key).await {
            Ok(stats) => {
                let stats_json = serde_json::to_string(&stats).unwrap_or_default();
                let _ =
                    import_task_ops::mark_completed(&db_clone, task_clone.id, &stats_json).await;
            }
            Err(e) => {
                let message = user_visible_failure(&e);
                let _ = import_task_ops::mark_failed(&db_clone, task_clone.id, &message).await;
            }
        }
    });

    // Re-fetch to get the persisted record
    import_task_ops::find_by_id(db, task.id)
        .await?
        .ok_or_else(|| CoreError::internal("import task not found after creation"))
}

/// Text stored in the import task's `error` field, which the owner can read
/// back over the API. Failures the user can act on (bad URL, unknown target
/// owner, rejected token) keep their message; everything else is logged for
/// operators and replaced by a generic line so paths, SQL text and tokens
/// embedded in lower-level errors never reach the client.
fn user_visible_failure(error: &anyhow::Error) -> String {
    match error.downcast_ref::<CoreError>() {
        Some(CoreError::NotFound(msg))
        | Some(CoreError::Forbidden(msg))
        | Some(CoreError::Conflict(msg))
        | Some(CoreError::InvalidInput(msg)) => msg.clone(),
        _ => {
            tracing::error!(error = %format!("{error:#}"), "import task failed");
            "import failed due to an internal error; see the server logs".to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Self-hosted GitLab is the common case; a client pointed at gitlab.com
    /// reports "project not found" for a project that exists.
    #[test]
    fn gitlab_api_base_is_derived_from_the_repository_url() {
        assert_eq!(
            gitlab_api_base("https://gitlab.example.com/group/project").unwrap(),
            "https://gitlab.example.com/api/v4"
        );
        assert_eq!(
            gitlab_api_base("http://localhost:8929/group/sub/project.git").unwrap(),
            "http://localhost:8929/api/v4"
        );
        assert!(gitlab_api_base("no-scheme/group/project").is_err());
    }

    /// The API base comes from the repository URL the user pasted. Ports and
    /// nested paths must survive: self-hosted Gitea is often on one or both.
    #[test]
    fn gitea_api_base_is_derived_from_the_repository_url() {
        assert_eq!(
            gitea_api_base("https://gitea.example.com/alice/project").unwrap(),
            "https://gitea.example.com/api/v1"
        );
        assert_eq!(
            gitea_api_base("https://git.example.com:3000/alice/project/").unwrap(),
            "https://git.example.com:3000/api/v1"
        );
        assert_eq!(
            gitea_api_base("http://localhost:8080/alice/project.git").unwrap(),
            "http://localhost:8080/api/v1"
        );
        assert!(gitea_api_base("not-a-url").is_err());
    }

    /// Gitea omits fields GitHub requires on two of the four payloads. Both
    /// took a real migration down (milestone at 25 %, release would have been
    /// next), so both shapes are pinned against what Gitea 1.22 actually sent.
    #[test]
    fn gitea_milestone_and_release_payloads_parse() {
        // Copied from `GET /api/v1/repos/{owner}/{repo}/milestones` on Gitea
        // 1.22.6 — note the absence of `number`.
        let milestone = r#"{
            "id": 1,
            "title": "v1.0",
            "description": "first release",
            "state": "open",
            "open_issues": 2,
            "closed_issues": 0,
            "created_at": "2026-11-24T15:15:46Z",
            "updated_at": "2026-11-24T15:15:46Z",
            "closed_at": null,
            "due_on": null
        }"#;
        let parsed: crate::import::github_client::GitHubMilestone =
            serde_json::from_str(milestone).expect("Gitea milestone must parse");
        assert_eq!(parsed.title, "v1.0");
        assert_eq!(parsed.number, 0, "absent number falls back to 0");

        // `GET /repos/{owner}/{repo}/releases` — note the absence of
        // `created_at`, which GitHub always sends.
        let release = r#"{
            "id": 1,
            "tag_name": "v1.0.0",
            "target_commitish": "main",
            "name": "First",
            "body": "release notes",
            "url": "http://localhost:3300/api/v1/repos/alice/gitea-demo/releases/1",
            "html_url": "http://localhost:3300/alice/gitea-demo/releases/tag/v1.0.0",
            "tarball_url": "",
            "zipball_url": "",
            "draft": false,
            "prerelease": false,
            "created_at": null,
            "published_at": "2026-11-24T15:15:46Z",
            "author": {"id": 1, "login": "alice"},
            "assets": []
        }"#;
        let parsed: crate::import::github_client::GitHubRelease =
            serde_json::from_str(release).expect("Gitea release must parse");
        assert_eq!(parsed.tag_name, "v1.0.0");
        assert_eq!(parsed.name.as_deref(), Some("First"));
    }

    /// Gitea answers with the GitHub-shaped payload. If this stops parsing, the
    /// import silently loses issues — so the shape is pinned here rather than
    /// discovered per migration.
    #[test]
    fn a_gitea_issue_payload_parses_with_the_github_client_types() {
        let payload = r#"{
            "id": 12,
            "number": 3,
            "title": "Something broke",
            "body": "steps to reproduce",
            "state": "closed",
            "labels": [{"id": 1, "name": "bug", "color": "e11d21", "description": null}],
            "milestone": null,
            "user": {"id": 7, "login": "alice", "avatar_url": "https://example/avatar.png"},
            "assignees": null,
            "comments": 4,
            "created_at": "2026-01-02T03:04:05Z",
            "updated_at": "2026-01-03T03:04:05Z",
            "closed_at": "2026-01-03T03:04:05Z"
        }"#;
        let issue: crate::import::github_client::GitHubIssue =
            serde_json::from_str(payload).expect("Gitea payload must parse");
        assert_eq!(issue.number, 3);
        assert_eq!(issue.state, "closed");
        assert_eq!(issue.labels.len(), 1);
        assert_eq!(issue.labels[0].name, "bug");
        assert_eq!(issue.user.as_ref().map(|u| u.login.as_str()), Some("alice"));
        assert!(issue.pull_request.is_none());
    }

    /// The exact header gix sends. These platforms compare
    /// `base64(user:token)` literally, so the bytes matter.
    #[test]
    fn the_basic_auth_header_is_the_standard_form() {
        assert_eq!(
            http_auth_override("x-access-token", "ghp_example"),
            "http.extraHeader=Authorization: Basic eC1hY2Nlc3MtdG9rZW46Z2hwX2V4YW1wbGU="
        );
        // GitLab's convention: any user works, `oauth2` is the documented one.
        assert_eq!(
            http_auth_override("oauth2", "glpat-example"),
            "http.extraHeader=Authorization: Basic b2F1dGgyOmdscGF0LWV4YW1wbGU="
        );
    }

    /// A token must never be spliced into the URL: gix writes the clone URL into
    /// the new repository's config as `remote.origin.url`.
    #[test]
    fn a_token_never_appears_in_the_clone_url() {
        let auth = SourceAuth {
            username: "x-access-token".to_string(),
            token: "super-secret".to_string(),
        };
        let override_value = http_auth_override(&auth.username, &auth.token);
        assert!(!override_value.contains("super-secret"));
        // ...but it does decode back to user:token, so it is the right credential.
        use base64::Engine as _;
        let encoded = override_value
            .strip_prefix("http.extraHeader=Authorization: Basic ")
            .expect("basic scheme");
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .expect("base64");
        assert_eq!(
            String::from_utf8(decoded).unwrap(),
            "x-access-token:super-secret"
        );
        assert!(!is_http_url("git://example.com/a.git"));
        assert!(is_http_url("https://example.com/a.git"));
    }

    /// The column has always been named `auth_token_encrypted`, and rows written
    /// before this change hold plaintext in it. Both have to read back.
    #[test]
    fn stored_tokens_are_read_as_ciphertext_or_legacy_plaintext() {
        let key = crate::auth::encryption::derive_key("import-test-secret");
        let encrypted = crate::auth::encryption::encrypt("ghp_live_token", &key).unwrap();
        assert_eq!(
            decode_task_token(&encrypted, &key).unwrap(),
            "ghp_live_token"
        );
        // Legacy row: plaintext that is not shaped like ciphertext.
        assert_eq!(decode_task_token("ghp_legacy", &key).unwrap(), "ghp_legacy");
        // Ciphertext we cannot open is an error, not a plaintext guess.
        let other = crate::auth::encryption::derive_key("different-secret");
        let error = decode_task_token(&encrypted, &other).unwrap_err();
        assert!(
            format!("{error}").contains("cannot be decrypted"),
            "unexpected error: {error}"
        );
    }

    /// `clone_repo` replaced `git clone --bare`, so the result still has to be
    /// The target directory is created before anything is fetched, so a bare
    /// repository that exists proves nothing. Asking about refs is what stops a
    /// failed clone from being reported as a successful, empty migration.
    #[test]
    fn a_bare_repository_without_refs_is_not_a_finished_clone() {
        let temp = tempfile::tempdir().unwrap();
        let missing = temp.path().join("never-created");
        assert!(!repository_has_refs(&missing));

        let empty = temp.path().join("empty.git");
        gix::init_bare(&empty).unwrap();
        assert!(!repository_has_refs(&empty), "HEAD alone is not a clone");
    }

    /// a bare repository that Git itself accepts, with the source branches and
    /// tags intact.
    #[test]
    fn clone_repo_builds_a_bare_repository_git_can_read() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        let git = rg_git::cli_gateway::global_gateway().as_ref().unwrap();
        let run = |args: &[&str], cwd: Option<&Path>| {
            assert!(
                git.run(args, cwd).unwrap().success(),
                "git {args:?} failed while setting up the fixture"
            );
        };
        run(&["init", "-b", "main", &source.to_string_lossy()], None);
        run(
            &["config", "user.email", "import@example.com"],
            Some(&source),
        );
        run(&["config", "user.name", "Import"], Some(&source));
        std::fs::write(source.join("README.md"), "imported\n").unwrap();
        run(&["add", "-A"], Some(&source));
        run(&["commit", "-m", "initial"], Some(&source));
        run(&["tag", "v1"], Some(&source));
        let head = git
            .run(&["rev-parse", "HEAD"], Some(&source))
            .unwrap()
            .stdout_str()
            .trim()
            .to_owned();

        clone_repo(
            &source.to_string_lossy(),
            temp.path(),
            "owner",
            "name",
            None,
        )
        .unwrap();

        let target = temp.path().join("owner/name.git");
        let cloned = gix::open(&target).expect("cloned repository must open");
        assert!(cloned.is_bare(), "the clone must be a bare repository");
        let cloned_head = cloned.head_id().expect("HEAD must be resolvable");
        assert_eq!(cloned_head.detach().to_string(), head);

        // Git is the reference implementation: it must accept the repository
        // and resolve the same branch and tag.
        let branch = git.run(&["rev-parse", "main"], Some(&target)).unwrap();
        assert!(branch.success(), "git must resolve refs/heads/main");
        assert_eq!(branch.stdout_str().trim(), head);
        let tag = git.run(&["rev-parse", "v1"], Some(&target)).unwrap();
        assert!(tag.success(), "tags must be cloned as well");
        assert_eq!(tag.stdout_str().trim(), head);

        // Re-running is a no-op instead of a destructive re-clone.
        clone_repo(
            &source.to_string_lossy(),
            temp.path(),
            "owner",
            "name",
            None,
        )
        .unwrap();
        assert_eq!(cloned.head_id().unwrap().detach().to_string(), head);
    }
}
