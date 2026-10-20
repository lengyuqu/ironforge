//! gix-native `git blame`: attribute every line of a file at a given revision
//! to the commit that introduced it.
//!
//! Wraps `gix::Repository::blame_file` (rename tracking on, like the CLI) and
//! enriches each hunk with the introducing commit's author and summary. The
//! walk cost grows with history depth, so callers should run this on a
//! blocking thread; the size guards below keep a single request bounded.

use std::collections::HashMap;
use std::path::Path;

use anyhow::Context;

/// Files larger than this are refused — blame diffs the file against every
/// ancestor that touched it, so cost scales with size × history.
pub const MAX_BLAME_BYTES: usize = 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum BlameError {
    #[error("path '{0}' not found at the requested revision")]
    NotFound(String),
    #[error("'{0}' is not a regular file")]
    NotAFile(String),
    #[error("binary files cannot be blamed")]
    Binary,
    #[error("file is {size} bytes; blame is limited to {limit} bytes")]
    TooLarge { size: usize, limit: usize },
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

/// The commit a hunk is attributed to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlameCommit {
    pub sha: String,
    pub author_name: String,
    pub author_email: String,
    /// Author time as Unix seconds.
    pub author_time: i64,
    /// First line of the commit message.
    pub summary: String,
}

/// A run of consecutive lines introduced by one commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlameHunk {
    /// 1-based line number of the first line in the blamed revision.
    pub start_line: u32,
    /// 1-based line number of the first line in the introducing commit's file.
    pub original_start_line: u32,
    /// Set when the introducing commit knew the file under another name.
    pub original_path: Option<String>,
    pub commit: BlameCommit,
    pub lines: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Blame {
    /// SHA of the commit the revision resolved to.
    pub commit_sha: String,
    pub total_lines: u32,
    /// Ordered, non-overlapping, covering every line of the file.
    pub hunks: Vec<BlameHunk>,
}

/// Blame `path` as of `commitish` (branch, tag, or SHA).
pub fn blame_file(repo_path: &Path, commitish: &str, path: &str) -> Result<Blame, BlameError> {
    let repo = gix::open(repo_path)
        .with_context(|| format!("failed to open repository: {}", repo_path.display()))?;

    let tip = repo
        .rev_parse_single(commitish)
        .with_context(|| format!("failed to resolve revision '{commitish}'"))?
        .object()
        .with_context(|| format!("failed to read object for '{commitish}'"))?
        .peel_to_commit()
        .with_context(|| format!("'{commitish}' is not a commit"))?;

    let tree = tip.tree().context("failed to read commit tree")?;
    let entry = tree
        .lookup_entry_by_path(path)
        .context("failed to look up path in tree")?
        .ok_or_else(|| BlameError::NotFound(path.to_string()))?;
    if !entry.mode().is_blob() {
        return Err(BlameError::NotAFile(path.to_string()));
    }
    // Check size and binary-ness up front: gix would otherwise diff the whole
    // history before we get a chance to refuse.
    let blob = entry.id().object().context("failed to read blob")?;
    if blob.data.len() > MAX_BLAME_BYTES {
        return Err(BlameError::TooLarge {
            size: blob.data.len(),
            limit: MAX_BLAME_BYTES,
        });
    }
    if blob.data.contains(&0) {
        return Err(BlameError::Binary);
    }

    let options = gix::repository::blame_file::Options {
        rewrites: Some(gix::diff::Rewrites::default()),
        ..Default::default()
    };
    let outcome = repo
        .blame_file(path.into(), tip.id, options)
        .map_err(|e| anyhow::anyhow!("blame failed for '{path}': {e}"))?;

    let lines: Vec<String> = split_lines(&outcome.blob);
    let mut commits: HashMap<gix::ObjectId, BlameCommit> = HashMap::new();
    let mut hunks = Vec::with_capacity(outcome.entries.len());
    for entry in &outcome.entries {
        let commit = match commits.get(&entry.commit_id) {
            Some(c) => c.clone(),
            None => {
                let c = load_commit(&repo, entry.commit_id)?;
                commits.insert(entry.commit_id, c.clone());
                c
            }
        };
        let range = entry.range_in_blamed_file();
        let hunk_lines = lines
            .get(range.clone())
            .map(<[String]>::to_vec)
            .with_context(|| {
                format!("blame range {range:?} outside file ({} lines)", lines.len())
            })?;
        hunks.push(BlameHunk {
            start_line: entry.start_in_blamed_file + 1,
            original_start_line: entry.start_in_source_file + 1,
            original_path: entry
                .source_file_name
                .as_ref()
                .map(|name| String::from_utf8_lossy(name).into_owned()),
            commit,
            lines: hunk_lines,
        });
    }

    Ok(Blame {
        commit_sha: tip.id.to_string(),
        total_lines: lines.len() as u32,
        hunks,
    })
}

