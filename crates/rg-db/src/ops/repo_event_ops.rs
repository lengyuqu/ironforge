//! Repository event log operations.

use sea_orm::*;

use crate::entities::repo_event::{self, Entity};

/// Append an event. Returns its id (the new cursor).
pub async fn append(
    db: &DatabaseConnection,
    repo_id: i64,
    event_type: &str,
    payload: Option<&str>,
) -> Result<i64, DbErr> {
    let row = repo_event::ActiveModel {
        id: NotSet,
        repo_id: Set(repo_id),
        event_type: Set(event_type.to_string()),
        payload: Set(payload.map(str::to_string)),
        created_at: Set(chrono::Utc::now()),
    }
    .insert(db)
    .await?;
    Ok(row.id)
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
