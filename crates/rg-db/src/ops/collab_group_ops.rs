//! Database operations for `collab_coordination_groups` / `_links`.
//!
//! Link changes and the group's version bump happen in **one transaction whose
//! first statement is the conditional `UPDATE` of the group** (CLAUDE.md
//! pitfall 21): of two writers holding the same `expected_version` exactly one
//! matches the row, and on SQLite no transaction starts with a read it could
//! not upgrade. Idempotent replays are looked up *before* the transaction.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use sea_orm::sea_query::Expr;
use sea_orm::*;

use crate::entities::collab_coordination_group::{
    self as group, Entity as GroupEntity, Model as Group,
};
use crate::entities::collab_coordination_link::{
    self as link, ActiveModel as LinkActive, Entity as LinkEntity, Model as Link,
};

// ── groups ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum CreateGroupOutcome {
    Created(Group),
    /// `(actor_kind, actor_id, client_group_id)` already exists; compare `payload_hash`.
    Existing(Group),
}

pub async fn find_group_by_client_id<C: ConnectionTrait>(
    db: &C,
    actor_kind: &str,
    actor_id: &str,
    client_group_id: &str,
) -> Result<Option<Group>> {
    GroupEntity::find()
        .filter(group::Column::ActorKind.eq(actor_kind))
        .filter(group::Column::ActorId.eq(actor_id))
        .filter(group::Column::ClientGroupId.eq(client_group_id))
        .one(db)
        .await
        .context("db: find coordination group by client id")
}

/// A group, scoped to its anchor repository (a group id of another anchor is
/// "not found", so ids never confirm a group elsewhere).
pub async fn find_group_in_anchor<C: ConnectionTrait>(
    db: &C,
    anchor_repo_id: i64,
    id: &str,
) -> Result<Option<Group>> {
    GroupEntity::find_by_id(id.to_string())
        .filter(group::Column::AnchorRepoId.eq(anchor_repo_id))
        .one(db)
        .await
        .context("db: find coordination group")
}

pub async fn create_group(db: &DatabaseConnection, model: group::ActiveModel) -> Result<CreateGroupOutcome> {
    let actor_kind = model.actor_kind.clone().unwrap();
    let actor_id = model.actor_id.clone().unwrap();
    let client_id = model.client_group_id.clone().unwrap();
    if let Some(existing) = find_group_by_client_id(db, &actor_kind, &actor_id, &client_id).await? {
        return Ok(CreateGroupOutcome::Existing(existing));
    }
    match model.insert(db).await {
        Ok(created) => Ok(CreateGroupOutcome::Created(created)),
        Err(err) if matches!(err.sql_err(), Some(SqlErr::UniqueConstraintViolation(_))) => {
            let existing = find_group_by_client_id(db, &actor_kind, &actor_id, &client_id)
                .await?
                .context("db: group idempotency key vanished after conflict")?;
            Ok(CreateGroupOutcome::Existing(existing))
        }
        Err(err) => Err(err).context("db: create coordination group"),
    }
}

// ── links ───────────────────────────────────────────────────────────

pub async fn find_link_by_client_id<C: ConnectionTrait>(
    db: &C,
    actor_kind: &str,
    actor_id: &str,
    client_link_id: &str,
) -> Result<Option<Link>> {
    LinkEntity::find()
        .filter(link::Column::ActorKind.eq(actor_kind))
        .filter(link::Column::ActorId.eq(actor_id))
        .filter(link::Column::ClientLinkId.eq(client_link_id))
        .one(db)
        .await
        .context("db: find coordination link by client id")
}

pub async fn find_link_by_remove_client_id<C: ConnectionTrait>(
    db: &C,
    actor_kind: &str,
    actor_id: &str,
    remove_client_id: &str,
) -> Result<Option<Link>> {
    LinkEntity::find()
        .filter(link::Column::RemovedActorKind.eq(actor_kind))
        .filter(link::Column::RemovedActorId.eq(actor_id))
        .filter(link::Column::RemoveClientId.eq(remove_client_id))
        .one(db)
        .await
        .context("db: find coordination link by remove id")
}

/// A link (active or removed) of a group.
pub async fn find_link_in_group<C: ConnectionTrait>(
    db: &C,
    group_id: &str,
    id: &str,
) -> Result<Option<Link>> {
    LinkEntity::find_by_id(id.to_string())
        .filter(link::Column::GroupId.eq(group_id))
        .one(db)
        .await
        .context("db: find coordination link")
}

pub async fn find_active_by_key<C: ConnectionTrait>(db: &C, active_key: &str) -> Result<Option<Link>> {
    LinkEntity::find()
        .filter(link::Column::ActiveKey.eq(active_key))
        .one(db)
        .await
        .context("db: find active coordination link")
}

