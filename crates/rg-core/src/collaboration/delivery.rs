//! Delivery receipts of targeted messages (P1 U04, R09).
//!
//! `saved` only says the message is stored. `read_at` is set only when the
//! *target* session explicitly confirms. `replied_at` is the target's own direct
//! reply. None of these is agreement. The read watermark (`collab_reads`) is
//! unrelated: it never creates a receipt, a receipt never moves it, and the
//! absence of a receipt means "unknown", not "unread".

use chrono::{DateTime, Utc};
use rg_db::entities::agent_session;
use rg_db::ops::collab_message_ops;
use rg_db::ops::collab_notice_ops::{self, ReceiptOutcome};
use sea_orm::DatabaseConnection;
use serde::Serialize;

use super::error::{CollabError, CollabResult};
use super::{task, validation};

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct DeliveryStatus {
    pub message_id: String,
    pub task_id: String,
    /// The message is persisted. Says nothing about anyone having seen it.
    pub saved: bool,
    pub target_session_id: Option<String>,
    /// Confirmed by the target session itself.
    pub read_at: Option<DateTime<Utc>>,
    /// The target's earliest direct reply (`reply_to_id`); not agreement.
    pub replied_at: Option<DateTime<Utc>>,
    pub reply_message_id: Option<String>,
    /// `not_targeted` | `unknown` | `read` | `replied`. `unknown` = no
    /// confirmation exists, which is not the same as unread.
    pub status: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct Receipt {
    pub message_id: String,
    pub task_id: String,
    pub target_session_id: String,
    pub read_at: DateTime<Utc>,
    /// `false` when the target had already confirmed (original `read_at` kept).
    pub newly_confirmed: bool,
}

async fn targeted_message(
    db: &DatabaseConnection,
    repo_id: i64,
    task_id: &str,
    message_id: &str,
) -> CollabResult<(rg_db::entities::collab_message::Model, String)> {
    let task = task::find(db, repo_id, task_id).await?;
    if !validation::is_uuid(message_id) {
        return Err(CollabError::not_found("message not found"));
    }
    let message = collab_message_ops::find_in_task(db, &task.id, message_id)
        .await?
        .ok_or_else(|| CollabError::not_found("message not found"))?;
    Ok((message, task.id))
}

/// The target session confirms it read `message_id`. Idempotent; only the
/// target can do it, and an expired target cannot be impersonated (the caller
/// has authenticated `session` as live).
pub async fn confirm(
    db: &DatabaseConnection,
    repo_id: i64,
    task_id: &str,
    message_id: &str,
    session: &agent_session::Model,
    now: DateTime<Utc>,
) -> CollabResult<Receipt> {
    let (message, task_id) = targeted_message(db, repo_id, task_id, message_id).await?;
    match &message.target_session_id {
        Some(target) if *target == session.id => {}
        Some(_) => {
            return Err(CollabError::Forbidden(
                "only the target session of a message may confirm it".to_string(),
            ))
        }
        None => {
            return Err(CollabError::Forbidden(
                "the message is not addressed to a session; there is nothing to confirm"
                    .to_string(),
            ))
        }
    }
    let outcome =
        collab_notice_ops::confirm_receipt(db, repo_id, &task_id, &message.id, &session.id, now)
            .await?;
    let (row, newly_confirmed) = match outcome {
        ReceiptOutcome::Created(row) => (row, true),
        ReceiptOutcome::Existing(row) => (row, false),
    };
    Ok(Receipt {
        message_id: row.message_id,
        task_id: row.task_id,
        target_session_id: row.target_session_id,
        read_at: row.read_at,
        newly_confirmed,
    })
}

/// Delivery state of a message, for its author only: a receipt is another
/// session's personal reading state and is not shown to other readers.
pub async fn status(
    db: &DatabaseConnection,
    repo_id: i64,
    task_id: &str,
    message_id: &str,
    author: &agent_session::Model,
) -> CollabResult<DeliveryStatus> {
    let (message, task_id) = targeted_message(db, repo_id, task_id, message_id).await?;
    if message.author_session_id != author.id {
        return Err(CollabError::Forbidden(
            "only the author of a message may read its delivery state".to_string(),
        ));
    }
    let Some(target) = message.target_session_id.clone() else {
        return Ok(DeliveryStatus {
            message_id: message.id,
            task_id,
            saved: true,
            target_session_id: None,
            read_at: None,
            replied_at: None,
            reply_message_id: None,
            status: "not_targeted",
        });
    };
    let receipt = collab_notice_ops::find_receipt(db, &message.id, &target).await?;
    let reply = collab_message_ops::first_reply_by(db, &task_id, &message.id, &target).await?;
    let status = if reply.is_some() {
        "replied"
    } else if receipt.is_some() {
        "read"
    } else {
        "unknown"
    };
    Ok(DeliveryStatus {
        message_id: message.id,
        task_id,
        saved: true,
        target_session_id: Some(target),
        read_at: receipt.map(|r| r.read_at),
        replied_at: reply.as_ref().map(|r| r.created_at),
        reply_message_id: reply.map(|r| r.id),
        status,
    })
}
