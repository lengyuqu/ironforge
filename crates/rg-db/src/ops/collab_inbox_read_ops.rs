//! Database operations for `collab_inbox_reads` (personal inbox read marks).

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use sea_orm::sea_query::Expr;
use sea_orm::*;

use crate::entities::collab_inbox_read::{self, ActiveModel, Entity as ReadEntity, Model as Mark};

/// Marks of `actor` for the given item keys.
pub async fn list_for_keys<C: ConnectionTrait>(
    db: &C,
    repo_id: i64,
    actor_kind: &str,
    actor_id: &str,
    keys: &[String],
) -> Result<Vec<Mark>> {
    if keys.is_empty() {
        return Ok(Vec::new());
    }
    ReadEntity::find()
        .filter(collab_inbox_read::Column::RepoId.eq(repo_id))
        .filter(collab_inbox_read::Column::ActorKind.eq(actor_kind))
        .filter(collab_inbox_read::Column::ActorId.eq(actor_id))
        .filter(collab_inbox_read::Column::ItemKey.is_in(keys.iter().cloned()))
        .all(db)
        .await
        .context("db: list inbox read marks")
}

async fn get<C: ConnectionTrait>(
    db: &C,
    repo_id: i64,
    actor_kind: &str,
    actor_id: &str,
    item_key: &str,
) -> Result<Option<Mark>> {
    ReadEntity::find_by_id((
        repo_id,
        actor_kind.to_string(),
        actor_id.to_string(),
        item_key.to_string(),
    ))
    .one(db)
    .await
    .context("db: find inbox read mark")
}

/// Raise the stored version to `version` (monotonic: a lower value never moves
/// it back) and return the mark as stored afterwards.
///
/// Concurrent callers converge on the maximum: the conditional `UPDATE` only
/// matches a lower stored value, and the first insert wins a unique-key race
/// (`ON CONFLICT DO NOTHING` is not portable to MySQL).
pub async fn raise(
    db: &DatabaseConnection,
    repo_id: i64,
    actor_kind: &str,
    actor_id: &str,
    item_key: &str,
    version: i64,
    now: DateTime<Utc>,
) -> Result<Mark> {
    for _ in 0..8 {
        let updated = ReadEntity::update_many()
            .col_expr(
                collab_inbox_read::Column::ObservedVersion,
                Expr::value(version),
            )
            .col_expr(collab_inbox_read::Column::ReadAt, Expr::value(now))
            .filter(collab_inbox_read::Column::RepoId.eq(repo_id))
            .filter(collab_inbox_read::Column::ActorKind.eq(actor_kind))
            .filter(collab_inbox_read::Column::ActorId.eq(actor_id))
            .filter(collab_inbox_read::Column::ItemKey.eq(item_key))
            .filter(collab_inbox_read::Column::ObservedVersion.lt(version))
            .exec(db)
            .await
            .context("db: raise inbox read mark")?;
        if updated.rows_affected > 0 {
            break;
        }
        // No row was lower. Either the stored mark already covers `version`, or
        // the row was missing when the UPDATE ran and a lower one has appeared
        // since: only the first case is done, the second must retry the UPDATE.
        if let Some(stored) = get(db, repo_id, actor_kind, actor_id, item_key).await? {
            if stored.observed_version >= version {
                break;
            }
            continue;
        }
        match (ActiveModel {
            repo_id: Set(repo_id),
            actor_kind: Set(actor_kind.to_string()),
            actor_id: Set(actor_id.to_string()),
            item_key: Set(item_key.to_string()),
            observed_version: Set(version),
            read_at: Set(now),
        })
        .insert(db)
        .await
        {
            Ok(_) => break,
            Err(err) if matches!(err.sql_err(), Some(SqlErr::UniqueConstraintViolation(_))) => {}
            Err(err) => return Err(err).context("db: insert inbox read mark"),
        }
    }
    get(db, repo_id, actor_kind, actor_id, item_key)
        .await?
        .context("db: inbox read mark vanished")
}
