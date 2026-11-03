//! Database operations for `collab_messages`.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use sea_orm::sea_query::Expr;
use sea_orm::*;

use crate::entities::collab_message::{
    self, ActiveModel, Entity as MessageEntity, Model as Message,
};
use crate::entities::collab_task;

/// Everything needed to append a message; the server assigns `id` and `seq`.
#[derive(Debug, Clone)]
pub struct NewMessage {
    pub id: String,
    pub repo_id: i64,
    pub task_id: String,
    pub author_session_id: String,
    pub kind: String,
    pub body: String,
    pub reply_to_id: Option<String>,
    pub target_session_id: Option<String>,
    pub commit_sha: Option<String>,
    pub client_message_id: String,
    pub payload_hash: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AppendOutcome {
    /// A new message was committed.
    Created(Message),
    /// The same `(author_session_id, client_message_id)` was committed before.
    /// The caller compares `payload_hash` to tell a retry from key reuse.
    Existing(Message),
}

/// Append a message and assign the next per-task sequence number.
///
/// The transaction starts with an `UPDATE` that increments
/// `collab_tasks.next_seq`. That statement takes the task's row lock, so
/// concurrent appenders to one task are serialised until commit and the
/// numbers they obtain are in commit order: a reader that has seen `seq = n`
/// can never later see a committed message with a lower number. The
/// idempotency lookup runs after the lock is held, so concurrent retries of
/// one `client_message_id` observe each other's committed row and exactly one
/// message is created; the duplicate rolls back and leaves no gap in `seq`.
///
/// Returns `None` when the task does not exist in `repo_id`.
pub async fn append(db: &DatabaseConnection, new: NewMessage) -> Result<Option<AppendOutcome>> {
    let txn = db.begin().await.context("db: begin append message")?;

    let locked = collab_task::Entity::update_many()
        .col_expr(
            collab_task::Column::NextSeq,
            Expr::col(collab_task::Column::NextSeq).add(1),
        )
        .filter(collab_task::Column::Id.eq(new.task_id.as_str()))
        .filter(collab_task::Column::RepoId.eq(new.repo_id))
        .exec(&txn)
        .await
        .context("db: lock task for append")?;
    if locked.rows_affected == 0 {
        txn.rollback().await.context("db: rollback append message")?;
        return Ok(None);
    }

    if let Some(existing) = find_by_client_id(
        &txn,
        &new.author_session_id,
        &new.client_message_id,
    )
    .await?
    {
        txn.rollback().await.context("db: rollback duplicate message")?;
        return Ok(Some(AppendOutcome::Existing(existing)));
    }

    let seq = collab_task::Entity::find_by_id(new.task_id.clone())
        .one(&txn)
        .await
        .context("db: read assigned seq")?
        .context("db: task vanished while locked")?
        .next_seq;

    let inserted = ActiveModel {
        id: Set(new.id),
        repo_id: Set(new.repo_id),
        task_id: Set(new.task_id),
        seq: Set(seq),
        author_session_id: Set(new.author_session_id),
        kind: Set(new.kind),
        body: Set(new.body),
        reply_to_id: Set(new.reply_to_id),
        target_session_id: Set(new.target_session_id),
        commit_sha: Set(new.commit_sha),
        client_message_id: Set(new.client_message_id),
        payload_hash: Set(new.payload_hash),
        created_at: Set(new.created_at),
    }
    .insert(&txn)
    .await
    .context("db: insert collab message")?;

    txn.commit().await.context("db: commit append message")?;
    Ok(Some(AppendOutcome::Created(inserted)))
}

/// Look up a message by its idempotency key.
pub async fn find_by_client_id<C: ConnectionTrait>(
    db: &C,
    author_session_id: &str,
    client_message_id: &str,
) -> Result<Option<Message>> {
    MessageEntity::find()
        .filter(collab_message::Column::AuthorSessionId.eq(author_session_id))
        .filter(collab_message::Column::ClientMessageId.eq(client_message_id))
        .one(db)
        .await
        .context("db: find collab message by client id")
}

/// Find a message of a given task.
pub async fn find_in_task<C: ConnectionTrait>(
    db: &C,
    task_id: &str,
    id: &str,
) -> Result<Option<Message>> {
    MessageEntity::find_by_id(id.to_string())
        .filter(collab_message::Column::TaskId.eq(task_id))
        .one(db)
        .await
        .context("db: find collab message")
}

/// Messages of a task with `seq > after_seq`, ascending, at most `limit`.
pub async fn list_after<C: ConnectionTrait>(
    db: &C,
    task_id: &str,
    after_seq: i64,
    limit: u64,
) -> Result<Vec<Message>> {
    MessageEntity::find()
        .filter(collab_message::Column::TaskId.eq(task_id))
        .filter(collab_message::Column::Seq.gt(after_seq))
        .order_by_asc(collab_message::Column::Seq)
        .limit(limit)
        .all(db)
        .await
        .context("db: list collab messages")
}
