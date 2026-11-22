//! Push mirror service — forward this repository's refs to an external host.
//!
//! The mirror image of [`super::service`]: that one pulls a remote repository
//! in, this one pushes this repository out. Both share the same outbound URL
//! guard, the same transport lockdown, and the same credential dictionary —
//! a token configured once for `github.com` authenticates the pull *and* the
//! push.
//!
//! # What is pushed
//!
//! Branches, plus tags unless the owner turned them off. Nothing else: this
//! instance's `refs/pull/*` belongs to this instance and would collide by
//! number with the target's own pull requests.
//!
//! Each refspec carries a leading `+`, so a non-fast-forward update still lands.
//! That is the point of a mirror — the target is supposed to look like this
//! repository, including after a history rewrite. It is also the reason the
//! target should be a repository nobody else pushes to.
//!
//! Refs that exist on the target but not here are **left alone** (no `--prune`).
//! Deleting somebody else's branch requires an explicit decision, and there is
//! no "did you mean it" prompt in a background task.
//!
//! # Why there is no interval
//!
//! A push mirror fires after the push that changed something. On a timer it
//! would publish whatever half-finished series of commits happens to be in the
//! repository at that moment, and do the same work again a minute later.

use crate::error::{CoreError, CoreResult};
use crate::net_guard;
use crate::remote_credential::service as credential_service;
use anyhow::{Context, Result};
use chrono::Utc;
use rg_db::entities::push_mirror::{
    ActiveModel, Model as PushMirror, STATUS_ACTIVE, STATUS_ERROR, STATUS_PAUSED, TRIGGER_MANUAL,
    TRIGGER_ON_PUSH,
};
use rg_db::entities::repository;
use rg_git::cli_gateway::global_gateway;
use sea_orm::ActiveValue::Set;
use sea_orm::{DatabaseConnection, EntityTrait};
use std::collections::HashSet;
use std::path::Path;
use std::sync::{Mutex, OnceLock};

use super::service::GIT_TRANSPORT_LOCKDOWN;

/// Branches are always pushed; the `+` makes the update forced.
const REFSPEC_HEADS: &str = "+refs/heads/*:refs/heads/*";
/// Tags follow the same rule when enabled.
const REFSPEC_TAGS: &str = "+refs/tags/*:refs/tags/*";

/// Fields accepted when configuring (or reconfiguring) the push target.
#[derive(Debug, Default)]
pub struct PushMirrorInput {
    /// Where the refs go. Required on create; `None` leaves it unchanged.
    pub url: Option<String>,
    /// Push tags too. `None` leaves the current setting.
    pub push_tags: Option<bool>,
    /// `on_push` or `manual`. `None` leaves the current setting.
    pub trigger_mode: Option<String>,
}

/// Configure the push target for a repository, replacing an existing one.
///
/// Upsert rather than create-or-conflict: a repository has at most one push
/// target, so "configure it again" is how an owner fixes a wrong URL. The row
/// is also re-activated and its error cleared — reconfiguring is an explicit
/// "try again", and leaving it in `error` would mean the fix silently never
/// runs.
pub async fn upsert_push_mirror(
    db: &DatabaseConnection,
    repo_id: i64,
    input: PushMirrorInput,
) -> CoreResult<PushMirror> {
    let repo = repository::Entity::find_by_id(repo_id).one(db).await?;
    if repo.is_none() {
        return Err(CoreError::not_found("repository not found"));
    }

    let existing = rg_db::ops::push_mirror_ops::find_by_repo_id(db, repo_id).await?;

    let url = match (&input.url, &existing) {
        (Some(url), _) => {
            net_guard::validate_mirror_url(url)
                .await
                .map_err(|e| CoreError::invalid_input(format!("invalid push URL: {e}")))?;
            Some(url.clone())
        }
        (None, Some(_)) => None,
        (None, None) => {
            return Err(CoreError::invalid_input(
                "push URL is required when configuring a push mirror",
            ))
        }
    };

    if let Some(mode) = input.trigger_mode.as_deref() {
        if mode != TRIGGER_ON_PUSH && mode != TRIGGER_MANUAL {
            return Err(CoreError::invalid_input(format!(
                "unknown trigger mode {mode:?}: expected '{TRIGGER_ON_PUSH}' or '{TRIGGER_MANUAL}'"
            )));
        }
    }

    let now = Utc::now();
    if let Some(existing) = existing {
        let mut model: ActiveModel = existing.into();
        if let Some(url) = url {
            model.url = Set(url);
        }
        if let Some(push_tags) = input.push_tags {
            model.push_tags = Set(push_tags);
        }
        if let Some(mode) = input.trigger_mode {
            model.trigger_mode = Set(mode);
        }
        model.status = Set(STATUS_ACTIVE.to_string());
        model.last_push_error = Set(None);
        model.updated_at = Set(now);
        return Ok(rg_db::ops::push_mirror_ops::update(db, model).await?);
    }

    let model = ActiveModel {
        repo_id: Set(repo_id),
        url: Set(url.expect("checked above: a create always has a URL")),
        push_tags: Set(input.push_tags.unwrap_or(true)),
        trigger_mode: Set(input
            .trigger_mode
            .unwrap_or_else(|| TRIGGER_ON_PUSH.to_string())),
        status: Set(STATUS_ACTIVE.to_string()),
        last_push_at: Set(None),
        last_push_error: Set(None),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    };
    match rg_db::ops::push_mirror_ops::create(db, model).await {
        Ok(model) => Ok(model),
        Err(error) if crate::user::service::is_unique_violation(&error) => Err(
            CoreError::conflict("a push mirror already exists for this repository"),
        ),
        Err(error) => Err(error.into()),
    }
}

