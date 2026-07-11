//! Repository-scoped FIFO merge queue.

use std::collections::HashSet;
use std::path::Path;

use anyhow::{bail, Context, Result};
use chrono::{Duration, Utc};
use sea_orm::{DatabaseConnection, EntityTrait, Set};

use rg_db::entities::{merge_queue_entry, pull_request, repository};
use rg_db::ops::{merge_queue_ops, pull_request_ops};

use super::service::{self, MergeStrategy};

#[derive(Debug, serde::Serialize)]
pub struct MergeQueueProcessResult {
    pub merged: Vec<i64>,
    pub failed: Vec<i64>,
    pub waiting_reason: Option<String>,
}

pub async fn enqueue(
    db: &DatabaseConnection,
    repository: &repository::Model,
    pr: &pull_request::Model,
    actor_id: i64,
    strategy: MergeStrategy,
) -> Result<merge_queue_entry::Model> {
    if pr.repo_id != repository.id {
        bail!("pull request does not belong to this repository");
    }
    if pr.state != "open" || pr.is_draft {
        bail!("only an open, non-draft pull request can enter the merge queue");
    }

    // Queue ordering owns merge execution once a PR is enqueued.
    if pr.auto_merge_enabled {
        let mut active: pull_request::ActiveModel = pr.clone().into();
        active.auto_merge_enabled = Set(false);
        active.auto_merge_strategy = Set(None);
        active.auto_merge_enabled_by_id = Set(None);
        active.auto_merge_enabled_at = Set(None);
        active.updated_at = Set(Utc::now());
        pull_request_ops::update(db, active).await?;
    }
    merge_queue_ops::enqueue(db, repository.id, pr.id, actor_id, strategy.as_str()).await
}

pub async fn cancel(db: &DatabaseConnection, pr_id: i64) -> Result<bool> {
    merge_queue_ops::cancel(db, pr_id).await
}

pub async fn process_repository(
    db: &DatabaseConnection,
    repo_root: &Path,
    repository: &repository::Model,
) -> Result<MergeQueueProcessResult> {
    let namespace = service::repository_namespace(db, repository).await?;
    let mut result = MergeQueueProcessResult {
        merged: Vec::new(),
        failed: Vec::new(),
        waiting_reason: None,
    };

    loop {
        let Some(entry) = merge_queue_ops::list_by_repo(db, repository.id)
            .await?
            .into_iter()
            .next()
        else {
            break;
        };

        if entry.status == "running" {
            let stale = entry
                .started_at
                .is_some_and(|started| started < Utc::now() - Duration::minutes(30));
            if stale {
                merge_queue_ops::finish(
                    db,
                    entry.id,
                    "failed",
                    Some("merge-queue worker lease expired".into()),
                )
                .await?;
                result.failed.push(entry.pr_id);
                continue;
            }
            result.waiting_reason =
                Some("merge-queue worker is already processing the head".into());
            break;
        }

        let Some(pr) = pull_request::Entity::find_by_id(entry.pr_id)
            .one(db)
            .await?
        else {
            merge_queue_ops::finish(
                db,
                entry.id,
                "failed",
                Some("pull request not found".into()),
            )
            .await?;
            result.failed.push(entry.pr_id);
            continue;
        };
        if pr.state == "merged" {
            merge_queue_ops::finish(db, entry.id, "merged", None).await?;
            result.merged.push(pr.id);
            continue;
        }
        if pr.state != "open" || pr.is_draft {
            merge_queue_ops::finish(
                db,
                entry.id,
                "failed",
                Some(format!("pull request is {} or draft", pr.state)),
            )
            .await?;
            result.failed.push(pr.id);
            continue;
        }
        if let Err(error) = crate::branch_protection::service::check_merge_allowed(
            db,
            repository.id,
            &pr.base_branch,
            pr.id,
        )
        .await
        {
            result.waiting_reason = Some(error.to_string());
            break;
        }
        if !merge_queue_ops::claim(db, entry.id).await? {
            result.waiting_reason = Some("merge-queue head was claimed concurrently".into());
            break;
        }

        let strategy = MergeStrategy::parse(&entry.strategy)?;
        match service::merge_pr(
            db,
            repo_root,
            &namespace,
            &repository.name,
            pr.number,
            strategy,
        )
        .await
        {
            Ok(_) => {
                merge_queue_ops::finish(db, entry.id, "merged", None).await?;
                result.merged.push(pr.id);
            }
            Err(error) => {
                merge_queue_ops::finish(db, entry.id, "failed", Some(error.to_string())).await?;
                result.failed.push(pr.id);
            }
        }
    }
    Ok(result)
}

pub async fn process_for_head_commit(
    db: &DatabaseConnection,
    repo_root: &Path,
    source_repo_id: i64,
    commit_sha: &str,
) -> Result<Vec<MergeQueueProcessResult>> {
    let prs = pull_request_ops::list_open_for_head_commit(db, source_repo_id, commit_sha).await?;
    let mut seen = HashSet::new();
    let mut results = Vec::new();
    for pr in prs {
        if !seen.insert(pr.repo_id) {
            continue;
        }
        let repository = repository::Entity::find_by_id(pr.repo_id)
            .one(db)
            .await?
            .context("merge-queue repository not found")?;
        results.push(process_repository(db, repo_root, &repository).await?);
    }
    Ok(results)
}