pub async fn count_active<C: ConnectionTrait>(db: &C, group_id: &str) -> Result<u64> {
    LinkEntity::find()
        .filter(link::Column::GroupId.eq(group_id))
        .filter(link::Column::RemovedAt.is_null())
        .count(db)
        .await
        .context("db: count active coordination links")
}

/// Keyset batch of a group's *active* links ordered by `(created_at, id)`
/// ascending. Authorization is **not** applied here: the caller filters.
pub async fn list_active_batch<C: ConnectionTrait>(
    db: &C,
    group_id: &str,
    after: Option<(DateTime<Utc>, String)>,
    limit: u64,
) -> Result<Vec<Link>> {
    let mut query = LinkEntity::find()
        .filter(link::Column::GroupId.eq(group_id))
        .filter(link::Column::RemovedAt.is_null());
    if let Some((created, id)) = after {
        query = query.filter(
            Condition::any().add(link::Column::CreatedAt.gt(created)).add(
                Condition::all()
                    .add(link::Column::CreatedAt.eq(created))
                    .add(link::Column::Id.gt(id)),
            ),
        );
    }
    query
        .order_by_asc(link::Column::CreatedAt)
        .order_by_asc(link::Column::Id)
        .limit(limit)
        .all(db)
        .await
        .context("db: list coordination links")
}

// Results are consumed immediately; boxing the rows would only add noise.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq)]
pub enum AddLinkOutcome {
    Added { group: Group, link: Link },
    /// The idempotency key already owns a link; compare `payload_hash`.
    Existing(Link),
    /// The group's version is not `expected_version`.
    Conflict { current_version: i64 },
    /// An active link to the same target already exists.
    Duplicate { link_id: String },
    /// The group already holds the maximum number of active links.
    LimitReached,
    GroupNotFound,
}

/// Bump the group's version iff it is still `expected_version`, and insert
/// `model` — one transaction. `max_active` bounds the group's active links.
pub async fn add_link_cas(
    db: &DatabaseConnection,
    anchor_repo_id: i64,
    group_id: &str,
    expected_version: i64,
    model: LinkActive,
    max_active: u64,
    now: DateTime<Utc>,
) -> Result<AddLinkOutcome> {
    let actor_kind = model.actor_kind.clone().unwrap();
    let actor_id = model.actor_id.clone().unwrap();
    let client_id = model.client_link_id.clone().unwrap();
    let active_key = model.active_key.clone().unwrap().unwrap_or_default();

    if let Some(existing) = find_link_by_client_id(db, &actor_kind, &actor_id, &client_id).await? {
        return Ok(AddLinkOutcome::Existing(existing));
    }
    let txn = db.begin().await.context("db: begin add link")?;
    let res = bump_version(&txn, anchor_repo_id, group_id, expected_version, now).await?;
    if res.rows_affected == 0 {
        // The version moved. If it moved because *this very request* (same key)
        // just won a race against its own copy, answer with that result.
        let replay = find_link_by_client_id(&txn, &actor_kind, &actor_id, &client_id).await?;
        let current = find_group_in_anchor(&txn, anchor_repo_id, group_id).await?;
        txn.commit().await.context("db: commit add link")?;
        if let Some(existing) = replay {
            return Ok(AddLinkOutcome::Existing(existing));
        }
        return Ok(match current {
            Some(g) => AddLinkOutcome::Conflict { current_version: g.version },
            None => AddLinkOutcome::GroupNotFound,
        });
    }
    if count_active(&txn, group_id).await? >= max_active {
        txn.rollback().await.context("db: rollback add link")?;
        return Ok(AddLinkOutcome::LimitReached);
    }
    match model.insert(&txn).await {
        Ok(link) => {
            let group = find_group_in_anchor(&txn, anchor_repo_id, group_id)
                .await?
                .context("db: group vanished inside add-link transaction")?;
            txn.commit().await.context("db: commit add link")?;
            Ok(AddLinkOutcome::Added { group, link })
        }
        Err(err) if matches!(err.sql_err(), Some(SqlErr::UniqueConstraintViolation(_))) => {
            // Either the same idempotency key committed concurrently or the
            // target is already linked. Undo our version bump, then tell which.
            txn.rollback().await.context("db: rollback add link")?;
            if let Some(existing) =
                find_link_by_client_id(db, &actor_kind, &actor_id, &client_id).await?
            {
                return Ok(AddLinkOutcome::Existing(existing));
            }
            let duplicate = find_active_by_key(db, &active_key)
                .await?
                .context("db: unique violation without a conflicting link")?;
            Ok(AddLinkOutcome::Duplicate { link_id: duplicate.id })
        }
        Err(err) => Err(err).context("db: insert coordination link"),
    }
}

