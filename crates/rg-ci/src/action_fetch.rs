//! Third-party Action resolution: reference parsing, fetching, and
//! `action.yml` parsing.
//!
//! This module is deliberately a **read side** only. It fetches an Action's
//! source so the workflow converter can expand a `composite` Action into plain
//! steps; it never executes anything. Node and Docker Actions are recognised
//! here and then refused by the caller, because executing them needs the
//! `actions-v1` runner described by ADR-0001.
//!
//! # What is supported
//!
//! | `uses` form | Behaviour |
//! |---|---|
//! | `actions/checkout@*`, `actions/cache@*` | Not resolved here — the converter keeps its existing built-in handling |
//! | `./local/path` | Read from the workflow source (a checkout directory, or the commit tree of the bare repository) |
//! | `owner/repo[/subdir]@ref` | Read from `{cache_root}/{owner}/{repo}/{ref}/{subdir}`, cloning once on a miss |
//! | `docker://image` | Recognised, then refused |
//!
//! # Cache
//!
//! Remote Actions are cloned to `{cache_root}/{owner}/{repo}/{ref}/`. A cache
//! hit is "the directory exists and contains `action.yml` or `action.yaml`" —
//! the same predicate GitHub applies to a checked-out Action. `ref` may contain
//! `/` (branch names); any `..` segment is rejected before it reaches the
//! filesystem, and owner/repo/ref characters are validated so a reference can
//! never be read as a command-line option or a path outside the cache.
//!
//! # Failure classification
//!
//! Cloning distinguishes "the ref does not exist" from "the network failed",
//! because the two need different user actions (fix the workflow vs. retry).
//! See [`ActionErrorKind`].

use anyhow::Result;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::gitea_actions::GiteaStep;

/// Runner capability that a third-party Action needs (ADR-0001 §2.1).
///
/// The runner-side reporting mechanism does not exist yet: the `runners` table
/// carries `labels`/`os`/`arch` only. This type is the read side — a label set
/// can be interpreted, and a manifest can be asked what it needs — so the
/// scheduling work later has a fixed interface to consume.
pub const ACTIONS_V1_CAPABILITY: &str = "actions-v1";

/// Maximum composite nesting depth. The first Action invoked from a workflow is
/// depth 1; a sixth level is refused instead of being expanded.
pub const MAX_COMPOSITE_DEPTH: usize = 5;

/// The workspace path the runner mounts the repository at (see
/// `crates/rg-ci/src/runner.rs`). Used to make `github.action_path` point at a
/// path that exists inside the job container.
pub const WORKSPACE_ROOT: &str = "/workspace";

/// Manifest file names, in the order GitHub looks for them.
pub const MANIFEST_NAMES: [&str; 2] = ["action.yml", "action.yaml"];

/// Capabilities a job needs from a runner.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RunnerCapabilities {
    /// The job contains an Action that must run under the `actions-v1`
    /// executor (Node, Docker, or an unknown runtime).
    pub actions_v1: bool,
}

impl RunnerCapabilities {
    /// Interpret a runner's label list. Until the runner reports structured
    /// capabilities, an `actions-v1` label is the only way to advertise the
    /// executor, and no schema change is needed for it.
    pub fn from_labels<I, S>(labels: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        Self {
            actions_v1: labels
                .into_iter()
                .any(|label| label.as_ref().eq_ignore_ascii_case(ACTIONS_V1_CAPABILITY)),
        }
    }
}

/// Why an Action could not be resolved, expanded, or executed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionErrorKind {
    /// The Action is valid but needs a runtime IronForge does not have yet
    /// (Node, Docker, unknown `runs.using`).
    Unsupported,
    /// The `uses` string (or a path/ref inside it) is malformed or escapes the
    /// allowed root.
    InvalidReference,
    /// The Action directory has no `action.yml` / `action.yaml`.
    ManifestMissing,
    /// The manifest is valid YAML but not a valid Action definition.
    ManifestInvalid,
    /// The remote exists but the requested ref does not.
    RefNotFound,
    /// The remote could not be reached.
    Network,
    /// Fetching failed for another reason.
    Fetch,
    /// Reading the local workflow source failed.
    Io,
    /// A composite Action (indirectly) invokes itself.
    Cycle,
    /// Composite nesting exceeded [`MAX_COMPOSITE_DEPTH`].
    DepthLimit,
}