/// Split on `\n` the way blame counts lines: a trailing newline does not
/// start an extra empty line.
fn split_lines(blob: &[u8]) -> Vec<String> {
    let mut lines: Vec<String> = blob
        .split(|b| *b == b'\n')
        .map(|l| String::from_utf8_lossy(l.strip_suffix(b"\r").unwrap_or(l)).into_owned())
        .collect();
    if blob.ends_with(b"\n") || blob.is_empty() {
        lines.pop();
    }
    lines
}

fn load_commit(repo: &gix::Repository, id: gix::ObjectId) -> anyhow::Result<BlameCommit> {
    let commit = repo
        .find_object(id)
        .with_context(|| format!("blame commit {id} missing"))?
        .try_into_commit()
        .with_context(|| format!("{id} is not a commit"))?;
    let author = commit.author().context("failed to read commit author")?;
    let author_time = author
        .time
        .split_whitespace()
        .next()
        .and_then(|secs| secs.parse::<i64>().ok())
        .unwrap_or(0);
    let message = commit.message_raw_sloppy();
    Ok(BlameCommit {
        sha: id.to_string(),
        author_name: String::from_utf8_lossy(author.name).into_owned(),
        author_email: String::from_utf8_lossy(author.email).into_owned(),
        author_time,
        summary: String::from_utf8_lossy(message)
            .lines()
            .next()
            .unwrap_or("")
            .to_string(),
    })
}

