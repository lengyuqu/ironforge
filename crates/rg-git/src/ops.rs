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

// ─── Final CLI coverage: graph walking, pack I/O, tree-edit commits ─────────

/// gix-native replacement for `git rev-list <include> [^<exclude>]`: returns
/// the hex SHAs of every commit reachable from `include`, minus those
/// reachable from `exclude` when given.
pub fn commits_between(
    repo: &gix::Repository,
    include: gix::hash::ObjectId,
    exclude: Option<gix::hash::ObjectId>,
) -> anyhow::Result<Vec<String>> {
    let mut walk = repo.rev_walk([include]);
    if let Some(hidden) = exclude {
        walk = walk.with_hidden([hidden]);
    }
    Ok(walk
        .all()
        .map_err(|error| anyhow::anyhow!("failed to walk commit graph from {include}: {error}"))?
        .filter_map(|item| item.ok().map(|info| info.id.to_string()))
        .collect())
}

/// gix-native replacement for
/// `git rev-list --parents [--max-age=<ts>] <starts> --not <excluded>`:
/// returns `(oid_hex, parent_hexes)` pairs for every commit reachable from
/// `starts` except those reachable from `excluded`, limited to commits newer
/// than `max_age` when given (traversal stops at the cutoff like the CLI).
pub fn commit_graph(
    repo: &gix::Repository,
    starts: &[gix::hash::ObjectId],
    max_age: Option<i64>,
    excluded: &[gix::hash::ObjectId],
) -> anyhow::Result<Vec<(String, Vec<String>)>> {
    use gix::revision::walk::Sorting;
    use gix::traverse::commit::simple::CommitTimeOrder;

    let mut walk = repo.rev_walk(starts.iter().copied());
    if let Some(cutoff) = max_age {
        walk = walk.sorting(Sorting::ByCommitTimeCutoff {
            order: CommitTimeOrder::NewestFirst,
            seconds: cutoff,
        });
    }
    if !excluded.is_empty() {
        walk = walk.with_hidden(excluded.iter().copied());
    }
    let mut graph = Vec::new();
    for item in walk
        .all()
        .map_err(|error| anyhow::anyhow!("failed to walk commit graph: {error}"))?
    {
        let info = item.map_err(|error| anyhow::anyhow!("commit graph walk failed: {error}"))?;
        let parents: Vec<String> = info
            .parent_ids
            .into_iter()
            .map(|parent| parent.to_string())
            .collect();
        graph.push((info.id.to_string(), parents));
    }
    Ok(graph)
}

/// gix-native replacement for `git pack-objects` — generates a version-2
/// packfile containing `input_ids` plus every object they expand to.
///
/// Inputs are usually commits (expanded with `TreeContents`: the commit plus
/// its full tree contents) and tag objects, which are included verbatim.
/// Callers pre-compute the commit list via `rev_walk` negotiation, mirroring
/// `pack-objects --revs` with `^have` exclusions.
pub fn generate_pack(
    repo: &gix::Repository,
    input_ids: Vec<gix::hash::ObjectId>,
) -> anyhow::Result<Vec<u8>> {
    use gix::features::progress::Discard;
    use gix::odb::pack::data::output;

    let should_interrupt = std::sync::atomic::AtomicBool::new(false);
    // `repo.objects` is a memory `Proxy` which does not implement
    // `gix_pack::Find`; unwrap down to the `Cache` (which does) for the
    // pack pipeline while writes elsewhere keep using the proxy.
    let db = repo.objects.clone().into_inner();
    let input: Box<
        dyn Iterator<
                Item = Result<
                    gix::hash::ObjectId,
                    Box<dyn std::error::Error + Send + Sync + 'static>,
                >,
            > + Send,
    > = Box::new(input_ids.into_iter().map(Ok));

    let (counts, _outcome) = output::count::objects(
        db.clone(),
        input,
        &Discard,
        &should_interrupt,
        output::count::objects::Options {
            thread_limit: None,
            chunk_size: 10,
            input_object_expansion: output::count::objects::ObjectExpansion::TreeContents,
        },
    )
    .map_err(|error| anyhow::anyhow!("failed to count pack objects: {error}"))?;

    let num_entries = counts.len() as u32;
    let entries = output::entry::iter_from_counts(
        counts,
        db,
        Box::new(Discard),
        output::entry::iter_from_counts::Options {
            allow_thin_pack: false,
            ..Default::default()
        },
    );

    let mut pack = Vec::new();
    {
        use gix::features::parallel::InOrderIter;
        let in_order = InOrderIter::from(entries);
        let mut writer = output::bytes::FromEntriesIter::new(
            in_order,
            &mut pack,
            num_entries,
            gix::odb::pack::data::Version::V2,
            repo.object_hash(),
        );
        for chunk in &mut writer {
            chunk.map_err(|error| anyhow::anyhow!("failed to write pack entry: {error}"))?;
        }
    }
    Ok(pack)
}

