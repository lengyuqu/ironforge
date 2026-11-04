//! Database operations for `agent_sessions`.
//!
//! These functions only touch rows; secret verification, TTL policy and
//! permission checks live in `rg-core`.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use sea_orm::sea_query::Expr;
use sea_orm::*;

use crate::entities::agent_session::{
    self, ActiveModel, Entity as SessionEntity, Model as Session,
};

/// Insert a new session row.
pub async fn create<C: ConnectionTrait>(db: &C, model: ActiveModel) -> Result<Session> {
    model.insert(db).await.context("db: create agent session")
}

/// Find a session by id, scoped to a repository (other repos' ids read as absent).
pub async fn find_in_repo<C: ConnectionTrait>(
    db: &C,
    repo_id: i64,
    id: &str,
) -> Result<Option<Session>> {
    SessionEntity::find_by_id(id.to_string())
        .filter(agent_session::Column::RepoId.eq(repo_id))
        .one(db)
        .await
        .context("db: find agent session")
}

/// Record a heartbeat. Only a session that is neither ended nor past its hard
/// expiry is touched; returns the number of rows updated (0 = not renewable).
/// `expires_at` is deliberately left alone.
pub async fn touch<C: ConnectionTrait>(
    db: &C,
    repo_id: i64,
    id: &str,
    now: DateTime<Utc>,
) -> Result<u64> {
    let res = SessionEntity::update_many()
        .col_expr(agent_session::Column::LastSeenAt, Expr::value(now))
        .filter(agent_session::Column::Id.eq(id))
        .filter(agent_session::Column::RepoId.eq(repo_id))
        .filter(agent_session::Column::EndedAt.is_null())
        .filter(agent_session::Column::ExpiresAt.gt(now))
        .exec(db)
        .await
        .context("db: touch agent session")?;
    Ok(res.rows_affected)
}

/// Mark a session ended. Idempotent: the first end time is kept.
pub async fn end<C: ConnectionTrait>(
    db: &C,
    repo_id: i64,
    id: &str,
    now: DateTime<Utc>,
) -> Result<()> {
    SessionEntity::update_many()
        .col_expr(agent_session::Column::EndedAt, Expr::value(now))
        .filter(agent_session::Column::Id.eq(id))
        .filter(agent_session::Column::RepoId.eq(repo_id))
        .filter(agent_session::Column::EndedAt.is_null())
        .exec(db)
        .await
        .context("db: end agent session")?;
    Ok(())
}

/// Keyset page of a repository's sessions, newest heartbeat first.
///
/// `after` is the `(last_seen_at, id)` of the last row of the previous page,
/// taken from a row read back from the database (never from client time).
pub async fn list_page<C: ConnectionTrait>(
    db: &C,
    repo_id: i64,
    after: Option<(DateTime<Utc>, String)>,
    limit: u64,
) -> Result<Vec<Session>> {
    let mut query = SessionEntity::find().filter(agent_session::Column::RepoId.eq(repo_id));
    if let Some((seen, id)) = after {
        query = query.filter(
            Condition::any()
                .add(agent_session::Column::LastSeenAt.lt(seen))
                .add(
                    Condition::all()
                        .add(agent_session::Column::LastSeenAt.eq(seen))
                        .add(agent_session::Column::Id.lt(id)),
                ),
        );
    }
    query
        .order_by_desc(agent_session::Column::LastSeenAt)
        .order_by_desc(agent_session::Column::Id)
        .limit(limit)
        .all(db)
        .await
        .context("db: list agent sessions")
}
