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

/// Outcome of [`rebase_merge`].
#[derive(Debug)]
pub enum RebaseOutcome {
    /// The base ref was moved to the rebased head; carries the new base SHA.
    Rebased(String),
    /// A head commit conflicts while being replayed; carries a description.
    /// The base ref is left untouched.
    Conflict(String),
    /// The base ref advanced while commits were being replayed; the update
    /// was rejected (concurrency guard, equivalent to the non-fast-forward
    /// push rejection of the previous `git rebase` + `git push` flow).
    BaseAdvanced,
}

/// Rebase `head_rev` (any revspec: a branch ref, a fork ref, or a SHA) onto
/// `base_branch` (the PR "Rebase and merge" strategy) using only gix
/// primitives - the pure-Rust replacement for the previous
/// `git clone --no-checkout` / `fetch` / `checkout --detach` / `rebase` /
/// `push` worktree chain.
///
/// Semantics mirror `git rebase` defaults:
/// - commits are replayed along the head's first-parent chain since the
///   merge base, oldest first;
/// - merge commits are dropped (linearization);
/// - replays that would produce an empty commit are skipped;
/// - original author identity and message are preserved; the committer is
///   `IronForge <noreply@ironforge.local>`;
/// - the base ref is updated with an expected-value guard so a concurrently
///   advanced base is rejected instead of overwritten.
///
/// The implementation is deliberately a thin, single function so it can be
/// swapped for a future native gix rebase API without touching call sites;
/// the characterization tests in `ops::tests` pin the semantics for that day.
pub fn rebase_merge(
    repo_path: &Path,
    base_branch: &str,
    head_rev: &str,
) -> anyhow::Result<RebaseOutcome> {
    let repo = open_repo(repo_path)?;
    let base_ref: gix::refs::FullName = format!("refs/heads/{base_branch}")
        .try_into()
        .map_err(|e| anyhow::anyhow!("invalid base branch ref: {e}"))?;

    let base_id = repo
        .rev_parse_single(format!("refs/heads/{base_branch}").as_str())
        .with_context(|| format!("base branch '{base_branch}' not found"))?;
    let head_id = repo
        .rev_parse_single(head_rev)
        .with_context(|| format!("head revision '{head_rev}' not found"))?;
    let base_sha = base_id.detach();
    let head_sha = head_id.detach();

    if base_sha == head_sha {
        // Already up to date: nothing to do, leave the base ref alone.
        return Ok(RebaseOutcome::Rebased(base_sha.to_string()));
    }

    let merge_base = repo
        .merge_base(base_sha, head_sha)
        .with_context(|| format!("no merge base between '{base_branch}' and '{head_rev}'"))?
        .detach();

    // When the head is a strict descendant of the base, `git rebase`
    // fast-forwards without rewriting any commit.
    if merge_base == base_sha {
        return match update_base_ref(&repo, base_ref, base_sha, head_sha) {
            Ok(()) => Ok(RebaseOutcome::Rebased(head_sha.to_string())),
            Err(_) => Ok(RebaseOutcome::BaseAdvanced),
        };
    }

    // Collect the commits to replay: everything reachable from head but not
    // from the base (i.e. `git rev-list base..head`), then order parents
    // before children (topological, oldest-first among ready commits) like
    // `git rev-list --topo-order --reverse`.
    let replay_set: Vec<gix::hash::ObjectId> = repo
        .rev_walk([head_sha])
        .with_hidden([base_sha])
        .all()
        .map_err(|error| anyhow::anyhow!("failed to walk head history: {error}"))?
        .filter_map(|item| item.ok().map(|info| info.id))
        .collect();

    struct Node {
        id: gix::hash::ObjectId,
        time: i64,
        is_merge: bool,
        parents_in_set: Vec<gix::hash::ObjectId>,
    }
    let mut nodes: Vec<Node> = Vec::with_capacity(replay_set.len());
    for id in &replay_set {
        let commit = repo
            .find_object(*id)
            .with_context(|| format!("failed to read commit {id}"))?
            .into_commit();
        let parents: Vec<gix::hash::ObjectId> = commit
            .parent_ids()
            .map(|parent| parent.detach())
            .filter(|parent| replay_set.contains(parent))
            .collect();
        let time = commit.time().map(|time| time.seconds).unwrap_or_default();
        nodes.push(Node {
            id: *id,
            time,
            is_merge: commit.parent_ids().count() > 1,
            parents_in_set: parents,
        });
    }

    // Kahn's algorithm, oldest-commit-time first among ready nodes.
    let mut chain: Vec<gix::hash::ObjectId> = Vec::with_capacity(nodes.len());
    let mut done: std::collections::HashSet<gix::hash::ObjectId> =
        std::collections::HashSet::with_capacity(nodes.len());
    while !nodes.is_empty() {
        let pick = nodes
            .iter()
            .position(|node| {
                node.parents_in_set
                    .iter()
                    .all(|parent| done.contains(parent))
            })
            .and_then(|first_ready| {
                nodes[first_ready..]
                    .iter()
                    .enumerate()
                    .filter(|(_, node)| {
                        node.parents_in_set
                            .iter()
                            .all(|parent| done.contains(parent))
                    })
                    .min_by_key(|(_, node)| node.time)
                    .map(|(offset, _)| first_ready + offset)
            });
        let Some(pick) = pick else {
            anyhow::bail!("cycle detected while ordering replay commits");
        };
        let node = nodes.swap_remove(pick);
        done.insert(node.id);
        if !node.is_merge {
            // `git rebase` drops merge commits (default linearization).
            chain.push(node.id);
        }
    }

    let options: gix::merge::tree::Options = repo
        .tree_merge_options()
        .map_err(|error| anyhow::anyhow!("failed to get tree merge options: {error}"))?;
    let now = gix::date::Time::now_utc();
    // `SignatureRef.time` is the raw git timestamp string (`<seconds> <offset>`).
    let committer_time = format!("{} +0000", now.seconds);
    let committer = gix::actor::SignatureRef {
        name: "IronForge".as_bytes().into(),
        email: "noreply@ironforge.local".as_bytes().into(),
        time: committer_time.as_str(),
    };

    let mut running_head = base_sha;
    let mut running_tree = repo
        .find_object(running_head)?
        .into_commit()
        .tree_id()
        .with_context(|| format!("failed to resolve tree of base commit {running_head}"))?
        .detach();

    for commit_sha in chain {
        let commit = repo
            .find_object(commit_sha)
            .with_context(|| format!("failed to read commit {commit_sha}"))?
            .into_commit();

        // `git rebase` drops merge commits (default linearization).
        if commit.parent_ids().count() > 1 {
            continue;
        }

        let Some(first_parent) = commit.parent_ids().next() else {
            anyhow::bail!("commit {commit_sha} on the head chain has no parent");
        };
        let ancestor_tree = repo
            .find_object(first_parent)?
            .into_commit()
            .tree_id()
            .with_context(|| format!("failed to resolve parent tree for replay of {commit_sha}"))?
            .detach();

        let other_label = commit_sha.to_string();
        let labels = gix::merge::blob::builtin_driver::text::Labels {
            current: Some("base".into()),
            other: Some(other_label.as_str().into()),
            ancestor: None,
        };
        let mut outcome = repo
            .merge_trees(
                ancestor_tree,
                running_tree,
                commit.tree_id()?.detach(),
                labels,
                options.clone(),
            )
            .map_err(|error| {
                anyhow::anyhow!("failed to merge tree while replaying {commit_sha}: {error}")
            })?;
        if !outcome.conflicts.is_empty() {
            return Ok(RebaseOutcome::Conflict(format!(
                "commit {commit_sha} conflicts while rebasing onto {base_branch}"
            )));
        }
        let new_tree = outcome
            .tree
            .write()
            .map_err(|error| anyhow::anyhow!("failed to write replayed tree: {error}"))?
            .detach();

        // A replay that does not change the tree is an empty commit (already
        // applied to the base) - `git rebase` skips it.
        if new_tree == running_tree {
            continue;
        }

        // Preserve the original author identity and message verbatim.
        let author = commit
            .author()
            .with_context(|| format!("failed to read author of {commit_sha}"))?;
        let message = commit
            .message_raw()
            .with_context(|| format!("failed to read message of {commit_sha}"))?;
        let message = std::str::from_utf8(message)
            .with_context(|| format!("commit message of {commit_sha} is not UTF-8"))?;

        let rebased = repo
            .new_commit_as(committer, author, message, new_tree, [running_head])
            .map_err(|error| {
                anyhow::anyhow!("failed to create rebased commit for {commit_sha}: {error}")
            })?;
        running_head = rebased.id().detach();
        running_tree = new_tree;
    }

    if running_head == base_sha {
        // Every head commit was already applied; nothing to move.
        return Ok(RebaseOutcome::Rebased(base_sha.to_string()));
    }

    match update_base_ref(&repo, base_ref, base_sha, running_head) {
        Ok(()) => Ok(RebaseOutcome::Rebased(running_head.to_string())),
        Err(_) => Ok(RebaseOutcome::BaseAdvanced),
    }
}

