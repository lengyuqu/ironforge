//! Repository-scoped FIFO merge queue.

use std::collections::HashSet;
use std::path::Path;

use anyhow::{bail, Context, Result};
use chrono::{Duration, Utc};
use sea_orm::{DatabaseConnection, EntityTrait, Set, TransactionTrait};

use rg_db::entities::{merge_queue_entry, pull_request, repository};
use rg_db::ops::{merge_queue_ops, pull_request_ops};

use super::service::{self, MergeStrategy};

pub struct MergeQueueCi<'a> {
    pub trigger: &'a dyn crate::ci::CiTrigger,
    pub docker_enabled: bool,
    pub external_runners: bool,
    pub jwt_secret: Option<&'a str>,
    pub external_url: Option<&'a str>,
}

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

    // All writes (auto-merge flag off, queue entry upsert, events) go through
    // one transaction: a PR must never end up half-enqueued.
    let txn = db.begin().await.context("db: begin merge-queue enqueue transaction")?;

    // Queue ordering owns merge execution once a PR is enqueued.
    if pr.auto_merge_enabled {
        let mut active: pull_request::ActiveModel = pr.clone().into();
        active.auto_merge_enabled = Set(false);
        active.auto_merge_strategy = Set(None);
        active.auto_merge_enabled_by_id = Set(None);
        active.auto_merge_enabled_at = Set(None);
        active.updated_at = Set(Utc::now());
        pull_request_ops::update(&txn, active).await?;
        rg_db::ops::pr_event_ops::record(
            &txn,
            pr.repo_id,
            pr.id,
            Some(actor_id),
            "auto_merge_disabled",
            None,
            serde_json::json!({"reason": "merge_queue_enqueued"}),
        )
        .await?;
    }
    let entry =
        merge_queue_ops::enqueue(&txn, repository.id, pr.id, actor_id, strategy.as_str()).await?;
    rg_db::ops::pr_event_ops::record(
        &txn,
        pr.repo_id,
        pr.id,
        Some(actor_id),
        "merge_queue_enqueued",
        None,
        serde_json::json!({"entry_id": entry.id, "strategy": entry.strategy}),
    )
    .await?;
    txn.commit().await.context("db: commit merge-queue enqueue transaction")?;
    Ok(entry)
}

pub async fn cancel(
    db: &DatabaseConnection,
    repo_root: &Path,
    repository: &repository::Model,
    pr: &pull_request::Model,
    actor_id: i64,
) -> Result<bool> {
    let canceled = merge_queue_ops::cancel(db, pr.id).await?;
    if canceled {
        rg_db::ops::pr_event_ops::record(
            db,
            pr.repo_id,
            pr.id,
            Some(actor_id),
            "merge_queue_canceled",
            None,
            serde_json::json!({}),
        )
        .await?;
        cleanup_merge_group_ref(db, repo_root, repository, pr.id).await;
    }
    Ok(canceled)
}

async fn finish_entry(
    db: &DatabaseConnection,
    repo_root: &Path,
    entry: &merge_queue_entry::Model,
    status: &str,
    failure_reason: Option<String>,
) -> Result<merge_queue_entry::Model> {
    // Queue status + audit event are atomic; ref cleanup (a Git side effect)
    // runs after commit.
    let txn = db.begin().await.context("db: begin merge-queue finish transaction")?;
    let finished = merge_queue_ops::finish(&txn, entry.id, status, failure_reason.clone()).await?;
    rg_db::ops::pr_event_ops::record(
        &txn,
        entry.repo_id,
        entry.pr_id,
        None,
        &format!("merge_queue_{status}"),
        failure_reason,
        serde_json::json!({"entry_id": entry.id, "strategy": entry.strategy}),
    )
    .await?;
    txn.commit().await.context("db: commit merge-queue finish transaction")?;
    if let Ok(Some(repository)) = repository::Entity::find_by_id(entry.repo_id).one(db).await {
        cleanup_merge_group_ref(db, repo_root, &repository, entry.pr_id).await;
    }
    Ok(finished)
}

