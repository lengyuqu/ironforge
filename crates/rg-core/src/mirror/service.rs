//! Mirror service — business logic for repository mirroring.
//!
//! Supports creating a mirror of an external Git repository, periodic
//! sync via cron-like scheduling, and manual sync triggers.
//!
//! # Authentication
//!
//! A private upstream needs a token. Two sources are honoured, in order:
//!
//! 1. the mirror's own `username` / `password_encrypted` columns (legacy
//!    per-mirror configuration, still written by the API);
//! 2. the owner's remote-credential row for the upstream host — see
//!    [`crate::remote_credential`], which is what makes one configured token
//!    cover every mirror on the same platform.
//!
//! The token never appears in the URL or in the argument vector; it reaches git
//! through the process environment only (see
//! [`crate::remote_credential::service::git_auth_env`]).

use crate::auth::encryption::{decrypt, encrypt};
use crate::error::{CoreError, CoreResult};
use crate::remote_credential::service as credential_service;
use anyhow::{Context, Result};
use chrono::Utc;
use rg_db::entities::mirror::{ActiveModel, Model as Mirror};
use rg_db::entities::remote_credential::DEFAULT_BASIC_USERNAME;
use rg_db::entities::repository;
use rg_git::cli_gateway::global_gateway;
use sea_orm::ActiveValue::Set;
use sea_orm::{DatabaseConnection, EntityTrait};
use std::path::Path;

/// Create a new mirror for a repository.
///
/// `password` is plaintext on the way in and encrypted before it is stored: the
/// column is called `password_encrypted`, and until this was wired up it held
/// whatever the caller sent.
pub async fn create_mirror(
    db: &DatabaseConnection,
    repo_id: i64,
    url: String,
    username: Option<String>,
    password: Option<String>,
    sync_interval_seconds: i64,
    enc_key: &[u8; 32],
) -> CoreResult<Mirror> {
    // SSRF/RCE guard: reject ext::, file://, ssh:// and other dangerous
    // transports before persisting the remote URL.
    crate::net_guard::validate_mirror_url(&url)
        .await
        .map_err(|e| CoreError::invalid_input(format!("invalid mirror URL: {e}")))?;

    // Ensure the repository exists
    let repo = repository::Entity::find_by_id(repo_id).one(db).await?;
    if repo.is_none() {
        return Err(CoreError::not_found("repository not found"));
    }

    // Check for existing mirror
    if rg_db::ops::mirror_ops::find_by_repo_id(db, repo_id)
        .await?
        .is_some()
    {
        return Err(CoreError::conflict(
            "mirror already exists for this repository",
        ));
    }

    let now = Utc::now();
    let next_sync = now + chrono::Duration::seconds(sync_interval_seconds);
    let password_encrypted = match password {
        Some(plain) => Some(encrypt(&plain, enc_key)?),
        None => None,
    };

    let model = ActiveModel {
        repo_id: Set(repo_id),
        url: Set(url),
        username: Set(username),
        password_encrypted: Set(password_encrypted),
        sync_interval_seconds: Set(sync_interval_seconds),
        next_sync_at: Set(Some(next_sync)),
        last_sync_at: Set(None),
        last_sync_error: Set(None),
        status: Set("active".to_string()),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    };

    // Two concurrent creates can both pass the check above; the loser hits
    // the unique index on repo_id and must see the same conflict.
    let mirror = match rg_db::ops::mirror_ops::create(db, model).await {
        Ok(mirror) => mirror,
        Err(error) if crate::user::service::is_unique_violation(&error) => {
            return Err(CoreError::conflict(
                "mirror already exists for this repository",
            ))
        }
        Err(error) => return Err(error.into()),
    };
    Ok(mirror)
}

/// Get mirror for a repository.
pub async fn get_mirror(db: &DatabaseConnection, repo_id: i64) -> CoreResult<Option<Mirror>> {
    Ok(rg_db::ops::mirror_ops::find_by_repo_id(db, repo_id).await?)
}