/// The push target configured for a repository, if any.
pub async fn get_push_mirror(
    db: &DatabaseConnection,
    repo_id: i64,
) -> CoreResult<Option<PushMirror>> {
    Ok(rg_db::ops::push_mirror_ops::find_by_repo_id(db, repo_id).await?)
}

/// Pause or resume forwarding without losing the configuration.
pub async fn set_status(
    db: &DatabaseConnection,
    repo_id: i64,
    status: &str,
) -> CoreResult<PushMirror> {
    if status != STATUS_ACTIVE && status != STATUS_PAUSED {
        return Err(CoreError::invalid_input(format!(
            "unknown status {status:?}: expected '{STATUS_ACTIVE}' or '{STATUS_PAUSED}'"
        )));
    }
    let existing = rg_db::ops::push_mirror_ops::find_by_repo_id(db, repo_id)
        .await?
        .ok_or_else(|| CoreError::not_found("no push mirror configured for this repository"))?;
    let mut model: ActiveModel = existing.into();
    model.status = Set(status.to_string());
    if status == STATUS_ACTIVE {
        model.last_push_error = Set(None);
    }
    model.updated_at = Set(Utc::now());
    Ok(rg_db::ops::push_mirror_ops::update(db, model).await?)
}

/// Remove the push target.
pub async fn delete_push_mirror(db: &DatabaseConnection, repo_id: i64) -> CoreResult<()> {
    let existing = rg_db::ops::push_mirror_ops::find_by_repo_id(db, repo_id)
        .await?
        .ok_or_else(|| CoreError::not_found("no push mirror configured for this repository"))?;
    Ok(rg_db::ops::push_mirror_ops::delete_by_id(db, existing.id).await?)
}

/// Run one push, recording the outcome on the row.
///
/// Returns `Ok(true)` when the push succeeded, `Ok(false)` when it ran and
/// failed (the error is stored), and an `Err` only for the cases where nothing
/// could be attempted (repository or configuration missing).
pub async fn run_push(
    db: &DatabaseConnection,
    push_mirror: &PushMirror,
    repo: &repository::Model,
    repo_path: &Path,
    enc_key: &[u8; 32],
) -> CoreResult<bool> {
    // Re-validate every time: the row may predate the guard, and the URL is the
    // one user-controlled string that decides where a token gets sent.
    if let Err(e) = net_guard::validate_mirror_url(&push_mirror.url).await {
        record_failure(
            db,
            push_mirror,
            format!("push URL rejected by outbound guard: {e}"),
        )
        .await?;
        return Ok(false);
    }

    // Credentials come from the host dictionary, resolved in the repository's
    // scope (org repositories use the org's token). A missing row is not an
    // error: the target may be public.
    let scope = credential_service::Scope::for_repository(repo);
    let mut env: Vec<(String, String)> =
        match credential_service::resolve(db, scope, &push_mirror.url, enc_key).await {
            Ok(Some(cred)) => credential_service::git_auth_env(&push_mirror.url, &cred)?,
            Ok(None) => Vec::new(),
            Err(e) => {
                record_failure(db, push_mirror, format!("credentials unusable: {e}")).await?;
                return Ok(false);
            }
        };
    // A background push must never block on a username prompt.
    env.push(("GIT_TERMINAL_PROMPT".to_string(), "0".to_string()));
    let env_refs: Vec<(&str, &str)> = env
        .iter()
        .map(|(key, value)| (key.as_str(), value.as_str()))
        .collect();

    let mut specs: Vec<&str> = vec![REFSPEC_HEADS];
    if push_mirror.push_tags {
        specs.push(REFSPEC_TAGS);
    }

    let outcome = run_git_push(
        &push_mirror.url,
        &specs,
        repo_path,
        &env_refs,
        &GIT_TRANSPORT_LOCKDOWN,
    );

    match outcome {
        Ok(output) if output.success() => {
            let mut model: ActiveModel = push_mirror.clone().into();
            model.last_push_at = Set(Some(Utc::now()));
            model.last_push_error = Set(None);
            model.status = Set(STATUS_ACTIVE.to_string());
            model.updated_at = Set(Utc::now());
            rg_db::ops::push_mirror_ops::update(db, model).await?;
            Ok(true)
        }
        Ok(output) => {
            // stderr is the only place git explains a rejected ref; the URL is
            // credential-free, so this text is safe to show the owner.
            let detail = output.stderr_str().trim().to_string();
            record_failure(
                db,
                push_mirror,
                if detail.is_empty() {
                    "git push failed".to_string()
                } else {
                    detail
                },
            )
            .await?;
            Ok(false)
        }
        Err(e) => {
            record_failure(db, push_mirror, format!("{e:#}")).await?;
            Ok(false)
        }
    }
}

