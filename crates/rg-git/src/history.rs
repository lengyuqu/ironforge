//! gix-native `git log -- <path>`: the commits that changed a file or directory.
//!
//! Follows git's default history simplification so the result matches the CLI:
//! a commit is listed when the entry at `path` differs from its parent (or from
//! every parent, for a merge). A merge whose entry equals one parent's is
//! skipped and only that parent is followed, so side branches that were
//! discarded by the merge do not leak into the history. Renames are not
//! followed (same as `git log` without `--follow`).

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashSet};
use std::path::Path;

use anyhow::Context;

/// Object id and kind of the entry at a path; `None` when the path is absent.
type PathState = Option<(gix::ObjectId, gix::object::tree::EntryKind)>;

/// Up to `limit` commits reachable from `commitish` that changed `path`,
/// newest first (commit time, ties in discovery order like git).
pub fn commits_touching_path(
    repo_path: &Path,
    commitish: &str,
    path: &str,
    limit: usize,
) -> anyhow::Result<Vec<gix::ObjectId>> {
    let repo = gix::open(repo_path)
        .with_context(|| format!("failed to open repository: {}", repo_path.display()))?;
    let path = path.trim_matches('/');
    let tip = repo
        .rev_parse_single(commitish)
        .with_context(|| format!("failed to resolve revision '{commitish}'"))?
        .object()
        .with_context(|| format!("failed to read object for '{commitish}'"))?
        .peel_to_commit()
        .with_context(|| format!("'{commitish}' is not a commit"))?;

    let mut found = Vec::new();
    if limit == 0 {
        return Ok(found);
    }

    // Max-heap on (commit time, earlier discovery first).
    let mut queue: BinaryHeap<(i64, Reverse<u64>, gix::ObjectId)> = BinaryHeap::new();
    let mut seen: HashSet<gix::ObjectId> = HashSet::new();
    let mut seq = 0u64;
    let mut push = |queue: &mut BinaryHeap<_>, id: gix::ObjectId, time: i64| {
        if seen.insert(id) {
            queue.push((time, Reverse(seq), id));
            seq += 1;
        }
    };
    push(&mut queue, tip.id, commit_time(&tip));

    while let Some((_, _, id)) = queue.pop() {
        let commit = repo
            .find_object(id)
            .with_context(|| format!("commit {id} missing"))?
            .try_into_commit()
            .with_context(|| format!("{id} is not a commit"))?;
        let state = path_state(&commit, path)?;
        let parents: Vec<gix::ObjectId> = commit.parent_ids().map(|p| p.detach()).collect();

        if parents.is_empty() {
            if state.is_some() {
                found.push(id);
            }
        } else {
            let mut parent_commits = Vec::with_capacity(parents.len());
            let mut same_as = None;
            for parent_id in &parents {
                let parent = repo
                    .find_object(*parent_id)
                    .with_context(|| format!("parent {parent_id} missing"))?
                    .try_into_commit()
                    .with_context(|| format!("{parent_id} is not a commit"))?;
                if same_as.is_none() && path_state(&parent, path)? == state {
                    same_as = Some(parent_commits.len());
                }
                parent_commits.push(parent);
            }
            match same_as {
                // TREESAME to a parent: not listed, follow only that parent.
                Some(index) => {
                    let parent = &parent_commits[index];
                    push(&mut queue, parent.id, commit_time(parent));
                }
                None => {
                    found.push(id);
                    for parent in &parent_commits {
                        push(&mut queue, parent.id, commit_time(parent));
                    }
                }
            }
        }

        if found.len() >= limit {
            break;
        }
    }
    Ok(found)
}

fn path_state(commit: &gix::Commit<'_>, path: &str) -> anyhow::Result<PathState> {
    let tree = commit.tree().context("failed to read commit tree")?;
    if path.is_empty() {
        return Ok(Some((tree.id, gix::object::tree::EntryKind::Tree)));
    }
    Ok(tree
        .lookup_entry_by_path(path)
        .context("failed to look up path in tree")?
        .map(|entry| (entry.object_id(), entry.mode().kind())))
}