/// Update mirror settings.
///
/// `password` is plaintext and encrypted before storage; `None` leaves the
/// stored token untouched, so a caller that only changes the interval does not
/// have to resend it.
// `create_mirror` and this function take the same shape on purpose: the HTTP
// layer destructures one DTO into both, and a parameter object would only move
// the same fields into a struct that mirrors the DTO anyway.
#[allow(clippy::too_many_arguments)]
pub async fn update_mirror(
    db: &DatabaseConnection,
    repo_id: i64,
    url: Option<String>,
    username: Option<String>,
    password: Option<String>,
    sync_interval_seconds: Option<i64>,
    status: Option<String>,
    enc_key: &[u8; 32],
) -> CoreResult<Mirror> {
    let existing = rg_db::ops::mirror_ops::find_by_repo_id(db, repo_id)
        .await?
        .ok_or_else(|| CoreError::not_found("mirror not found"))?;

    if let Some(v) = &url {
        // SSRF/RCE guard on updated remote URL.
        crate::net_guard::validate_mirror_url(v)
            .await
            .map_err(|e| CoreError::invalid_input(format!("invalid mirror URL: {e}")))?;
    }

    let mut model: ActiveModel = existing.into();
    if let Some(v) = url {
        model.url = Set(v);
    }
    if let Some(v) = username {
        model.username = Set(Some(v));
    }
    if let Some(v) = password {
        model.password_encrypted = Set(Some(encrypt(&v, enc_key)?));
    }
    if let Some(v) = sync_interval_seconds {
        model.sync_interval_seconds = Set(v);
    }
    if let Some(v) = status {
        model.status = Set(v);
    }
    model.updated_at = Set(Utc::now());

    Ok(rg_db::ops::mirror_ops::update(db, model).await?)
}

/// Delete a mirror.
pub async fn delete_mirror(db: &DatabaseConnection, repo_id: i64) -> CoreResult<()> {
    let mirror = rg_db::ops::mirror_ops::find_by_repo_id(db, repo_id)
        .await?
        .ok_or_else(|| CoreError::not_found("mirror not found"))?;
    Ok(rg_db::ops::mirror_ops::delete_by_id(db, mirror.id).await?)
}

/// Credentials resolved for one sync, plus the bookkeeping that follows it.
struct MirrorAuth {
    /// Environment for the git invocation. Empty means "anonymous".
    env: Vec<(String, String)>,
    /// Credential row whose `last_used_at` should move after a successful sync.
    credential_id: Option<i64>,
    /// A legacy plaintext mirror password that must be re-encrypted once the
    /// sync succeeds.
    reencrypt: Option<String>,
}

/// Resolve the credentials a sync of `mirror` must use.
///
/// Order matters: the mirror's own token wins over the host dictionary, because
/// it is the more specific configuration — a user who typed a token into this
/// mirror's settings expects *that* token, not whatever the dictionary holds for
/// the same host.
async fn mirror_auth(
    db: &DatabaseConnection,
    mirror: &Mirror,
    enc_key: &[u8; 32],
) -> CoreResult<MirrorAuth> {
    if let Some(stored) = mirror.password_encrypted.as_deref() {
        let (plain, legacy) = decode_mirror_password(stored, enc_key)?;
        let cred = credential_service::ResolvedCredential {
            credential_id: 0,
            username: mirror
                .username
                .clone()
                .unwrap_or_else(|| DEFAULT_BASIC_USERNAME.to_string()),
            token: plain.clone(),
            scheme: credential_service::AuthScheme::Basic,
        };
        return Ok(MirrorAuth {
            env: credential_service::git_auth_env(&mirror.url, &cred)?,
            credential_id: None,
            reencrypt: legacy.then_some(plain),
        });
    }

    let repo = repository::Entity::find_by_id(mirror.repo_id)
        .one(db)
        .await?
        .ok_or_else(|| CoreError::not_found("repository not found"))?;
    let scope = credential_service::Scope::for_repository(&repo);
    match credential_service::resolve(db, scope, &mirror.url, enc_key).await? {
        Some(cred) => {
            let credential_id = cred.credential_id;
            Ok(MirrorAuth {
                env: credential_service::git_auth_env(&mirror.url, &cred)?,
                credential_id: Some(credential_id),
                reencrypt: None,
            })
        }
        None => Ok(MirrorAuth {
            env: Vec::new(),
            credential_id: None,
            reencrypt: None,
        }),
    }
}