/// `git push --porcelain <url> <refspecs...>` from `repo_path`.
///
/// The lockdown is a parameter so tests can drive the same code against a local
/// target (production always passes [`GIT_TRANSPORT_LOCKDOWN`]).
fn run_git_push(
    url: &str,
    specs: &[&str],
    repo_path: &Path,
    env: &[(&str, &str)],
    lockdown: &[&str],
) -> Result<rg_git::cli_gateway::GitOutput> {
    let git = global_gateway()
        .as_ref()
        .map_err(|e| anyhow::anyhow!("git gateway unavailable: {e}"))?;
    let mut args: Vec<&str> = lockdown.to_vec();
    args.extend(["push", "--porcelain", url]);
    args.extend(specs);
    git.run_with_env(&args, Some(repo_path), env)
        .context("git push (mirror)")
}

async fn record_failure(
    db: &DatabaseConnection,
    push_mirror: &PushMirror,
    message: String,
) -> CoreResult<()> {
    tracing::warn!(
        repo_id = push_mirror.repo_id,
        url = %push_mirror.url,
        error = %message,
        "push mirror failed"
    );
    let mut model: ActiveModel = push_mirror.clone().into();
    model.status = Set(STATUS_ERROR.to_string());
    model.last_push_error = Set(Some(message));
    model.updated_at = Set(Utc::now());
    rg_db::ops::push_mirror_ops::update(db, model).await?;
    Ok(())
}

/// Everything the post-push hook needs, resolved once.
pub fn repo_path_for(repo_root: &Path, owner: &str, repo_name: &str) -> std::path::PathBuf {
    repo_root.join(format!("{owner}/{repo_name}.git"))
}

// ── In-flight de-duplication ────────────────────────────────────────────

/// Repositories with a push currently running.
///
/// A burst of pushes (a force-push of ten branches, a scripted `git push
/// --all`) would otherwise start one external push per receive-pack call, all
/// racing to update the same refs on the target. Only the newest one matters —
/// the refspec reads whatever is in the repository when git starts — so the
/// later triggers are skipped rather than queued.
fn inflight() -> &'static Mutex<HashSet<i64>> {
    static INFLIGHT: OnceLock<Mutex<HashSet<i64>>> = OnceLock::new();
    INFLIGHT.get_or_init(|| Mutex::new(HashSet::new()))
}

/// Marks one repository as having a push in flight; dropping it clears the mark.
struct PushInFlight {
    repo_id: i64,
}

impl Drop for PushInFlight {
    fn drop(&mut self) {
        let mut set = inflight()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        set.remove(&self.repo_id);
    }
}

/// Claim the right to push this repository, or `None` if one is already running.
fn try_begin_push(repo_id: i64) -> Option<PushInFlight> {
    let mut set = inflight()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if !set.insert(repo_id) {
        return None;
    }
    Some(PushInFlight { repo_id })
}

