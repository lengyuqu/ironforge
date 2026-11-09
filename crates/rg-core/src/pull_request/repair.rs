//! One-off repair for `refs/pull/<n>/head`.
//!
//! The ref is written when a pull request is opened, when its head branch is
//! pushed to, and when a file is committed through the REST API (see
//! [`super::service::publish_head_ref`]). Pull requests opened before that
//! existed therefore have no anchor until their head branch is pushed again — for
//! a fork pull request, whose head branch may live in a fork nobody touches, that
//! can be forever.
//!
//! This walks the pull requests of one or every repository and re-publishes the
//! ref where it is missing or points somewhere other than the live head. It is
//! idempotent, it writes nothing but the ref, and a pull request whose head
//! branch is gone is reported, not repaired: the branch being deleted is not an
//! error, and the ref it used to have cannot be reconstructed from nothing.
//!
//! The base commit recorded when a pull request was opened
//! (`anchors.base_sha_when_opened`) is **not** backfilled: it was never stored
//! for older pull requests, and guessing it from the current base branch would
//! invent an anchor that was not there. The command says so in its output.

use sea_orm::{DatabaseConnection, EntityTrait};

use crate::error::{CoreError, CoreResult};
use rg_db::entities::{pull_request as pr_entity, repository as repo_entity};

/// What to look at. `repo` limits the walk to one repository (`owner/name`);
/// without it every repository is visited.
#[derive(Debug, Clone, Default)]
pub struct RepairOptions {
    pub owner: Option<String>,
    pub repo_name: Option<String>,
    /// Also repair merged and closed pull requests. Open ones are always
    /// included (a `merging` one counts as open: that state is transient).
    pub include_closed: bool,
    /// Report what would change without writing anything.
    pub dry_run: bool,
}

/// One ref this command would write, or did write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefAction {
    /// `owner/name` of the repository the ref belongs to (the target of the PR).
    pub repo: String,
    pub number: i64,
    pub head_sha: String,
    /// What the ref pointed at before, `None` when it did not exist.
    pub previous: Option<String>,
    pub is_fork: bool,
}

impl RefAction {
    /// `missing` when the ref had to be created, `stale` when it pointed
    /// somewhere else.
    pub fn kind(&self) -> &'static str {
        if self.previous.is_none() {
            "missing"
        } else {
            "stale"
        }
    }
}

/// One ref that was left alone, with the reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skipped {
    pub repo: String,
    /// `None` when the whole repository was skipped.
    pub number: Option<i64>,
    pub reason: String,
}

#[derive(Debug, Clone, Default)]
pub struct RepairReport {
    /// Pull requests examined.
    pub scanned: usize,
    /// Ref already at the live head.
    pub current: usize,
    /// Writes performed (always empty with `dry_run`).
    pub applied: Vec<RefAction>,
    /// Writes that `dry_run` would perform.
    pub planned: Vec<RefAction>,
    pub skipped: Vec<Skipped>,
}

impl RepairReport {
    /// The actions to print: what was written, or what would be with `dry_run`.
    pub fn actions(&self) -> &[RefAction] {
        if self.planned.is_empty() {
            &self.applied
        } else {
            &self.planned
        }
    }
}

/// Re-publish `refs/pull/<n>/head` where it is missing or stale.
pub async fn repair_pull_refs(
    db: &DatabaseConnection,
    repo_root: &std::path::Path,
    options: &RepairOptions,
) -> CoreResult<RepairReport> {
    let repos = repositories_in_scope(db, options).await?;
    let mut report = RepairReport::default();

    for repo in repos {
        let namespace = super::service::repository_namespace(db, &repo).await?;
        let label = format!("{}/{}", namespace, repo.name);
        let target_path = repo_root.join(format!("{}/{}.git", namespace, repo.name));
        if !target_path.exists() {
            // A repository row without a directory on disk is an operational
            // problem, not something this command can repair.
            report.skipped.push(Skipped {
                repo: label,
                number: None,
                reason: "repository directory is missing".to_string(),
            });
            continue;
        }

        for pr in pull_requests_in_scope(db, &repo, options.include_closed).await? {
            report.scanned += 1;
            let Some(head_sha) =
                super::service::live_head_sha(db, repo_root, &namespace, &repo.name, &pr).await?
            else {
                report.skipped.push(Skipped {
                    repo: label.clone(),
                    number: Some(pr.number),
                    reason: "head branch no longer exists".to_string(),
                });
                continue;
            };

            let refname = format!("refs/pull/{}/head", pr.number);
            let previous = rg_git::ops::try_rev_parse(&target_path, &refname).map_err(|error| {
                CoreError::internal(format!("failed to read {refname}: {error}"))
            })?;
            if previous.as_deref() == Some(head_sha.as_str()) {
                report.current += 1;
                continue;
            }

            let action = RefAction {
                repo: label.clone(),
                number: pr.number,
                head_sha: head_sha.clone(),
                previous,
                is_fork: pr.head_repo_id.is_some(),
            };
            if options.dry_run {
                report.planned.push(action);
                continue;
            }
            super::service::publish_head_ref(db, repo_root, &pr, &head_sha).await?;
            report.applied.push(action);
        }
    }

    Ok(report)
}

async fn repositories_in_scope(
    db: &DatabaseConnection,
    options: &RepairOptions,
) -> CoreResult<Vec<repo_entity::Model>> {
    match (&options.owner, &options.repo_name) {
        (Some(owner), Some(name)) => {
            let repo = crate::repo::service::find_repo_by_owner_name(db, owner, name)
                .await?
                .ok_or_else(|| {
                    CoreError::not_found(format!("repository {owner}/{name} not found"))
                })?;
            Ok(vec![repo])
        }
        (None, None) => repo_entity::Entity::find()
            .all(db)
            .await
            .map_err(|error| CoreError::internal(format!("failed to list repositories: {error}"))),
        _ => Err(CoreError::InvalidInput(
            "--repo takes the form owner/name, and both halves are required".to_string(),
        )),
    }
}

async fn pull_requests_in_scope(
    db: &DatabaseConnection,
    repo: &repo_entity::Model,
    include_closed: bool,
) -> CoreResult<Vec<pr_entity::Model>> {
    use sea_orm::{ColumnTrait, QueryFilter, QueryOrder};

    let mut query = pr_entity::Entity::find()
        .filter(pr_entity::Column::RepoId.eq(repo.id))
        .order_by_asc(pr_entity::Column::Number);
    if !include_closed {
        query = query.filter(pr_entity::Column::State.is_in(["open", "merging"]));
    }
    query
        .all(db)
        .await
        .map_err(|error| CoreError::internal(format!("failed to list pull requests: {error}")))
}