/// An error with a machine-readable kind.
///
/// Returned inside `anyhow::Error`; use [`ActionError::kind`] via
/// `error.downcast_ref::<ActionError>()` when the distinction matters.
#[derive(Debug, Clone)]
pub struct ActionError {
    kind: ActionErrorKind,
    message: String,
}

impl ActionError {
    pub fn new(kind: ActionErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    pub fn kind(&self) -> ActionErrorKind {
        self.kind
    }
}

impl fmt::Display for ActionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ActionError {}

/// Convenience wrapper so call sites can stay `?`-shaped.
fn fail(kind: ActionErrorKind, message: impl Into<String>) -> anyhow::Error {
    anyhow::Error::new(ActionError::new(kind, message))
}

/// `actions/checkout` and `actions/cache` keep their built-in adapters.
///
/// The prefixes match the converter's existing checks exactly: a workflow that
/// worked before must not start being resolved as a third-party Action.
pub fn is_builtin_action(uses: &str) -> bool {
    uses.starts_with("actions/checkout") || uses.starts_with("actions/cache@")
}

/// A parsed `uses:` reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActionReference {
    /// Handled by the converter's built-in adapters.
    Builtin { uses: String },
    /// `./path`, relative to the workflow source root.
    Local { path: String },
    /// `owner/repo[/subdir]@ref`.
    Remote {
        owner: String,
        repo: String,
        subdir: String,
        git_ref: String,
    },
    /// `docker://image`.
    Docker { image: String },
}

impl ActionReference {
    /// Parse a `uses:` value.
    ///
    /// Empty or malformed references are errors rather than "unknown action":
    /// a workflow that says `uses: something` without a ref is broken, and
    /// silently treating it as a no-op is the failure mode ADR-0001 forbids.
    pub fn parse(uses: &str) -> Result<Self> {
        let uses = uses.trim();
        if uses.is_empty() {
            return Err(fail(ActionErrorKind::InvalidReference, "empty `uses:`"));
        }
        if is_builtin_action(uses) {
            return Ok(Self::Builtin {
                uses: uses.to_string(),
            });
        }
        if let Some(image) = uses.strip_prefix("docker://") {
            if image.trim().is_empty() {
                return Err(fail(
                    ActionErrorKind::InvalidReference,
                    "`docker://` reference has no image",
                ));
            }
            return Ok(Self::Docker {
                image: image.to_string(),
            });
        }
        if uses.starts_with("./") || uses == "." {
            let path = normalize_relative(uses)?.ok_or_else(|| {
                fail(
                    ActionErrorKind::InvalidReference,
                    format!("action reference '{uses}' points at the source root, not an action"),
                )
            })?;
            return Ok(Self::Local { path });
        }

        let Some((target, git_ref)) = uses.rsplit_once('@') else {
            return Err(fail(
                ActionErrorKind::InvalidReference,
                format!("action '{uses}' has no @ref; use owner/repo@ref"),
            ));
        };
        if git_ref.is_empty() {
            return Err(fail(
                ActionErrorKind::InvalidReference,
                format!("action '{uses}' has an empty @ref"),
            ));
        }
        validate_ref(git_ref).map_err(|reason| {
            fail(
                ActionErrorKind::InvalidReference,
                format!("action '{uses}' has an invalid ref: {reason}"),
            )
        })?;

        let mut segments = target.split('/');
        let owner = segments.next().unwrap_or_default();
        let repo = segments.next().unwrap_or_default();
        let subdir = segments.collect::<Vec<_>>().join("/");
        for (label, value) in [("owner", owner), ("repository", repo)] {
            if value.is_empty() {
                return Err(fail(
                    ActionErrorKind::InvalidReference,
                    format!("action '{uses}' is missing a {label}"),
                ));
            }
            if !value
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
            {
                return Err(fail(
                    ActionErrorKind::InvalidReference,
                    format!("action '{uses}' has an invalid {label} '{value}'"),
                ));
            }
        }
        let subdir = if subdir.is_empty() {
            String::new()
        } else {
            normalize_relative(&subdir)?.ok_or_else(|| {
                fail(
                    ActionErrorKind::InvalidReference,
                    format!("action '{uses}' has an empty subdirectory"),
                )
            })?
        };
        Ok(Self::Remote {
            owner: owner.to_string(),
            repo: repo.to_string(),
            subdir,
            git_ref: git_ref.to_string(),
        })
    }
}