/// Read a stored mirror password, tolerating the pre-encryption rows.
///
/// Returns `(plaintext, was_plaintext_row)`.
fn decode_mirror_password(stored: &str, enc_key: &[u8; 32]) -> CoreResult<(String, bool)> {
    match decrypt(stored, enc_key) {
        Ok(plain) => Ok((plain, false)),
        Err(e) => {
            // Rows written before encryption was wired up hold plaintext in a
            // column named `password_encrypted`. Telling the two apart is not
            // cosmetic: reading ciphertext as plaintext sends garbage to the
            // remote, and reading a working plaintext as ciphertext disables a
            // mirror that used to work.
            if looks_like_ciphertext(stored) {
                return Err(CoreError::internal(format!(
                    "stored mirror password cannot be decrypted (instance secret changed?): {e}"
                )));
            }
            Ok((stored.to_string(), true))
        }
    }
}

/// Is this value shaped like `encrypt` output (base64url of nonce + ciphertext)?
fn looks_like_ciphertext(value: &str) -> bool {
    use base64::Engine as _;
    match base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(value) {
        // 12-byte nonce plus the 16-byte GCM tag is the floor for a real
        // ciphertext; anything shorter decodes but cannot be one.
        Ok(bytes) => bytes.len() >= 28,
        Err(_) => false,
    }
}

/// Sync a single mirror: clone (first time) or fetch (subsequent).
///
/// Returns Ok(true) if this call moved the mirror forward, Ok(false) if the
/// mirror is inactive or its configuration is unusable (in which case the row
/// is marked `error` so the scheduler stops retrying it every tick).
pub async fn sync_mirror(
    db: &DatabaseConnection,
    mirror: &Mirror,
    repo_root: &Path,
    enc_key: &[u8; 32],
) -> Result<bool> {
    if mirror.status != "active" {
        return Ok(false);
    }

    // Re-validate the stored remote: rows created before the URL guard was
    // introduced (or written directly to the DB) must not be able to use
    // dangerous transports (ext:: RCE, file:// local reads).
    if let Err(e) = crate::net_guard::validate_mirror_url(&mirror.url).await {
        tracing::error!(
            repo_id = mirror.repo_id,
            url = %mirror.url,
            error = %e,
            "mirror URL rejected by outbound guard; disabling mirror"
        );
        let mut model: ActiveModel = mirror.clone().into();
        model.last_sync_error = Set(Some(format!("mirror URL rejected: {e}")));
        model.status = Set("error".to_string());
        model.updated_at = Set(Utc::now());
        rg_db::ops::mirror_ops::update(db, model).await?;
        return Ok(false);
    }

    let repo_path = repo_root.join(format!("{}.mirror", mirror.repo_id));

    // Credentials are resolved *before* the git call, and an unusable row fails
    // the sync instead of falling back to anonymous: retrying without a token
    // against a private upstream produces a 401 that points at the wrong
    // problem.
    let auth = match mirror_auth(db, mirror, enc_key).await {
        Ok(auth) => auth,
        Err(e) => {
            tracing::error!(
                repo_id = mirror.repo_id,
                error = %e,
                "mirror credentials unusable; sync skipped"
            );
            let mut model: ActiveModel = mirror.clone().into();
            model.last_sync_error = Set(Some(format!("credentials unusable: {e}")));
            model.status = Set("error".to_string());
            model.updated_at = Set(Utc::now());
            rg_db::ops::mirror_ops::update(db, model).await?;
            return Ok(false);
        }
    };

    let mut env = auth.env.clone();
    // A server-side git must never block waiting for a username on a tty that
    // does not exist.
    env.push(("GIT_TERMINAL_PROMPT".to_string(), "0".to_string()));
    let env_refs: Vec<(&str, &str)> = env
        .iter()
        .map(|(key, value)| (key.as_str(), value.as_str()))
        .collect();

    let result = if repo_path.join("HEAD").exists() {
        // Existing mirror: git remote update
        fetch_with(&repo_path, &GIT_TRANSPORT_LOCKDOWN, &env_refs)
    } else {
        // First time: git clone --mirror
        clone_mirror_with(&mirror.url, &repo_path, &GIT_TRANSPORT_LOCKDOWN, &env_refs)
    };

    let now = Utc::now();
    let next_sync = now + chrono::Duration::seconds(mirror.sync_interval_seconds);

    let mut model: ActiveModel = mirror.clone().into();
    model.last_sync_at = Set(Some(now));
    model.next_sync_at = Set(Some(next_sync));
    model.updated_at = Set(now);

    match result {
        Ok(()) => {
            model.last_sync_error = Set(None);
            model.status = Set("active".to_string());
            // Upgrade a legacy plaintext row on the first sync that used it. A
            // backfill migration cannot do this: it has no instance secret.
            if let Some(plain) = &auth.reencrypt {
                model.password_encrypted = Set(Some(encrypt(plain, enc_key)?));
            }
            if let Some(credential_id) = auth.credential_id {
                // Bookkeeping only: failing to record the use must not fail a
                // sync that already moved the refs.
                if let Err(e) =
                    rg_db::ops::remote_credential_ops::touch_last_used(db, credential_id, now).await
                {
                    tracing::warn!(
                        credential_id,
                        error = %e,
                        "could not record remote credential use"
                    );
                }
            }
        }
        Err(e) => {
            model.last_sync_error = Set(Some(format!("{e}")));
            model.status = Set("error".to_string());
            tracing::error!("Mirror sync failed for repo {}: {e}", mirror.repo_id);
        }
    }

    rg_db::ops::mirror_ops::update(db, model).await?;
    Ok(true)
}

