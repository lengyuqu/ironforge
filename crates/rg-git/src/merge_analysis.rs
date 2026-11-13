//! What a merge of two commits would run into, in a form a caller can act on.
//!
//! `gix` already finds the conflicts of a tree merge; the pull-request merge
//! path used to look only at *whether* the list was empty and threw the files
//! away. This module keeps them: which paths, what kind of conflict, which
//! blobs, and — for text conflicts — where the conflict blocks are.
//!
//! Everything here is a pure function of a bare repository and two commit ids.
//! It moves no reference. What it does write into the object database are the
//! objects the merge itself produces (conflict-marker blobs, and — only when
//! asked — the merged tree); they are unreachable until somebody points a ref at
//! them and are collected like any other loose garbage.
//!
//! # Sides
//!
//! The merge is always computed as *base branch* (`ours`) with *head branch*
//! (`theirs`), the same orientation the pull-request merge uses. Updating a head
//! branch with its base is the same merge: the merged tree does not depend on
//! which side is called ours, so the same report serves both.
//!
//! # What "conflict" means here
//!
//! A merge conflicts when `gix` reports **any** entry, exactly as the
//! pull-request merge has always decided. An entry `gix` resolved on its own
//! (an auto-resolved text hunk, a path moved along with a renamed directory) is
//! still listed — flagged [`ConflictEntry::auto_resolved`] — because it makes
//! the merge refuse; hiding it would make the preview say "clean" about a merge
//! that then fails.
//!
//! # Blocks
//!
//! Text blocks are read from the conflict-marker blob `gix` wrote for the path
//! (`<<<<<<<` / `=======` / `>>>>>>>`), not recomputed with a second diff, so
//! they describe the same conflict the merge hit. Line numbers are lines of
//! that merged blob. A file that already contains marker-looking lines can add
//! phantom blocks; that is a limit of reading markers, and the entry still has
//! its three blob ids to re-derive the truth from.
//!
//! # Limits
//!
//! Hard-coded, not tunable per request: [`MAX_CONFLICT_FILES`],
//! [`MAX_BLOCKS_PER_FILE`], [`MAX_EXCERPT_BYTES`], [`MAX_TOTAL_EXCERPT_BYTES`],
//! [`MAX_BLOB_BYTES`]. Going over one sets a `*_truncated` flag; nothing is
//! cut silently.

use std::path::Path;

use anyhow::{anyhow, Context};

/// Conflicting paths listed individually; the rest are only counted.
pub const MAX_CONFLICT_FILES: usize = 50;
/// Conflict blocks listed per file; the rest are only counted.
pub const MAX_BLOCKS_PER_FILE: usize = 20;
/// Bytes of one side of one block kept as an excerpt.
pub const MAX_EXCERPT_BYTES: usize = 4096;
/// Bytes of excerpts across the whole report.
pub const MAX_TOTAL_EXCERPT_BYTES: usize = 256 * 1024;
/// A merged blob larger than this is not opened for blocks.
pub const MAX_BLOB_BYTES: u64 = 1024 * 1024;

/// The shape of one conflict.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictKind {
    /// Both sides changed the same lines of a file.
    Content,
    /// Both sides added the same path with different content.
    AddAdd,
    /// One side changed (or renamed) a file the other deleted.
    DeleteModify,
    /// The two sides renamed the same file differently, or two files to the
    /// same place.
    RenameRename,
    /// A file on one side, a directory (or a symlink) on the other.
    TypeMismatch,
    /// A submodule pointer both sides moved.
    Submodule,
    /// `gix` reports a conflict this module has no better name for.
    Other,
}

impl ConflictKind {
    /// The stable name used on the wire.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Content => "content",
            Self::AddAdd => "add_add",
            Self::DeleteModify => "delete_modify",
            Self::RenameRename => "rename_rename",
            Self::TypeMismatch => "type_mismatch",
            Self::Submodule => "submodule",
            Self::Other => "other",
        }
    }
}

/// Why a conflicting path has no blocks to show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentHint {
    /// The merged blob is over [`MAX_BLOB_BYTES`].
    Large,
    /// The content is binary.
    Binary,
    /// The path is a submodule.
    Submodule,
    /// The path is a symlink.
    Symlink,
    /// The merged blob could not be read, or there is none (a structural
    /// conflict has no merged text).
    Unavailable,
}

impl ContentHint {
    /// The stable name used on the wire.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Large => "large",
            Self::Binary => "binary",
            Self::Submodule => "submodule",
            Self::Symlink => "symlink",
            Self::Unavailable => "unavailable",
        }
    }
}

