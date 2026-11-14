//! Server-side `git apply` of a caller-supplied unified diff onto one branch of
//! a bare repository.
//!
//! gix has no apply primitive, so the patch is applied to a **temporary index**
//! and the commit is assembled with plumbing, never by checking anything out
//! (a bare repository has no worktree):
//!
//! ```text
//! GIT_INDEX_FILE=<scratch>/index
//! git read-tree <expected tip>      # seed the index from the tree the caller read
//! git apply --cached <patch>        # index only; the worktree is never consulted
//! git write-tree                    # the tree the commit will carry
//! git commit-tree <tree> -p <tip>   # one commit on top of the expected tip
//! ```
//!
//! # Two phases, so a refused patch writes nothing
//!
//! [`preview_patch`] runs the same sequence with `GIT_OBJECT_DIRECTORY` pointing
//! at a scratch directory and the repository's objects as an alternate: every
//! object the patch produces lands in the scratch directory and is thrown away
//! with it, so a conflict, an over-limit patch or a secret-scan rejection
//! leaves the repository byte-for-byte unchanged. It also returns the contents
//! of the files the patch adds or modifies, which is what the caller scans.
//!
//! [`commit_patch`] repeats the sequence against the real object database and
//! then moves the branch with an expected-old-value guard, so the branch is
//! either at the tip the caller named or at the new commit — never in between.
//!
//! # What is supported
//!
//! Whatever `git apply --cached` understands: additions, deletions,
//! modifications, renames (as a delete plus an add, which is how a diff spells
//! them), type changes and git binary patches. The paths are git's own: the
//! patch is read with the default `-p1` prefix strip, so the caller sends a
//! `git diff`-shaped patch (`a/…`, `b/…`, `/dev/null` for a create or delete).
//!
//! `--3way` is **never** used: it would let a patch land with fuzzy context and
//! an unreported merge, which is a different operation than "apply this diff".
//! A patch whose context does not match the tip is a conflict, full stop.
//!
//! # What is refused
//!
//! A patch over [`MAX_PATCH_BYTES`] or touching more than [`MAX_PATCH_FILES`]
//! paths, and any path that is absolute, escapes the repository (`..`), or
//! names git's own `.git` directory. git would refuse most of these itself; the
//! check runs first so the caller gets a `400` with a reason instead of a
//! translated CLI error, and so no parsing of the patch happens on a path that
//! git might one day accept.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::Context;
use gix::refs::transaction::{Change, LogChange, PreviousValue, RefEdit, RefLog};

use crate::cli_gateway::{global_gateway, GitCommandGateway, GitOutput};

/// Largest patch text accepted, in bytes. Also the effective ceiling on how
/// much work one request can make the server do.
pub const MAX_PATCH_BYTES: usize = 1024 * 1024;

/// Largest number of paths one patch may touch.
pub const MAX_PATCH_FILES: usize = 500;

/// A patch the caller has to fix, as opposed to a fault in the repository.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
struct PatchRejected(String);

fn rejected<T>(reason: impl Into<String>) -> anyhow::Result<T> {
    Err(PatchRejected(reason.into()).into())
}

/// One patch to apply, with the branch state the caller read.
#[derive(Debug, Clone)]
pub struct PatchRequest<'a> {
    /// Branch the new commit goes on. It must already exist.
    pub branch: &'a str,
    /// The commit `branch` must currently point at.
    pub expected_tip: &'a str,
    /// The unified diff, as the caller wrote it.
    pub patch: &'a str,
    /// Commit message; `None` generates one from the branch and the change.
    pub message: Option<&'a str>,
    pub identity_name: &'a str,
    pub identity_email: &'a str,
}

/// What happened to one path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileStatus {
    Added,
    Modified,
    Deleted,
    /// The entry changed type (regular file ↔ symlink ↔ submodule).
    TypeChanged,
}

impl FileStatus {
    /// Stable lowercase name used in API bodies.
    pub fn as_str(self) -> &'static str {
        match self {
            FileStatus::Added => "added",
            FileStatus::Modified => "modified",
            FileStatus::Deleted => "deleted",
            FileStatus::TypeChanged => "type_changed",
        }
    }
}

/// One path the patch touches, in `git diff-tree` order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileChange {
    pub path: String,
    pub status: FileStatus,
}

/// A patch that applies cleanly, computed without touching the repository.
#[derive(Debug, Clone)]
pub struct PatchPreview {
    /// The tree the commit would carry. Names no stored object: it was written
    /// to the scratch object directory, which is gone by the time this is read.
    pub tree: String,
    pub tip_before: String,
    pub files_changed: usize,
    pub changes: Vec<FileChange>,
    /// Resulting contents of the added, modified and type-changed files, for
    /// the caller's secret scan. Files above
    /// [`crate::secret_scan::MAX_FILE_BYTES`] are left out — the scanner skips
    /// them anyway, and they are exactly the blobs not worth loading.
    pub new_blobs: Vec<(String, Vec<u8>)>,
}

