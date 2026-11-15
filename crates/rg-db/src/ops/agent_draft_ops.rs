//! Agent draft namespace operations (#18).
//!
//! The row-level half of the draft area: the lifecycle (`active` → `expiring` →
//! `deleted`) and the two queries the push path and the cleanup tick need. The
//! prefix rules, the longest-prefix lookup over a set of branches, and the
//! extend arithmetic live in `rg_core::repo::draft`, which is pure and testable
//! without a database.

use chrono::{DateTime, Utc};
use sea_orm::sea_query::Expr;
use sea_orm::*;

use crate::entities::agent_draft_namespace::{
    self, ActiveModel, Entity, Model, STATE_ACTIVE, STATE_DELETED, STATE_EXPIRING,
};

/// Register a prefix. The unique `(repo_id, prefix)` key is the only
/// concurrency control needed: two simultaneous creates leave one row and the
/// caller sees the conflict and re-reads it.
pub async fn create(
    db: &DatabaseConnection,
    repo_id: i64,
    prefix: &str,
    owner_user_id: i64,
    ttl_secs: i64,
    expires_at: DateTime<Utc>,
    grace_secs: i64,
) -> Result<Model, DbErr> {
    let now = Utc::now();
    ActiveModel {
        id: NotSet,
        repo_id: Set(repo_id),
        prefix: Set(prefix.to_owned()),
        owner_user_id: Set(owner_user_id),
        ttl_secs: Set(ttl_secs),
        expires_at: Set(expires_at),
        grace_secs: Set(grace_secs),
        state: Set(STATE_ACTIVE.to_owned()),
        extended_count: Set(0),
        created_at: Set(now),
    }
    .insert(db)
    .await
}

pub async fn find_by_id(db: &DatabaseConnection, id: i64) -> Result<Option<Model>, DbErr> {
    Entity::find_by_id(id).one(db).await
}

pub async fn find_by_repo_and_prefix(
    db: &DatabaseConnection,
    repo_id: i64,
    prefix: &str,
) -> Result<Option<Model>, DbErr> {
    Entity::find()
        .filter(agent_draft_namespace::Column::RepoId.eq(repo_id))
        .filter(agent_draft_namespace::Column::Prefix.eq(prefix))
        .one(db)
        .await
}

/// Every namespace of a repository, in creation order. The push path loads this
/// once per push and matches refs against it in memory (a repository has a
/// handful of these, not thousands).
pub async fn list_by_repo(db: &DatabaseConnection, repo_id: i64) -> Result<Vec<Model>, DbErr> {
    Entity::find()
        .filter(agent_draft_namespace::Column::RepoId.eq(repo_id))
        .order_by_asc(agent_draft_namespace::Column::Id)
        .all(db)
        .await
}

/// Namespaces whose gate has not been closed yet (`active`).
pub async fn list_active(db: &DatabaseConnection, repo_id: i64) -> Result<Vec<Model>, DbErr> {
    Entity::find()
        .filter(agent_draft_namespace::Column::RepoId.eq(repo_id))
        .filter(agent_draft_namespace::Column::State.eq(STATE_ACTIVE))
        .all(db)
        .await
}

/// Active namespaces whose `expires_at` has passed: the cleanup tick closes
/// each one's gate before it deletes anything.
pub async fn list_expired_active(
    db: &DatabaseConnection,
    now: DateTime<Utc>,
) -> Result<Vec<Model>, DbErr> {
    Entity::find()
        .filter(agent_draft_namespace::Column::State.eq(STATE_ACTIVE))
        .filter(agent_draft_namespace::Column::ExpiresAt.lte(now))
        .all(db)
        .await
}

/// Namespaces whose gate is closed and whose grace period may be over. The
/// `expires_at + grace_secs` comparison is done by the caller in Rust: the
/// grace period is a per-row value, and the alternative is a backend-specific
/// interval expression for a table with a handful of rows.
pub async fn list_expiring(db: &DatabaseConnection) -> Result<Vec<Model>, DbErr> {
    Entity::find()
        .filter(agent_draft_namespace::Column::State.eq(STATE_EXPIRING))
        .all(db)
        .await
}