/// Sync all due mirrors (called by background task / cron).
pub async fn sync_due_mirrors(
    db: &DatabaseConnection,
    repo_root: &Path,
    limit: u64,
    enc_key: &[u8; 32],
) -> Result<usize> {
    let mirrors = rg_db::ops::mirror_ops::list_due_sync(db, limit).await?;
    let mut count = 0;
    for mirror in &mirrors {
        match sync_mirror(db, mirror, repo_root, enc_key).await {
            Ok(true) => count += 1,
            Ok(false) => { /* inactive or misconfigured, skip */ }
            Err(e) => tracing::error!("Mirror sync error: {e}"),
        }
    }
    Ok(count)
}

/// Manually trigger a sync for a mirror.
pub async fn trigger_sync(
    db: &DatabaseConnection,
    repo_id: i64,
    repo_root: &Path,
    enc_key: &[u8; 32],
) -> CoreResult<()> {
    let mirror = rg_db::ops::mirror_ops::find_by_repo_id(db, repo_id)
        .await?
        .ok_or_else(|| CoreError::not_found("mirror not found"))?;
    sync_mirror(db, &mirror, repo_root, enc_key).await?;
    Ok(())
}

// ── Git helpers ─────────────────────────────────────────────────────────

/// Git transport lockdown passed as `-c` options to every mirror git
/// invocation. `ext::` executes arbitrary commands (RCE), `file://` reads
/// local paths, and ssh is not supported for mirrors — all three are
/// disabled at the git layer as defence in depth on top of the URL guard.
pub(crate) const GIT_TRANSPORT_LOCKDOWN: [&str; 6] = [
    "-c",
    "protocol.ext.allow=never",
    "-c",
    "protocol.file.allow=never",
    "-c",
    "protocol.ssh.allow=never",
];