/// gix-native replacement for `git pack-objects --all --stdout`: packs every
/// object reachable from every ref in the repository (initial-clone path).
pub fn pack_universe(repo_path: &Path) -> anyhow::Result<Vec<u8>> {
    let repo = open_repo(repo_path)?;
    let mut inputs: Vec<gix::hash::ObjectId> = Vec::new();
    let mut walk_tips: Vec<gix::hash::ObjectId> = Vec::new();

    let platform = repo
        .references()
        .map_err(|error| anyhow::anyhow!("failed to list refs: {error}"))?;
    let refs = platform
        .all()
        .map_err(|error| anyhow::anyhow!("failed to iterate refs: {error}"))?;
    for reference in refs.flatten() {
        let target = reference.target();
        let Some(id) = target.try_id() else {
            continue;
        };
        let id = gix::hash::ObjectId::from(id);
        // Tag objects (and any other non-commit ref targets) go in verbatim.
        inputs.push(id);
        if let Some(commit) = repo
            .find_object(id)
            .ok()
            .and_then(|object| object.peel_to_commit().ok())
            .map(|commit| commit.id().detach())
        {
            walk_tips.push(commit);
        }
    }

    if !walk_tips.is_empty() {
        let commits: Vec<gix::hash::ObjectId> = repo
            .rev_walk(walk_tips)
            .all()
            .map_err(|error| anyhow::anyhow!("failed to walk all refs: {error}"))?
            .filter_map(|item| item.ok().map(|info| info.id))
            .collect();
        inputs.extend(commits);
    }
    generate_pack(&repo, inputs)
}

/// gix-native replacement for `git pack-objects --revs --stdout [--thin]`:
/// packs everything reachable from `wants` minus everything reachable from
/// `haves`. When `shallow_boundaries` is non-empty (shallow client), the
/// boundaries take the place of the haves: traversal stops at them while
/// their tree contents are still included, exactly like `--shallow <sha>`
/// revs fed to the CLI.
pub fn pack_for_wants(
    repo_path: &Path,
    wants: &[String],
    haves: &[String],
    shallow_boundaries: &[String],
) -> anyhow::Result<Vec<u8>> {
    let repo = open_repo(repo_path)?;
    let peel_to_commit = |spec: &str| -> anyhow::Result<gix::hash::ObjectId> {
        let id = repo
            .rev_parse_single(spec)
            .with_context(|| format!("failed to resolve '{spec}'"))?
            .detach();
        let commit = repo
            .find_object(id)
            .ok()
            .and_then(|object| object.peel_to_commit().ok())
            .map(|commit| commit.id().detach())
            .with_context(|| format!("'{spec}' is not a commit"))?;
        Ok(commit)
    };

    let mut starts = Vec::with_capacity(wants.len());
    let mut extra_inputs = Vec::new();
    for want in wants {
        let id = repo
            .rev_parse_single(want.as_str())
            .with_context(|| format!("failed to resolve want '{want}'"))?
            .detach();
        if repo.find_object(id)?.kind == gix::object::Kind::Tag {
            extra_inputs.push(id);
        }
        starts.push(peel_to_commit(want)?);
    }

    let peel_list = |specs: &[String]| -> anyhow::Result<Vec<gix::hash::ObjectId>> {
        specs.iter().map(|spec| peel_to_commit(spec)).collect()
    };
    let hidden = if !shallow_boundaries.is_empty() {
        peel_list(shallow_boundaries)?
    } else {
        peel_list(haves)?
    };

    let mut inputs: Vec<gix::hash::ObjectId> = if starts.is_empty() {
        Vec::new()
    } else {
        let mut walk = repo.rev_walk(starts);
        if !hidden.is_empty() {
            walk = walk.with_hidden(hidden);
        }
        walk.all()
            .map_err(|error| anyhow::anyhow!("failed to walk want history: {error}"))?
            .filter_map(|item| item.ok().map(|info| info.id))
            .collect()
    };
    if !shallow_boundaries.is_empty() {
        // Shallow clients own the boundary commits but not their trees —
        // TreeContents expansion of the boundary commit supplies exactly that.
        for boundary in shallow_boundaries {
            inputs.push(peel_to_commit(boundary)?);
        }
    }
    inputs.extend(extra_inputs);
    generate_pack(&repo, inputs)
}

