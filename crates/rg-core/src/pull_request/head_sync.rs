//! Keep a pull request's stored head commit equal to the branch it tracks.
//!
//! Everything that decides whether a pull request may merge — the review votes,
//! the CI conclusions, the protection gates — is judged against the **stored**
//! `head_sha`, not against the branch tip read at merge time. That is deliberate:
//! it pins a verdict to the commit it was formed about. What was missing was the
//! other half: nothing moved the stored head forward when the branch did, so a
//! pull request could keep a head commit from the day it was opened while the
//! branch — and what a merge would actually land — moved on.
//!
//! This module is that half. Every push that lands in a repository calls
//! [`sync_pr_head_on_push`]; each open pull request whose head branch is the
//! pushed branch is moved to the pushed commit.
//!
//! # Compare-and-set, not last-write-wins
//!
//! The update is `UPDATE ... WHERE id = ? AND head_sha = <the value we read>`.
//! A push notification can arrive twice (two receive-pack tasks, a retried
//! hook), and a server-side branch update can race with a client push. The
//! loser of such a race must not overwrite the winner: writing the older of two
//! notifications would roll the head back and make the votes for the newer
//! commit stop counting. Zero affected rows therefore means "someone else got
//! there first" ([`HeadSyncOutcome::Raced`]) and the sync stops; it does not
//! retry.
//!
//! # What "the old approvals are void" means
//!
//! Review votes live in `pr_reviews` and are not copied anywhere. A vote counts
//! only while its `commit_id` equals the stored head, so moving the head is what
//! voids the old votes — there is nothing to delete. Tickets filed through the
//! generic approval primitive (`authorization_requests`, `#19` step 2) are
//! different: a pending request is bound to `(action, subject, head_sha)` and
//! would keep sitting there. They are expired in place (never deleted) through
//! `rg_db::ops::authorization_ops::expire_on_head_sha_change`, so the audit trail
//! keeps the row and its `head_sha_changed` reason.
//!
//! # Legacy rows without a head commit
//!
//! A pull request created before the head was recorded has `head_sha = NULL`.
//! Such a row cannot be "stale" (there is nothing to be stale against), so it is
//! filled in from the push instead of being treated as a race. See
//! `docs/plans/T07-plan.md` §4.

use chrono::Utc;
use sea_orm::sea_query::Expr;
use sea_orm::{ColumnTrait, Condition, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder};

use rg_db::entities::pull_request::{self, Model as PullRequest};
use rg_db::ops::authorization_ops;

use crate::authorization::Action;
use crate::error::{CoreContext, CoreResult};

/// One open pull request that tracks the pushed branch, and what the push did
/// to its stored head.
#[derive(Debug, Clone)]
pub struct SyncedPullRequest {
    /// The row as it was read, before the update. Its `head_sha` is the
    /// "previous" value and its `number`/`repo_id` let the caller publish the
    /// `refs/pull/<n>/head` mirror.
    pub pull_request: PullRequest,
    pub outcome: HeadSyncOutcome,
}

/// The result of syncing one pull request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeadSyncOutcome {
    /// The stored head already was the pushed commit. Nothing was written and
    /// no ticket was touched.
    Unchanged,
    /// The stored head moved from `previous_head_sha` to the pushed commit.
    /// `expired_requests` is the number of pending approval tickets that were
    /// bound to another commit and are now `expired`.
    Updated {
        previous_head_sha: Option<String>,
        expired_requests: u64,
    },
    /// A concurrent writer changed the row first. It was **not** overwritten.
    Raced,
}