/// Who removes a link, and under which idempotency key.
#[derive(Debug, Clone)]
pub struct RemoveInfo {
    pub user_id: i64,
    pub actor_kind: String,
    pub actor_id: String,
    pub client_id: String,
    pub payload_hash: String,
}

// Results are consumed immediately; boxing the rows would only add noise.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq)]
pub enum RemoveLinkOutcome {
    Removed { group: Group, link: Link },
    /// The remove key already removed a link; compare `remove_payload_hash`.
    Existing(Link),
    Conflict { current_version: i64 },
    /// The link is missing from the group or was removed by someone else.
    NotActive,
    GroupNotFound,
}

/// Bump the group's version iff it is still `expected_version` and soft-remove
/// the link — one transaction.
pub async fn remove_link_cas(
    db: &DatabaseConnection,
    anchor_repo_id: i64,
    group_id: &str,
    link_id: &str,
    expected_version: i64,
    by: &RemoveInfo,
    now: DateTime<Utc>,
) -> Result<RemoveLinkOutcome> {
    if let Some(existing) =
        find_link_by_remove_client_id(db, &by.actor_kind, &by.actor_id, &by.client_id).await?
    {
        return Ok(RemoveLinkOutcome::Existing(existing));
    }
    let txn = db.begin().await.context("db: begin remove link")?;
    let res = bump_version(&txn, anchor_repo_id, group_id, expected_version, now).await?;
    if res.rows_affected == 0 {
        // As in `add_link_cas`: a lost race against our own copy is a replay.
        let replay =
            find_link_by_remove_client_id(&txn, &by.actor_kind, &by.actor_id, &by.client_id).await?;
        let current = find_group_in_anchor(&txn, anchor_repo_id, group_id).await?;
        txn.commit().await.context("db: commit remove link")?;
        if let Some(existing) = replay {
            return Ok(RemoveLinkOutcome::Existing(existing));
        }
        return Ok(match current {
            Some(g) => RemoveLinkOutcome::Conflict { current_version: g.version },
            None => RemoveLinkOutcome::GroupNotFound,
        });
    }
    let removed = LinkEntity::update_many()
        .col_expr(link::Column::RemovedAt, Expr::value(now))
        .col_expr(link::Column::ActiveKey, Expr::value(Option::<String>::None))
        .col_expr(link::Column::RemovedByUserId, Expr::value(by.user_id))
        .col_expr(link::Column::RemovedActorKind, Expr::value(by.actor_kind.clone()))
        .col_expr(link::Column::RemovedActorId, Expr::value(by.actor_id.clone()))
        .col_expr(link::Column::RemoveClientId, Expr::value(by.client_id.clone()))
        .col_expr(link::Column::RemovePayloadHash, Expr::value(by.payload_hash.clone()))
        .filter(link::Column::Id.eq(link_id))
        .filter(link::Column::GroupId.eq(group_id))
        .filter(link::Column::RemovedAt.is_null())
        .exec(&txn)
        .await;
    match removed {
        Ok(r) if r.rows_affected == 1 => {
            let group = find_group_in_anchor(&txn, anchor_repo_id, group_id)
                .await?
                .context("db: group vanished inside remove-link transaction")?;
            let link = find_link_in_group(&txn, group_id, link_id)
                .await?
                .context("db: link vanished inside remove-link transaction")?;
            txn.commit().await.context("db: commit remove link")?;
            Ok(RemoveLinkOutcome::Removed { group, link })
        }
        Ok(_) => {
            txn.rollback().await.context("db: rollback remove link")?;
            Ok(RemoveLinkOutcome::NotActive)
        }
        Err(err) if matches!(err.sql_err(), Some(SqlErr::UniqueConstraintViolation(_))) => {
            // The same remove key committed concurrently (on another link).
            txn.rollback().await.context("db: rollback remove link")?;
            let existing =
                find_link_by_remove_client_id(db, &by.actor_kind, &by.actor_id, &by.client_id)
                    .await?
                    .context("db: remove key vanished after conflict")?;
            Ok(RemoveLinkOutcome::Existing(existing))
        }
        Err(err) => Err(err).context("db: soft-remove coordination link"),
    }
}

async fn bump_version<C: ConnectionTrait>(
    txn: &C,
    anchor_repo_id: i64,
    group_id: &str,
    expected_version: i64,
    now: DateTime<Utc>,
) -> Result<UpdateResult> {
    GroupEntity::update_many()
        .col_expr(group::Column::UpdatedAt, Expr::value(now))
        .col_expr(group::Column::Version, Expr::col(group::Column::Version).add(1))
        .filter(group::Column::Id.eq(group_id))
        .filter(group::Column::AnchorRepoId.eq(anchor_repo_id))
        .filter(group::Column::Version.eq(expected_version))
        .exec(txn)
        .await
        .context("db: bump coordination group version")
}