/// `git clone --mirror`, then narrow the mirror to branches and tags.
///
/// The lockdown is a parameter so the tests can drive the same code with the
/// local `file` transport allowed (production always passes
/// [`GIT_TRANSPORT_LOCKDOWN`]; the guard itself is tested in `net_guard`).
/// `env` carries the credential (`GIT_CONFIG_*`, see
/// [`crate::remote_credential::service::git_auth_env`]) and is empty for a
/// public upstream.
fn clone_mirror_with(
    url: &str,
    path: &Path,
    lockdown: &[&str],
    env: &[(&str, &str)],
) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("invalid path: no parent directory"))?;
    std::fs::create_dir_all(parent).context("create mirror dir")?;

    let git = global_gateway()
        .as_ref()
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    let path_str = path.to_string_lossy();
    let mut args: Vec<&str> = lockdown.to_vec();
    args.extend(["clone", "--mirror", url, &path_str]);
    git.run_with_env(&args, None, env)
        .context("git clone --mirror")?
        .ensure_success()
        .context("git clone --mirror")?;
    adopt_heads_and_tags(path, lockdown)
}

/// What a mirror carries: branches and tags, nothing else.
///
/// `refs/pull/*` is deliberately outside: that namespace belongs to this
/// instance (`publish_head_ref` writes `refs/pull/<n>/head` here), so a sync
/// must neither prune it nor import the upstream's.
const MIRROR_REFSPECS: [&str; 2] = ["+refs/heads/*:refs/heads/*", "+refs/tags/*:refs/tags/*"];

/// Narrow a fresh `clone --mirror` to [`MIRROR_REFSPECS`].
///
/// Two things have to go: the wildcard fetch refspec `+refs/*:refs/*` that
/// `--mirror` writes (it is what made `remote update --prune` delete anything
/// the upstream did not have, `refs/pull/*` included), and whatever the clone
/// did import outside those namespaces -- notably the upstream's own
/// `refs/pull/*`, which would collide by number with the pull requests of this
/// instance.
fn adopt_heads_and_tags(path: &Path, lockdown: &[&str]) -> Result<()> {
    let git = global_gateway()
        .as_ref()
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    let mut replace = lockdown.to_vec();
    replace.extend([
        "config",
        "--replace-all",
        "remote.origin.fetch",
        MIRROR_REFSPECS[0],
    ]);
    git.run_or_bail(&replace, Some(path))
        .context("narrow remote.origin.fetch")?;

    let mut add = lockdown.to_vec();
    add.extend(["config", "--add", "remote.origin.fetch", MIRROR_REFSPECS[1]]);
    git.run_or_bail(&add, Some(path))
        .context("narrow remote.origin.fetch")?;

    let mut list = lockdown.to_vec();
    list.extend(["for-each-ref", "--format=%(refname)", "refs/pull/"]);
    let imported = git.run(&list, Some(path)).context("list refs/pull")?;
    imported.ensure_success().context("list refs/pull")?;
    for refname in imported
        .stdout_str()
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        let mut delete = lockdown.to_vec();
        delete.extend(["update-ref", "-d", refname]);
        git.run_or_bail(&delete, Some(path))
            .with_context(|| format!("drop imported {refname}"))?;
    }
    Ok(())
}