/// gix-native replacement for `git index-pack --fix-thin --stdin`: ingests a
/// pack byte stream into the repository object database, resolving thin-pack
/// deltas against objects already present, and writes pack + index into
/// `objects/pack`. Any `.keep` file left behind is removed so the pack
/// participates in future repacks.
pub fn index_pack_bytes(repo_path: &Path, pack: &[u8]) -> anyhow::Result<()> {
    use gix::features::progress::Discard;
    use gix::odb::pack::Bundle;

    let repo = open_repo(repo_path)?;
    let pack_dir = repo.objects.store().path().join("pack");
    std::fs::create_dir_all(&pack_dir)
        .with_context(|| format!("failed to ensure pack dir: {}", pack_dir.display()))?;

    let mut reader = std::io::Cursor::new(pack);
    let should_interrupt = std::sync::atomic::AtomicBool::new(false);
    let outcome = Bundle::write_to_directory(
        &mut reader,
        Some(&pack_dir),
        &mut Discard,
        &should_interrupt,
        Some(repo.objects.clone()),
        repo.object_hash(),
        gix::odb::pack::bundle::write::Options::default(),
    )
    .map_err(|error| anyhow::anyhow!("failed to index pack: {error}"))?;
    if let Some(keep) = outcome.keep_path {
        let _ = std::fs::remove_file(keep);
    }
    Ok(())
}

/// gix-native replacement for `git fetch <source-path> <tip>` between two
/// local bare repositories: copies every object reachable from `tip` (a
/// revspec in the source repo) that is not already reachable in the target
/// repo, and returns the hex SHA of `tip`.
///
/// Pruning happens at commit level (`rev_walk` with the target's refs as
/// hidden tips) plus per-commit tree deltas
/// (`TreeAdditionsComparedToAncestor`), so shared history is not re-copied.
pub fn copy_objects_from_source(
    source_repo_path: &Path,
    tip: &str,
    target_repo_path: &Path,
) -> anyhow::Result<String> {
    use gix::features::progress::Discard;

    let src = open_repo(source_repo_path)?;
    let dst = open_repo(target_repo_path)?;
    let tip_commit = src
        .rev_parse_single(tip)
        .with_context(|| format!("failed to resolve tip '{tip}' in source repository"))?
        .object()
        .ok()
        .and_then(|object| object.peel_to_commit().ok())
        .map(|commit| commit.id().detach())
        .with_context(|| format!("tip '{tip}' is not a commit"))?;

    // Commits already reachable in the target repository prune the walk.
    let platform = dst
        .references()
        .map_err(|error| anyhow::anyhow!("failed to list target refs: {error}"))?;
    let refs = platform
        .all()
        .map_err(|error| anyhow::anyhow!("failed to iterate target refs: {error}"))?;
    let dst_tips: Vec<gix::hash::ObjectId> = refs
        .filter_map(|reference| {
            reference.ok().and_then(|reference| {
                let target = reference.target();
                let commit_id = target
                    .try_id()
                    .and_then(|id| dst.find_object(gix::hash::ObjectId::from(id)).ok())
                    .and_then(|object| object.peel_to_commit().ok())
                    .map(|commit| commit.id().detach());
                commit_id
            })
        })
        .collect();

    let mut walk = src.rev_walk([tip_commit]);
    if !dst_tips.is_empty() {
        // Hidden tips missing from the source (commits the target has but
        // the source lost, e.g. force-push) cannot be reached from `tip`
        // anyway; skip them instead of failing the walk.
        let hidden: Vec<gix::hash::ObjectId> = dst_tips
            .into_iter()
            .filter(|id| src.find_object(*id).is_ok())
            .collect();
        if !hidden.is_empty() {
            walk = walk.with_hidden(hidden);
        }
    }
    let new_commits: Vec<gix::hash::ObjectId> = walk
        .all()
        .map_err(|error| anyhow::anyhow!("failed to walk source history: {error}"))?
        .filter_map(|item| item.ok().map(|info| info.id))
        .collect();
    if new_commits.is_empty() {
        return Ok(tip_commit.to_string());
    }

    let should_interrupt = std::sync::atomic::AtomicBool::new(false);
    let input: Box<
        dyn Iterator<
                Item = Result<
                    gix::hash::ObjectId,
                    Box<dyn std::error::Error + Send + Sync + 'static>,
                >,
            > + Send,
    > = Box::new(new_commits.into_iter().map(Ok));
    let (counts, _outcome) = gix::odb::pack::data::output::count::objects(
        src.objects.clone().into_inner(),
        input,
        &Discard,
        &should_interrupt,
        gix::odb::pack::data::output::count::objects::Options {
            thread_limit: None,
            chunk_size: 10,
            input_object_expansion:
                gix::odb::pack::data::output::count::objects::ObjectExpansion::TreeAdditionsComparedToAncestor,
        },
    )
    .map_err(|error| anyhow::anyhow!("failed to expand source objects: {error}"))?;

    for count in counts {
        let object = src
            .find_object(count.id)
            .with_context(|| format!("failed to read object {} from source repository", count.id))?;
        // Re-writing an object that already exists is a no-op on all
        // platforms (persist succeeds or the existing file is kept).
        use gix::prelude::Write as _;
        dst.objects
            .write_buf_with_known_id(object.kind, &object.data, count.id)
            .map_err(|error| {
                anyhow::anyhow!("failed to copy object {} into target repository: {error}", count.id)
            })?;
    }
    Ok(tip_commit.to_string())
}