/// One `<<<<<<<` … `>>>>>>>` region of a conflicted text file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConflictBlock {
    /// Position among the file's blocks, from 0.
    pub index: usize,
    /// 1-based line of the opening marker in the merged blob.
    pub merged_start_line: usize,
    /// Lines the base branch contributed.
    pub ours_lines: usize,
    /// Lines the head branch contributed.
    pub theirs_lines: usize,
    /// The base branch's lines, cut to [`MAX_EXCERPT_BYTES`].
    pub ours_excerpt: String,
    /// The head branch's lines, cut to [`MAX_EXCERPT_BYTES`].
    pub theirs_excerpt: String,
    /// An excerpt was cut (by the per-side or the whole-report budget).
    pub excerpt_truncated: bool,
}

/// One conflicting path.
#[derive(Debug, Clone)]
pub struct ConflictEntry {
    /// Where the path is in the merged tree (the base branch's name for it).
    pub path: String,
    /// The path before a rename, when it differs from `path`.
    pub path_on_base: Option<String>,
    pub kind: ConflictKind,
    /// `gix` settled this one itself; it is listed because it still makes the
    /// merge refuse (see the module docs).
    pub auto_resolved: bool,
    /// Blob of the common ancestor, when the path existed there.
    pub base_blob: Option<String>,
    /// Blob on the base branch.
    pub ours_blob: Option<String>,
    /// Blob on the head branch.
    pub theirs_blob: Option<String>,
    /// `true` when blocks could be read (an entry with none is not "no blocks":
    /// see `content_hint`).
    pub content_available: bool,
    pub content_hint: Option<ContentHint>,
    pub blocks: Vec<ConflictBlock>,
    /// The file has more blocks than were listed.
    pub blocks_truncated: bool,
    /// How many blocks the file has in all (`blocks.len()` unless truncated).
    pub blocks_total: usize,
}

/// The conflicts of a merge.
#[derive(Debug, Clone)]
pub struct ConflictReport {
    /// Every conflicting path `gix` reported.
    pub total_files: usize,
    /// The first [`MAX_CONFLICT_FILES`] of them, in path order.
    pub files: Vec<ConflictEntry>,
    /// `total_files` is more than `files.len()`.
    pub files_truncated: bool,
}

/// What merging the head branch into the base branch comes to.
#[derive(Debug, Clone)]
pub enum MergeVerdict {
    /// No conflicts. `tree` is the merged tree when the caller asked for it to
    /// be written.
    Clean { tree: Option<String> },
    /// At least one path conflicts.
    Conflicts(ConflictReport),
    /// The two commits share no history, so there is nothing to merge against.
    NoMergeBase,
}

/// A merge computed by [`analyze_merge`].
#[derive(Debug, Clone)]
pub struct MergeAnalysis {
    /// The common ancestor, when there is one.
    pub merge_base: Option<String>,
    pub verdict: MergeVerdict,
}

fn open_repo(repo_path: &Path) -> anyhow::Result<gix::Repository> {
    gix::open(repo_path)
        .with_context(|| format!("failed to open repository: {}", repo_path.display()))
}

fn parse_oid(sha: &str) -> anyhow::Result<gix::hash::ObjectId> {
    gix::hash::ObjectId::from_hex(sha.as_bytes())
        .map_err(|e| anyhow!("invalid commit id '{sha}': {e}"))
}

/// Merge `head_sha` into `base_sha` in memory and describe the outcome.
///
/// Moves no ref. With `write_tree` a clean merge also writes its tree into the
/// object database and returns its id (needed to build a merge commit);
/// without it nothing but the objects `gix` creates on the way is written.
pub fn analyze_merge(
    repo_path: &Path,
    base_sha: &str,
    head_sha: &str,
    write_tree: bool,
) -> anyhow::Result<MergeAnalysis> {
    use gix::merge::blob::builtin_driver::text::Labels;

    let repo = open_repo(repo_path)?;
    let base = parse_oid(base_sha)?;
    let head = parse_oid(head_sha)?;

    let merge_base = repo
        .merge_base(base, head)
        .map_err(|e| anyhow!("failed to find the merge base: {e}"))?
        .map(|id| id.to_string());
    if merge_base.is_none() {
        return Ok(MergeAnalysis {
            merge_base: None,
            verdict: MergeVerdict::NoMergeBase,
        });
    }

    let labels = Labels {
        current: Some("base".into()),
        other: Some("head".into()),
        ancestor: None,
    };
    let options: gix::merge::commit::Options = repo
        .tree_merge_options()
        .map_err(|e| anyhow!("failed to get tree merge options: {e}"))?
        .into();
    let mut outcome = repo
        .merge_commits(base, head, labels, options)
        .map_err(|e| anyhow!("merge failed: {e}"))?;

    if outcome.tree_merge.conflicts.is_empty() {
        let tree = if write_tree {
            Some(
                outcome
                    .tree_merge
                    .tree
                    .write()
                    .map_err(|e| anyhow!("failed to write merged tree: {e}"))?
                    .to_string(),
            )
        } else {
            None
        };
        return Ok(MergeAnalysis {
            merge_base,
            verdict: MergeVerdict::Clean { tree },
        });
    }

    let report = build_report(&repo, &outcome.tree_merge.conflicts);
    Ok(MergeAnalysis {
        merge_base,
        verdict: MergeVerdict::Conflicts(report),
    })
}

