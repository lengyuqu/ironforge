//! gix-native repository operations that replace individual `git` CLI calls.
//!
//! Every helper here mirrors exactly one CLI invocation (or a tightly coupled
//! pair, like `show <rev>:<path>` + `rev-parse <rev>:<path>`), so call sites
//! can migrate incrementally without changing their surrounding error-handling
//! shape. The regression guard in `cli_gateway.rs` still applies: raw git
//! process spawning is only allowed inside `cli_gateway.rs`.
//!
//! # Error semantics
//!
//! Helpers return `anyhow::Result`. Where the CLI call was used in an
//! "optional" style (e.g. `cat-file` failing → treat as missing), callers turn
//! the `Option`/`Err` into their existing fallback; see `codeowners.rs` for an
//! example.

use std::path::Path;

use anyhow::{Context, bail};

fn open_repo(repo_path: &Path) -> anyhow::Result<gix::Repository> {
    gix::open(repo_path)
        .with_context(|| format!("failed to open repository: {}", repo_path.display()))
}

/// Equivalent of `git rev-parse <spec>` — resolve a revspec
/// (`refs/heads/main`, `HEAD`, a hex SHA, `<ref>^{commit}`, …) to a hex SHA.
pub fn rev_parse(repo_path: &Path, spec: &str) -> anyhow::Result<String> {
    let repo = open_repo(repo_path)?;
    let id = repo
        .rev_parse_single(spec)
        .with_context(|| format!("failed to resolve revision '{spec}'"))?;
    Ok(id.to_string())
}

/// Equivalent of `git rev-parse --verify <spec>` in an optional style:
/// `Ok(Some(sha))` when the spec resolves, `Ok(None)` when it does not.
pub fn try_rev_parse(repo_path: &Path, spec: &str) -> anyhow::Result<Option<String>> {
    let repo = open_repo(repo_path)?;
    match repo.rev_parse_single(spec) {
        Ok(id) => Ok(Some(id.to_string())),
        Err(_) => Ok(None),
    }
}

/// Equivalent of `git show <commitish>:<path>` (content) plus
/// `git rev-parse <commitish>:<path>` (blob SHA): returns
/// `(blob_sha, contents)` for the blob at `path` inside the tree of
/// `commitish`. `Ok(None)` when `path` does not exist in that tree.
pub fn blob_at(
    repo_path: &Path,
    commitish: &str,
    path: &str,
) -> anyhow::Result<Option<(String, Vec<u8>)>> {
    let repo = open_repo(repo_path)?;
    let id = repo
        .rev_parse_single(commitish)
        .with_context(|| format!("failed to resolve revision '{commitish}'"))?;
    let tree = id
        .object()
        .with_context(|| format!("failed to read object for '{commitish}'"))?
        .peel_to_tree()
        .with_context(|| format!("failed to peel '{commitish}' to a tree"))?;
    let Some(entry) = tree
        .lookup_entry_by_path(path)
        .with_context(|| format!("failed to look up '{path}' in tree of '{commitish}'"))?
    else {
        return Ok(None);
    };
    let object = entry
        .id()
        .object()
        .with_context(|| format!("failed to read blob for '{path}'"))?;
    if object.kind != gix::object::Kind::Blob {
        bail!("'{path}' in '{commitish}' is not a blob but {}", object.kind);
    }
    Ok(Some((entry.id().to_string(), object.data.to_vec())))
}

/// Equivalent of `git update-ref <ref> <sha>` — create or move a reference to
/// `new_sha`, writing `log_message` into the reflog.
pub fn update_ref(
    repo_path: &Path,
    ref_name: &str,
    new_sha: &str,
    log_message: &str,
) -> anyhow::Result<()> {
    let repo = open_repo(repo_path)?;
    let id = repo
        .rev_parse_single(new_sha)
        .with_context(|| format!("failed to resolve new value '{new_sha}' for '{ref_name}'"))?;
    repo.reference(
        ref_name,
        id.detach(),
        gix::refs::transaction::PreviousValue::Any,
        log_message,
    )
    .with_context(|| format!("failed to update ref '{ref_name}' to {new_sha}"))?;
    Ok(())
}