fn commit_time(commit: &gix::Commit<'_>) -> i64 {
    commit
        .committer()
        .ok()
        .and_then(|sig| {
            sig.time
                .split_whitespace()
                .next()
                .and_then(|s| s.parse().ok())
        })
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    //! Parity tests against `git log --format=%H -- <path>`.

    use super::*;
    use crate::cli_gateway::GitCommandGateway;

    struct Repo {
        git: GitCommandGateway,
        dir: tempfile::TempDir,
        clock: i64,
    }

    impl Repo {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let git = GitCommandGateway::new().expect("git CLI available");
            let r = Self {
                git,
                dir,
                clock: 1_700_000_000,
            };
            r.run(&["init", "-b", "main"]);
            r.run(&["config", "user.email", "tester@example.com"]);
            r.run(&["config", "user.name", "Tester"]);
            r
        }

        fn path(&self) -> &Path {
            self.dir.path()
        }

        fn run(&self, args: &[&str]) {
            self.git
                .run(args, Some(self.path()))
                .unwrap()
                .ensure_success()
                .unwrap();
        }

        fn out(&self, args: &[&str]) -> String {
            self.git
                .run(args, Some(self.path()))
                .unwrap()
                .stdout_str()
                .trim_end_matches('\n')
                .to_string()
        }

        /// Commit with a distinct, increasing commit time so ordering is unambiguous.
        fn commit(&mut self, files: &[(&str, &str)], message: &str) -> String {
            for (name, content) in files {
                let full = self.path().join(name);
                std::fs::create_dir_all(full.parent().unwrap()).unwrap();
                std::fs::write(full, content).unwrap();
            }
            self.run(&["add", "-A"]);
            self.clock += 60;
            let date = format!("{} +0000", self.clock);
            let env = [
                ("GIT_AUTHOR_DATE", date.as_str()),
                ("GIT_COMMITTER_DATE", date.as_str()),
            ];
            self.git
                .run_with_env(
                    &["commit", "-m", message, "--allow-empty"],
                    Some(self.path()),
                    &env,
                )
                .unwrap()
                .ensure_success()
                .unwrap();
            self.out(&["rev-parse", "HEAD"])
        }

        fn merge(&mut self, branch: &str, extra: &[&str]) -> String {
            self.clock += 60;
            let date = format!("{} +0000", self.clock);
            let env = [
                ("GIT_AUTHOR_DATE", date.as_str()),
                ("GIT_COMMITTER_DATE", date.as_str()),
            ];
            let mut args = vec!["merge", "--no-ff", "-m", "merge"];
            args.extend_from_slice(extra);
            args.push(branch);
            self.git
                .run_with_env(&args, Some(self.path()), &env)
                .unwrap()
                .ensure_success()
                .unwrap();
            self.out(&["rev-parse", "HEAD"])
        }

        fn cli(&self, rev: &str, path: &str, limit: usize) -> Vec<String> {
            let n = format!("-n{limit}");
            let out = self.out(&["log", "--format=%H", &n, rev, "--", path]);
            out.lines()
                .filter(|l| !l.is_empty())
                .map(str::to_string)
                .collect()
        }

        fn ours(&self, rev: &str, path: &str, limit: usize) -> Vec<String> {
            commits_touching_path(self.path(), rev, path, limit)
                .unwrap()
                .into_iter()
                .map(|id| id.to_string())
                .collect()
        }

        fn assert_parity(&self, rev: &str, path: &str) {
            assert_eq!(
                self.ours(rev, path, 1000),
                self.cli(rev, path, 1000),
                "path '{path}'"
            );
        }
    }

    #[test]
    fn linear_history_lists_only_commits_that_changed_the_file() {
        let mut r = Repo::new();
        let a1 = r.commit(&[("a.txt", "1\n"), ("b.txt", "1\n")], "add both");
        let _b = r.commit(&[("b.txt", "2\n")], "touch b only");
        let a2 = r.commit(&[("a.txt", "2\n")], "touch a");
        let _c = r.commit(&[("c.txt", "x\n")], "unrelated");

        assert_eq!(r.ours("main", "a.txt", 100), vec![a2, a1]);
        r.assert_parity("main", "a.txt");
        r.assert_parity("main", "b.txt");
        r.assert_parity("main", "c.txt");
    }

    #[test]
    fn directories_and_nested_paths_match_git() {
        let mut r = Repo::new();
        r.commit(&[("src/lib.rs", "1\n"), ("README", "r\n")], "init");
        r.commit(&[("src/deep/mod.rs", "m\n")], "nested");
        r.commit(&[("README", "r2\n")], "readme");
        r.commit(&[("src/lib.rs", "2\n")], "lib");

        r.assert_parity("main", "src");
        r.assert_parity("main", "src/");
        r.assert_parity("main", "src/deep");
        r.assert_parity("main", "src/lib.rs");
        assert_eq!(r.ours("main", "src", 100).len(), 3);
    }

    #[test]
    fn missing_path_and_deleted_file() {
        let mut r = Repo::new();
        r.commit(&[("keep", "k\n"), ("gone.txt", "g\n")], "init");
        r.run(&["rm", "gone.txt"]);
        let del = r.commit(&[], "delete gone");

        assert!(r.ours("main", "never-existed", 100).is_empty());
        r.assert_parity("main", "never-existed");
        // Deleting a file counts as a change to it.
        assert_eq!(r.ours("main", "gone.txt", 100)[0], del);
        r.assert_parity("main", "gone.txt");
    }

    #[test]
    fn limit_truncates_newest_first() {
        let mut r = Repo::new();
        for i in 0..5 {
            r.commit(&[("f", &format!("{i}\n"))], &format!("c{i}"));
        }
        assert_eq!(r.ours("main", "f", 2), r.cli("main", "f", 2));
        assert_eq!(r.ours("main", "f", 2).len(), 2);
        assert!(commits_touching_path(r.path(), "main", "f", 0)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn merge_that_is_treesame_to_one_parent_is_skipped() {
        let mut r = Repo::new();
        r.commit(&[("f.txt", "base\n"), ("g.txt", "base\n")], "base");
        r.run(&["checkout", "-b", "feature"]);
        r.commit(&[("f.txt", "feature\n")], "feature edits f");
        r.run(&["checkout", "main"]);
        r.commit(&[("g.txt", "main\n")], "main edits g");
        r.merge("feature", &[]);

        // The merge result for f.txt equals the feature parent: git follows
        // that parent and does not list the merge itself.
        r.assert_parity("main", "f.txt");
        r.assert_parity("main", "g.txt");
        assert!(!r
            .ours("main", "f.txt", 100)
            .contains(&r.out(&["rev-parse", "main"])));
    }

    #[test]
    fn discarded_side_branch_is_not_followed() {
        let mut r = Repo::new();
        r.commit(&[("f.txt", "base\n")], "base");
        r.run(&["checkout", "-b", "feature"]);
        let side = r.commit(&[("f.txt", "side\n")], "side edit");
        r.run(&["checkout", "main"]);
        r.commit(&[("other", "o\n")], "main moves on");
        // `-s ours` records the merge but keeps main's f.txt: the side edit is discarded.
        r.merge("feature", &["-s", "ours"]);

        r.assert_parity("main", "f.txt");
        assert!(
            !r.ours("main", "f.txt", 100).contains(&side),
            "a branch the merge discarded must not appear in the file's history"
        );
    }

    #[test]
    fn conflict_merge_that_differs_from_both_parents_is_listed() {
        let mut r = Repo::new();
        r.commit(&[("f.txt", "line\n")], "base");
        r.run(&["checkout", "-b", "feature"]);
        r.commit(&[("f.txt", "feature\n")], "feature edit");
        r.run(&["checkout", "main"]);
        r.commit(&[("f.txt", "main\n")], "main edit");
        r.git
            .run(&["merge", "--no-ff", "feature"], Some(r.path()))
            .unwrap(); // conflicts
        std::fs::write(r.path().join("f.txt"), "resolved\n").unwrap();
        r.run(&["add", "f.txt"]);
        r.clock += 60;
        let date = format!("{} +0000", r.clock);
        let env = [
            ("GIT_AUTHOR_DATE", date.as_str()),
            ("GIT_COMMITTER_DATE", date.as_str()),
        ];
        r.git
            .run_with_env(&["commit", "-m", "resolve"], Some(r.path()), &env)
            .unwrap()
            .ensure_success()
            .unwrap();
        let resolve = r.out(&["rev-parse", "HEAD"]);

        r.assert_parity("main", "f.txt");
        assert_eq!(r.ours("main", "f.txt", 100)[0], resolve);
    }

    #[test]
    fn works_from_an_older_revision_and_unknown_revision_errors() {
        let mut r = Repo::new();
        let first = r.commit(&[("f", "1\n")], "one");
        r.commit(&[("f", "2\n")], "two");

        assert_eq!(r.ours(&first, "f", 100), vec![first.clone()]);
        r.assert_parity(&first, "f");
        assert!(commits_touching_path(r.path(), "no-such-ref", "f", 10).is_err());
    }
}