/// Result of [`preview_patch`].
#[derive(Debug, Clone)]
pub enum PreviewOutcome {
    /// The patch applies cleanly; nothing was written.
    Previewed(PatchPreview),
    /// The patch does not apply to the tip; `paths` are the ones git named.
    Conflict { paths: Vec<String>, tip: String },
    /// The patch applies but produces the tip's own tree.
    Empty { tip: String },
    /// The branch is not at `expected_tip` (`None`: the branch does not exist).
    TipAdvanced { actual: Option<String> },
    /// The patch itself is not acceptable; `reason` says why.
    Invalid { reason: String },
}

/// Result of [`commit_patch`].
#[derive(Debug, Clone)]
pub enum CommitOutcome {
    /// The commit was written and the branch moved to it.
    Committed {
        commit: String,
        tree: String,
        tip_before: String,
        files_changed: usize,
        changes: Vec<FileChange>,
    },
    Conflict {
        paths: Vec<String>,
        tip: String,
    },
    Empty {
        tip: String,
    },
    TipAdvanced {
        actual: Option<String>,
    },
    Invalid {
        reason: String,
    },
}

// ── Scratch directory ────────────────────────────────────────────────

/// A private temporary directory removed on drop.
///
/// `tempfile` is a dev-dependency of this crate and a runtime dependency of
/// nothing here; adding it to `[dependencies]` would touch `Cargo.toml`, which a
/// task-scoped change must not do. The names are process-unique and created
/// with `create_dir`, which fails rather than following a pre-planted symlink.
struct Scratch {
    path: PathBuf,
}

impl Scratch {
    fn new() -> anyhow::Result<Self> {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let base = std::env::temp_dir();
        let pid = std::process::id();
        let mut last_error = None;
        for _ in 0..32 {
            let name = format!(
                "ironforge-apply-{pid}-{}",
                COUNTER.fetch_add(1, Ordering::Relaxed)
            );
            let path = base.join(name);
            match create_private_dir(&path) {
                Ok(()) => return Ok(Self { path }),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    last_error = Some(error);
                    continue;
                }
                Err(error) => {
                    return Err(anyhow::anyhow!(
                        "failed to create a scratch directory under {}: {error}",
                        base.display()
                    ))
                }
            }
        }
        Err(anyhow::anyhow!(
            "could not find a free scratch directory name under {}: {last_error:?}",
            base.display()
        ))
    }

    fn join(&self, name: &str) -> PathBuf {
        self.path.join(name)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        // A failure here leaves a directory of unreferenced objects in the
        // system temporary directory; it is not worth failing the request over.
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

#[cfg(unix)]
fn create_private_dir(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new().mode(0o700).create(path)
}

#[cfg(not(unix))]
fn create_private_dir(path: &Path) -> std::io::Result<()> {
    std::fs::create_dir(path)
}

// ── git invocation ───────────────────────────────────────────────────

fn gateway() -> anyhow::Result<&'static GitCommandGateway> {
    global_gateway()
        .as_ref()
        .map_err(|error| anyhow::anyhow!("git CLI unavailable: {error}"))
}

/// Run git with `-C <repo>` and the given environment.
fn run(
    gateway: &GitCommandGateway,
    repo_path: &Path,
    env: &[(&str, &str)],
    args: &[&str],
) -> anyhow::Result<GitOutput> {
    let output = if env.is_empty() {
        gateway.run(args, Some(repo_path))
    } else {
        gateway.run_with_env(args, Some(repo_path), env)
    };
    output.with_context(|| format!("failed to run `git {}`", args.join(" ")))
}

// ── Validation ───────────────────────────────────────────────────────

/// The paths a patch touches, according to git's own parser.
///
/// `git apply --numstat` parses the patch and writes nothing; `-z` gives one
/// NUL-terminated `<added>\t<deleted>\t<path>` record per file with the path
/// unquoted. Using git's parser here means the paths validated below are
/// exactly the paths `git apply` will write to (including `a/`/`b/` prefix
/// stripping), rather than a second, weaker parser's idea of them.
fn validate_patch(
    gateway: &GitCommandGateway,
    repo_path: &Path,
    scratch: &Scratch,
    patch: &str,
) -> anyhow::Result<Vec<String>> {
    if patch.trim().is_empty() {
        return rejected("patch is empty");
    }
    if patch.len() > MAX_PATCH_BYTES {
        return rejected(format!(
            "patch is {} bytes, above the {MAX_PATCH_BYTES} byte limit",
            patch.len()
        ));
    }
    let patch_file = scratch.join("patch.diff");
    std::fs::write(&patch_file, patch.as_bytes())
        .with_context(|| format!("failed to stage the patch at {}", patch_file.display()))?;

    let output = run(
        gateway,
        repo_path,
        &[("LC_ALL", "C")],
        &["apply", "--numstat", "-z", &patch_file.to_string_lossy()],
    )?;
    if !output.success() {
        return rejected(format!(
            "not a unified diff git can apply: {}",
            first_line(&output.stderr_str())
        ));
    }

    let mut paths = Vec::new();
    for record in output.stdout.split(|byte| *byte == 0) {
        if record.is_empty() {
            continue;
        }
        let text = String::from_utf8_lossy(record);
        // `<added>\t<deleted>\t<path>`
        let Some((_, rest)) = text.split_once('\t') else {
            return rejected("not a unified diff git can apply");
        };
        let Some((_, path)) = rest.split_once('\t') else {
            return rejected("not a unified diff git can apply");
        };
        paths.push(path.to_string());
    }
    if paths.is_empty() {
        return rejected("patch touches no file");
    }
    if paths.len() > MAX_PATCH_FILES {
        return rejected(format!(
            "patch touches {} files, above the {MAX_PATCH_FILES} file limit",
            paths.len()
        ));
    }
    for path in &paths {
        check_patch_path(path)?;
    }
    Ok(paths)
}