/// Move the base ref from `expected_old` to `new_sha`, rejecting the update
/// if the ref is no longer at `expected_old` (concurrent advance guard,
/// equivalent to a non-fast-forward push rejection).
fn update_base_ref(
    repo: &gix::Repository,
    base_ref: gix::refs::FullName,
    expected_old: gix::hash::ObjectId,
    new_sha: gix::hash::ObjectId,
) -> anyhow::Result<()> {
    use gix::refs::transaction::{Change, LogChange, PreviousValue, RefEdit, RefLog};

    repo.edit_reference(RefEdit {
        change: Change::Update {
            log: LogChange {
                mode: RefLog::AndReference,
                force_create_reflog: false,
                message: "rebase merge".into(),
            },
            expected: PreviousValue::ExistingMustMatch(gix::refs::Target::Object(expected_old)),
            new: gix::refs::Target::Object(new_sha),
        },
        name: base_ref,
        deref: false,
    })
    .map_err(|error| anyhow::anyhow!("base ref update rejected: {error}"))?;
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

#[cfg(test)]
mod rebase_tests {
    //! Characterization tests for [`rebase_merge`]. These pin the semantics
    //! of the self-built rebase so a future swap to a native gix rebase API
    //! can be validated against the exact same expectations.

    use super::*;
    use crate::cli_gateway::GitCommandGateway;

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

    fn init_repo(dir: &Path) -> GitCommandGateway {
        std::fs::create_dir_all(dir).expect("create repo dir");
        let git = GitCommandGateway::new().expect("git CLI available");
        run_git(&git, &["init", "-b", "main"], dir);
        run_git(&git, &["config", "user.email", "tester@example.com"], dir);
        run_git(&git, &["config", "user.name", "Tester"], dir);
        git
    }

    fn commit(git: &GitCommandGateway, dir: &Path, file: &str, content: &str, message: &str) {
        std::fs::write(dir.join(file), content).expect("write file");
        run_git(git, &["add", "-A"], dir);
        run_git(git, &["commit", "-m", message], dir);
    }

    #[test]
    fn rebase_replays_linear_commits_preserving_authors() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();
        let git = init_repo(dir);
        commit(&git, dir, "a.txt", "one\n", "a");
        run_git(&git, &["checkout", "-b", "feature"], dir);
        commit(&git, dir, "b.txt", "b\n", "b");
        commit(&git, dir, "c.txt", "c\n", "c");
        run_git(&git, &["checkout", "main"], dir);
        commit(&git, dir, "d.txt", "d\n", "d");
        let main_before = git_out(&git, &["rev-parse", "refs/heads/main"], dir);
        let feature_head = git_out(&git, &["rev-parse", "refs/heads/feature"], dir);

        let outcome = rebase_merge(dir, "main", "feature").expect("rebase runs");
        let RebaseOutcome::Rebased(new_base) = outcome else {
            panic!("expected Rebased, got {outcome:?}");
        };

        // The base moved to a fresh chain; the head ref stays untouched.
        assert_ne!(new_base, main_before);
        assert_ne!(new_base, feature_head);
        assert_eq!(git_out(&git, &["rev-parse", "refs/heads/main"], dir), new_base);
        assert_eq!(git_out(&git, &["rev-parse", "refs/heads/feature"], dir), feature_head);

        // Final tree merges both sides.
        for (file, content) in [("a.txt", "one\n"), ("b.txt", "b\n"), ("c.txt", "c\n"), ("d.txt", "d\n")] {
            assert_eq!(git_out(&git, &["show", &format!("refs/heads/main:{file}")], dir), content.trim_end());
        }
        // Newest two commits on main are the replayed b and c.
        assert_eq!(git_out(&git, &["log", "--format=%s", "-2"], dir), "c\nb");
        // Authors preserved, committer is IronForge.
        assert_eq!(git_out(&git, &["log", "--format=%ae", "-2"], dir), "tester@example.com\ntester@example.com");
        assert_eq!(git_out(&git, &["log", "--format=%ce", "-2"], dir), "noreply@ironforge.local\nnoreply@ironforge.local");
    }

    #[test]
    fn rebase_skips_already_applied_commits() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();
        let git = init_repo(dir);
        commit(&git, dir, "a.txt", "one\n", "a");
        run_git(&git, &["checkout", "-b", "feature"], dir);
        commit(&git, dir, "x.txt", "x\n", "x");
        let x_sha = git_out(&git, &["rev-parse", "refs/heads/feature"], dir);
        run_git(&git, &["checkout", "main"], dir);
        run_git(&git, &["cherry-pick", &x_sha], dir);
        let main_before = git_out(&git, &["rev-parse", "refs/heads/main"], dir);

        let outcome = rebase_merge(dir, "main", "feature").expect("rebase runs");
        let RebaseOutcome::Rebased(new_base) = outcome else {
            panic!("expected Rebased, got {outcome:?}");
        };

        // The replayed commit is empty, so the base does not move.
        assert_eq!(new_base, main_before);
        assert_eq!(git_out(&git, &["rev-parse", "refs/heads/main"], dir), main_before);
    }

    #[test]
    fn rebase_drops_merge_commits_and_replays_side_branch() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();
        let git = init_repo(dir);
        commit(&git, dir, "a.txt", "one\n", "a");
        run_git(&git, &["checkout", "-b", "feature"], dir);
        commit(&git, dir, "b.txt", "b\n", "b");
        run_git(&git, &["checkout", "-b", "side"], dir);
        commit(&git, dir, "s.txt", "s\n", "s");
        run_git(&git, &["checkout", "feature"], dir);
        run_git(&git, &["merge", "--no-ff", "-m", "merge side", "side"], dir);
        run_git(&git, &["checkout", "main"], dir);
        commit(&git, dir, "d.txt", "d\n", "d");

        let outcome = rebase_merge(dir, "main", "feature").expect("rebase runs");
        let RebaseOutcome::Rebased(new_base) = outcome else {
            panic!("expected Rebased, got {outcome:?}");
        };

        // Like `git rev-list --no-merges main..feature` + rebase: both b and
        // the side-branch commit s are replayed, the merge commit is dropped.
        assert_eq!(git_out(&git, &["show", &format!("refs/heads/main:s.txt")], dir), "s");
        assert_eq!(git_out(&git, &["show", &format!("refs/heads/main:b.txt")], dir), "b");
        assert_eq!(git_out(&git, &["show", &format!("refs/heads/main:d.txt")], dir), "d");
        let subjects = git_out(&git, &["log", "--format=%s", "-3"], dir);
        // Newest first: s, b, then d (the base tip).
        assert_eq!(subjects, "s\nb\nd");
        assert_ne!(new_base, "");
    }

    #[test]
    fn rebase_conflict_leaves_base_untouched() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();
        let git = init_repo(dir);
        commit(&git, dir, "a.txt", "one\n", "a");
        run_git(&git, &["checkout", "-b", "feature"], dir);
        commit(&git, dir, "a.txt", "feature\n", "c");
        run_git(&git, &["checkout", "main"], dir);
        commit(&git, dir, "a.txt", "main\n", "d");
        let main_before = git_out(&git, &["rev-parse", "refs/heads/main"], dir);

        let outcome = rebase_merge(dir, "main", "feature").expect("rebase runs");
        let RebaseOutcome::Conflict(message) = outcome else {
            panic!("expected Conflict, got {outcome:?}");
        };
        assert!(message.contains("conflicts while rebasing onto main"));

        // Base ref untouched; no partial replay state is visible.
        assert_eq!(git_out(&git, &["rev-parse", "refs/heads/main"], dir), main_before);
    }

    #[test]
    fn rebase_fast_forwards_when_head_descends_from_base() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();
        let git = init_repo(dir);
        commit(&git, dir, "a.txt", "one\n", "a");
        run_git(&git, &["checkout", "-b", "feature"], dir);
        commit(&git, dir, "b.txt", "b\n", "b");
        let feature_head = git_out(&git, &["rev-parse", "refs/heads/feature"], dir);

        let outcome = rebase_merge(dir, "main", "feature").expect("rebase runs");
        let RebaseOutcome::Rebased(new_base) = outcome else {
            panic!("expected Rebased, got {outcome:?}");
        };

        // Head is a strict descendant: plain fast-forward, no rewriting.
        assert_eq!(new_base, feature_head);
        assert_eq!(git_out(&git, &["rev-parse", "refs/heads/main"], dir), feature_head);
        assert_eq!(git_out(&git, &["log", "--format=%s", "-1"], dir), "b");
    }
}