/// Move every open pull request whose head branch is `branch_name` in
/// `source_repo_id` to `new_sha`.
///
/// A repository can have more than one open pull request for the same branch
/// (for example a fork pull request and a same-repository one), so the result is
/// one entry per pull request, ordered by id. An empty vector means no open pull
/// request tracks the branch — the common case for a plain push.
///
/// The database row is the unit of the compare-and-set; a failure to expire
/// tickets is returned as an error after the head has already moved (the votes
/// themselves stop counting either way, see the module docs).
pub async fn sync_pr_head_on_push(
    db: &DatabaseConnection,
    source_repo_id: i64,
    branch_name: &str,
    new_sha: &str,
) -> CoreResult<Vec<SyncedPullRequest>> {
    let pull_requests = pull_request::Entity::find()
        .filter(head_repository_condition(source_repo_id))
        .filter(pull_request::Column::HeadBranch.eq(branch_name))
        .filter(pull_request::Column::State.eq("open"))
        .order_by_asc(pull_request::Column::Id)
        .all(db)
        .await
        .context("db: find open pull requests for pushed branch")?;

    let now = Utc::now();
    let mut synced = Vec::with_capacity(pull_requests.len());
    for pr in pull_requests {
        let previous_head_sha = pr.head_sha.clone();
        if previous_head_sha.as_deref() == Some(new_sha) {
            synced.push(SyncedPullRequest {
                pull_request: pr,
                outcome: HeadSyncOutcome::Unchanged,
            });
            continue;
        }

        let affected = cas_set_head(db, pr.id, previous_head_sha.as_deref(), new_sha, now).await?;
        if affected == 0 {
            // Another writer moved the row between our read and our write.
            synced.push(SyncedPullRequest {
                pull_request: pr,
                outcome: HeadSyncOutcome::Raced,
            });
            continue;
        }

        // The head really changed, so every pending merge ticket bound to a
        // different commit is void. Expired in place: the row stays, with
        // `state_reason = head_sha_changed`.
        let expired_requests = authorization_ops::expire_on_head_sha_change(
            db,
            pr.repo_id,
            Action::ProtectedBranchMerge.as_str(),
            &pr.id.to_string(),
            new_sha,
            now,
        )
        .await?;

        synced.push(SyncedPullRequest {
            pull_request: pr,
            outcome: HeadSyncOutcome::Updated {
                previous_head_sha,
                expired_requests,
            },
        });
    }
    Ok(synced)
}

/// `UPDATE pull_requests SET head_sha = ? WHERE id = ? AND head_sha IS <expected>`.
///
/// Returns the number of rows affected, which is 0 when the stored value is no
/// longer `expected` (or is no longer NULL). See the module docs for why the
/// caller must not retry on 0.
async fn cas_set_head(
    db: &DatabaseConnection,
    pr_id: i64,
    expected: Option<&str>,
    new_sha: &str,
    now: chrono::DateTime<Utc>,
) -> CoreResult<u64> {
    let update = pull_request::Entity::update_many()
        .col_expr(
            pull_request::Column::HeadSha,
            Expr::value(Some(new_sha.to_string())),
        )
        .col_expr(pull_request::Column::UpdatedAt, Expr::value(now))
        .filter(pull_request::Column::Id.eq(pr_id));
    let update = match expected {
        Some(sha) => update.filter(pull_request::Column::HeadSha.eq(sha)),
        // A pull request from before the head was recorded. Filling it in is
        // not a race, and comparing `NULL = ?` would never match.
        None => update.filter(pull_request::Column::HeadSha.is_null()),
    };
    let result = update
        .exec(db)
        .await
        .context("db: compare-and-set pull request head")?;
    Ok(result.rows_affected)
}