/// Reject a path that would land outside the repository or inside `.git`.
fn check_patch_path(path: &str) -> anyhow::Result<()> {
    let shown = || {
        let mut text: String = path.chars().take(200).collect();
        if text.len() < path.len() {
            text.push('…');
        }
        text
    };
    if path.is_empty() {
        return rejected("patch names an empty path");
    }
    if path.len() > 4096 {
        return rejected("patch names a path longer than 4096 bytes");
    }
    if path.starts_with('/') {
        return rejected(format!("patch names an absolute path: {}", shown()));
    }
    if path.chars().any(|c| c.is_control()) {
        return rejected(format!(
            "patch names a path with control characters: {}",
            shown()
        ));
    }
    for component in path.split('/') {
        if component.is_empty() || component == "." || component == ".." {
            return rejected(format!(
                "patch names a path outside the repository: {}",
                shown()
            ));
        }
        if component.eq_ignore_ascii_case(".git") {
            return rejected(format!("patch names a path inside .git: {}", shown()));
        }
    }
    Ok(())
}

fn first_line(text: &str) -> String {
    text.lines().next().unwrap_or("").trim().to_string()
}

// ── Applying ─────────────────────────────────────────────────────────

/// The environment that makes git use a scratch index (and, for a preview,
/// a scratch object database with the repository as an alternate).
fn scratch_env(
    index: &Path,
    objects: Option<&Path>,
    alternates: Option<&Path>,
) -> Vec<(&'static str, String)> {
    let mut env = vec![
        ("GIT_INDEX_FILE", index.to_string_lossy().to_string()),
        // Keep git's diagnostics parseable regardless of the server locale.
        ("LC_ALL", "C".to_string()),
    ];
    if let Some(objects) = objects {
        env.push((
            "GIT_OBJECT_DIRECTORY",
            objects.to_string_lossy().to_string(),
        ));
    }
    if let Some(alternates) = alternates {
        env.push((
            "GIT_ALTERNATE_OBJECT_DIRECTORIES",
            alternates.to_string_lossy().to_string(),
        ));
    }
    env
}