/// `active` → `expiring`: stop accepting pushes. Idempotent in effect (the
/// filter keeps a concurrently updated row from being overwritten).
pub async fn close_gate(db: &DatabaseConnection, id: i64) -> Result<bool, DbErr> {
    let result = Entity::update_many()
        .col_expr(
            agent_draft_namespace::Column::State,
            Expr::value(STATE_EXPIRING),
        )
        .filter(agent_draft_namespace::Column::Id.eq(id))
        .filter(agent_draft_namespace::Column::State.eq(STATE_ACTIVE))
        .exec(db)
        .await?;
    Ok(result.rows_affected > 0)
}

/// `expiring` → `active` after a successful extend.
pub async fn reopen_gate(
    db: &DatabaseConnection,
    id: i64,
    expires_at: DateTime<Utc>,
    extended_count: i32,
) -> Result<bool, DbErr> {
    let result = Entity::update_many()
        .col_expr(
            agent_draft_namespace::Column::State,
            Expr::value(STATE_ACTIVE),
        )
        .col_expr(
            agent_draft_namespace::Column::ExpiresAt,
            Expr::value(expires_at),
        )
        .col_expr(
            agent_draft_namespace::Column::ExtendedCount,
            Expr::value(extended_count),
        )
        .filter(agent_draft_namespace::Column::Id.eq(id))
        .exec(db)
        .await?;
    Ok(result.rows_affected > 0)
}

/// Move an `active` namespace's deadline forward.
pub async fn extend(
    db: &DatabaseConnection,
    id: i64,
    expires_at: DateTime<Utc>,
    extended_count: i32,
) -> Result<bool, DbErr> {
    let result = Entity::update_many()
        .col_expr(
            agent_draft_namespace::Column::ExpiresAt,
            Expr::value(expires_at),
        )
        .col_expr(
            agent_draft_namespace::Column::ExtendedCount,
            Expr::value(extended_count),
        )
        .filter(agent_draft_namespace::Column::Id.eq(id))
        .filter(agent_draft_namespace::Column::State.eq(STATE_ACTIVE))
        .exec(db)
        .await?;
    Ok(result.rows_affected > 0)
}

/// Close the gate immediately (`DELETE` on the namespace): the deadline becomes
/// `at`, so the grace period starts now and the branches go away on the next
/// tick after it. The row stays `expiring` — the caller may still extend it back.
pub async fn soft_delete(
    db: &DatabaseConnection,
    id: i64,
    at: DateTime<Utc>,
) -> Result<bool, DbErr> {
    let result = Entity::update_many()
        .col_expr(agent_draft_namespace::Column::ExpiresAt, Expr::value(at))
        .col_expr(
            agent_draft_namespace::Column::State,
            Expr::value(STATE_EXPIRING),
        )
        .filter(agent_draft_namespace::Column::Id.eq(id))
        .filter(agent_draft_namespace::Column::State.eq(STATE_ACTIVE))
        .exec(db)
        .await?;
    Ok(result.rows_affected > 0)
}

