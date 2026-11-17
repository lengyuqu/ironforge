//! Repository event log operations.

use sea_orm::sea_query::Expr;
use sea_orm::*;

use crate::entities::repo_event::{self, Entity};

/// Append an event. Returns its id (the new cursor).
///
/// Takes any [`ConnectionTrait`], so the row can be written on a
/// [`DatabaseTransaction`](sea_orm::DatabaseTransaction) and commit or roll back
/// together with the business change that produced it (#15, step 1).
///
/// `dispatched_at` is left `NULL`: the caller is about to hand the event to
/// webhook delivery, and until it does the fallback sweep still owes it a
/// first attempt (#15, step 7).
pub async fn append<C: ConnectionTrait>(
    conn: &C,
    repo_id: i64,
    event_type: &str,
    payload: Option<&str>,
) -> Result<i64, DbErr> {
    append_handled(conn, repo_id, event_type, payload, None).await
}

/// [`append`], with the delivery decision already final.
///
/// `dispatched_at = Some(at)` records an event nobody owes a delivery: it was
/// handed to the delivery system in the same breath, or recorded deliberately
/// without one (the quiet path of the agent draft area). The sweep ignores such
/// rows, which is what keeps a silent event silent.
pub async fn append_handled<C: ConnectionTrait>(
    conn: &C,
    repo_id: i64,
    event_type: &str,
    payload: Option<&str>,
    dispatched_at: Option<chrono::DateTime<chrono::Utc>>,
) -> Result<i64, DbErr> {
    let row = repo_event::ActiveModel {
        id: NotSet,
        repo_id: Set(repo_id),
        event_type: Set(event_type.to_string()),
        payload: Set(payload.map(str::to_string)),
        created_at: Set(chrono::Utc::now()),
        dispatched_at: Set(dispatched_at),
    }
    .insert(conn)
    .await?;
    Ok(row.id)
}

/// Mark one event as handed to delivery. Returns whether this call was the one
/// that marked it — `false` when it was already marked (or gone).
///
/// Guarded on `dispatched_at IS NULL` so a concurrent marker (or a redelivery
/// racing the sweep) cannot overwrite the original timestamp.
pub async fn mark_dispatched<C: ConnectionTrait>(conn: &C, id: i64) -> Result<bool, DbErr> {
    let result = Entity::update_many()
        .col_expr(
            repo_event::Column::DispatchedAt,
            Expr::value(chrono::Utc::now()),
        )
        .filter(repo_event::Column::Id.eq(id))
        .filter(repo_event::Column::DispatchedAt.is_null())
        .exec(conn)
        .await?;
    Ok(result.rows_affected > 0)
}

/// Events the fallback sweep must consider: written in `[since, until]` and
/// never handed to delivery, oldest first.
pub async fn list_undispatched_between(
    db: &DatabaseConnection,
    since: chrono::DateTime<chrono::Utc>,
    until: chrono::DateTime<chrono::Utc>,
) -> Result<Vec<repo_event::Model>, DbErr> {
    Entity::find()
        .filter(repo_event::Column::DispatchedAt.is_null())
        .filter(repo_event::Column::CreatedAt.gte(since))
        .filter(repo_event::Column::CreatedAt.lte(until))
        .order_by_asc(repo_event::Column::Id)
        .all(db)
        .await
}

/// Up to `limit` undispatched events of one type in one repository, newest
/// first. Used to identify the row a transaction wrote when its id could not be
/// carried out of the transaction (#15, step 7 — see `deliver_event`).
pub async fn list_undispatched_of_type(
    db: &DatabaseConnection,
    repo_id: i64,
    event_type: &str,
    since: chrono::DateTime<chrono::Utc>,
    limit: u64,
) -> Result<Vec<repo_event::Model>, DbErr> {
    Entity::find()
        .filter(repo_event::Column::RepoId.eq(repo_id))
        .filter(repo_event::Column::EventType.eq(event_type))
        .filter(repo_event::Column::DispatchedAt.is_null())
        .filter(repo_event::Column::CreatedAt.gte(since))
        .order_by_desc(repo_event::Column::Id)
        .limit(limit)
        .all(db)
        .await
}

/// The newest event id of a repository, `0` when it has none.
pub async fn latest_id(db: &DatabaseConnection, repo_id: i64) -> Result<i64, DbErr> {
    Ok(Entity::find()
        .filter(repo_event::Column::RepoId.eq(repo_id))
        .order_by_desc(repo_event::Column::Id)
        .one(db)
        .await?
        .map(|row| row.id)
        .unwrap_or(0))
}

/// When the oldest retained event of a repository was recorded, if it has any.
pub async fn oldest_created_at(
    db: &DatabaseConnection,
    repo_id: i64,
) -> Result<Option<chrono::DateTime<chrono::Utc>>, DbErr> {
    Ok(Entity::find()
        .filter(repo_event::Column::RepoId.eq(repo_id))
        .order_by_asc(repo_event::Column::Id)
        .one(db)
        .await?
        .map(|row| row.created_at))
}

/// Up to `limit` events of a repository after `since`, oldest first, optionally
/// only of the given types.
pub async fn list_after(
    db: &DatabaseConnection,
    repo_id: i64,
    since: i64,
    types: &[String],
    limit: u64,
) -> Result<Vec<repo_event::Model>, DbErr> {
    let mut query = Entity::find()
        .filter(repo_event::Column::RepoId.eq(repo_id))
        .filter(repo_event::Column::Id.gt(since));
    if !types.is_empty() {
        query = query.filter(repo_event::Column::EventType.is_in(types.to_vec()));
    }
    query
        .order_by_asc(repo_event::Column::Id)
        .limit(limit)
        .all(db)
        .await
}

/// Delete events older than `before`. Returns how many.
pub async fn delete_older_than(
    db: &DatabaseConnection,
    before: chrono::DateTime<chrono::Utc>,
) -> Result<u64, DbErr> {
    Ok(Entity::delete_many()
        .filter(repo_event::Column::CreatedAt.lt(before))
        .exec(db)
        .await?
        .rows_affected)
}