/// Reject absolute paths and `..`; collapse `.` and empty segments.
fn normalize_relative(input: &str) -> Result<Option<String>> {
    let replaced = input.replace('\\', "/");
    if replaced.starts_with('/') || Path::new(&replaced).is_absolute() {
        return Err(fail(
            ActionErrorKind::InvalidReference,
            format!("'{input}' must be relative to the workflow source"),
        ));
    }
    let mut parts = Vec::new();
    for part in replaced.split('/') {
        match part {
            "" | "." => continue,
            ".." => {
                return Err(fail(
                    ActionErrorKind::InvalidReference,
                    format!("'{input}' escapes its root with '..'"),
                ))
            }
            other => parts.push(other),
        }
    }
    Ok((!parts.is_empty()).then(|| parts.join("/")))
}

/// Validate a git ref that will be interpolated into a clone command and a
/// cache path.
fn validate_ref(git_ref: &str) -> std::result::Result<(), &'static str> {
    if git_ref.starts_with('-') {
        return Err("must not start with '-'");
    }
    if git_ref.split('/').any(|part| part == "..") {
        return Err("must not contain '..'");
    }
    if git_ref
        .chars()
        .any(|c| c.is_whitespace() || matches!(c, '\\' | ':' | '~' | '^' | '*' | '?' | '['))
    {
        return Err("contains a character git does not allow (space, colon, wildcard, ...)");
    }
    Ok(())
}

/// A readable Action source tree.
///
/// Paths passed to [`read`](ActionFileSource::read) are always relative,
/// normalised, and already checked for `..`; implementations must still verify
/// that the path stays inside their root (a symlink can point outside).
pub trait ActionFileSource: Send + Sync {
    /// Read a file, or `None` when it does not exist.
    fn read(&self, relative: &str) -> std::result::Result<Option<Vec<u8>>, ActionError>;

    /// A source rooted at the same place: used when a composite Action's nested
    /// `./path` reference must resolve against the workspace root rather than
    /// the Action's own directory.
    fn root(&self) -> Box<dyn ActionFileSource>;

    /// Human-readable location, used in errors.
    fn location(&self) -> String;

    /// Stable identity, used for cycle detection.
    fn identity(&self) -> String;
}

/// Reads from a directory tree (a checkout, a cache entry, or a test fixture).
#[derive(Debug, Clone)]
pub struct DirectorySource {
    root: PathBuf,
}

impl DirectorySource {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    fn canonical_root(&self) -> PathBuf {
        std::fs::canonicalize(&self.root).unwrap_or_else(|_| self.root.clone())
    }
}

impl ActionFileSource for DirectorySource {
    fn read(&self, relative: &str) -> std::result::Result<Option<Vec<u8>>, ActionError> {
        let full = self.root.join(relative);
        if !full.is_file() {
            return Ok(None);
        }
        // Defense in depth: `..` is already rejected, but a symlink inside the
        // tree could still point outside the root.
        let canonical = std::fs::canonicalize(&full).map_err(|error| {
            ActionError::new(
                ActionErrorKind::Io,
                format!("failed to resolve '{}': {error}", self.location()),
            )
        })?;
        if !canonical.starts_with(self.canonical_root()) {
            return Err(ActionError::new(
                ActionErrorKind::InvalidReference,
                format!("'{}' resolves outside '{}'", relative, self.location()),
            ));
        }
        std::fs::read(&full).map(Some).map_err(|error| {
            ActionError::new(
                ActionErrorKind::Io,
                format!("failed to read '{}': {error}", self.location()),
            )
        })
    }

    fn root(&self) -> Box<dyn ActionFileSource> {
        Box::new(self.clone())
    }

    fn location(&self) -> String {
        self.root.display().to_string()
    }

    fn identity(&self) -> String {
        format!("dir:{}", self.canonical_root().display())
    }
}

/// Reads from a commit in a git repository (bare or not) via `git show`.
///
/// The server keeps bare repositories, not checkouts; reading the Action from
/// the same immutable commit the workflow came from is what makes expansion
/// reproducible and keeps it out of the working tree.
#[derive(Debug, Clone)]
pub struct GitTreeSource {
    repo_path: PathBuf,
    commit: String,
    dir: String,
}