/// Mark the reservation as served. The row stays: a push to the old prefix must
/// still be refused, and the audit trail is the row's job, not the deletion's.
pub async fn mark_deleted(db: &DatabaseConnection, id: i64) -> Result<bool, DbErr> {
    let result = Entity::update_many()
        .col_expr(
            agent_draft_namespace::Column::State,
            Expr::value(STATE_DELETED),
        )
        .filter(agent_draft_namespace::Column::Id.eq(id))
        .exec(db)
        .await?;
    Ok(result.rows_affected > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;
    use sea_orm_migration::MigratorTrait;

    async fn db() -> DatabaseConnection {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        crate::migrations::Migrator::up(&db, None).await.unwrap();
        db
    }

    /// A user and a repository for the foreign key to point at.
    async fn fixture(db: &DatabaseConnection) -> i64 {
        use crate::entities::repository::ActiveModel as RepoActiveModel;
        let user = crate::ops::user_ops::create_user(db, "owner", "owner@example.com", "", "")
            .await
            .unwrap();
        RepoActiveModel {
            id: NotSet,
            owner_id: Set(user.id),
            name: Set("repo".into()),
            description: Set(None),
            is_private: Set(false),
            default_branch: Set("main".into()),
            fork_id: Set(None),
            stars_count: Set(0),
            forks_count: Set(0),
            org_id: Set(None),
            created_at: Set(Utc::now()),
            updated_at: Set(Utc::now()),
            deleted_at: Set(None),
            origin_repo_id: Set(None),
        }
        .insert(db)
        .await
        .unwrap()
        .id
    }

    #[tokio::test]
    async fn the_lifecycle_moves_forward_and_a_repeated_transition_is_a_no_op() {
        let db = db().await;
        let repo_id = fixture(&db).await;
        let expires = Utc::now() + Duration::hours(1);
        let created = create(&db, repo_id, "agent/1/", 1, 3600, expires, 60)
            .await
            .unwrap();
        assert_eq!(created.state, STATE_ACTIVE);
        assert_eq!(created.extended_count, 0);

        assert!(close_gate(&db, created.id).await.unwrap());
        assert!(
            !close_gate(&db, created.id).await.unwrap(),
            "closing a closed gate is not a transition"
        );
        assert_eq!(
            find_by_id(&db, created.id).await.unwrap().unwrap().state,
            STATE_EXPIRING
        );

        assert!(mark_deleted(&db, created.id).await.unwrap());
        assert_eq!(
            find_by_id(&db, created.id).await.unwrap().unwrap().state,
            STATE_DELETED
        );
        // A deleted reservation is never reopened by the lifecycle.
        assert!(!close_gate(&db, created.id).await.unwrap());
    }

    #[tokio::test]
    async fn expired_and_expiring_queries_partition_the_table() {
        let db = db().await;
        let repo_id = fixture(&db).await;
        let now = Utc::now();
        let live = create(
            &db,
            repo_id,
            "agent/1/",
            1,
            3600,
            now + Duration::hours(1),
            60,
        )
        .await
        .unwrap();
        let expired = create(
            &db,
            repo_id,
            "agent/2/",
            1,
            3600,
            now - Duration::hours(1),
            60,
        )
        .await
        .unwrap();
        create(
            &db,
            repo_id,
            "agent/3/",
            1,
            3600,
            now + Duration::hours(1),
            60,
        )
        .await
        .unwrap();

        let due = list_expired_active(&db, now).await.unwrap();
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].id, expired.id);

        close_gate(&db, expired.id).await.unwrap();
        assert!(list_expired_active(&db, now).await.unwrap().is_empty());
        assert_eq!(list_expiring(&db).await.unwrap().len(), 1);
        assert_eq!(list_active(&db, repo_id).await.unwrap().len(), 2);
        assert!(find_by_id(&db, live.id).await.unwrap().is_some());
    }

    #[tokio::test]
    async fn the_unique_prefix_is_per_repository() {
        let db = db().await;
        let repo_id = fixture(&db).await;
        let expires = Utc::now() + Duration::hours(1);
        create(&db, repo_id, "agent/1/", 1, 3600, expires, 60)
            .await
            .unwrap();
        assert!(
            create(&db, repo_id, "agent/1/", 2, 3600, expires, 60)
                .await
                .is_err(),
            "the same prefix twice in one repository must conflict"
        );
    }

    #[tokio::test]
    async fn extend_is_a_compare_and_set_on_an_active_row() {
        let db = db().await;
        let repo_id = fixture(&db).await;
        let expires = Utc::now() + Duration::hours(1);
        let ns = create(&db, repo_id, "agent/1/", 1, 3600, expires, 60)
            .await
            .unwrap();
        let later = expires + Duration::hours(1);
        assert!(extend(&db, ns.id, later, 1).await.unwrap());
        let reloaded = find_by_id(&db, ns.id).await.unwrap().unwrap();
        assert_eq!(reloaded.extended_count, 1);
        assert!(reloaded.expires_at > expires);

        close_gate(&db, ns.id).await.unwrap();
        assert!(
            !extend(&db, ns.id, later, 2).await.unwrap(),
            "a closed gate is reopened by `reopen_gate`, not by `extend`"
        );
        assert!(reopen_gate(&db, ns.id, later, 2).await.unwrap());
        let reloaded = find_by_id(&db, ns.id).await.unwrap().unwrap();
        assert_eq!(reloaded.state, STATE_ACTIVE);
        assert_eq!(reloaded.extended_count, 2);
    }
}