/// Event-driven entry point, called from the post-push hook.
///
/// Best-effort by contract: a push mirror that fails must never fail the push
/// that triggered it. Returns `Ok(true)` when a push ran and succeeded,
/// `Ok(false)` when nothing was attempted or the push failed (the row carries
/// the reason).
pub async fn push_after_receive(
    db: &DatabaseConnection,
    repo: &repository::Model,
    repo_path: &Path,
    enc_key: &[u8; 32],
) -> CoreResult<bool> {
    let Some(push_mirror) = rg_db::ops::push_mirror_ops::find_by_repo_id(db, repo.id).await? else {
        return Ok(false);
    };
    if push_mirror.status != STATUS_ACTIVE || push_mirror.trigger_mode != TRIGGER_ON_PUSH {
        return Ok(false);
    }
    let Some(_guard) = try_begin_push(repo.id) else {
        tracing::debug!(
            repo_id = repo.id,
            "push mirror already in flight; skipping this trigger"
        );
        return Ok(false);
    };
    run_push(db, &push_mirror, repo, repo_path, enc_key).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use rg_db::entities::push_mirror::Model as PushMirrorModel;

    fn model(url: &str, push_tags: bool) -> PushMirrorModel {
        let now = Utc::now();
        PushMirrorModel {
            id: 1,
            repo_id: 1,
            url: url.to_string(),
            push_tags,
            trigger_mode: TRIGGER_ON_PUSH.to_string(),
            status: STATUS_ACTIVE.to_string(),
            last_push_at: None,
            last_push_error: None,
            created_at: now,
            updated_at: now,
        }
    }

    /// The refspecs are the whole behaviour: branches forced, tags optional,
    /// and no prune, so a branch that only exists on the target survives.
    fn refspecs(push_tags: bool) -> Vec<&'static str> {
        let mut specs = vec![REFSPEC_HEADS];
        if push_tags {
            specs.push(REFSPEC_TAGS);
        }
        specs
    }

    #[test]
    fn branches_are_forced_and_tags_are_optional() {
        assert_eq!(refspecs(false), vec!["+refs/heads/*:refs/heads/*"]);
        assert_eq!(
            refspecs(true),
            vec!["+refs/heads/*:refs/heads/*", "+refs/tags/*:refs/tags/*"]
        );
        // A leading `+` is what makes a rewritten history land instead of being
        // rejected as non-fast-forward.
        for spec in refspecs(true) {
            assert!(spec.starts_with('+'), "unforced refspec: {spec}");
        }
    }

    #[test]
    fn pull_refs_are_never_pushed() {
        // `refs/pull/*` belongs to this instance; forwarding it would collide
        // with the target's own pull requests by number.
        for spec in refspecs(true) {
            assert!(!spec.contains("refs/pull"), "leaked pull refs: {spec}");
        }
    }

    #[test]
    fn nothing_is_pruned() {
        // Deleting a ref on the target requires the owner to mean it; a
        // background task has no way to ask.
        for spec in refspecs(true) {
            assert!(!spec.contains(':') || !spec.contains("prune"));
        }
        // The refspec form without a colon on the left is what would delete a
        // ref (`:refs/heads/x`); assert none of ours is that shape.
        for spec in refspecs(true) {
            assert!(!spec.starts_with(':'), "deleting refspec: {spec}");
        }
    }

    #[test]
    fn only_one_push_runs_per_repository_at_a_time() {
        // The guard is process-wide; use an unlikely id so other tests cannot
        // interfere.
        let repo_id = 9_876_543;
        let first = try_begin_push(repo_id);
        assert!(first.is_some(), "first claim must succeed");
        assert!(
            try_begin_push(repo_id).is_none(),
            "a second concurrent claim must be refused"
        );
        drop(first);
        assert!(
            try_begin_push(repo_id).is_some(),
            "the claim must be released when the guard drops"
        );
    }

    #[test]
    fn a_repo_path_is_owner_name_git() {
        let path = repo_path_for(Path::new("/srv/repos"), "acme", "demo");
        assert_eq!(path, Path::new("/srv/repos/acme/demo.git"));
    }

    /// Run git in `cwd` with no lockdown, for building fixtures.
    fn git(args: &[&str], cwd: Option<&Path>) -> String {
        let gateway = global_gateway().as_ref().expect("git gateway");
        let out = gateway.run(args, cwd).expect("git runs");
        out.ensure_success().expect("git succeeds");
        out.stdout_str().trim().to_string()
    }

    fn refs(repo: &Path) -> Vec<String> {
        git(&["for-each-ref", "--format=%(refname)"], Some(repo))
            .lines()
            .map(str::to_string)
            .collect()
    }

    /// A bare repository with one branch (`main`) and one tag (`v1`).
    fn init_upstream(path: &Path) {
        std::fs::create_dir_all(path).expect("mkdir");
        let bare = path.to_string_lossy().to_string();
        git(&["init", "--bare", &bare], None);

        let work = path.with_extension("work");
        std::fs::create_dir_all(&work).expect("mkdir work");
        let work_str = work.to_string_lossy().to_string();
        git(&["init", &work_str], None);
        git(&["config", "user.email", "test@example.com"], Some(&work));
        git(&["config", "user.name", "test"], Some(&work));
        std::fs::write(work.join("file.txt"), "one\n").expect("write");
        git(&["add", "file.txt"], Some(&work));
        git(&["commit", "-m", "one"], Some(&work));
        git(&["branch", "-M", "main"], Some(&work));
        git(&["tag", "v1"], Some(&work));
        git(&["push", &bare, "main", "v1"], Some(&work));
        std::fs::remove_dir_all(&work).ok();
    }

    /// The real `git push` this service runs, against a local target: branches
    /// and tags land, a ref only the target has survives, and a rewritten
    /// history is forced through rather than rejected.
    #[tokio::test]
    async fn a_push_copies_refs_forcibly_and_deletes_nothing() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let source = tmp.path().join("source.git");
        init_upstream(&source);
        let target = tmp.path().join("target.git");
        std::fs::create_dir_all(&target).expect("mkdir target");
        let target_str = target.to_string_lossy().to_string();
        git(&["init", "--bare", &target_str], None);

        // First push: branches and tags land.
        let outcome =
            run_git_push(&target_str, &refspecs(true), &source, &[], &[]).expect("git push runs");
        assert!(outcome.success(), "push failed: {}", outcome.stderr_str());

        let after = refs(&target);
        assert!(after.contains(&"refs/heads/main".to_string()));
        assert!(after.contains(&"refs/tags/v1".to_string()));

        // Now a branch that only exists on the target. It has to be created
        // after the first push: `update-ref` needs the object to be present,
        // and before the push the target has no objects at all.
        let head = git(&["rev-parse", "refs/heads/main"], Some(&target));
        git(
            &["update-ref", "refs/heads/target-only", &head],
            Some(&target),
        );

        let outcome = run_git_push(&target_str, &refspecs(true), &source, &[], &[])
            .expect("second push runs");
        assert!(outcome.success(), "push failed: {}", outcome.stderr_str());
        let after = refs(&target);
        assert!(
            after.contains(&"refs/heads/target-only".to_string()),
            "a ref that exists only on the target must not be deleted: {after:?}"
        );

        // Rewrite history on the source and push again: the leading `+` must
        // make it land instead of failing as non-fast-forward.
        let work = tmp.path().join("rewrite");
        std::fs::create_dir_all(&work).expect("mkdir rewrite");
        let work_str = work.to_string_lossy().to_string();
        git(&["clone", &source.to_string_lossy(), &work_str], None);
        git(&["config", "user.email", "test@example.com"], Some(&work));
        git(&["config", "user.name", "test"], Some(&work));
        git(&["commit", "--amend", "-m", "one rewritten"], Some(&work));
        git(
            &["push", "--force", &source.to_string_lossy(), "HEAD:main"],
            Some(&work),
        );

        let outcome = run_git_push(&target_str, &refspecs(true), &source, &[], &[])
            .expect("second push runs");
        assert!(
            outcome.success(),
            "a rewritten history must be forced through: {}",
            outcome.stderr_str()
        );
        let rewritten = git(&["rev-parse", "refs/heads/main"], Some(&target));
        assert_eq!(
            rewritten,
            git(&["rev-parse", "refs/heads/main"], Some(&source))
        );
    }

    /// Tags stay put when the owner turns them off.
    #[tokio::test]
    async fn tags_are_skipped_when_disabled() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let source = tmp.path().join("source.git");
        init_upstream(&source);
        let target = tmp.path().join("target.git");
        std::fs::create_dir_all(&target).expect("mkdir target");
        let target_str = target.to_string_lossy().to_string();
        git(&["init", "--bare", &target_str], None);

        let outcome =
            run_git_push(&target_str, &refspecs(false), &source, &[], &[]).expect("git push runs");
        assert!(outcome.success(), "push failed: {}", outcome.stderr_str());

        let after = refs(&target);
        assert!(after.contains(&"refs/heads/main".to_string()));
        assert!(
            !after.iter().any(|r| r.starts_with("refs/tags/")),
            "tags must not be pushed when disabled: {after:?}"
        );
    }

    #[test]
    fn the_row_model_carries_no_credential() {
        // Structural guarantee: a push mirror stores only where to send refs.
        // If someone adds a token column, this fails and forces a discussion.
        let row = model("https://github.com/acme/private.git", true);
        let json = serde_json::to_string(&row).unwrap();
        assert!(!json.contains("token"));
        assert!(!json.contains("password"));
    }
}