impl GitTreeSource {
    pub fn new(repo_path: impl Into<PathBuf>, commit: impl Into<String>) -> Self {
        Self {
            repo_path: repo_path.into(),
            commit: commit.into(),
            dir: String::new(),
        }
    }

    fn spec(&self, relative: &str) -> String {
        if self.dir.is_empty() {
            format!("{}:{relative}", self.commit)
        } else {
            format!("{}:{}/{relative}", self.commit, self.dir)
        }
    }
}

impl ActionFileSource for GitTreeSource {
    fn read(&self, relative: &str) -> std::result::Result<Option<Vec<u8>>, ActionError> {
        let spec = self.spec(relative);
        // `git show <rev>:<path>`. The commit is a SHA from the pipeline, and
        // the path is normalised; neither can start with '-'.
        let output = Command::new("git")
            .arg("-C")
            .arg(&self.repo_path)
            .args(["show", &spec])
            .output()
            .map_err(|error| {
                ActionError::new(
                    ActionErrorKind::Io,
                    format!("failed to run git while reading actions: {error}"),
                )
            })?;
        if output.status.success() {
            return Ok(Some(output.stdout));
        }
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("does not exist")
            || stderr.contains("exists on disk, but not in")
            || stderr.contains("not in")
        {
            return Ok(None);
        }
        Err(ActionError::new(
            ActionErrorKind::Io,
            format!("failed to read '{spec}' from the workflow commit: {stderr}"),
        ))
    }

    fn root(&self) -> Box<dyn ActionFileSource> {
        Box::new(Self {
            repo_path: self.repo_path.clone(),
            commit: self.commit.clone(),
            dir: String::new(),
        })
    }

    fn location(&self) -> String {
        if self.dir.is_empty() {
            format!("{}:", self.commit)
        } else {
            format!("{}:{}", self.commit, self.dir)
        }
    }

    fn identity(&self) -> String {
        format!("git:{}:{}", self.commit, self.dir)
    }
}

/// An Action resolved to a readable source plus its parsed manifest.
pub struct ResolvedAction {
    /// The original `uses:` string, for error messages.
    pub reference: String,
    /// Value substituted for `${{ github.action_path }}`.
    pub action_path: String,
    /// Stable identity for cycle detection.
    pub identity: String,
    /// Parsed `action.yml`.
    pub manifest: ActionManifest,
    /// Root for resolving nested `./path` references inside this Action.
    pub child_source: Box<dyn ActionFileSource>,
}

impl fmt::Debug for ResolvedAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ResolvedAction")
            .field("reference", &self.reference)
            .field("action_path", &self.action_path)
            .field("manifest", &self.manifest)
            .finish()
    }
}

/// Resolves `uses:` references for one workflow.
pub struct ActionFetcher {
    source: Box<dyn ActionFileSource>,
    cache_root: PathBuf,
    remote_base: Option<String>,
}

impl ActionFetcher {
    /// Resolve `./path` references inside a directory tree.
    pub fn for_directory(root: impl Into<PathBuf>, cache_root: impl Into<PathBuf>) -> Self {
        Self {
            source: Box::new(DirectorySource::new(root)),
            cache_root: cache_root.into(),
            remote_base: None,
        }
    }

    /// Resolve `./path` references from a commit of a repository on disk.
    pub fn for_git_commit(
        repo_path: impl Into<PathBuf>,
        commit: impl Into<String>,
        cache_root: impl Into<PathBuf>,
    ) -> Self {
        Self {
            source: Box::new(GitTreeSource::new(repo_path, commit)),
            cache_root: cache_root.into(),
            remote_base: None,
        }
    }

    /// Enable remote fetching. Without a base URL, an uncached remote Action is
    /// refused instead of being fetched from an implicit host (ADR-0001 §2.3).
    pub fn with_remote_base(mut self, base: impl Into<String>) -> Self {
        let base = base.into();
        self.remote_base = Some(base.trim_end_matches('/').to_string());
        self
    }

    pub fn cache_root(&self) -> &Path {
        &self.cache_root
    }

    /// Resolve a `uses:` reference against the workflow source root.
    pub fn resolve(&self, uses: &str) -> Result<ResolvedAction> {
        self.resolve_within(self.source.root().as_ref(), uses)
    }

