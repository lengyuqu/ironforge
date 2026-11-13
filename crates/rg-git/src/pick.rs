//! Server-side `cherry-pick` and `revert` on a branch of a bare repository,
//! built on gix only (no `git` CLI, no worktree, no index).
//!
//! Both are one three-way tree merge followed by one new commit on top of the
//! branch tip:
//!
//! | operation   | merge base       | ours       | theirs           |
//! |-------------|------------------|------------|------------------|
//! | cherry-pick | tree of `C^`     | tip tree   | tree of `C`      |
//! | revert      | tree of `C`      | tip tree   | tree of `C^`     |
//!
//! `C^` is the first parent, or the parent picked with `mainline` for a merge
//! commit; for a root commit it is the empty tree.
//!
//! # Nothing half-done
//!
//! The merge is first run against an **in-memory** object store
//! ([`gix::Repository::with_object_memory`]), so a conflict or a dry run
//! writes no object at all. Only a clean, non-dry-run merge is repeated against
//! the real object database, the commit written, and the branch moved with an
//! expected-old-value guard — so the branch is either at the tip the caller
//! named or at the new commit, never in between. A failed ref update (the
//! branch moved while the commit was being written) leaves at most an
//! unreferenced commit object, collected by the next `git gc`.
//!
//! This module only *adds* functions; it does not touch [`crate::ops`].

use std::collections::BTreeSet;
use std::path::Path;

use anyhow::Context;
use gix::merge::tree::TreatAsUnresolved;
use gix::refs::transaction::{Change, LogChange, PreviousValue, RefEdit, RefLog};

/// What to apply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickKind {
    /// Re-apply the change a commit made.
    CherryPick,
    /// Undo the change a commit made.
    Revert,
}

impl PickKind {
    /// Stable lowercase name used in API bodies and audit rows.
    pub fn as_str(self) -> &'static str {
        match self {
            PickKind::CherryPick => "cherry_pick",
            PickKind::Revert => "revert",
        }
    }
}

/// One cherry-pick or revert to perform.
#[derive(Debug, Clone)]
pub struct PickRequest<'a> {
    pub kind: PickKind,
    /// Hex id (4..=64 chars) of the commit to apply. A revspec is not accepted.
    pub source: &'a str,
    /// Branch the new commit goes on.
    pub branch: &'a str,
    /// The commit `branch` must currently point at.
    pub expected_tip: &'a str,
    /// 1-based parent to treat as the mainline; required for a merge commit,
    /// refused for any other commit.
    pub mainline: Option<u32>,
    /// Replaces the generated subject and body. The standard trailer
    /// (`This reverts commit …` / `(cherry picked from commit …)`) is still
    /// appended.
    pub message: Option<&'a str>,
    /// Author and committer of the new commit.
    pub identity_name: &'a str,
    pub identity_email: &'a str,
    /// Merge and report, but write neither commit nor ref.
    pub dry_run: bool,
}

/// How a pick ended. Everything the caller can act on is a variant; only a
/// fault in the repository itself is an `Err`.
#[derive(Debug, PartialEq, Eq)]
pub enum PickOutcome {
    /// The commit was written and the branch moved to it.
    Committed {
        commit: String,
        tree: String,
        tip_before: String,
        /// Full id of the applied commit (the request may have abbreviated it).
        source: String,
    },
    /// Dry run: the merge is clean and would produce `tree` on top of
    /// `tip_before`. Nothing was written, so `tree` names no stored object.
    Planned {
        tree: String,
        tip_before: String,
        source: String,
    },
    /// The change does not apply cleanly; `paths` are the conflicting paths.
    /// Nothing was written and the branch did not move.
    Conflict {
        paths: Vec<String>,
        tip: String,
        source: String,
    },
    /// The result equals the tip's tree: the change is already there (or
    /// already gone), so there is nothing to commit.
    Empty { tip: String },
    /// The branch is not at `expected_tip` (`None`: the branch does not exist).
    TipAdvanced { actual: Option<String> },
    /// `source` is not a commit this repository holds.
    SourceNotFound,
    /// `source` is a merge commit with `parents` parents and no `mainline`.
    MergeNeedsMainline { parents: usize },
    /// `mainline` does not fit `source` (not a merge, or out of range).
    MainlineInvalid { parents: usize, mainline: u32 },
}