async fn cleanup_merge_group_ref(
    db: &DatabaseConnection,
    repo_root: &Path,
    repository: &repository::Model,
    pr_id: i64,
) {
    let Some(entry) = merge_queue_ops::find_by_pr(db, pr_id).await.ok().flatten() else {
        return;
    };
    if entry.merge_group_sha.is_none() {
        return;
    }
    let Ok(namespace) = service::repository_namespace(db, repository).await else {
        return;
    };
    let repo_path = repo_root.join(format!("{namespace}/{}.git", repository.name));
    let group_ref = format!("refs/merge-queue/{}", entry.id);
    if let Err(error) = rg_git::ops::delete_ref(&repo_path, &group_ref) {
        tracing::warn!(entry_id = entry.id, %error, "failed to delete merge-group ref");
    }
}

pub async fn process_repository(
    db: &DatabaseConnection,
    repo_root: &Path,
    repository: &repository::Model,
) -> Result<MergeQueueProcessResult> {
    process_repository_inner(db, repo_root, repository, None).await
}

pub async fn process_repository_with_ci(
    db: &DatabaseConnection,
    repo_root: &Path,
    repository: &repository::Model,
    ci: &MergeQueueCi<'_>,
) -> Result<MergeQueueProcessResult> {
    process_repository_inner(db, repo_root, repository, Some(ci)).await
}

async fn process_repository_inner(
    db: &DatabaseConnection,
    repo_root: &Path,
    repository: &repository::Model,
    ci: Option<&MergeQueueCi<'_>>,
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
                finish_entry(
                    db,
                    repo_root,
                    &entry,
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
            finish_entry(
                db,
                repo_root,
                &entry,
                "failed",
                Some("pull request not found".into()),
            )
            .await?;
            result.failed.push(entry.pr_id);
            continue;
        };
        if pr.state == "merged" {
            finish_entry(db, repo_root, &entry, "merged", None).await?;
            result.merged.push(pr.id);
            continue;
        }
        if pr.state != "open" || pr.is_draft {
            finish_entry(
                db,
                repo_root,
                &entry,
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
        if let Some(ci) = ci {
            match ensure_merge_group_ci(db, repo_root, repository, &entry, &pr, ci).await? {
                MergeGroupState::Ready => {}
                MergeGroupState::Waiting(reason) => {
                    result.waiting_reason = Some(reason);
                    break;
                }
                MergeGroupState::Failed => {
                    result.failed.push(pr.id);
                    continue;
                }
            }
        } else if let Some(pipeline_id) = entry.merge_group_pipeline_id {
            let pipeline = rg_db::ops::pipeline_ops::get_pipeline(db, pipeline_id).await?;
            match pipeline.as_ref().map(|pipeline| pipeline.status.as_str()) {
                Some("success") => {}
                Some("failed" | "canceled") => {
                    finish_entry(
                        db,
                        repo_root,
                        &entry,
                        "failed",
                        Some("merge-group CI failed".into()),
                    )
                    .await?;
                    result.failed.push(pr.id);
                    continue;
                }
                _ => {
                    result.waiting_reason = Some("merge-group CI is still running".into());
                    break;
                }
            }
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
                finish_entry(db, repo_root, &entry, "merged", None).await?;
                result.merged.push(pr.id);
            }
            Err(error) => {
                finish_entry(db, repo_root, &entry, "failed", Some(error.to_string())).await?;
                result.failed.push(pr.id);
            }
        }
    }
    Ok(result)
}

enum MergeGroupState {
    Ready,
    Waiting(String),
    Failed,
}