    /// Resolve a `uses:` reference against an explicit root.
    ///
    /// `parent` is the root a nested `./path` is relative to: the workspace for
    /// a repository-local Action, or the checkout of a remote Action.
    pub fn resolve_within(&self, parent: &dyn ActionFileSource, uses: &str) -> Result<ResolvedAction> {
        match ActionReference::parse(uses)? {
            ActionReference::Builtin { .. } => Err(fail(
                ActionErrorKind::InvalidReference,
                format!("'{uses}' is a built-in action and must not be resolved as a third-party action"),
            )),
            ActionReference::Docker { image } => Err(fail(
                ActionErrorKind::Unsupported,
                format!(
                    "action '{uses}' runs a Docker container (image '{image}'); Docker actions \
require an {ACTIONS_V1_CAPABILITY} runner, which is not implemented"
                ),
            )),
            ActionReference::Local { path } => {
                let manifest = read_manifest(parent, &path)?;
                Ok(ResolvedAction {
                    reference: uses.to_string(),
                    action_path: format!("{WORKSPACE_ROOT}/{path}"),
                    identity: format!("{}:{path}", parent.identity()),
                    manifest,
                    child_source: parent.root(),
                })
            }
            ActionReference::Remote {
                owner,
                repo,
                subdir,
                git_ref,
            } => {
                let dir = self.ensure_remote(&owner, &repo, &git_ref, &subdir)?;
                let source = DirectorySource::new(dir);
                let manifest = read_manifest(&source, "")?;
                Ok(ResolvedAction {
                    reference: uses.to_string(),
                    action_path: source.location(),
                    identity: source.identity(),
                    manifest,
                    child_source: Box::new(source),
                })
            }
        }
    }

    /// Return the directory containing a remote Action's manifest, cloning the
    /// repository on a miss.
    ///
    /// `subdir` is part of the hit test: `owner/repo/sub@ref` keeps its manifest
    /// in the checkout's `sub/`, so testing the repository root would re-clone
    /// the same commit on every run.
    fn ensure_remote(
        &self,
        owner: &str,
        repo: &str,
        git_ref: &str,
        subdir: &str,
    ) -> Result<PathBuf> {
        let root = self.cache_root.join(owner).join(repo).join(git_ref);
        let action_dir = if subdir.is_empty() {
            root.clone()
        } else {
            root.join(subdir)
        };
        if manifest_file(&action_dir).is_some() {
            return Ok(action_dir);
        }
        if root.is_dir() {
            // Already cloned at some point: the reference simply points at a
            // directory without a manifest. Re-cloning cannot fix that.
            return Err(fail(
                ActionErrorKind::ManifestMissing,
                if subdir.is_empty() {
                    format!("action '{owner}/{repo}@{git_ref}' has no action.yml or action.yaml")
                } else {
                    format!(
                        "action '{owner}/{repo}/{subdir}@{git_ref}' has no action.yml or action.yaml"
                    )
                },
            ));
        }

        let Some(base) = self.remote_base.as_deref() else {
            return Err(fail(
                ActionErrorKind::Unsupported,
                format!(
                    "action '{owner}/{repo}@{git_ref}' is not cached and no remote action source is \
configured; remote actions require an {ACTIONS_V1_CAPABILITY} runner, which is not implemented"
                ),
            ));
        };

        let url = format!("{base}/{owner}/{repo}.git");
        let tmp = self.cache_root.join(".tmp").join(format!(
            "{owner}-{repo}-{}-{}",
            sanitize_for_temp(git_ref),
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&tmp);
        if let Some(parent) = tmp.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                fail(ActionErrorKind::Io, format!("failed to create the action cache: {error}"))
            })?;
        }

        let output = Command::new("git")
            .env("GIT_TERMINAL_PROMPT", "0")
            .args(["clone", "--quiet", "--depth", "1", "--single-branch", "--branch"])
            .arg(git_ref)
            .arg(&url)
            .arg(&tmp)
            .output()
            .map_err(|error| {
                fail(
                    ActionErrorKind::Fetch,
                    format!("failed to run git while fetching '{owner}/{repo}@{git_ref}': {error}"),
                )
            })?;

        if !output.status.success() {
            let _ = std::fs::remove_dir_all(&tmp);
            let stderr = String::from_utf8_lossy(&output.stderr);
            let kind = classify_clone_error(&stderr);
            return Err(fail(kind, clone_error_message(kind, owner, repo, git_ref)));
        }

        if let Some(parent) = root.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        match std::fs::rename(&tmp, &root) {
            Ok(()) => {}
            // Another pipeline won the race; its copy is as good as ours.
            Err(_) if root.is_dir() => {
                let _ = std::fs::remove_dir_all(&tmp);
            }
            Err(error) => {
                let _ = std::fs::remove_dir_all(&tmp);
                return Err(fail(
                    ActionErrorKind::Fetch,
                    format!("failed to store '{owner}/{repo}@{git_ref}' in the action cache: {error}"),
                ));
            }
        }

        if manifest_file(&action_dir).is_none() {
            return Err(fail(
                ActionErrorKind::ManifestMissing,
                if subdir.is_empty() {
                    format!("action '{owner}/{repo}@{git_ref}' has no action.yml or action.yaml")
                } else {
                    format!(
                        "action '{owner}/{repo}/{subdir}@{git_ref}' has no action.yml or action.yaml"
                    )
                },
            ));
        }
        Ok(action_dir)
    }
}