fn open_repo(repo_path: &Path) -> anyhow::Result<gix::Repository> {
    gix::open(repo_path).with_context(|| format!("failed to open repository {repo_path:?}"))
}

/// Result of the merge step.
enum Merged {
    Clean(gix::hash::ObjectId),
    Conflict(Vec<String>),
}

/// The three trees of a pick, already resolved to ids.
struct Trees {
    base: gix::hash::ObjectId,
    ours: gix::hash::ObjectId,
    theirs: gix::hash::ObjectId,
}

fn tree_of(
    repo: &gix::Repository,
    commit: gix::hash::ObjectId,
) -> anyhow::Result<gix::hash::ObjectId> {
    Ok(repo
        .find_object(commit)
        .with_context(|| format!("failed to read commit {commit}"))?
        .try_into_commit()
        .with_context(|| format!("{commit} is not a commit"))?
        .tree_id()
        .with_context(|| format!("failed to read tree of commit {commit}"))?
        .detach())
}

/// Three-way merge of `trees` in `repo`; a clean result tree is written to
/// `repo`'s object store (the in-memory one for the probe).
fn merge(repo: &gix::Repository, trees: &Trees) -> anyhow::Result<Merged> {
    let options: gix::merge::tree::Options = repo
        .tree_merge_options()
        .map_err(|error| anyhow::anyhow!("failed to get tree merge options: {error}"))?;
    let labels = gix::merge::blob::builtin_driver::text::Labels {
        current: Some("branch".into()),
        other: Some("picked".into()),
        ancestor: None,
    };
    let mut outcome = repo
        .merge_trees(trees.base, trees.ours, trees.theirs, labels, options)
        .map_err(|error| anyhow::anyhow!("failed to merge trees: {error}"))?;
    if outcome.has_unresolved_conflicts(TreatAsUnresolved::git()) {
        let mut paths = BTreeSet::new();
        for conflict in outcome
            .conflicts
            .iter()
            .filter(|c| c.is_unresolved(TreatAsUnresolved::git()))
        {
            paths.insert(conflict.ours.location().to_string());
            paths.insert(conflict.theirs.location().to_string());
        }
        return Ok(Merged::Conflict(paths.into_iter().collect()));
    }
    let tree = outcome
        .tree
        .write()
        .map_err(|error| anyhow::anyhow!("failed to write merged tree: {error}"))?
        .detach();
    Ok(Merged::Clean(tree))
}

/// First line of a commit message, as git's `%s` would show it.
fn subject(message: &str) -> &str {
    message.lines().next().unwrap_or("").trim_end()
}

/// The commit message of the new commit.
///
/// * revert: `Revert "<subject>"`, a blank line, `This reverts commit <sha>.`
///   (`, reversing\nchanges made to <parent>.` when a mainline was chosen) —
///   what `git revert` writes.
/// * cherry-pick: the source message verbatim, a blank line, then
///   `(cherry picked from commit <sha>)` — what `git cherry-pick -x` writes.
fn compose_message(
    kind: PickKind,
    source: &str,
    source_message: &str,
    mainline_parent: Option<&str>,
    override_message: Option<&str>,
) -> String {
    match kind {
        PickKind::Revert => {
            let head = match override_message {
                Some(message) => message.trim_end().to_string(),
                None => format!("Revert \"{}\"", subject(source_message)),
            };
            let trailer = match mainline_parent {
                Some(parent) => {
                    format!("This reverts commit {source}, reversing\nchanges made to {parent}.")
                }
                None => format!("This reverts commit {source}."),
            };
            format!("{head}\n\n{trailer}\n")
        }
        PickKind::CherryPick => {
            let head = match override_message {
                Some(message) => message.trim_end(),
                None => source_message.trim_end(),
            };
            format!("{head}\n\n(cherry picked from commit {source})\n")
        }
    }
}