#[cfg(test)]
mod tests {
    //! Parity tests: results must match `git blame --porcelain` line by line.

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
            .trim_end_matches('\n')
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
        let full = dir.join(file);
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(full, content).expect("write file");
        run_git(git, &["add", "-A"], dir);
        run_git(git, &["commit", "-m", message], dir);
    }

    /// `(sha, text)` per line from `git blame --porcelain`.
    fn cli_blame(
        git: &GitCommandGateway,
        dir: &Path,
        rev: &str,
        file: &str,
    ) -> Vec<(String, String)> {
        let out = git_out(git, &["blame", "--porcelain", rev, "--", file], dir);
        let mut result = Vec::new();
        let mut current = String::new();
        for line in out.lines() {
            if let Some(text) = line.strip_prefix('\t') {
                result.push((current.clone(), text.to_string()));
            } else if line.len() >= 40
                && line.as_bytes()[..40].iter().all(u8::is_ascii_hexdigit)
                && line.as_bytes().get(40) == Some(&b' ')
            {
                current = line[..40].to_string();
            }
        }
        result
    }

    fn per_line(blame: &Blame) -> Vec<(String, String)> {
        blame
            .hunks
            .iter()
            .flat_map(|h| h.lines.iter().map(|l| (h.commit.sha.clone(), l.clone())))
            .collect()
    }

    #[test]
    fn matches_cli_across_edits_and_inserts() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();
        let git = init_repo(dir);
        commit(&git, dir, "f.txt", "one\ntwo\nthree\n", "first");
        commit(&git, dir, "f.txt", "one\ntwo\ninserted\nthree\n", "insert");
        commit(
            &git,
            dir,
            "f.txt",
            "ONE\ntwo\ninserted\nthree\nfour\n",
            "edit and append",
        );

        let blame = blame_file(dir, "main", "f.txt").expect("blame");
        assert_eq!(blame.total_lines, 5);
        assert_eq!(per_line(&blame), cli_blame(&git, dir, "main", "f.txt"));
        assert_eq!(blame.commit_sha, git_out(&git, &["rev-parse", "main"], dir));

        // Metadata is filled from the introducing commit.
        let first_hunk = &blame.hunks[0];
        assert_eq!(first_hunk.start_line, 1);
        assert_eq!(first_hunk.commit.summary, "edit and append");
        assert_eq!(first_hunk.commit.author_name, "Tester");
        assert_eq!(first_hunk.commit.author_email, "tester@example.com");
        assert!(first_hunk.commit.author_time > 0);
    }

    #[test]
    fn hunks_are_contiguous_and_cover_the_file() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();
        let git = init_repo(dir);
        commit(&git, dir, "f.txt", "a\nb\nc\nd\n", "base");
        commit(&git, dir, "f.txt", "a\nB\nc\nD\n", "edit");

        let blame = blame_file(dir, "main", "f.txt").unwrap();
        let mut expected_next = 1;
        for hunk in &blame.hunks {
            assert_eq!(hunk.start_line, expected_next, "hunks must be gap-free");
            expected_next += hunk.lines.len() as u32;
        }
        assert_eq!(expected_next - 1, blame.total_lines);
    }

    #[test]
    fn follows_renames_and_reports_original_path() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();
        let git = init_repo(dir);
        commit(
            &git,
            dir,
            "old.txt",
            "alpha\nbeta\ngamma\ndelta\nepsilon\n",
            "add old",
        );
        run_git(&git, &["mv", "old.txt", "new.txt"], dir);
        run_git(&git, &["commit", "-m", "rename"], dir);
        let add_sha = git_out(&git, &["rev-parse", "HEAD~1"], dir);

        let blame = blame_file(dir, "main", "new.txt").unwrap();
        assert_eq!(per_line(&blame), cli_blame(&git, dir, "main", "new.txt"));
        assert!(blame.hunks.iter().all(|h| h.commit.sha == add_sha));
        assert_eq!(blame.hunks[0].original_path.as_deref(), Some("old.txt"));
    }

    #[test]
    fn blames_at_an_older_revision_and_nested_path() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();
        let git = init_repo(dir);
        commit(&git, dir, "src/lib.rs", "x\ny\n", "v1");
        let v1 = git_out(&git, &["rev-parse", "HEAD"], dir);
        commit(&git, dir, "src/lib.rs", "x\nz\n", "v2");

        let old = blame_file(dir, &v1, "src/lib.rs").unwrap();
        assert_eq!(per_line(&old), cli_blame(&git, dir, &v1, "src/lib.rs"));
        let new = blame_file(dir, "main", "src/lib.rs").unwrap();
        assert_eq!(new.hunks.len(), 2);
        assert_eq!(new.hunks[1].commit.summary, "v2");
    }

    #[test]
    fn handles_missing_trailing_newline_and_crlf() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();
        let git = init_repo(dir);
        commit(&git, dir, "f.txt", "one\r\ntwo", "no trailing newline");

        let blame = blame_file(dir, "main", "f.txt").unwrap();
        assert_eq!(blame.total_lines, 2);
        assert_eq!(
            blame.hunks[0].lines,
            vec!["one".to_string(), "two".to_string()]
        );
    }

    #[test]
    fn rejects_missing_directory_binary_and_oversized() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();
        let git = init_repo(dir);
        commit(&git, dir, "dir/a.txt", "a\n", "dir");
        commit(&git, dir, "bin.dat", "a\0b\n", "bin");
        let big = "x\n".repeat(MAX_BLAME_BYTES / 2 + 1);
        commit(&git, dir, "big.txt", &big, "big");

        assert!(matches!(
            blame_file(dir, "main", "nope.txt"),
            Err(BlameError::NotFound(_))
        ));
        assert!(matches!(
            blame_file(dir, "main", "dir"),
            Err(BlameError::NotAFile(_))
        ));
        assert!(matches!(
            blame_file(dir, "main", "bin.dat"),
            Err(BlameError::Binary)
        ));
        assert!(matches!(
            blame_file(dir, "main", "big.txt"),
            Err(BlameError::TooLarge { .. })
        ));
        assert!(blame_file(dir, "no-such-ref", "dir/a.txt").is_err());
    }
}