/// Equivalent of `git update-ref -d <ref>` — delete a reference.
pub fn delete_ref(repo_path: &Path, ref_name: &str) -> anyhow::Result<()> {
    let repo = open_repo(repo_path)?;
    use gix::refs::transaction::{Change, PreviousValue, RefEdit, RefLog};

    let full_name: gix::refs::FullName = ref_name
        .try_into()
        .map_err(|e| anyhow::anyhow!("invalid ref name '{ref_name}': {e}"))?;

    repo.edit_reference(RefEdit {
        change: Change::Delete {
            expected: PreviousValue::Any,
            log: RefLog::AndReference,
        },
        name: full_name,
        deref: false,
    })
    .with_context(|| format!("failed to delete ref '{ref_name}'"))?;
    Ok(())
}

/// Archive container formats, mirroring the `git archive --format=` values
/// the HTTP API exposes (`.tar`, `.tar.gz`/`.tgz`, `.zip`).
#[derive(Clone, Copy, Debug)]
pub enum ArchiveFormat {
    Tar,
    TarGz,
    Zip,
}

/// Equivalent of `git archive --format=<fmt> <commitish>` — returns the
/// archive bytes for the tree of `commitish`.
///
/// Uses gix's `worktree_stream` + `worktree_archive` (gix-archive), which
/// apply `export-ignore` attributes and checkout filters like the CLI does.
pub fn archive(
    repo_path: &Path,
    commitish: &str,
    format: ArchiveFormat,
) -> anyhow::Result<Vec<u8>> {
    use std::sync::atomic::AtomicBool;

    let repo = open_repo(repo_path)?;
    let commit = repo
        .rev_parse_single(commitish)
        .with_context(|| format!("failed to resolve revision '{commitish}'"))?
        .object()
        .with_context(|| format!("failed to read object for '{commitish}'"))?
        .peel_to_commit()
        .with_context(|| format!("'{commitish}' is not a commit"))?;
    let tree_id = commit
        .tree_id()
        .with_context(|| format!("failed to resolve tree of '{commitish}'"))?;
    let (stream, _index) = repo
        .worktree_stream(tree_id.detach())
        .map_err(|error| anyhow::anyhow!("failed to stream tree for archive: {error}"))?;

    let format = match format {
        ArchiveFormat::Tar => gix::worktree::archive::Format::Tar,
        ArchiveFormat::TarGz => gix::worktree::archive::Format::TarGz {
            compression_level: None,
        },
        ArchiveFormat::Zip => gix::worktree::archive::Format::Zip {
            compression_level: None,
        },
    };
    // Like `git archive`, stamp entries with the commit's timestamp.
    let modification_time = commit
        .time()
        .map(|time| time.seconds)
        .unwrap_or_default();
    let options = gix::worktree::archive::Options {
        format,
        tree_prefix: None,
        modification_time,
    };
    let should_interrupt = AtomicBool::new(false);
    let mut out = std::io::Cursor::new(Vec::new());
    repo.worktree_archive(
        stream,
        &mut out,
        gix::features::progress::Discard,
        &should_interrupt,
        options,
    )
    .map_err(|error| anyhow::anyhow!("failed to write archive: {error}"))?;
    Ok(out.into_inner())
}

/// Equivalent of `git verify-commit <sha>` — returns `true` when the commit
/// carries a cryptographically valid signature.
///
/// Requires the gix `command` feature: verification shells out to the
/// configured GPG program, exactly like the CLI does.
pub fn verify_commit(repo_path: &Path, sha: &str) -> anyhow::Result<bool> {
    let repo = open_repo(repo_path)?;
    verify_commit_with_repo(&repo, sha)
}

/// Same as [`verify_commit`] but takes an already-open `Repository`.
pub fn verify_commit_with_repo(repo: &gix::Repository, sha: &str) -> anyhow::Result<bool> {
    let commit = repo
        .rev_parse_single(sha)
        .with_context(|| format!("failed to resolve commit '{sha}'"))?
        .object()
        .with_context(|| format!("failed to read object for commit '{sha}'"))?
        .peel_to_commit()
        .with_context(|| format!("'{sha}' is not a commit"))?;
    let outcome = commit
        .verify_signature()
        .with_context(|| format!("signature verification failed for commit {sha}"))?;
    Ok(outcome.is_some_and(|signed| signed.is_valid()))
}