/// The `git fetch` a sync runs: branches and tags, pruned, and nothing else.
fn fetch_with(path: &Path, lockdown: &[&str], env: &[(&str, &str)]) -> Result<()> {
    let git = global_gateway()
        .as_ref()
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    let mut args: Vec<&str> = lockdown.to_vec();
    args.extend(["fetch", "--prune", "origin"]);
    args.extend(MIRROR_REFSPECS);
    git.run_with_env(&args, Some(path), env)
        .context("git fetch --prune origin (branches and tags)")?
        .ensure_success()
        .context("git fetch --prune origin (branches and tags)")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Run git in `cwd` with no transport lockdown, so the test can use a
    /// local upstream. Returns stdout.
    fn git(args: &[&str], cwd: &Path) -> String {
        let gateway = global_gateway().as_ref().expect("git gateway");
        let out = gateway.run(args, Some(cwd)).expect("git runs");
        out.ensure_success().expect("git succeeds");
        out.stdout_str().trim().to_string()
    }

    fn init_upstream(path: &Path) {
        std::fs::create_dir_all(path).expect("mkdir");
        let gateway = global_gateway().as_ref().expect("git gateway");
        let path_str = path.to_string_lossy().to_string();
        gateway
            .run(&["init", "--bare", &path_str], None)
            .expect("git init")
            .ensure_success()
            .expect("bare init");

        let work = path.with_extension("work");
        std::fs::create_dir_all(&work).expect("mkdir work");
        let work_str = work.to_string_lossy().to_string();
        gateway
            .run(&["init", &work_str], None)
            .expect("git init work")
            .ensure_success()
            .expect("work init");
        git(&["config", "user.email", "test@example.com"], &work);
        git(&["config", "user.name", "test"], &work);
        std::fs::write(work.join("file.txt"), "one\n").expect("write");
        git(&["add", "file.txt"], &work);
        git(&["commit", "-m", "one"], &work);
        git(&["branch", "-M", "main"], &work);
        git(&["tag", "v1"], &work);
        git(
            &["push", &path_str, "main", "v1", "main:refs/heads/feat"],
            &work,
        );
        std::fs::remove_dir_all(&work).ok();
    }

    fn refs(repo: &Path) -> Vec<String> {
        let out = git(&["for-each-ref", "--format=%(refname)"], repo);
        out.lines().map(str::to_string).collect()
    }

    #[test]
    fn a_sync_keeps_the_pull_refs_this_instance_wrote() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let upstream = tmp.path().join("upstream.git");
        init_upstream(&upstream);
        let mirror = tmp.path().join("mirror.git");
        clone_mirror_with(&upstream.to_string_lossy(), &mirror, &[], &[]).expect("clone");

        // This instance writes refs/pull/<n>/head here (publish_head_ref).
        let head = git(&["rev-parse", "refs/heads/main"], &mirror);
        git(&["update-ref", "refs/pull/1/head", &head], &mirror);

        // Upstream moves on: a branch is renamed, another deleted, a tag added
        // and one removed. Done on the bare upstream directly -- a working copy
        // would depend on `init.defaultBranch`, which is not `main` everywhere.
        let tip = git(&["rev-parse", "refs/heads/feat"], &upstream);
        git(&["update-ref", "refs/heads/feat2", &tip], &upstream);
        git(&["update-ref", "-d", "refs/heads/feat"], &upstream);
        git(&["tag", "v2", &tip], &upstream);
        git(&["tag", "-d", "v1"], &upstream);

        fetch_with(&mirror, &[], &[]).expect("sync");

        let after = refs(&mirror);
        assert_eq!(
            after,
            vec![
                "refs/heads/feat2".to_string(),
                "refs/heads/main".to_string(),
                "refs/pull/1/head".to_string(),
                "refs/tags/v2".to_string(),
            ]
        );
        assert_eq!(git(&["rev-parse", "refs/pull/1/head"], &mirror), head);
    }

    #[test]
    fn a_clone_does_not_import_the_upstreams_pull_refs() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let upstream = tmp.path().join("upstream.git");
        init_upstream(&upstream);
        // The upstream service publishes pull-request refs of its own.
        let head = git(&["rev-parse", "refs/heads/main"], &upstream);
        git(&["update-ref", "refs/pull/9/head", &head], &upstream);

        let mirror = tmp.path().join("mirror.git");
        clone_mirror_with(&upstream.to_string_lossy(), &mirror, &[], &[]).expect("clone");

        assert!(
            !refs(&mirror).iter().any(|r| r.starts_with("refs/pull/")),
            "the clone must not adopt the upstream's pull refs: {:?}",
            refs(&mirror)
        );
        // ... and they must not come back on the next sync either.
        fetch_with(&mirror, &[], &[]).expect("sync");
        assert!(!refs(&mirror).iter().any(|r| r.starts_with("refs/pull/")));
    }
}