fn is_hex_id(source: &str) -> bool {
    (4..=64).contains(&source.len()) && source.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Perform `request` on the bare repository at `repo_path`.
///
/// See the module documentation for the merge shape and the "nothing
/// half-done" guarantee.
pub fn pick_commit(repo_path: &Path, request: &PickRequest<'_>) -> anyhow::Result<PickOutcome> {
    let repo = open_repo(repo_path)?;

    // The branch tip must be exactly what the caller read.
    let branch_ref = format!("refs/heads/{}", request.branch);
    let tip = repo
        .try_find_reference(branch_ref.as_str())
        .ok()
        .flatten()
        .and_then(|mut reference| reference.peel_to_id().ok().map(|id| id.detach()));
    let Some(tip) = tip else {
        return Ok(PickOutcome::TipAdvanced { actual: None });
    };
    let expected = gix::hash::ObjectId::from_hex(request.expected_tip.as_bytes()).ok();
    if expected != Some(tip) {
        return Ok(PickOutcome::TipAdvanced {
            actual: Some(tip.to_string()),
        });
    }

    // The commit to apply. Only a plain hex id is resolved: no `HEAD~3`, no
    // `branch@{1}`, so the path segment cannot become a revspec.
    if !is_hex_id(request.source) {
        return Ok(PickOutcome::SourceNotFound);
    }
    let Ok(source_id) = repo.rev_parse_single(request.source) else {
        return Ok(PickOutcome::SourceNotFound);
    };
    let source_id = source_id.detach();
    let source_commit = match repo.find_object(source_id) {
        Ok(object) => match object.try_into_commit() {
            Ok(commit) => commit,
            Err(_) => return Ok(PickOutcome::SourceNotFound),
        },
        Err(_) => return Ok(PickOutcome::SourceNotFound),
    };

    // Which parent is "before" the commit.
    let parents: Vec<gix::hash::ObjectId> =
        source_commit.parent_ids().map(|p| p.detach()).collect();
    let parent_count = parents.len();
    let parent = match (parent_count, request.mainline) {
        (0, None) => None,
        (1, None) => Some(parents[0]),
        (n, None) => return Ok(PickOutcome::MergeNeedsMainline { parents: n }),
        (n, Some(m)) if n < 2 || m == 0 || m as usize > n => {
            return Ok(PickOutcome::MainlineInvalid {
                parents: n,
                mainline: m,
            })
        }
        (_, Some(m)) => Some(parents[m as usize - 1]),
    };

    // Probe in memory: a conflict or a dry run must leave no object behind.
    let probe = open_repo(repo_path)?.with_object_memory();
    let trees = trees_for(&probe, request.kind, tip, source_id, parent)?;
    let tip_tree = trees.ours;
    let merged_tree = match merge(&probe, &trees)? {
        Merged::Conflict(paths) => {
            return Ok(PickOutcome::Conflict {
                paths,
                tip: tip.to_string(),
                source: source_id.to_string(),
            })
        }
        Merged::Clean(tree) => tree,
    };
    if merged_tree == tip_tree {
        return Ok(PickOutcome::Empty {
            tip: tip.to_string(),
        });
    }
    if request.dry_run {
        return Ok(PickOutcome::Planned {
            tree: merged_tree.to_string(),
            tip_before: tip.to_string(),
            source: source_id.to_string(),
        });
    }

    // Real run: the same merge, now written to the object database.
    let trees = trees_for(&repo, request.kind, tip, source_id, parent)?;
    let tree = match merge(&repo, &trees)? {
        Merged::Clean(tree) => tree,
        Merged::Conflict(paths) => {
            // The probe said clean; a different answer means the merge is not
            // deterministic, which must not be papered over.
            anyhow::bail!("merge was clean in memory but conflicted on disk: {paths:?}");
        }
    };
    anyhow::ensure!(
        tree == merged_tree,
        "merge produced different trees in memory ({merged_tree}) and on disk ({tree})"
    );

    let source_message = String::from_utf8_lossy(
        source_commit
            .message_raw()
            .with_context(|| format!("failed to read message of {source_id}"))?,
    )
    .into_owned();
    let message = compose_message(
        request.kind,
        &source_id.to_string(),
        &source_message,
        request
            .mainline
            .and(parent)
            .map(|parent| parent.to_string())
            .as_deref(),
        request.message,
    );

    let now = gix::date::Time::now_utc();
    let stamp = format!("{} +0000", now.seconds);
    let signature = gix::actor::SignatureRef {
        name: request.identity_name.as_bytes().into(),
        email: request.identity_email.as_bytes().into(),
        time: stamp.as_str(),
    };
    let commit = repo
        .new_commit_as(signature, signature, message.as_str(), tree, [tip])
        .map_err(|error| anyhow::anyhow!("failed to create commit: {error}"))?
        .id()
        .detach();

    let reflog = format!(
        "{}: {}",
        request.kind.as_str().replace('_', "-"),
        subject(&message)
    );
    let edited = repo.edit_reference(RefEdit {
        change: Change::Update {
            log: LogChange {
                mode: RefLog::AndReference,
                force_create_reflog: false,
                message: reflog.into(),
            },
            expected: PreviousValue::ExistingMustMatch(gix::refs::Target::Object(tip)),
            new: gix::refs::Target::Object(commit),
        },
        name: branch_ref
            .as_str()
            .try_into()
            .map_err(|error| anyhow::anyhow!("invalid branch ref: {error}"))?,
        deref: false,
    });
    if edited.is_err() {
        // The branch moved while the commit was being written.
        let actual = repo
            .rev_parse_single(branch_ref.as_str())
            .ok()
            .map(|id| id.detach().to_string());
        return Ok(PickOutcome::TipAdvanced { actual });
    }
    Ok(PickOutcome::Committed {
        commit: commit.to_string(),
        tree: tree.to_string(),
        tip_before: tip.to_string(),
        source: source_id.to_string(),
    })
}

/// Resolve the three trees of a pick in `repo` (writing the empty tree there
/// when a root commit needs it as one side).
fn trees_for(
    repo: &gix::Repository,
    kind: PickKind,
    tip: gix::hash::ObjectId,
    source: gix::hash::ObjectId,
    parent: Option<gix::hash::ObjectId>,
) -> anyhow::Result<Trees> {
    let source_tree = tree_of(repo, source)?;
    let parent_tree = match parent {
        Some(parent) => tree_of(repo, parent)?,
        None => repo
            .write_object(gix::objs::Tree::empty())
            .context("failed to write the empty tree")?
            .detach(),
    };
    let ours = tree_of(repo, tip)?;
    Ok(match kind {
        PickKind::CherryPick => Trees {
            base: parent_tree,
            ours,
            theirs: source_tree,
        },
        PickKind::Revert => Trees {
            base: source_tree,
            ours,
            theirs: parent_tree,
        },
    })
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
        let git = GitCommandGateway::new().expect("git CLI available");
        run_git(&git, &["init", "-b", "main"], dir);
        run_git(&git, &["config", "user.email", "tester@example.com"], dir);
        run_git(&git, &["config", "user.name", "Tester"], dir);
        git
    }

    fn commit(
        git: &GitCommandGateway,
        dir: &Path,
        file: &str,
        content: &str,
        message: &str,
    ) -> String {
        std::fs::write(dir.join(file), content).expect("write file");
        run_git(git, &["add", "-A"], dir);
        run_git(git, &["commit", "-m", message], dir);
        git_out(git, &["rev-parse", "HEAD"], dir)
    }

    fn request<'a>(kind: PickKind, source: &'a str, tip: &'a str) -> PickRequest<'a> {
        PickRequest {
            kind,
            source,
            branch: "main",
            expected_tip: tip,
            mainline: None,
            message: None,
            identity_name: "Picker",
            identity_email: "picker@example.com",
            dry_run: false,
        }
    }

    fn tip(git: &GitCommandGateway, dir: &Path) -> String {
        git_out(git, &["rev-parse", "refs/heads/main"], dir)
    }

    #[test]
    fn cherry_pick_applies_the_change_like_git_does() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();
        let git = init_repo(dir);
        commit(&git, dir, "a.txt", "one\n", "a");
        run_git(&git, &["checkout", "-b", "feature"], dir);
        commit(&git, dir, "x.txt", "x\n", "add x\n\nbody line");
        let x = git_out(&git, &["rev-parse", "HEAD"], dir);
        run_git(&git, &["checkout", "main"], dir);
        let before = commit(&git, dir, "d.txt", "d\n", "d");

        let outcome = pick_commit(dir, &request(PickKind::CherryPick, &x, &before)).unwrap();
        let PickOutcome::Committed {
            commit, tip_before, ..
        } = outcome
        else {
            panic!("expected Committed, got {outcome:?}");
        };
        assert_eq!(tip_before, before);
        assert_eq!(tip(&git, dir), commit);

        // The tree is what `git cherry-pick` produces.
        run_git(&git, &["checkout", "-b", "reference", &before], dir);
        run_git(&git, &["cherry-pick", &x], dir);
        assert_eq!(
            git_out(&git, &["rev-parse", &format!("{commit}^{{tree}}")], dir),
            git_out(&git, &["rev-parse", "reference^{tree}"], dir)
        );
        // One new commit on top of the old tip, identity and trailer as documented.
        assert_eq!(
            git_out(&git, &["rev-parse", &format!("{commit}^")], dir),
            before
        );
        assert_eq!(
            git_out(
                &git,
                &["log", "-1", "--format=%an <%ae> / %cn <%ce>", &commit],
                dir
            ),
            "Picker <picker@example.com> / Picker <picker@example.com>"
        );
        let body = git_out(&git, &["log", "-1", "--format=%B", &commit], dir);
        assert_eq!(
            body,
            format!("add x\n\nbody line\n\n(cherry picked from commit {x})")
        );
    }

    #[test]
    fn revert_undoes_the_change_and_writes_the_git_message() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();
        let git = init_repo(dir);
        commit(&git, dir, "a.txt", "one\n", "a");
        let x = commit(&git, dir, "x.txt", "x\n", "add x");
        let before = commit(&git, dir, "d.txt", "d\n", "d");

        let outcome = pick_commit(dir, &request(PickKind::Revert, &x, &before)).unwrap();
        let PickOutcome::Committed { commit, .. } = outcome else {
            panic!("expected Committed, got {outcome:?}");
        };
        assert_eq!(tip(&git, dir), commit);
        // x.txt is gone, the rest stays.
        let files = git_out(&git, &["ls-tree", "--name-only", &commit], dir);
        assert_eq!(files, "a.txt\nd.txt");
        assert_eq!(
            git_out(&git, &["log", "-1", "--format=%B", &commit], dir),
            format!("Revert \"add x\"\n\nThis reverts commit {x}.")
        );
    }

    #[test]
    fn a_custom_message_replaces_the_subject_but_keeps_the_trailer() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();
        let git = init_repo(dir);
        commit(&git, dir, "a.txt", "one\n", "a");
        let x = commit(&git, dir, "x.txt", "x\n", "add x");
        let before = tip(&git, dir);
        let mut req = request(PickKind::Revert, &x, &before);
        req.message = Some("Back out x: it broke the build\n");
        let PickOutcome::Committed { commit, .. } = pick_commit(dir, &req).unwrap() else {
            panic!("expected Committed");
        };
        assert_eq!(
            git_out(&git, &["log", "-1", "--format=%B", &commit], dir),
            format!("Back out x: it broke the build\n\nThis reverts commit {x}.")
        );
    }

    #[test]
    fn a_conflict_reports_the_paths_and_writes_nothing() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();
        let git = init_repo(dir);
        commit(&git, dir, "a.txt", "one\n", "a");
        run_git(&git, &["checkout", "-b", "feature"], dir);
        let c = commit(&git, dir, "a.txt", "feature\n", "c");
        run_git(&git, &["checkout", "main"], dir);
        let before = commit(&git, dir, "a.txt", "main\n", "d");
        let objects_before = git_out(&git, &["count-objects", "-v"], dir);

        for dry_run in [false, true] {
            let mut req = request(PickKind::CherryPick, &c, &before);
            req.dry_run = dry_run;
            let outcome = pick_commit(dir, &req).unwrap();
            assert_eq!(
                outcome,
                PickOutcome::Conflict {
                    paths: vec!["a.txt".into()],
                    tip: before.clone(),
                    source: c.clone()
                }
            );
        }
        assert_eq!(tip(&git, dir), before);
        // No object was written: the in-memory probe kept everything to itself.
        assert_eq!(git_out(&git, &["count-objects", "-v"], dir), objects_before);
    }

    #[test]
    fn a_dry_run_moves_nothing_and_a_stale_tip_is_reported() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();
        let git = init_repo(dir);
        let a = commit(&git, dir, "a.txt", "one\n", "a");
        let x = commit(&git, dir, "x.txt", "x\n", "add x");
        let before = commit(&git, dir, "d.txt", "d\n", "d");

        let mut req = request(PickKind::Revert, &x, &before);
        req.dry_run = true;
        assert!(matches!(
            pick_commit(dir, &req).unwrap(),
            PickOutcome::Planned { .. }
        ));
        assert_eq!(tip(&git, dir), before);

        // `a` is not the tip any more.
        let stale = request(PickKind::Revert, &x, &a);
        assert_eq!(
            pick_commit(dir, &stale).unwrap(),
            PickOutcome::TipAdvanced {
                actual: Some(before.clone())
            }
        );
        let mut missing = request(PickKind::Revert, &x, &before);
        missing.branch = "nope";
        assert_eq!(
            pick_commit(dir, &missing).unwrap(),
            PickOutcome::TipAdvanced { actual: None }
        );
    }

    #[test]
    fn applying_a_change_that_is_already_there_is_empty() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();
        let git = init_repo(dir);
        commit(&git, dir, "a.txt", "one\n", "a");
        let x = commit(&git, dir, "x.txt", "x\n", "add x");
        let before = tip(&git, dir);
        // x is already on main: picking it again changes nothing.
        assert_eq!(
            pick_commit(dir, &request(PickKind::CherryPick, &x, &before)).unwrap(),
            PickOutcome::Empty {
                tip: before.clone()
            }
        );
        // Reverting twice: the second revert has nothing left to undo.
        let PickOutcome::Committed {
            commit: reverted, ..
        } = pick_commit(dir, &request(PickKind::Revert, &x, &before)).unwrap()
        else {
            panic!("first revert applies");
        };
        assert_eq!(
            pick_commit(dir, &request(PickKind::Revert, &x, &reverted)).unwrap(),
            PickOutcome::Empty { tip: reverted }
        );
    }

    #[test]
    fn merge_commits_need_a_mainline_and_other_commits_refuse_one() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();
        let git = init_repo(dir);
        commit(&git, dir, "a.txt", "one\n", "a");
        run_git(&git, &["checkout", "-b", "side"], dir);
        commit(&git, dir, "s.txt", "s\n", "s");
        run_git(&git, &["checkout", "main"], dir);
        let plain = commit(&git, dir, "m.txt", "m\n", "m");
        run_git(&git, &["merge", "--no-ff", "-m", "merge side", "side"], dir);
        let merge_commit = git_out(&git, &["rev-parse", "HEAD"], dir);
        let before = commit(&git, dir, "z.txt", "z\n", "z");

        assert_eq!(
            pick_commit(dir, &request(PickKind::Revert, &merge_commit, &before)).unwrap(),
            PickOutcome::MergeNeedsMainline { parents: 2 }
        );
        for bad in [0, 3] {
            let mut req = request(PickKind::Revert, &merge_commit, &before);
            req.mainline = Some(bad);
            assert_eq!(
                pick_commit(dir, &req).unwrap(),
                PickOutcome::MainlineInvalid {
                    parents: 2,
                    mainline: bad
                }
            );
        }
        let mut not_a_merge = request(PickKind::Revert, &plain, &before);
        not_a_merge.mainline = Some(1);
        assert_eq!(
            pick_commit(dir, &not_a_merge).unwrap(),
            PickOutcome::MainlineInvalid {
                parents: 1,
                mainline: 1
            }
        );

        // Reverting the merge relative to parent 1 removes what `side` brought.
        let mut req = request(PickKind::Revert, &merge_commit, &before);
        req.mainline = Some(1);
        let PickOutcome::Committed { commit, .. } = pick_commit(dir, &req).unwrap() else {
            panic!("revert with mainline applies");
        };
        let files = git_out(&git, &["ls-tree", "--name-only", &commit], dir);
        assert_eq!(files, "a.txt\nm.txt\nz.txt");
        let parent1 = git_out(&git, &["rev-parse", &format!("{merge_commit}^1")], dir);
        assert_eq!(
            git_out(&git, &["log", "-1", "--format=%B", &commit], dir),
            format!(
                "Revert \"merge side\"\n\nThis reverts commit {merge_commit}, reversing\nchanges made to {parent1}."
            )
        );
    }

    #[test]
    fn a_root_commit_can_be_picked_and_reverted() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();
        let git = init_repo(dir);
        let root = commit(&git, dir, "a.txt", "one\n", "root");
        let before = commit(&git, dir, "b.txt", "b\n", "b");

        // Reverting the root removes its file; b.txt stays.
        let PickOutcome::Committed {
            commit: reverted, ..
        } = pick_commit(dir, &request(PickKind::Revert, &root, &before)).unwrap()
        else {
            panic!("revert of an untouched root applies");
        };
        assert_eq!(
            git_out(&git, &["ls-tree", "--name-only", &reverted], dir),
            "b.txt"
        );

        // Cherry-picking a root onto an unrelated branch adds its files.
        run_git(&git, &["checkout", "--orphan", "other"], dir);
        run_git(&git, &["rm", "-rf", "."], dir);
        let other = commit(&git, dir, "o.txt", "o\n", "other root");
        run_git(&git, &["branch", "-f", "main", &other], dir);
        let PickOutcome::Committed { commit: picked, .. } =
            pick_commit(dir, &request(PickKind::CherryPick, &root, &other)).unwrap()
        else {
            panic!("cherry-pick of a root applies");
        };
        assert_eq!(
            git_out(&git, &["ls-tree", "--name-only", &picked], dir),
            "a.txt\no.txt"
        );
    }

    #[test]
    fn a_source_that_is_not_a_commit_is_not_found() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();
        let git = init_repo(dir);
        commit(&git, dir, "a.txt", "one\n", "a");
        let before = tip(&git, dir);
        let blob = git_out(&git, &["rev-parse", "HEAD:a.txt"], dir);
        for source in [
            "deadbeefdeadbeefdeadbeefdeadbeefdeadbeef",
            blob.as_str(),
            "HEAD",
            "main~1",
            "zz",
            "",
        ] {
            assert_eq!(
                pick_commit(dir, &request(PickKind::Revert, source, &before)).unwrap(),
                PickOutcome::SourceNotFound,
                "source {source:?}"
            );
        }
    }
}