async fn ensure_merge_group_ci(
    db: &DatabaseConnection,
    repo_root: &Path,
    repository: &repository::Model,
    entry: &merge_queue_entry::Model,
    pr: &pull_request::Model,
    ci: &MergeQueueCi<'_>,
) -> Result<MergeGroupState> {
    let namespace = service::repository_namespace(db, repository).await?;
    let repo_path = repo_root.join(format!("{namespace}/{}.git", repository.name));
    let base_ref = format!("refs/heads/{}", pr.base_branch);
    let base_sha = rg_git::ops::rev_parse(&repo_path, &base_ref)?;
    let head_sha = pr
        .head_sha
        .clone()
        .context("pull request head SHA is missing")?;

    // Object fetch from the (possibly forked) head repository — gix-native
    // in-process object copy (no git CLI, no transport subprocess).
    if let Some(head_repo_id) = pr.head_repo_id {
        let head_repo = repository::Entity::find_by_id(head_repo_id)
            .one(db)
            .await?
            .context("pull request head repository not found")?;
        let head_namespace = service::repository_namespace(db, &head_repo).await?;
        let head_repo_path = repo_root.join(format!("{head_namespace}/{}.git", head_repo.name));
        let target_path = repo_path.clone();
        let head_sha = head_sha.clone();
        tokio::task::spawn_blocking(move || {
            rg_git::ops::copy_objects_from_source(&head_repo_path, &head_sha, &target_path)
        })
        .await
        .map_err(|error| anyhow::anyhow!("fork fetch task failed: {error}"))?
        .map_err(|error| anyhow::anyhow!("failed to fetch objects from head repo: {error}"))?;
    }

    if entry.merge_group_base_sha.as_deref() == Some(&base_sha)
        && entry.merge_group_head_sha.as_deref() == Some(&head_sha)
    {
        if let Some(pipeline_id) = entry.merge_group_pipeline_id {
            let pipeline = rg_db::ops::pipeline_ops::get_pipeline(db, pipeline_id)
                .await?
                .context("merge-group pipeline not found")?;
            return Ok(match pipeline.status.as_str() {
                "success" => MergeGroupState::Ready,
                "failed" | "canceled" => {
                    finish_entry(
                        db,
                        repo_root,
                        entry,
                        "failed",
                        Some(format!(
                            "merge-group pipeline #{} is {}",
                            pipeline.id, pipeline.status
                        )),
                    )
                    .await?;
                    MergeGroupState::Failed
                }
                _ => MergeGroupState::Waiting(format!(
                    "merge-group pipeline #{} is {}",
                    pipeline.id, pipeline.status
                )),
            });
        }
    }

    // `git merge-tree --write-tree` + `git commit-tree` equivalent, run on a
    // blocking thread: gix's Repository is !Send and must not live across
    // await points.
    enum GroupOutcome {
        Conflicts(usize),
        Committed(String),
    }
    let group_sha = tokio::task::spawn_blocking({
        let repo_path = repo_path.clone();
        let base_sha = base_sha.clone();
        let head_sha = head_sha.clone();
        let pr_number = pr.number;
        move || -> anyhow::Result<GroupOutcome> {
            let repo = gix::open(&repo_path)
                .map_err(|error| anyhow::anyhow!("failed to open repository: {error}"))?;
            let base_id = repo
                .rev_parse_single(base_sha.as_str())
                .map_err(|error| anyhow::anyhow!("failed to resolve base commit {base_sha}: {error}"))?;
            let head_id = repo
                .rev_parse_single(head_sha.as_str())
                .map_err(|error| anyhow::anyhow!("failed to resolve head commit {head_sha}: {error}"))?;

            let labels = gix::merge::blob::builtin_driver::text::Labels {
                current: Some(base_sha.as_str().into()),
                other: Some(head_sha.as_str().into()),
                ancestor: None,
            };
            let options: gix::merge::commit::Options = repo
                .tree_merge_options()
                .map_err(|error| anyhow::anyhow!("failed to get tree merge options: {error}"))?
                .into();
            let mut outcome = repo
                .merge_commits(base_id, head_id, labels, options)
                .map_err(|error| anyhow::anyhow!("merge group tree merge failed: {error}"))?;
            if !outcome.tree_merge.conflicts.is_empty() {
                return Ok(GroupOutcome::Conflicts(outcome.tree_merge.conflicts.len()));
            }
            let tree_id = outcome
                .tree_merge
                .tree
                .write()
                .map_err(|error| anyhow::anyhow!("failed to write merged tree: {error}"))?
                .detach();
            let message = format!("Merge queue group for PR #{pr_number}");
            let now = gix::date::Time::now_utc();
            // `SignatureRef.time` is the raw git timestamp string (`<seconds> <offset>`).
            let time_str = format!("{} +0000", now.seconds);
            let identity = gix::actor::SignatureRef {
                name: "IronForge Merge Queue".as_bytes().into(),
                email: "merge-queue@ironforge.local".as_bytes().into(),
                time: time_str.as_str(),
            };
            // `git commit-tree <tree> -p <base> -p <head> -m <message>` equivalent:
            // write the commit object without touching any reference.
            let group_commit = repo
                .new_commit_as(
                    identity,
                    identity,
                    &message,
                    tree_id,
                    [base_id.detach(), head_id.detach()],
                )
                .map_err(|error| anyhow::anyhow!("failed to create merge group commit: {error}"))?;
            Ok(GroupOutcome::Committed(
                group_commit.id().detach().to_string(),
            ))
        }
    })
    .await
    .map_err(|error| anyhow::anyhow!("merge group task join failed: {error}"))??;

    let group_sha = match group_sha {
        GroupOutcome::Conflicts(count) => {
            finish_entry(
                db,
                repo_root,
                entry,
                "failed",
                Some(format!("merge group conflicts: {count} files with conflicts")),
            )
            .await?;
            return Ok(MergeGroupState::Failed);
        }
        GroupOutcome::Committed(group_sha) => group_sha,
    };
    let group_ref = format!("refs/merge-queue/{}", entry.id);
    rg_git::ops::update_ref(&repo_path, &group_ref, &group_sha, "merge queue group")?;

    if !ci.trigger.has_ci_config(&repo_path, &group_sha) {
        return Ok(MergeGroupState::Ready);
    }
    let pipeline_id = ci
        .trigger
        .trigger_pipeline(crate::ci::TriggerPipelineParams {
            db,
            repo_path: &repo_path,
            repo_id: repository.id,
            commit_sha: &group_sha,
            ref_name: &group_ref,
            trigger_type: "merge_group",
            triggered_by: Some(entry.enqueued_by_id),
            docker_enabled: ci.docker_enabled,
            external_runners: ci.external_runners,
            jwt_secret: ci.jwt_secret,
            external_url: ci.external_url,
        })
        .await?;
    merge_queue_ops::set_merge_group(db, entry.id, &group_sha, &base_sha, &head_sha, pipeline_id)
        .await?;
    rg_db::ops::pr_event_ops::record(
        db,
        repository.id,
        pr.id,
        None,
        "merge_group_created",
        None,
        serde_json::json!({"commit_sha": group_sha, "pipeline_id": pipeline_id}),
    )
    .await?;
    Ok(MergeGroupState::Waiting(format!(
        "merge-group pipeline #{pipeline_id} is pending"
    )))
}