fn build_report(
    repo: &gix::Repository,
    conflicts: &[gix::merge::tree::Conflict],
) -> ConflictReport {
    let mut entries: Vec<ConflictEntry> = conflicts
        .iter()
        .map(|c| describe_conflict(repo, c))
        .collect();
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    let total_files = entries.len();
    let files_truncated = total_files > MAX_CONFLICT_FILES;
    entries.truncate(MAX_CONFLICT_FILES);

    // One budget for every excerpt in the report.
    let mut budget = MAX_TOTAL_EXCERPT_BYTES;
    for entry in &mut entries {
        for block in &mut entry.blocks {
            for excerpt in [&mut block.ours_excerpt, &mut block.theirs_excerpt] {
                if excerpt.len() > budget {
                    cut_at_char_boundary(excerpt, budget);
                    block.excerpt_truncated = true;
                }
                budget -= excerpt.len();
            }
        }
    }

    ConflictReport {
        total_files,
        files: entries,
        files_truncated,
    }
}

fn cut_at_char_boundary(text: &mut String, max: usize) {
    if text.len() <= max {
        return;
    }
    let mut end = max;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    text.truncate(end);
}

fn describe_conflict(
    repo: &gix::Repository,
    conflict: &gix::merge::tree::Conflict,
) -> ConflictEntry {
    use gix::merge::tree::{Resolution, ResolutionFailure};

    let (our_change, their_change) = conflict.changes_in_resolution();
    let path = our_change.location().to_string();
    let source = our_change.source_location().to_string();
    let path_on_base = (source != path).then_some(source);

    let auto_resolved = !conflict.is_unresolved(gix::merge::tree::TreatAsUnresolved::git());

    let content = conflict.content_merge();
    let both_added = matches!(
        (our_change, their_change),
        (
            gix::diff::tree_with_rewrites::Change::Addition { .. },
            gix::diff::tree_with_rewrites::Change::Addition { .. }
        )
    );
    let kind = match &conflict.resolution {
        Err(failure) => match failure {
            ResolutionFailure::SubmoduleMerge | ResolutionFailure::SubmoduleAddAdd => {
                ConflictKind::Submodule
            }
            ResolutionFailure::OursRenamedTheirsRenamedToSameLocation
            | ResolutionFailure::OursRenamedTheirsRenamedDifferently { .. } => {
                ConflictKind::RenameRename
            }
            ResolutionFailure::OursModifiedTheirsDirectoryThenOursRenamed { .. }
            | ResolutionFailure::OursDirectoryTheirsNonDirectoryTheirsRenamed { .. }
            | ResolutionFailure::OursAddedTheirsAddedTypeMismatch { .. }
            | ResolutionFailure::OursModifiedTheirsRenamedTypeMismatch => {
                ConflictKind::TypeMismatch
            }
            ResolutionFailure::OursDeletedTheirsRenamed
            | ResolutionFailure::OursModifiedTheirsDeleted => ConflictKind::DeleteModify,
            ResolutionFailure::Unknown => ConflictKind::Other,
        },
        Ok(Resolution::Forced(_)) => ConflictKind::Other,
        Ok(_) if content.is_some() => {
            if both_added {
                ConflictKind::AddAdd
            } else {
                ConflictKind::Content
            }
        }
        Ok(_) => ConflictKind::Other,
    };

    let entries = conflict.entries();
    let blob_of = |slot: usize| entries[slot].map(|entry| entry.id.to_string());
    let base_blob = blob_of(0);
    let ours_blob = blob_of(1);
    let theirs_blob = blob_of(2);

    let is_link = entries.iter().flatten().any(|entry| entry.mode.is_link());
    let is_submodule = kind == ConflictKind::Submodule
        || entries.iter().flatten().any(|entry| entry.mode.is_commit());

    let mut entry = ConflictEntry {
        path,
        path_on_base,
        kind,
        auto_resolved,
        base_blob,
        ours_blob,
        theirs_blob,
        content_available: false,
        content_hint: None,
        blocks: Vec::new(),
        blocks_truncated: false,
        blocks_total: 0,
    };

    if is_submodule {
        entry.content_hint = Some(ContentHint::Submodule);
        return entry;
    }
    if is_link {
        entry.content_hint = Some(ContentHint::Symlink);
        return entry;
    }
    // Only a content merge leaves a merged blob with markers in it.
    let Some(merged) =
        content.filter(|merge| matches!(merge.resolution, gix::merge::blob::Resolution::Conflict))
    else {
        // A structural conflict has no merged text, and an auto-resolved text
        // merge has no markers left: nothing to show either way.
        entry.content_hint = Some(ContentHint::Unavailable);
        return entry;
    };

    match read_blob(repo, merged.merged_blob_id) {
        BlobRead::TooLarge => entry.content_hint = Some(ContentHint::Large),
        BlobRead::Missing => entry.content_hint = Some(ContentHint::Unavailable),
        BlobRead::Data(data) => {
            if looks_binary(&data) {
                entry.content_hint = Some(ContentHint::Binary);
            } else {
                let mut blocks = parse_blocks(&data);
                entry.content_available = true;
                entry.blocks_total = blocks.len();
                if blocks.len() > MAX_BLOCKS_PER_FILE {
                    blocks.truncate(MAX_BLOCKS_PER_FILE);
                    entry.blocks_truncated = true;
                }
                entry.blocks = blocks;
            }
        }
    }
    entry
}