/// The pull requests whose head lives in `source_repo_id`: a same-repository
/// pull request (`head_repo_id` NULL, target is the pushed repository) or a fork
/// pull request whose head repository is the pushed one.
///
/// Mirrors `rg_db::ops::pull_request_ops`'s private condition of the same name;
/// it is duplicated rather than exported because `rg-db` is outside this task's
/// owned paths (see `docs/plans/T07-plan.md` §2).
fn head_repository_condition(source_repo_id: i64) -> Condition {
    Condition::any()
        .add(
            Condition::all()
                .add(pull_request::Column::RepoId.eq(source_repo_id))
                .add(pull_request::Column::HeadRepoId.is_null()),
        )
        .add(pull_request::Column::HeadRepoId.eq(source_repo_id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rg_db::entities::authorization_request::{
        REASON_HEAD_SHA_CHANGED, STATE_EXPIRED, STATE_PENDING,
    };
    use sea_orm::{
        ConnectOptions, ConnectionTrait, Database, DatabaseBackend, DatabaseConnection, Statement,
    };

    const SHA_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const SHA_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    const SHA_C: &str = "cccccccccccccccccccccccccccccccccccccccc";

    async fn setup() -> DatabaseConnection {
        let mut opt = ConnectOptions::new("sqlite::memory:");
        opt.max_connections(1);
        let db = Database::connect(opt).await.expect("connect");
        rg_db::run_migrations(&db).await.expect("migrate");
        exec(&db, "INSERT INTO users(id, username, email, password_hash, is_admin, is_active, created_at, updated_at) VALUES(1, 'owner', 'owner@test.com', 'x', 0, 1, '2024-01-01 00:00:00', '2024-01-01 00:00:00')").await;
        exec(&db, "INSERT INTO repositories(id, owner_id, name, is_private, default_branch, stars_count, forks_count, created_at, updated_at) VALUES(1, 1, 'repo', 0, 'main', 0, 0, '2024-01-01 00:00:00', '2024-01-01 00:00:00')").await;
        exec(&db, "INSERT INTO repositories(id, owner_id, name, is_private, default_branch, stars_count, forks_count, created_at, updated_at) VALUES(2, 1, 'fork', 0, 'main', 0, 0, '2024-01-01 00:00:00', '2024-01-01 00:00:00')").await;
        db
    }

    async fn exec(db: &DatabaseConnection, sql: &str) {
        db.execute(Statement::from_string(
            DatabaseBackend::Sqlite,
            sql.to_string(),
        ))
        .await
        .unwrap_or_else(|error| panic!("{sql}: {error}"));
    }

    /// A pull request whose head branch is `feature` in repository 1, unless
    /// `head_repo_id` moves it to a fork.
    async fn insert_pr(
        db: &DatabaseConnection,
        id: i64,
        repo_id: i64,
        head_repo_id: Option<i64>,
        state: &str,
        head_sha: Option<&str>,
    ) {
        let head_repo = head_repo_id.map_or("NULL".to_string(), |id| id.to_string());
        let sha = head_sha.map_or("NULL".to_string(), |sha| format!("'{sha}'"));
        exec(
            db,
            &format!(
                "INSERT INTO pull_requests(id, repo_id, number, title, state, is_draft, auto_merge_enabled, author_id, head_branch, base_branch, head_repo_id, head_sha, created_at, updated_at) \
                 VALUES({id}, {repo_id}, {id}, 'change', '{state}', 0, 0, 1, 'feature', 'main', {head_repo}, {sha}, '2024-01-01 00:00:00', '2024-01-01 00:00:00')"
            ),
        )
        .await;
    }

    async fn insert_ticket(db: &DatabaseConnection, id: i64, subject: &str, head_sha: &str) {
        exec(
            db,
            &format!(
                "INSERT INTO authorization_requests(id, repo_id, action, subject, head_sha, requested_by_kind, requested_by_id, authorization_set_id, required_approvals, state, expires_at, client_request_id, payload_hash, created_at, updated_at) \
                 VALUES({id}, 1, 'protected_branch_merge', '{subject}', '{head_sha}', 'human', 1, 7, 1, '{STATE_PENDING}', '2024-01-02 00:00:00', 'key-{id}', 'hash-{id}', '2024-01-01 00:00:00', '2024-01-01 00:00:00')"
            ),
        )
        .await;
    }

    async fn pr(db: &DatabaseConnection, id: i64) -> PullRequest {
        pull_request::Entity::find_by_id(id)
            .one(db)
            .await
            .unwrap()
            .unwrap_or_else(|| panic!("pull request {id}"))
    }

    #[tokio::test]
    async fn a_push_moves_the_stored_head_of_the_open_pull_request() {
        let db = setup().await;
        insert_pr(&db, 1, 1, None, "open", Some(SHA_A)).await;

        let synced = sync_pr_head_on_push(&db, 1, "feature", SHA_B)
            .await
            .unwrap();
        assert_eq!(synced.len(), 1);
        assert_eq!(
            synced[0].outcome,
            HeadSyncOutcome::Updated {
                previous_head_sha: Some(SHA_A.to_string()),
                expired_requests: 0,
            }
        );
        assert_eq!(pr(&db, 1).await.head_sha.as_deref(), Some(SHA_B));
    }

    #[tokio::test]
    async fn a_push_at_the_recorded_head_writes_nothing() {
        let db = setup().await;
        insert_pr(&db, 1, 1, None, "open", Some(SHA_A)).await;

        let synced = sync_pr_head_on_push(&db, 1, "feature", SHA_A)
            .await
            .unwrap();
        assert_eq!(synced[0].outcome, HeadSyncOutcome::Unchanged);
        let row = pr(&db, 1).await;
        assert_eq!(row.head_sha.as_deref(), Some(SHA_A));
        // `updated_at` is the proof the row was not written: a hidden write
        // would bump it.
        assert_eq!(
            row.updated_at.format("%Y-%m-%d %H:%M:%S").to_string(),
            "2024-01-01 00:00:00"
        );
    }

    #[tokio::test]
    async fn a_concurrent_writer_is_not_overwritten() {
        let db = setup().await;
        insert_pr(&db, 1, 1, None, "open", Some(SHA_C)).await;

        // The sync's CAS expected SHA_A (what it read); the row now says SHA_C.
        let affected = cas_set_head(&db, 1, Some(SHA_A), SHA_B, Utc::now())
            .await
            .unwrap();
        assert_eq!(affected, 0, "the loser of the race writes nothing");
        assert_eq!(pr(&db, 1).await.head_sha.as_deref(), Some(SHA_C));
    }

    #[tokio::test]
    async fn a_legacy_pull_request_without_a_head_is_filled_in() {
        let db = setup().await;
        insert_pr(&db, 1, 1, None, "open", None).await;

        let synced = sync_pr_head_on_push(&db, 1, "feature", SHA_B)
            .await
            .unwrap();
        assert_eq!(
            synced[0].outcome,
            HeadSyncOutcome::Updated {
                previous_head_sha: None,
                expired_requests: 0,
            }
        );
        assert_eq!(pr(&db, 1).await.head_sha.as_deref(), Some(SHA_B));
    }

    #[tokio::test]
    async fn closed_pull_requests_and_other_branches_are_left_alone() {
        let db = setup().await;
        insert_pr(&db, 1, 1, None, "closed", Some(SHA_A)).await;
        insert_pr(&db, 2, 1, None, "open", Some(SHA_A)).await;
        exec(
            &db,
            "UPDATE pull_requests SET head_branch = 'other' WHERE id = 2",
        )
        .await;

        let synced = sync_pr_head_on_push(&db, 1, "feature", SHA_B)
            .await
            .unwrap();
        assert!(synced.is_empty(), "{synced:?}");
        assert_eq!(pr(&db, 1).await.head_sha.as_deref(), Some(SHA_A));
        assert_eq!(pr(&db, 2).await.head_sha.as_deref(), Some(SHA_A));
    }

    #[tokio::test]
    async fn a_fork_pull_request_is_synced_from_its_source_repository() {
        let db = setup().await;
        // id 1: fork pull request targeting repo 1, head branch lives in repo 2.
        insert_pr(&db, 1, 1, Some(2), "open", Some(SHA_A)).await;
        // id 2: same branch name, but its head lives in repo 1 — must not match
        // a push to repo 2.
        insert_pr(&db, 2, 1, None, "open", Some(SHA_A)).await;

        let synced = sync_pr_head_on_push(&db, 2, "feature", SHA_B)
            .await
            .unwrap();
        assert_eq!(synced.len(), 1);
        assert_eq!(synced[0].pull_request.id, 1);
        assert_eq!(pr(&db, 1).await.head_sha.as_deref(), Some(SHA_B));
        assert_eq!(pr(&db, 2).await.head_sha.as_deref(), Some(SHA_A));
    }

    #[tokio::test]
    async fn pending_tickets_bound_to_the_old_head_expire_in_place() {
        let db = setup().await;
        insert_pr(&db, 1, 1, None, "open", Some(SHA_A)).await;
        insert_ticket(&db, 1, "1", SHA_A).await;
        insert_ticket(&db, 2, "1", SHA_C).await;
        // Another pull request's ticket is not touched.
        insert_ticket(&db, 3, "2", SHA_A).await;

        let synced = sync_pr_head_on_push(&db, 1, "feature", SHA_B)
            .await
            .unwrap();
        assert_eq!(
            synced[0].outcome,
            HeadSyncOutcome::Updated {
                previous_head_sha: Some(SHA_A.to_string()),
                expired_requests: 2,
            }
        );

        for id in [1, 2] {
            let row = authorization_ops::find_by_id(&db, id)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(row.state, STATE_EXPIRED, "ticket {id}");
            assert_eq!(row.state_reason.as_deref(), Some(REASON_HEAD_SHA_CHANGED));
        }
        let other = authorization_ops::find_by_id(&db, 3)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            other.state, STATE_PENDING,
            "another PR's ticket is untouched"
        );
    }
}