/// A single path-level edit applied to the tip tree of a branch, replacing
/// the previous `clone` → `add`/`rm` → `commit` → `push` CLI chains.
pub enum TreeEdit {
    /// Create or replace a file. When `expected_blob_sha` is given the
    /// current blob must match (optimistic concurrency).
    SetFile {
        path: String,
        content: Vec<u8>,
        expected_blob_sha: Option<String>,
    },
    /// Remove an existing file.
    RemoveFile { path: String },
}

/// Outcome of [`commit_tree_edits`].
#[derive(Debug)]
pub enum CommitTreeOutcome {
    /// A new commit was written and the branch ref advanced; carries the SHA.
    Committed(String),
    /// The branch ref is no longer at the expected tip (concurrent advance);
    /// carries the actual tip when it could be resolved.
    TipAdvanced { actual: Option<String> },
}

/// Commit direct tree edits onto a bare repository branch — the pure-gix
/// replacement for the clone/add/commit/push file-editing chains.
///
/// The branch ref is updated with an expected-value guard: when it moved
/// between the caller's read and this commit, [`CommitTreeOutcome::TipAdvanced`]
/// is returned and nothing is written. `expected_tip == None` skips that check
/// (used together with `create_branch_if_missing` for first commits on empty
/// repositories, where the ref is guarded by `MustNotExist` instead).
#[allow(clippy::too_many_arguments)]
pub fn commit_tree_edits(
    repo_path: &Path,
    branch: &str,
    expected_tip: Option<&str>,
    edits: &[TreeEdit],
    message: &str,
    author_name: &str,
    author_email: &str,
    create_branch_if_missing: bool,
) -> anyhow::Result<CommitTreeOutcome> {
    use gix::objs::tree::EntryKind;
    use gix::refs::transaction::{Change, LogChange, PreviousValue, RefEdit, RefLog};

    let repo = open_repo(repo_path)?;
    let branch_ref: gix::refs::FullName = format!("refs/heads/{branch}")
        .try_into()
        .map_err(|error| anyhow::anyhow!("invalid branch ref: {error}"))?;
    let tip = match repo.rev_parse_single(format!("refs/heads/{branch}").as_str()) {
        Ok(id) => Some(id.detach()),
        Err(_) => None,
    };
    match (tip, expected_tip) {
        // Compare parsed ids so hex case differences cannot spuriously
        // report an advance; an unparseable expected tip defers to the
        // ref-transaction guard below.
        (Some(actual), Some(expected))
            if actual
                != gix::hash::ObjectId::from_hex(expected.as_bytes()).unwrap_or(actual) =>
        {
            return Ok(CommitTreeOutcome::TipAdvanced {
                actual: Some(actual.to_string()),
            });
        }
        // Fresh branch on an empty repository: commit with no parent and
        // create the ref (guarded by MustNotExist below).
        (None, None) if create_branch_if_missing => {}
        (None, _) => bail!("branch '{branch}' not found"),
        _ => {}
    }

    let tip_display = tip
        .map(|tip| tip.to_string())
        .unwrap_or_else(|| "<new branch>".to_string());
    let tip_tree = match tip {
        Some(tip) => Some(
            repo.find_object(tip)?
                .peel_to_tree()
                .with_context(|| format!("failed to read tree of branch tip {tip}"))?,
        ),
        None => None,
    };
    let base_tree_id = match &tip_tree {
        Some(tree) => tree.id().detach(),
        None => gix::hash::ObjectId::empty_tree(repo.object_hash()),
    };
    let mut editor = repo.edit_tree(base_tree_id)?;

    for edit in edits {
        match edit {
            TreeEdit::SetFile {
                path,
                content,
                expected_blob_sha,
            } => {
                let entry = match &tip_tree {
                    Some(tree) => tree
                        .lookup_entry_by_path(path)
                        .with_context(|| format!("failed to look up '{path}' in tree of {tip_display}"))?,
                    None => None,
                };
                match (entry, expected_blob_sha) {
                    (None, Some(_)) => {
                        bail!("file {path} is missing at HEAD");
                    }
                    (Some(entry), _) => {
                        if entry.mode().is_link() {
                            bail!("refusing to update symlink path: {path}");
                        }
                        if let Some(expected) = expected_blob_sha {
                            let actual = entry.id().to_string();
                            if actual != *expected {
                                bail!(
                                    "file SHA mismatch for {path}: expected {expected}, got {actual}"
                                );
                            }
                        }
                    }
                    // (None, None): plain create, nothing to verify.
                    (None, None) => {}
                }
                let blob = repo.write_blob(content.as_slice())?;
                editor
                    .upsert(path.as_str(), EntryKind::Blob, blob.detach())
                    .with_context(|| format!("failed to stage '{path}'"))?;
            }
            TreeEdit::RemoveFile { path } => {
                let exists = match &tip_tree {
                    Some(tree) => tree
                        .lookup_entry_by_path(path)
                        .with_context(|| format!("failed to look up '{path}' in tree of {tip_display}"))?
                        .is_some(),
                    None => false,
                };
                if !exists {
                    bail!("file {path} does not exist at HEAD");
                }
                editor
                    .remove(path.as_str())
                    .with_context(|| format!("failed to remove '{path}'"))?;
            }
        }
    }

    let new_tree = editor
        .write()
        .map_err(|error| anyhow::anyhow!("failed to write edited tree: {error}"))?
        .detach();

    let now = gix::date::Time::now_utc();
    let stamp = format!("{} +0000", now.seconds);
    let signature = gix::actor::SignatureRef {
        name: author_name.as_bytes().into(),
        email: author_email.as_bytes().into(),
        time: stamp.as_str(),
    };
    let parents: Vec<gix::hash::ObjectId> = tip.into_iter().collect();
    let commit = repo
        .new_commit_as(signature, signature, message, new_tree, parents)
        .map_err(|error| anyhow::anyhow!("failed to create commit: {error}"))?;
    let new_sha = commit.id().detach();

    let expected = match tip {
        Some(tip) => PreviousValue::ExistingMustMatch(gix::refs::Target::Object(tip)),
        None => PreviousValue::MustNotExist,
    };
    repo.edit_reference(RefEdit {
        change: Change::Update {
            log: LogChange {
                mode: RefLog::AndReference,
                force_create_reflog: false,
                message: message.into(),
            },
            expected,
            new: gix::refs::Target::Object(new_sha),
        },
        name: branch_ref,
        deref: false,
    })
    .map_err(|error| anyhow::anyhow!("branch ref update rejected: {error}"))?;
    Ok(CommitTreeOutcome::Committed(new_sha.to_string()))
}

/// Same as [`verify_commit_with_repo`] but returns the full verification
/// outcome so callers can surface signer identity and failure reasons.
/// `Ok(None)` means the commit carries no signature at all.
pub fn verify_commit_details_with_repo(
    repo: &gix::Repository,
    sha: &str,
) -> anyhow::Result<Option<gix::commit::verify::Outcome>> {
    let commit = repo
        .rev_parse_single(sha)
        .with_context(|| format!("failed to resolve commit '{sha}'"))?
        .object()
        .with_context(|| format!("failed to read object for commit '{sha}'"))?
        .peel_to_commit()
        .with_context(|| format!("'{sha}' is not a commit"))?;
    commit
        .verify_signature()
        .map_err(|error| anyhow::anyhow!("signature verification failed for commit {sha}: {error}"))
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