enum BlobRead {
    TooLarge,
    Missing,
    Data(Vec<u8>),
}

fn read_blob(repo: &gix::Repository, id: gix::hash::ObjectId) -> BlobRead {
    match repo.find_header(id) {
        Ok(header) if header.size() > MAX_BLOB_BYTES => return BlobRead::TooLarge,
        Ok(_) => {}
        Err(_) => return BlobRead::Missing,
    }
    match repo.find_object(id) {
        Ok(object) => BlobRead::Data(object.data.to_vec()),
        Err(_) => BlobRead::Missing,
    }
}

/// Git's own heuristic: a NUL byte in the first 8000 bytes.
fn looks_binary(data: &[u8]) -> bool {
    data.iter().take(8000).any(|byte| *byte == 0)
}

const MARKER_LEN: usize = 7;

fn is_marker(line: &[u8], ch: u8) -> bool {
    if line.len() < MARKER_LEN || line[..MARKER_LEN].iter().any(|b| *b != ch) {
        return false;
    }
    // A marker is exactly the marker run, then end of line or a space and a label.
    match line.get(MARKER_LEN) {
        None | Some(b'\n') | Some(b'\r') | Some(b' ') => true,
        Some(_) => false,
    }
}

/// Read the conflict blocks out of a merged text blob.
pub(crate) fn parse_blocks(data: &[u8]) -> Vec<ConflictBlock> {
    #[derive(PartialEq)]
    enum Part {
        Outside,
        Ours,
        Base,
        Theirs,
    }
    let mut part = Part::Outside;
    let mut blocks = Vec::new();
    let mut start_line = 0usize;
    let (mut ours, mut theirs): (Vec<u8>, Vec<u8>) = (Vec::new(), Vec::new());
    let (mut ours_lines, mut theirs_lines) = (0usize, 0usize);

    for (number, line) in data.split_inclusive(|b| *b == b'\n').enumerate() {
        let number = number + 1;
        match part {
            Part::Outside => {
                if is_marker(line, b'<') {
                    part = Part::Ours;
                    start_line = number;
                    ours.clear();
                    theirs.clear();
                    ours_lines = 0;
                    theirs_lines = 0;
                }
            }
            Part::Ours | Part::Base => {
                if is_marker(line, b'|') {
                    part = Part::Base;
                } else if is_marker(line, b'=') {
                    part = Part::Theirs;
                } else if part == Part::Ours {
                    ours_lines += 1;
                    ours.extend_from_slice(line);
                }
            }
            Part::Theirs => {
                if is_marker(line, b'>') {
                    let (ours_excerpt, ours_cut) = excerpt(&ours);
                    let (theirs_excerpt, theirs_cut) = excerpt(&theirs);
                    blocks.push(ConflictBlock {
                        index: blocks.len(),
                        merged_start_line: start_line,
                        ours_lines,
                        theirs_lines,
                        ours_excerpt,
                        theirs_excerpt,
                        excerpt_truncated: ours_cut || theirs_cut,
                    });
                    part = Part::Outside;
                } else {
                    theirs_lines += 1;
                    theirs.extend_from_slice(line);
                }
            }
        }
    }
    blocks
}