fn pairs<'a>(env: &'a [(&'static str, String)]) -> Vec<(&'a str, &'a str)> {
    env.iter()
        .map(|(key, value)| (*key, value.as_str()))
        .collect()
}

/// Read the tip of `branch`, or `None` when the branch does not exist.
fn branch_tip(repo: &gix::Repository, branch: &str) -> Option<gix::ObjectId> {
    let refname = format!("refs/heads/{branch}");
    repo.try_find_reference(refname.as_str())
        .ok()
        .flatten()
        .and_then(|mut reference| reference.peel_to_id().ok().map(|id| id.detach()))
}

/// The tree a commit points at.
fn tree_of(repo: &gix::Repository, commit: gix::ObjectId) -> anyhow::Result<gix::ObjectId> {
    Ok(repo
        .find_object(commit)
        .with_context(|| format!("failed to read commit {commit}"))?
        .peel_to_tree()
        .with_context(|| format!("failed to read the tree of {commit}"))?
        .id()
        .detach())
}

/// Apply `patch` to a scratch index seeded from `tip`, returning the resulting
/// tree id and the paths git reported as conflicting.
fn apply_to_scratch_index(
    gateway: &GitCommandGateway,
    repo_path: &Path,
    env: &[(&str, &str)],
    tip: &str,
    patch: &Path,
) -> anyhow::Result<std::result::Result<String, Vec<String>>> {
    run(gateway, repo_path, env, &["read-tree", tip])?.ensure_success()?;

    let output = run(
        gateway,
        repo_path,
        env,
        &["apply", "--cached", &patch.to_string_lossy()],
    )?;
    if !output.success() {
        return Ok(Err(conflict_paths(&output.stderr_str())));
    }

    let tree = run(gateway, repo_path, env, &["write-tree"])?;
    tree.ensure_success()?;
    Ok(Ok(tree.stdout_str().trim().to_string()))
}

/// The paths git named when `git apply` refused the patch.
///
/// `git apply` writes one `error: <path>: <reason>` line per refused file (plus
/// a `error: patch failed: <path>:<line>` summary). Only the former carries the
/// path; the reason text is dropped because it is neither stable nor ours.
fn conflict_paths(stderr: &str) -> Vec<String> {
    let mut paths = BTreeSet::new();
    for line in stderr.lines() {
        let Some(rest) = line.strip_prefix("error: ") else {
            continue;
        };
        let Some((path, _reason)) = rest.split_once(": ") else {
            continue;
        };
        if path == "patch failed" || path.is_empty() {
            continue;
        }
        paths.insert(path.to_string());
    }
    paths.into_iter().collect()
}

/// One entry of `git diff-tree --raw -z`.
struct RawChange {
    path: String,
    status: FileStatus,
    new_blob: Option<String>,
}

fn parse_raw_diff(stdout: &[u8]) -> Vec<RawChange> {
    let mut out = Vec::new();
    let fields: Vec<&[u8]> = stdout.split(|byte| *byte == 0).collect();
    let mut index = 0;
    while index + 1 < fields.len() {
        let meta = String::from_utf8_lossy(fields[index]);
        let path = String::from_utf8_lossy(fields[index + 1]).to_string();
        index += 2;
        let Some(meta) = meta.strip_prefix(':') else {
            continue;
        };
        let parts: Vec<&str> = meta.split(' ').collect();
        if parts.len() < 5 {
            continue;
        }
        let (new_sha, status) = (parts[3], parts[4]);
        let status = match status.chars().next() {
            Some('A') => FileStatus::Added,
            Some('M') => FileStatus::Modified,
            Some('D') => FileStatus::Deleted,
            Some('T') => FileStatus::TypeChanged,
            // `R`/`C` need rewrite detection, which is off; an unexpected letter
            // is still a change worth reporting as a modification.
            _ => FileStatus::Modified,
        };
        let new_blob = if status == FileStatus::Deleted || new_sha.chars().all(|c| c == '0') {
            None
        } else {
            Some(new_sha.to_string())
        };
        out.push(RawChange {
            path,
            status,
            new_blob,
        });
    }
    out
}

/// Every path that changed between `old_tree` and `new_tree`.
fn diff_changes(
    gateway: &GitCommandGateway,
    repo_path: &Path,
    env: &[(&str, &str)],
    old_tree: &str,
    new_tree: &str,
) -> anyhow::Result<Vec<RawChange>> {
    let output = run(
        gateway,
        repo_path,
        env,
        &[
            "diff-tree",
            "-r",
            "-z",
            "--no-commit-id",
            "--raw",
            old_tree,
            new_tree,
        ],
    )?;
    output.ensure_success()?;
    Ok(parse_raw_diff(&output.stdout))
}

/// Resulting contents of an added or modified file, or `None` when it is too
/// large to be worth loading (the scanner skips those anyway).
fn read_blob(
    gateway: &GitCommandGateway,
    repo_path: &Path,
    env: &[(&str, &str)],
    id: &str,
) -> anyhow::Result<Option<Vec<u8>>> {
    let size = run(gateway, repo_path, env, &["cat-file", "-s", id])?;
    size.ensure_success()?;
    let size: usize = size
        .stdout_str()
        .trim()
        .parse()
        .with_context(|| format!("unexpected `git cat-file -s {id}` output"))?;
    if size > crate::secret_scan::MAX_FILE_BYTES {
        return Ok(None);
    }
    let blob = run(gateway, repo_path, env, &["cat-file", "blob", id])?;
    blob.ensure_success()?;
    Ok(Some(blob.stdout))
}

// ── Public entry points ──────────────────────────────────────────────

/// Apply `request.patch` in a scratch object database and report what it would
/// do. Nothing in the repository is written, not even an unreferenced blob.
pub fn preview_patch(
    repo_path: &Path,
    request: &PatchRequest<'_>,
) -> anyhow::Result<PreviewOutcome> {
    match preview_inner(repo_path, request) {
        Ok(outcome) => Ok(outcome),
        Err(error) => match error.downcast::<PatchRejected>() {
            Ok(PatchRejected(reason)) => Ok(PreviewOutcome::Invalid { reason }),
            Err(error) => Err(error),
        },
    }
}

fn preview_inner(repo_path: &Path, request: &PatchRequest<'_>) -> anyhow::Result<PreviewOutcome> {
    let gateway = gateway()?;
    let repo =
        gix::open(repo_path).with_context(|| format!("failed to open repository {repo_path:?}"))?;

    let Some(tip) = branch_tip(&repo, request.branch) else {
        return Ok(PreviewOutcome::TipAdvanced { actual: None });
    };
    if gix::ObjectId::from_hex(request.expected_tip.as_bytes()).ok() != Some(tip) {
        return Ok(PreviewOutcome::TipAdvanced {
            actual: Some(tip.to_string()),
        });
    }
    let tip_tree = tree_of(&repo, tip)?;

    let scratch = Scratch::new()?;
    validate_patch(gateway, repo_path, &scratch, request.patch)?;

    let objects = scratch.join("objects");
    std::fs::create_dir(&objects)
        .with_context(|| format!("failed to create {}", objects.display()))?;
    let repo_objects = repo.path().join("objects");
    let env = scratch_env(&scratch.join("index"), Some(&objects), Some(&repo_objects));
    let env = pairs(&env);

    let patch_file = scratch.join("patch.diff");
    let tree =
        match apply_to_scratch_index(gateway, repo_path, &env, &tip.to_string(), &patch_file)? {
            Ok(tree) => tree,
            Err(paths) => {
                return Ok(PreviewOutcome::Conflict {
                    paths,
                    tip: tip.to_string(),
                })
            }
        };
    let tree_id = gix::ObjectId::from_hex(tree.as_bytes())
        .context("git write-tree returned an unparseable id")?;
    if tree_id == tip_tree {
        return Ok(PreviewOutcome::Empty {
            tip: tip.to_string(),
        });
    }

    let raw = diff_changes(gateway, repo_path, &env, &tip_tree.to_string(), &tree)?;
    let mut changes = Vec::with_capacity(raw.len());
    let mut new_blobs = Vec::new();
    for change in &raw {
        if let Some(id) = &change.new_blob {
            if let Some(content) = read_blob(gateway, repo_path, &env, id)? {
                new_blobs.push((change.path.clone(), content));
            }
        }
        changes.push(FileChange {
            path: change.path.clone(),
            status: change.status,
        });
    }

    Ok(PreviewOutcome::Previewed(PatchPreview {
        tree,
        tip_before: tip.to_string(),
        files_changed: changes.len(),
        changes,
        new_blobs,
    }))
}

/// Apply `request.patch` for real: write the tree and commit, then move the
/// branch with an expected-old-value guard.
pub fn commit_patch(repo_path: &Path, request: &PatchRequest<'_>) -> anyhow::Result<CommitOutcome> {
    match commit_inner(repo_path, request) {
        Ok(outcome) => Ok(outcome),
        Err(error) => match error.downcast::<PatchRejected>() {
            Ok(PatchRejected(reason)) => Ok(CommitOutcome::Invalid { reason }),
            Err(error) => Err(error),
        },
    }
}

fn commit_inner(repo_path: &Path, request: &PatchRequest<'_>) -> anyhow::Result<CommitOutcome> {
    let gateway = gateway()?;
    let repo =
        gix::open(repo_path).with_context(|| format!("failed to open repository {repo_path:?}"))?;

    let Some(tip) = branch_tip(&repo, request.branch) else {
        return Ok(CommitOutcome::TipAdvanced { actual: None });
    };
    if gix::ObjectId::from_hex(request.expected_tip.as_bytes()).ok() != Some(tip) {
        return Ok(CommitOutcome::TipAdvanced {
            actual: Some(tip.to_string()),
        });
    }
    let tip_tree = tree_of(&repo, tip)?;

    let scratch = Scratch::new()?;
    validate_patch(gateway, repo_path, &scratch, request.patch)?;

    let env = scratch_env(&scratch.join("index"), None, None);
    let env = pairs(&env);
    let patch_file = scratch.join("patch.diff");
    let tree =
        match apply_to_scratch_index(gateway, repo_path, &env, &tip.to_string(), &patch_file)? {
            Ok(tree) => tree,
            Err(paths) => {
                return Ok(CommitOutcome::Conflict {
                    paths,
                    tip: tip.to_string(),
                })
            }
        };
    let tree_id = gix::ObjectId::from_hex(tree.as_bytes())
        .context("git write-tree returned an unparseable id")?;
    if tree_id == tip_tree {
        return Ok(CommitOutcome::Empty {
            tip: tip.to_string(),
        });
    }

    let raw = diff_changes(gateway, repo_path, &env, &tip_tree.to_string(), &tree)?;
    let changes: Vec<FileChange> = raw
        .into_iter()
        .map(|change| FileChange {
            path: change.path,
            status: change.status,
        })
        .collect();

    let message = match request.message {
        Some(message) => message.trim_end().to_string(),
        None => format!(
            "Apply patch to {}\n\n{} file(s) changed.",
            request.branch,
            changes.len()
        ),
    };
    let commit = run(
        gateway,
        repo_path,
        &[
            ("LC_ALL", "C"),
            ("GIT_AUTHOR_NAME", request.identity_name),
            ("GIT_AUTHOR_EMAIL", request.identity_email),
            ("GIT_COMMITTER_NAME", request.identity_name),
            ("GIT_COMMITTER_EMAIL", request.identity_email),
        ],
        &["commit-tree", &tree, "-p", &tip.to_string(), "-m", &message],
    )?;
    commit.ensure_success()?;
    let commit = commit.stdout_str().trim().to_string();

    let reflog = format!("apply-patch: {}", message.lines().next().unwrap_or(""));
    let edited = repo.edit_reference(RefEdit {
        change: Change::Update {
            log: LogChange {
                mode: RefLog::AndReference,
                force_create_reflog: false,
                message: reflog.into(),
            },
            expected: PreviousValue::MustExistAndMatch(gix::refs::Target::Object(tip)),
            new: gix::refs::Target::Object(
                gix::ObjectId::from_hex(commit.as_bytes())
                    .context("git commit-tree returned an unparseable id")?,
            ),
        },
        name: format!("refs/heads/{}", request.branch)
            .as_str()
            .try_into()
            .map_err(|error| anyhow::anyhow!("invalid branch ref: {error}"))?,
        deref: false,
    });
    if edited.is_err() {
        // The branch moved between the read and the write.
        let actual = branch_tip(&repo, request.branch).map(|id| id.to_string());
        return Ok(CommitOutcome::TipAdvanced { actual });
    }

    Ok(CommitOutcome::Committed {
        commit,
        tree,
        tip_before: tip.to_string(),
        files_changed: changes.len(),
        changes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_git(git: &GitCommandGateway, args: &[&str], cwd: &Path) {
        git.run(args, Some(cwd))
            .expect("git runs")
            .ensure_success()
            .expect("git succeeds");
    }

    fn git_out(git: &GitCommandGateway, args: &[&str], cwd: &Path) -> String {
        git.run(args, Some(cwd))
            .expect("git runs")
            .stdout_str()
            .trim()
            .to_string()
    }

    /// A bare repository holding `files` as one commit on `main`.
    fn bare_repo() -> (tempfile::TempDir, GitCommandGateway, String) {
        let temp = tempfile::tempdir().unwrap();
        let work = temp.path().join("work");
        std::fs::create_dir(&work).unwrap();
        let git = GitCommandGateway::new().expect("git CLI available");
        run_git(&git, &["init", "-b", "main"], &work);
        run_git(&git, &["config", "user.email", "tester@example.com"], &work);
        run_git(&git, &["config", "user.name", "Tester"], &work);
        std::fs::write(work.join("a.txt"), "one\n").unwrap();
        run_git(&git, &["add", "-A"], &work);
        run_git(&git, &["commit", "-m", "seed"], &work);
        let bare = temp.path().join("repo.git");
        run_git(
            &git,
            &[
                "clone",
                "--bare",
                work.to_str().unwrap(),
                bare.to_str().unwrap(),
            ],
            temp.path(),
        );
        let tip = git_out(&git, &["rev-parse", "refs/heads/main"], &bare);
        (temp, git, tip)
    }

    fn request<'a>(patch: &'a str, tip: &'a str) -> PatchRequest<'a> {
        PatchRequest {
            branch: "main",
            expected_tip: tip,
            patch,
            message: None,
            identity_name: "Patcher",
            identity_email: "patcher@example.com",
        }
    }

    /// Modify `a.txt` and add `b.txt`.
    const ADD_AND_MODIFY: &str = "\
diff --git a/a.txt b/a.txt
index 5626abf..f719efd 100644
--- a/a.txt
+++ b/a.txt
@@ -1 +1 @@
-one
+ONE
diff --git a/b.txt b/b.txt
new file mode 100644
index 0000000..b6fc4c6
--- /dev/null
+++ b/b.txt
@@ -0,0 +1 @@
+hello
";

    #[test]
    fn a_patch_lands_as_one_commit_on_top_of_the_expected_tip() {
        let (temp, git, tip) = bare_repo();
        let repo = temp.path().join("repo.git");

        let preview = preview_patch(&repo, &request(ADD_AND_MODIFY, &tip)).unwrap();
        let PreviewOutcome::Previewed(preview) = preview else {
            panic!("expected a clean preview, got {preview:?}");
        };
        assert_eq!(preview.tip_before, tip);
        assert_eq!(preview.files_changed, 2);
        assert_eq!(
            preview
                .changes
                .iter()
                .map(|c| (c.path.as_str(), c.status))
                .collect::<Vec<_>>(),
            vec![
                ("a.txt", FileStatus::Modified),
                ("b.txt", FileStatus::Added)
            ]
        );
        let contents: std::collections::BTreeMap<_, _> =
            preview.new_blobs.iter().cloned().collect();
        assert_eq!(contents["a.txt"], b"ONE\n".to_vec());
        assert_eq!(contents["b.txt"], b"hello\n".to_vec());
        // The preview wrote nothing: the branch has not moved and the scratch
        // tree is not in the repository.
        assert_eq!(git_out(&git, &["rev-parse", "refs/heads/main"], &repo), tip);
        assert!(!git
            .run(&["cat-file", "-e", &preview.tree], Some(&repo))
            .unwrap()
            .success());

        let outcome = commit_patch(&repo, &request(ADD_AND_MODIFY, &tip)).unwrap();
        let CommitOutcome::Committed {
            commit,
            tree,
            tip_before,
            files_changed,
            ..
        } = outcome
        else {
            panic!("expected a commit, got {outcome:?}");
        };
        assert_eq!(tip_before, tip);
        assert_eq!(files_changed, 2);
        assert_eq!(tree, preview.tree);
        assert_eq!(
            git_out(&git, &["rev-parse", "refs/heads/main"], &repo),
            commit
        );
        assert_eq!(
            git_out(&git, &["rev-parse", &format!("{commit}^")], &repo),
            tip
        );
        assert_eq!(
            git_out(&git, &["show", &format!("{commit}:a.txt")], &repo),
            "ONE"
        );
        assert_eq!(
            git_out(&git, &["show", &format!("{commit}:b.txt")], &repo),
            "hello"
        );
        assert_eq!(
            git_out(
                &git,
                &["log", "-1", "--format=%an <%ae>|%cn <%ce>", &commit],
                &repo
            ),
            "Patcher <patcher@example.com>|Patcher <patcher@example.com>"
        );
        assert_eq!(
            git_out(&git, &["log", "-1", "--format=%B", &commit], &repo),
            "Apply patch to main\n\n2 file(s) changed."
        );
    }

    #[test]
    fn a_custom_message_is_kept() {
        let (temp, git, tip) = bare_repo();
        let repo = temp.path().join("repo.git");
        let mut req = request(ADD_AND_MODIFY, &tip);
        req.message = Some("Rename one to ONE\n");
        let CommitOutcome::Committed { commit, .. } = commit_patch(&repo, &req).unwrap() else {
            panic!("expected a commit");
        };
        assert_eq!(
            git_out(&git, &["log", "-1", "--format=%B", &commit], &repo),
            "Rename one to ONE"
        );
    }

    #[test]
    fn a_patch_that_does_not_fit_is_a_conflict_and_writes_nothing() {
        let (temp, git, tip) = bare_repo();
        let repo = temp.path().join("repo.git");
        let objects_before = git_out(&git, &["count-objects", "-v"], &repo);
        let stale = "\
diff --git a/a.txt b/a.txt
--- a/a.txt
+++ b/a.txt
@@ -1 +1 @@
-nosuchline
+other
";

        let outcome = preview_patch(&repo, &request(stale, &tip)).unwrap();
        assert!(
            matches!(&outcome, PreviewOutcome::Conflict { paths, tip: at } if paths == &vec!["a.txt".to_string()] && at == &tip),
            "got {outcome:?}"
        );
        // A real run refuses the same patch without leaving a blob behind: the
        // conflict is detected before anything is written to the repository.
        let outcome = commit_patch(&repo, &request(stale, &tip)).unwrap();
        assert!(
            matches!(outcome, CommitOutcome::Conflict { .. }),
            "got {outcome:?}"
        );
        assert_eq!(git_out(&git, &["rev-parse", "refs/heads/main"], &repo), tip);
        assert_eq!(
            git_out(&git, &["count-objects", "-v"], &repo),
            objects_before
        );
    }

    #[test]
    fn a_patch_that_changes_nothing_is_empty() {
        let (temp, _git, tip) = bare_repo();
        let repo = temp.path().join("repo.git");
        // Replace a line with itself: git applies it, the tree does not change.
        let noop = "\
diff --git a/a.txt b/a.txt
index 5626abf..5626abf 100644
--- a/a.txt
+++ b/a.txt
@@ -1 +1 @@
-one
+one
";
        let outcome = preview_patch(&repo, &request(noop, &tip)).unwrap();
        assert!(
            matches!(outcome, PreviewOutcome::Empty { .. }),
            "got {outcome:?}"
        );
    }

    #[test]
    fn a_stale_expected_tip_and_a_missing_branch_are_ref_advanced() {
        let (temp, git, tip) = bare_repo();
        let repo = temp.path().join("repo.git");
        git.run(&["update-ref", "refs/heads/main", &tip], Some(&repo))
            .unwrap();
        // A tip that is not the branch head.
        let other = git_out(&git, &["rev-parse", &format!("{tip}^")], &repo);
        let outcome = preview_patch(&repo, &request(ADD_AND_MODIFY, &other)).unwrap();
        assert!(
            matches!(&outcome, PreviewOutcome::TipAdvanced { actual: Some(actual) } if actual == &tip),
            "got {outcome:?}"
        );
        // A branch that does not exist is never created.
        let mut req = request(ADD_AND_MODIFY, &tip);
        req.branch = "nope";
        let outcome = commit_patch(&repo, &req).unwrap();
        assert!(
            matches!(outcome, CommitOutcome::TipAdvanced { actual: None }),
            "got {outcome:?}"
        );
        assert!(!git
            .run(&["rev-parse", "--verify", "refs/heads/nope"], Some(&repo))
            .unwrap()
            .success());
    }

    #[test]
    fn paths_outside_the_repository_are_refused() {
        let (temp, git, tip) = bare_repo();
        let repo = temp.path().join("repo.git");
        let objects_before = git_out(&git, &["count-objects", "-v"], &repo);
        for patch in [
            "diff --git a/a.txt b/../../evil.txt\n--- a/a.txt\n+++ b/../../evil.txt\n@@ -1 +1 @@\n-one\n+evil\n",
            "diff --git a/a.txt b//etc/passwd\n--- a/a.txt\n+++ b//etc/passwd\n@@ -1 +1 @@\n-one\n+evil\n",
            "diff --git a/a.txt b/.git/config\nnew file mode 100644\n--- /dev/null\n+++ b/.git/config\n@@ -0,0 +1 @@\n+x\n",
            "diff --git a/a.txt b/sub/.GIT/hooks/pre-commit\nnew file mode 100755\n--- /dev/null\n+++ b/sub/.GIT/hooks/pre-commit\n@@ -0,0 +1 @@\n+x\n",
            "diff --git a/a.txt b/x/../y\n--- a/a.txt\n+++ b/x/../y\n@@ -1 +1 @@\n-one\n+evil\n",
        ] {
            let outcome = preview_patch(&repo, &request(patch, &tip)).unwrap();
            assert!(
                matches!(outcome, PreviewOutcome::Invalid { .. }),
                "patch {patch:?} must be refused, got {outcome:?}"
            );
        }
        assert_eq!(git_out(&git, &["rev-parse", "refs/heads/main"], &repo), tip);
        assert_eq!(
            git_out(&git, &["count-objects", "-v"], &repo),
            objects_before
        );
    }

    #[test]
    fn a_patch_over_the_limits_is_refused() {
        let (temp, _git, tip) = bare_repo();
        let repo = temp.path().join("repo.git");

        // Above MAX_PATCH_BYTES.
        let huge = format!(
            "diff --git a/huge.txt b/huge.txt\nnew file mode 100644\n--- /dev/null\n+++ b/huge.txt\n@@ -0,0 +1 @@\n+{}\n",
            "x".repeat(MAX_PATCH_BYTES)
        );
        let outcome = preview_patch(&repo, &request(&huge, &tip)).unwrap();
        assert!(
            matches!(&outcome, PreviewOutcome::Invalid { reason } if reason.contains("limit")),
            "got {outcome:?}"
        );

        // Above MAX_PATCH_FILES.
        let mut many = String::new();
        for index in 0..=MAX_PATCH_FILES {
            many.push_str(&format!(
                "diff --git a/f{index}.txt b/f{index}.txt\nnew file mode 100644\n--- /dev/null\n+++ b/f{index}.txt\n@@ -0,0 +1 @@\n+{index}\n"
            ));
        }
        assert!(many.len() <= MAX_PATCH_BYTES);
        let outcome = preview_patch(&repo, &request(&many, &tip)).unwrap();
        assert!(
            matches!(&outcome, PreviewOutcome::Invalid { reason } if reason.contains("file limit")),
            "got {outcome:?}"
        );

        // Not a diff at all, and an empty one.
        for patch in ["not a patch\n", "", "   \n"] {
            let outcome = preview_patch(&repo, &request(patch, &tip)).unwrap();
            assert!(
                matches!(outcome, PreviewOutcome::Invalid { .. }),
                "patch {patch:?} must be refused, got {outcome:?}"
            );
        }
    }

    #[test]
    fn deletes_renames_and_binary_patches_are_supported() {
        let temp = tempfile::tempdir().unwrap();
        let work = temp.path().join("work");
        std::fs::create_dir(&work).unwrap();
        let git = GitCommandGateway::new().unwrap();
        run_git(&git, &["init", "-b", "main"], &work);
        run_git(&git, &["config", "user.email", "tester@example.com"], &work);
        run_git(&git, &["config", "user.name", "Tester"], &work);
        std::fs::write(work.join("old.txt"), "bye\n").unwrap();
        std::fs::write(work.join("binary.bin"), [0u8, 1, 2, 3]).unwrap();
        run_git(&git, &["add", "-A"], &work);
        run_git(&git, &["commit", "-m", "seed"], &work);
        let base = git_out(&git, &["rev-parse", "HEAD"], &work);

        // Delete one file, rename another, and replace a binary blob.
        std::fs::remove_file(work.join("old.txt")).unwrap();
        std::fs::write(work.join("binary.bin"), [0u8, 9, 9, 9, 9]).unwrap();
        run_git(&git, &["add", "-A"], &work);
        // Not `git_out`: a patch ends in a newline, and trimming it makes git
        // report a corrupt hunk.
        let patch = git
            .run(
                &["diff", "--binary", &base, "--", "old.txt", "binary.bin"],
                Some(&work),
            )
            .expect("git runs")
            .stdout_str();
        run_git(&git, &["reset", "--hard"], &work);

        let bare = temp.path().join("repo.git");
        run_git(
            &git,
            &[
                "clone",
                "--bare",
                work.to_str().unwrap(),
                bare.to_str().unwrap(),
            ],
            temp.path(),
        );
        let tip = git_out(&git, &["rev-parse", "refs/heads/main"], &bare);
        let outcome = commit_patch(&bare, &request(&patch, &tip)).unwrap();
        let CommitOutcome::Committed {
            commit, changes, ..
        } = outcome
        else {
            panic!("expected a commit, got {outcome:?}");
        };
        assert_eq!(
            changes
                .iter()
                .map(|c| (c.path.as_str(), c.status))
                .collect::<Vec<_>>(),
            vec![
                ("binary.bin", FileStatus::Modified),
                ("old.txt", FileStatus::Deleted)
            ]
        );
        assert_eq!(
            git_out(&git, &["ls-tree", "--name-only", &commit], &bare),
            "binary.bin"
        );
        let blob = git
            .run(
                &["cat-file", "blob", &format!("{commit}:binary.bin")],
                Some(&bare),
            )
            .unwrap();
        assert_eq!(blob.stdout, vec![0u8, 9, 9, 9, 9]);
    }
}