fn sanitize_for_temp(git_ref: &str) -> String {
    git_ref
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}

fn classify_clone_error(stderr: &str) -> ActionErrorKind {
    let stderr = stderr.to_ascii_lowercase();
    // Network first: a DNS failure message can also contain "could not read".
    if stderr.contains("could not resolve host")
        || stderr.contains("could not resolve hostname")
        || stderr.contains("connection refused")
        || stderr.contains("connection timed out")
        || stderr.contains("network is unreachable")
        || stderr.contains("unable to access")
        || stderr.contains("failed to connect")
    {
        return ActionErrorKind::Network;
    }
    if stderr.contains("remote branch")
        || stderr.contains("couldn't find remote ref")
        || stderr.contains("not found in upstream")
        || stderr.contains("no such remote branch")
    {
        return ActionErrorKind::RefNotFound;
    }
    ActionErrorKind::Fetch
}

fn clone_error_message(kind: ActionErrorKind, owner: &str, repo: &str, git_ref: &str) -> String {
    match kind {
        ActionErrorKind::Network => format!(
            "could not reach the source of action '{owner}/{repo}@{git_ref}': network error; retry \
when the remote is reachable"
        ),
        ActionErrorKind::RefNotFound => format!(
            "action '{owner}/{repo}@{git_ref}' does not exist: ref '{git_ref}' was not found"
        ),
        _ => format!("failed to fetch action '{owner}/{repo}@{git_ref}'"),
    }
}

fn manifest_file(dir: &Path) -> Option<PathBuf> {
    MANIFEST_NAMES
        .iter()
        .map(|name| dir.join(name))
        .find(|candidate| candidate.is_file())
}

fn read_manifest(source: &dyn ActionFileSource, dir: &str) -> Result<ActionManifest> {
    for name in MANIFEST_NAMES {
        let relative = if dir.is_empty() {
            name.to_string()
        } else {
            format!("{dir}/{name}")
        };
        if let Some(bytes) = source
            .read(&relative)
            .map_err(anyhow::Error::new)?
        {
            let text = String::from_utf8(bytes).map_err(|_| {
                fail(
                    ActionErrorKind::ManifestInvalid,
                    format!("'{relative}' in '{}' is not valid UTF-8", source.location()),
                )
            })?;
            return ActionManifest::parse(&text).map_err(|error| {
                fail(
                    ActionErrorKind::ManifestInvalid,
                    format!("'{relative}' in '{}' is not a valid action manifest: {error}", source.location()),
                )
            });
        }
    }
    Err(fail(
        ActionErrorKind::ManifestMissing,
        if dir.is_empty() {
            format!("no action.yml or action.yaml in '{}'", source.location())
        } else {
            format!("no action.yml or action.yaml in '{}/{dir}'", source.location())
        },
    ))
}

/// One declared input of an Action.
#[derive(Debug, Clone, Deserialize)]
pub struct ActionInput {
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub required: bool,
    #[serde(default, deserialize_with = "deserialize_scalar_default")]
    pub default: Option<String>,
}

/// One declared output of an Action.
#[derive(Debug, Clone, Deserialize)]
pub struct ActionOutput {
    #[serde(default)]
    pub description: Option<String>,
    /// Expression yielding the output value, usually
    /// `${{ steps.<id>.outputs.<name> }}`.
    #[serde(default)]
    pub value: Option<String>,
}