fn excerpt(bytes: &[u8]) -> (String, bool) {
    let mut text = String::from_utf8_lossy(bytes).into_owned();
    let cut = text.len() > MAX_EXCERPT_BYTES;
    cut_at_char_boundary(&mut text, MAX_EXCERPT_BYTES);
    (text, cut)
}

/// Write a commit object with `tree` and `parents` and return its id. Moves no ref.
///
/// Author and committer are the same identity, stamped now.
pub fn create_commit_object(
    repo_path: &Path,
    tree_sha: &str,
    parents: &[&str],
    message: &str,
    name: &str,
    email: &str,
) -> anyhow::Result<String> {
    let repo = open_repo(repo_path)?;
    let tree = parse_oid(tree_sha)?;
    let parents = parents
        .iter()
        .map(|sha| parse_oid(sha))
        .collect::<anyhow::Result<Vec<_>>>()?;
    // `SignatureRef.time` is the raw git timestamp string (`<seconds> <offset>`).
    let stamp = format!("{} +0000", gix::date::Time::now_utc().seconds);
    let signature = gix::actor::SignatureRef {
        name: name.as_bytes().into(),
        email: email.as_bytes().into(),
        time: stamp.as_str(),
    };
    let commit = repo
        .new_commit_as(signature, signature, message, tree, parents)
        .map_err(|e| anyhow!("failed to create commit: {e}"))?;
    Ok(commit.id().to_string())
}

/// Move `ref_name` to `new_sha`, but only if it is still at `expected_old`.
///
/// The compare-and-swap form of [`crate::ops::update_ref`]: when the ref moved
/// (or is gone) between the caller's read and this call nothing is written and
/// the error says so. `new_sha` must already be in the object database.
pub fn update_ref_expecting(
    repo_path: &Path,
    ref_name: &str,
    new_sha: &str,
    expected_old: &str,
    log_message: &str,
) -> anyhow::Result<()> {
    use gix::refs::transaction::{Change, LogChange, PreviousValue, RefEdit, RefLog};
    use gix::refs::Target;

    let repo = open_repo(repo_path)?;
    let name: gix::refs::FullName = ref_name
        .try_into()
        .map_err(|e| anyhow!("invalid ref name '{ref_name}': {e}"))?;
    let new = parse_oid(new_sha)?;
    let expected = parse_oid(expected_old)?;
    repo.edit_reference(RefEdit {
        change: Change::Update {
            log: LogChange {
                mode: RefLog::AndReference,
                force_create_reflog: false,
                message: log_message.into(),
            },
            expected: PreviousValue::ExistingMustMatch(Target::Object(expected)),
            new: Target::Object(new),
        },
        name,
        deref: false,
    })
    .with_context(|| format!("failed to update ref '{ref_name}' (it moved or no longer exists)"))?;
    Ok(())
}

/// Whether `ancestor` is reachable from `descendant` (or they are the same
/// commit). Used to tell "the head already contains the base".
pub fn is_ancestor(repo_path: &Path, ancestor: &str, descendant: &str) -> anyhow::Result<bool> {
    let repo = open_repo(repo_path)?;
    let ancestor = parse_oid(ancestor)?;
    let descendant = parse_oid(descendant)?;
    if ancestor == descendant {
        return Ok(true);
    }
    Ok(repo
        .merge_base(ancestor, descendant)
        .map_err(|e| anyhow!("failed to find the merge base: {e}"))?
        .is_some_and(|base| base.detach() == ancestor))
}

