//! Deleting a branch.
//!
//! Clients tidy up after a merge by deleting the head branch. The server refuses
//! the things that would lose work or break a rule, and names the reason:
//! the default branch, a branch a protection rule covers, a branch an open pull
//! request still uses, and a branch that moved since the caller looked at it.
//! Pull requests keep `refs/pull/<n>/head`, so a merged or closed pull request
//! survives its branch.

use rg_db::DatabaseConnection;

use crate::branch_protection::service::rule_matches_ref;

/// What a delete (or a dry run of one) did or would do.
#[derive(Debug, Clone)]
pub struct DeleteBranchReport {
    pub branch: String,
    /// The commit the branch pointed at, i.e. what is lost from the branch.
    pub sha: String,
    pub dry_run: bool,
}

/// Why a branch could not be deleted.
#[derive(Debug)]
pub enum DeleteBranchError {
    /// The name is not a usable branch name.
    InvalidName(String),
    NotFound,
    /// It is the repository's default branch.
    DefaultBranch,
    /// A branch protection rule covers it; carries the rule's pattern.
    Protected(String),
    /// Open pull requests still use it, as `(repository id, number)`.
    OpenPullRequests(Vec<(i64, i64)>),
    /// The branch is at `current`, not at `expected`.
    RefAdvanced {
        current: String,
        expected: String,
    },
    Other(anyhow::Error),
}

impl From<anyhow::Error> for DeleteBranchError {
    fn from(error: anyhow::Error) -> Self {
        Self::Other(error)
    }
}

/// A branch name safe to turn into `refs/heads/<name>`: non-empty, no control
/// characters, no `..`, no leading `-` or `/`, no trailing `/` or `.lock`, none
/// of git's other forbidden characters.
fn valid_branch_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with(['-', '/'])
        && !name.ends_with(['/', '.'])
        && !name.ends_with(".lock")
        && !name.contains("..")
        && !name.contains("//")
        && !name.contains("@{")
        && !name
            .chars()
            .any(|c| c.is_control() || matches!(c, ' ' | '~' | '^' | ':' | '?' | '*' | '[' | '\\'))
}

/// Delete `branch` of `repo` (a bare repository at `repo_path`).
///
/// `expected_sha` makes the delete a compare-and-swap: it only happens while the
/// branch is still at that commit. With `dry_run` every check runs and nothing is
/// deleted.
pub async fn delete_branch(
    db: &DatabaseConnection,
    repo: &rg_db::entities::repository::Model,
    repo_path: &std::path::Path,
    branch: &str,
    expected_sha: Option<&str>,
    dry_run: bool,
) -> Result<DeleteBranchReport, DeleteBranchError> {
    if !valid_branch_name(branch) {
        return Err(DeleteBranchError::InvalidName(format!(
            "'{branch}' is not a valid branch name"
        )));
    }
    let refname = format!("refs/heads/{branch}");
    let current = rg_git::ops::try_rev_parse(repo_path, &refname)
        .ok()
        .flatten()
        .ok_or(DeleteBranchError::NotFound)?;

    if branch == repo.default_branch {
        return Err(DeleteBranchError::DefaultBranch);
    }
    let rules = rg_db::ops::protected_branch_ops::list_by_repo(db, repo.id).await?;
    if let Some(rule) = rules.iter().find(|rule| rule_matches_ref(rule, &refname)) {
        return Err(DeleteBranchError::Protected(rule.branch_name.clone()));
    }
    let open = rg_db::ops::pull_request_ops::list_open_using_branch(db, repo.id, branch).await?;
    if !open.is_empty() {
        return Err(DeleteBranchError::OpenPullRequests(
            open.iter().map(|pr| (pr.repo_id, pr.number)).collect(),
        ));
    }
    if let Some(expected) = expected_sha {
        if current != expected {
            return Err(DeleteBranchError::RefAdvanced {
                current,
                expected: expected.to_string(),
            });
        }
    }
    if !dry_run {
        // Delete only if the branch is still where it was when it was checked.
        if let Err(error) = rg_git::ops::delete_ref_if(repo_path, &refname, &current) {
            let now = rg_git::ops::try_rev_parse(repo_path, &refname)
                .ok()
                .flatten();
            return Err(match now {
                Some(now) if now != current => DeleteBranchError::RefAdvanced {
                    current: now,
                    expected: current,
                },
                None => DeleteBranchError::NotFound,
                Some(_) => DeleteBranchError::Other(error),
            });
        }
    }
    Ok(DeleteBranchReport {
        branch: branch.to_string(),
        sha: current,
        dry_run,
    })
}

#[cfg(test)]
mod tests {
    use super::valid_branch_name;

    #[test]
    fn branch_names_are_checked_before_they_become_ref_names() {
        for good in ["feature", "agent/pass", "release/1.2", "a-b_c", "x.y"] {
            assert!(valid_branch_name(good), "{good}");
        }
        for bad in [
            "", "-x", "/x", "x/", "x.", "x.lock", "a..b", "a//b", "a b", "a~b", "a^b", "a:b",
            "a?b", "a*b", "a[b", "a\\b", "a@{b", "a\u{7}b",
        ] {
            assert!(!valid_branch_name(bad), "{bad:?}");
        }
    }
}