fn deserialize_scalar_default<'de, D>(
    deserializer: D,
) -> std::result::Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<rg_core::yaml::Value>::deserialize(deserializer)?;
    Ok(value.and_then(|value| match value {
        rg_core::yaml::Value::String(text) => Some(text),
        rg_core::yaml::Value::Bool(flag) => Some(flag.to_string()),
        rg_core::yaml::Value::Number(number) => Some(number.to_string()),
        _ => None,
    }))
}

/// How an Action declares it runs.
#[derive(Debug, Clone)]
pub enum ActionRuns {
    /// Pure YAML: expandable into the calling job's steps.
    Composite { steps: Vec<GiteaStep> },
    /// JavaScript Action; needs a Node runtime.
    Node { using: String },
    /// Container Action; needs an image build/run step.
    Docker { image: String },
    /// Anything else (`go`, plugin binaries, ...).
    Other { using: String },
}

impl ActionRuns {
    /// The runtime label used in error messages.
    pub fn runtime(&self) -> &str {
        match self {
            Self::Composite { .. } => "composite",
            Self::Node { using } => using,
            Self::Docker { image } => image,
            Self::Other { using } => using,
        }
    }
}

/// A parsed `action.yml`.
#[derive(Debug, Clone)]
pub struct ActionManifest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub inputs: BTreeMap<String, ActionInput>,
    pub outputs: BTreeMap<String, ActionOutput>,
    pub runs: ActionRuns,
}

#[derive(Debug, Deserialize)]
struct RawManifest {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    inputs: BTreeMap<String, ActionInput>,
    #[serde(default)]
    outputs: BTreeMap<String, ActionOutput>,
    #[serde(default)]
    runs: Option<rg_core::yaml::Value>,
}

impl ActionManifest {
    /// Parse `action.yml` text.
    pub fn parse(yaml: &str) -> Result<Self> {
        let raw: RawManifest = rg_core::yaml::from_str(yaml)?;
        let runs = raw.runs.as_ref().ok_or_else(|| {
            fail(
                ActionErrorKind::ManifestInvalid,
                "the manifest has no `runs` section",
            )
        })?;
        let using = runs
            .get("using")
            .and_then(|value| value.as_str())
            .ok_or_else(|| {
                fail(
                    ActionErrorKind::ManifestInvalid,
                    "`runs.using` is required",
                )
            })?;

        let runs = match using {
            "composite" => {
                let steps = runs.get("steps").cloned().unwrap_or_else(|| {
                    rg_core::yaml::Value::Array(Vec::new())
                });
                let steps: Vec<GiteaStep> = serde_json::from_value(steps).map_err(|error| {
                    fail(
                        ActionErrorKind::ManifestInvalid,
                        format!("`runs.steps` is not a valid step list: {error}"),
                    )
                })?;
                ActionRuns::Composite { steps }
            }
            "docker" => {
                let image = runs
                    .get("image")
                    .and_then(|value| value.as_str())
                    .unwrap_or("Dockerfile")
                    .to_string();
                ActionRuns::Docker { image }
            }
            other if other.starts_with("node") => ActionRuns::Node {
                using: other.to_string(),
            },
            other => ActionRuns::Other {
                using: other.to_string(),
            },
        };

        Ok(Self {
            name: raw.name,
            description: raw.description,
            inputs: raw.inputs,
            outputs: raw.outputs,
            runs,
        })
    }