#[cfg(test)]
mod tests {
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
        if let Some(parent) = Path::new(file).parent() {
            std::fs::create_dir_all(dir.join(parent)).unwrap();
        }
        std::fs::write(dir.join(file), content).expect("write file");
        run_git(git, &["add", "-A"], dir);
        run_git(git, &["commit", "-m", message], dir);
    }

    fn sha(git: &GitCommandGateway, dir: &Path, rev: &str) -> String {
        git_out(git, &["rev-parse", rev], dir)
    }

    fn conflicts(analysis: &MergeAnalysis) -> &ConflictReport {
        match &analysis.verdict {
            MergeVerdict::Conflicts(report) => report,
            other => panic!("expected conflicts, got {other:?}"),
        }
    }

    #[test]
    fn disjoint_changes_merge_clean_and_the_tree_is_only_written_on_request() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();
        let git = init_repo(dir);
        commit(&git, dir, "a.txt", "one\n", "a");
        run_git(&git, &["checkout", "-b", "feature"], dir);
        commit(&git, dir, "b.txt", "b\n", "b");
        run_git(&git, &["checkout", "main"], dir);
        commit(&git, dir, "c.txt", "c\n", "c");
        let (base, head) = (sha(&git, dir, "main"), sha(&git, dir, "feature"));

        let without = analyze_merge(dir, &base, &head, false).unwrap();
        assert!(matches!(
            without.verdict,
            MergeVerdict::Clean { tree: None }
        ));
        assert!(without.merge_base.is_some());

        let with = analyze_merge(dir, &base, &head, true).unwrap();
        let MergeVerdict::Clean { tree: Some(tree) } = with.verdict else {
            panic!("expected a written tree");
        };
        let listing = git_out(&git, &["ls-tree", "--name-only", &tree], dir);
        assert_eq!(listing, "a.txt\nb.txt\nc.txt");
    }

    #[test]
    fn same_lines_changed_on_both_sides_is_a_content_conflict_with_blocks() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();
        let git = init_repo(dir);
        commit(&git, dir, "f.txt", "top\nmiddle\nbottom\n", "base");
        run_git(&git, &["checkout", "-b", "feature"], dir);
        commit(
            &git,
            dir,
            "f.txt",
            "top\nhead one\nhead two\nbottom\n",
            "head",
        );
        run_git(&git, &["checkout", "main"], dir);
        commit(&git, dir, "f.txt", "top\nbase side\nbottom\n", "main");
        let (base, head) = (sha(&git, dir, "main"), sha(&git, dir, "feature"));

        let analysis = analyze_merge(dir, &base, &head, false).unwrap();
        let report = conflicts(&analysis);
        assert_eq!(report.total_files, 1);
        assert!(!report.files_truncated);
        let file = &report.files[0];
        assert_eq!(file.path, "f.txt");
        assert_eq!(file.kind, ConflictKind::Content);
        assert!(!file.auto_resolved);
        assert!(file.content_available);
        assert_eq!(file.blocks.len(), 1);
        let block = &file.blocks[0];
        assert_eq!(block.merged_start_line, 2);
        assert_eq!(block.ours_lines, 1);
        assert_eq!(block.theirs_lines, 2);
        assert_eq!(block.ours_excerpt, "base side\n");
        assert_eq!(block.theirs_excerpt, "head one\nhead two\n");
        assert!(file.base_blob.is_some() && file.ours_blob.is_some() && file.theirs_blob.is_some());
    }

    #[test]
    fn a_file_deleted_on_one_side_and_changed_on_the_other_is_delete_modify() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();
        let git = init_repo(dir);
        commit(&git, dir, "gone.txt", "keep\n", "base");
        commit(&git, dir, "other.txt", "x\n", "other");
        run_git(&git, &["checkout", "-b", "feature"], dir);
        commit(&git, dir, "gone.txt", "changed\n", "modify");
        run_git(&git, &["checkout", "main"], dir);
        run_git(&git, &["rm", "gone.txt"], dir);
        run_git(&git, &["commit", "-m", "delete"], dir);
        let (base, head) = (sha(&git, dir, "main"), sha(&git, dir, "feature"));

        let analysis = analyze_merge(dir, &base, &head, false).unwrap();
        let report = conflicts(&analysis);
        let file = &report.files[0];
        assert_eq!(file.path, "gone.txt");
        assert_eq!(file.kind, ConflictKind::DeleteModify);
        assert!(!file.content_available);
        assert!(file.blocks.is_empty());
        assert_eq!(file.content_hint, Some(ContentHint::Unavailable));
    }

    #[test]
    fn a_path_added_on_both_sides_with_different_content_is_add_add() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();
        let git = init_repo(dir);
        commit(&git, dir, "seed.txt", "seed\n", "seed");
        run_git(&git, &["checkout", "-b", "feature"], dir);
        commit(&git, dir, "new.txt", "from head\n", "head adds");
        run_git(&git, &["checkout", "main"], dir);
        commit(&git, dir, "new.txt", "from base\n", "base adds");
        let (base, head) = (sha(&git, dir, "main"), sha(&git, dir, "feature"));

        let analysis = analyze_merge(dir, &base, &head, false).unwrap();
        let file = &conflicts(&analysis).files[0];
        assert_eq!(file.path, "new.txt");
        assert_eq!(file.kind, ConflictKind::AddAdd);
        assert_eq!(file.blocks.len(), 1);
        assert!(file.base_blob.is_none());
    }

    #[test]
    fn a_file_against_a_directory_is_a_type_mismatch() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();
        let git = init_repo(dir);
        commit(&git, dir, "seed.txt", "seed\n", "seed");
        run_git(&git, &["checkout", "-b", "feature"], dir);
        commit(&git, dir, "thing", "a file\n", "head: file");
        run_git(&git, &["checkout", "main"], dir);
        commit(&git, dir, "thing/inner.txt", "in a dir\n", "base: dir");
        let (base, head) = (sha(&git, dir, "main"), sha(&git, dir, "feature"));

        let analysis = analyze_merge(dir, &base, &head, false).unwrap();
        let report = conflicts(&analysis);
        assert!(
            report
                .files
                .iter()
                .any(|f| f.kind == ConflictKind::TypeMismatch),
            "{report:?}"
        );
    }

    #[test]
    fn two_different_renames_of_one_file_are_rename_rename() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();
        let git = init_repo(dir);
        commit(
            &git,
            dir,
            "orig.txt",
            "line one\nline two\nline three\nline four\n",
            "base",
        );
        run_git(&git, &["checkout", "-b", "feature"], dir);
        run_git(&git, &["mv", "orig.txt", "head-name.txt"], dir);
        run_git(&git, &["commit", "-m", "head rename"], dir);
        run_git(&git, &["checkout", "main"], dir);
        run_git(&git, &["mv", "orig.txt", "base-name.txt"], dir);
        run_git(&git, &["commit", "-m", "base rename"], dir);
        let (base, head) = (sha(&git, dir, "main"), sha(&git, dir, "feature"));

        let analysis = analyze_merge(dir, &base, &head, false).unwrap();
        let report = conflicts(&analysis);
        assert!(
            report
                .files
                .iter()
                .any(|f| f.kind == ConflictKind::RenameRename),
            "{report:?}"
        );
    }

    #[test]
    fn a_binary_conflict_has_no_blocks_and_says_why() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();
        let git = init_repo(dir);
        std::fs::write(dir.join("bin.dat"), [0u8, 1, 2, 3]).unwrap();
        run_git(&git, &["add", "-A"], dir);
        run_git(&git, &["commit", "-m", "base"], dir);
        run_git(&git, &["checkout", "-b", "feature"], dir);
        std::fs::write(dir.join("bin.dat"), [0u8, 9, 9, 9]).unwrap();
        run_git(&git, &["commit", "-am", "head"], dir);
        run_git(&git, &["checkout", "main"], dir);
        std::fs::write(dir.join("bin.dat"), [0u8, 7, 7, 7]).unwrap();
        run_git(&git, &["commit", "-am", "base"], dir);
        let (base, head) = (sha(&git, dir, "main"), sha(&git, dir, "feature"));

        let analysis = analyze_merge(dir, &base, &head, false).unwrap();
        let file = &conflicts(&analysis).files[0];
        assert_eq!(file.path, "bin.dat");
        assert!(!file.content_available);
        assert!(file.blocks.is_empty());
        assert!(matches!(
            file.content_hint,
            Some(ContentHint::Binary) | Some(ContentHint::Unavailable)
        ));
    }

    #[test]
    fn many_conflicting_files_are_counted_but_only_the_first_are_listed() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();
        let git = init_repo(dir);
        let total = MAX_CONFLICT_FILES + 5;
        for i in 0..total {
            std::fs::write(dir.join(format!("f{i:03}.txt")), "base\n").unwrap();
        }
        run_git(&git, &["add", "-A"], dir);
        run_git(&git, &["commit", "-m", "base"], dir);
        run_git(&git, &["checkout", "-b", "feature"], dir);
        for i in 0..total {
            std::fs::write(dir.join(format!("f{i:03}.txt")), "head\n").unwrap();
        }
        run_git(&git, &["commit", "-am", "head"], dir);
        run_git(&git, &["checkout", "main"], dir);
        for i in 0..total {
            std::fs::write(dir.join(format!("f{i:03}.txt")), "main\n").unwrap();
        }
        run_git(&git, &["commit", "-am", "main"], dir);
        let (base, head) = (sha(&git, dir, "main"), sha(&git, dir, "feature"));

        let analysis = analyze_merge(dir, &base, &head, false).unwrap();
        let report = conflicts(&analysis);
        assert_eq!(report.total_files, total);
        assert_eq!(report.files.len(), MAX_CONFLICT_FILES);
        assert!(report.files_truncated);
        assert_eq!(report.files[0].path, "f000.txt");
    }

    #[test]
    fn unrelated_histories_have_no_merge_base() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();
        let git = init_repo(dir);
        commit(&git, dir, "a.txt", "a\n", "a");
        run_git(&git, &["checkout", "--orphan", "other"], dir);
        run_git(&git, &["rm", "-rf", "."], dir);
        commit(&git, dir, "z.txt", "z\n", "z");
        let (base, head) = (sha(&git, dir, "main"), sha(&git, dir, "other"));

        let analysis = analyze_merge(dir, &base, &head, false).unwrap();
        assert!(analysis.merge_base.is_none());
        assert!(matches!(analysis.verdict, MergeVerdict::NoMergeBase));
    }

    #[test]
    fn analysing_moves_no_ref() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();
        let git = init_repo(dir);
        commit(&git, dir, "f.txt", "a\n", "base");
        run_git(&git, &["checkout", "-b", "feature"], dir);
        commit(&git, dir, "f.txt", "head\n", "head");
        run_git(&git, &["checkout", "main"], dir);
        commit(&git, dir, "f.txt", "main\n", "main");
        let before = git_out(&git, &["for-each-ref"], dir);
        let (base, head) = (sha(&git, dir, "main"), sha(&git, dir, "feature"));
        analyze_merge(dir, &base, &head, true).unwrap();
        assert_eq!(git_out(&git, &["for-each-ref"], dir), before);
    }

    #[test]
    fn blocks_parse_diff3_style_and_cut_long_excerpts() {
        let mut text = Vec::new();
        text.extend_from_slice(b"keep\n<<<<<<< base\nours 1\n||||||| ancestor\norig\n=======\n");
        text.extend_from_slice(&vec![b'x'; MAX_EXCERPT_BYTES + 50]);
        text.extend_from_slice(b"\n>>>>>>> head\ntail\n");
        let blocks = parse_blocks(&text);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].merged_start_line, 2);
        assert_eq!(blocks[0].ours_lines, 1);
        assert_eq!(blocks[0].ours_excerpt, "ours 1\n");
        assert_eq!(blocks[0].theirs_lines, 1);
        assert_eq!(blocks[0].theirs_excerpt.len(), MAX_EXCERPT_BYTES);
        assert!(blocks[0].excerpt_truncated);
    }

    #[test]
    fn update_ref_expecting_refuses_a_moved_ref_and_moves_a_matching_one() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();
        let git = init_repo(dir);
        commit(&git, dir, "a.txt", "1\n", "one");
        let first = sha(&git, dir, "main");
        commit(&git, dir, "a.txt", "2\n", "two");
        let second = sha(&git, dir, "main");

        // `main` is at `second`; claiming it is at `first` must fail and change nothing.
        assert!(update_ref_expecting(dir, "refs/heads/main", &first, &first, "test").is_err());
        assert_eq!(sha(&git, dir, "main"), second);

        update_ref_expecting(dir, "refs/heads/main", &first, &second, "test").unwrap();
        assert_eq!(sha(&git, dir, "main"), first);
    }

    #[test]
    fn a_merge_commit_object_keeps_both_parents_and_moves_no_ref() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();
        let git = init_repo(dir);
        commit(&git, dir, "a.txt", "a\n", "a");
        run_git(&git, &["checkout", "-b", "feature"], dir);
        commit(&git, dir, "b.txt", "b\n", "b");
        run_git(&git, &["checkout", "main"], dir);
        commit(&git, dir, "c.txt", "c\n", "c");
        let (base, head) = (sha(&git, dir, "main"), sha(&git, dir, "feature"));
        let analysis = analyze_merge(dir, &base, &head, true).unwrap();
        let MergeVerdict::Clean { tree: Some(tree) } = analysis.verdict else {
            panic!("clean merge expected");
        };
        let refs_before = git_out(&git, &["for-each-ref"], dir);
        let commit_sha = create_commit_object(
            dir,
            &tree,
            &[&head, &base],
            "Merge",
            "alice-agent-bot",
            "alice-agent-bot@agents.invalid",
        )
        .unwrap();
        assert_eq!(git_out(&git, &["for-each-ref"], dir), refs_before);
        assert_eq!(
            git_out(&git, &["log", "-1", "--format=%P", &commit_sha], dir),
            format!("{head} {base}")
        );
        assert_eq!(
            git_out(&git, &["log", "-1", "--format=%an <%ae>", &commit_sha], dir),
            "alice-agent-bot <alice-agent-bot@agents.invalid>"
        );
    }

    #[test]
    fn is_ancestor_follows_history() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();
        let git = init_repo(dir);
        commit(&git, dir, "a.txt", "1\n", "one");
        let first = sha(&git, dir, "main");
        commit(&git, dir, "a.txt", "2\n", "two");
        let second = sha(&git, dir, "main");
        assert!(is_ancestor(dir, &first, &second).unwrap());
        assert!(is_ancestor(dir, &second, &second).unwrap());
        assert!(!is_ancestor(dir, &second, &first).unwrap());
    }
}