pub async fn process_for_head_commit(
    db: &DatabaseConnection,
    repo_root: &Path,
    source_repo_id: i64,
    commit_sha: &str,
) -> Result<Vec<MergeQueueProcessResult>> {
    process_for_head_commit_inner(db, repo_root, source_repo_id, commit_sha, None).await
}

pub async fn process_for_head_commit_with_ci(
    db: &DatabaseConnection,
    repo_root: &Path,
    source_repo_id: i64,
    commit_sha: &str,
    ci: &MergeQueueCi<'_>,
) -> Result<Vec<MergeQueueProcessResult>> {
    process_for_head_commit_inner(db, repo_root, source_repo_id, commit_sha, Some(ci)).await
}

async fn process_for_head_commit_inner(
    db: &DatabaseConnection,
    repo_root: &Path,
    source_repo_id: i64,
    commit_sha: &str,
    ci: Option<&MergeQueueCi<'_>>,
) -> Result<Vec<MergeQueueProcessResult>> {
    if let Some(entry) =
        merge_queue_ops::find_by_merge_group_sha(db, source_repo_id, commit_sha).await?
    {
        let repository = repository::Entity::find_by_id(entry.repo_id)
            .one(db)
            .await?
            .context("merge-group repository not found")?;
        let result = match ci {
            Some(ci) => process_repository_with_ci(db, repo_root, &repository, ci).await?,
            None => process_repository(db, repo_root, &repository).await?,
        };
        return Ok(vec![result]);
    }
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
        results.push(match ci {
            Some(ci) => process_repository_with_ci(db, repo_root, &repository, ci).await?,
            None => process_repository(db, repo_root, &repository).await?,
        });
    }
    Ok(results)
}