    /// Capabilities a job needs to run this Action.
    pub fn required_capabilities(&self) -> RunnerCapabilities {
        match self.runs {
            ActionRuns::Composite { .. } => RunnerCapabilities::default(),
            _ => RunnerCapabilities {
                actions_v1: true,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_prefixes_match_the_converter() {
        assert!(is_builtin_action("actions/checkout@v4"));
        assert!(is_builtin_action("actions/checkout"));
        assert!(is_builtin_action("actions/cache@v4"));
        assert!(!is_builtin_action("actions/cache-extra@v1"));
        assert!(!is_builtin_action("actions/setup-node@v4"));
    }

    #[test]
    fn references_are_parsed_into_their_four_shapes() {
        assert_eq!(
            ActionReference::parse("./a/b").unwrap(),
            ActionReference::Local {
                path: "a/b".into()
            }
        );
        assert_eq!(
            ActionReference::parse("owner/repo/sub@v1").unwrap(),
            ActionReference::Remote {
                owner: "owner".into(),
                repo: "repo".into(),
                subdir: "sub".into(),
                git_ref: "v1".into(),
            }
        );
        assert_eq!(
            ActionReference::parse("docker://alpine:3").unwrap(),
            ActionReference::Docker {
                image: "alpine:3".into()
            }
        );
        assert!(matches!(
            ActionReference::parse("actions/checkout@v4").unwrap(),
            ActionReference::Builtin { .. }
        ));
    }

    #[test]
    fn malformed_references_are_rejected() {
        for uses in [
            "",
            "owner/repo",
            "owner@",
            "./../escape",
            "/absolute",
            "owner/repo@../x",
            "owner/repo@-flag",
            "owner//repo@v1",
            "docker://",
        ] {
            assert!(
                ActionReference::parse(uses).is_err(),
                "'{uses}' should be rejected"
            );
        }
    }

    #[test]
    fn manifests_are_classified_by_runs_using() {
        let composite = ActionManifest::parse(
            "runs:\n  using: composite\n  steps:\n    - run: echo hi\n      shell: bash\n",
        )
        .unwrap();
        assert!(matches!(composite.runs, ActionRuns::Composite { .. }));
        assert!(!composite.required_capabilities().actions_v1);

        let node = ActionManifest::parse("runs:\n  using: node20\n  main: index.js\n").unwrap();
        assert!(matches!(node.runs, ActionRuns::Node { .. }));
        assert!(node.required_capabilities().actions_v1);

        let docker = ActionManifest::parse("runs:\n  using: docker\n  image: Dockerfile\n").unwrap();
        assert!(matches!(docker.runs, ActionRuns::Docker { .. }));
        assert!(docker.required_capabilities().actions_v1);

        let other = ActionManifest::parse("runs:\n  using: go\n").unwrap();
        assert!(matches!(other.runs, ActionRuns::Other { .. }));
        assert!(other.required_capabilities().actions_v1);
    }

    #[test]
    fn a_manifest_without_runs_using_is_invalid() {
        assert!(ActionManifest::parse("name: x\n").is_err());
        assert!(ActionManifest::parse("runs:\n  main: index.js\n").is_err());
    }

    #[test]
    fn input_defaults_accept_scalars() {
        let manifest = ActionManifest::parse(
            "inputs:\n  count:\n    default: 3\n  flag:\n    default: true\n  name:\n    default: hi\nruns:\n  using: composite\n  steps: []\n",
        )
        .unwrap();
        assert_eq!(manifest.inputs["count"].default.as_deref(), Some("3"));
        assert_eq!(manifest.inputs["flag"].default.as_deref(), Some("true"));
        assert_eq!(manifest.inputs["name"].default.as_deref(), Some("hi"));
    }

    #[test]
    fn label_sets_can_advertise_the_executor() {
        assert!(RunnerCapabilities::from_labels(["self-hosted", "ACTIONS-V1"]).actions_v1);
        assert!(!RunnerCapabilities::from_labels(["self-hosted"]).actions_v1);
    }

    #[test]
    fn clone_failures_are_classified_as_network_or_missing_ref() {
        assert_eq!(
            classify_clone_error(
                "fatal: unable to access 'https://example.invalid/a/b.git/': Could not resolve host: example.invalid"
            ),
            ActionErrorKind::Network
        );
        assert_eq!(
            classify_clone_error("fatal: Remote branch v9 not found in upstream origin"),
            ActionErrorKind::RefNotFound
        );
        assert_eq!(
            classify_clone_error("fatal: couldn't find remote ref refs/heads/nope"),
            ActionErrorKind::RefNotFound
        );
        assert_eq!(
            classify_clone_error("warning: Could not find remote branch v9 to clone."),
            ActionErrorKind::RefNotFound
        );
        assert_eq!(
            classify_clone_error("fatal: something unexpected"),
            ActionErrorKind::Fetch
        );
    }

    #[test]
    fn clone_error_messages_name_only_the_reference() {
        for kind in [
            ActionErrorKind::Network,
            ActionErrorKind::RefNotFound,
            ActionErrorKind::Fetch,
        ] {
            let message = clone_error_message(kind, "owner", "repo", "v1");
            assert!(message.contains("owner/repo@v1"), "{message}");
            assert!(
                !message.contains("http") && !message.contains(".tmp"),
                "no clone URL or cache path expected: {message}"
            );
        }
    }
}
