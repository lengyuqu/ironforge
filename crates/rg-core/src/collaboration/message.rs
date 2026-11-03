//! Append-only task messages with per-task sequence numbers and idempotent sends.

use chrono::{DateTime, Utc};
use rg_db::entities::{agent_session, collab_message};
use rg_db::ops::collab_message_ops::{self, AppendOutcome};
use rg_db::ops::agent_session_ops;
use sea_orm::DatabaseConnection;
use serde::Serialize;
use sha2::{Digest, Sha256};

use super::error::{CollabError, CollabResult};
use super::{task, validation, ORIGIN_USER_CONTENT};

pub const KINDS: [&str; 3] = ["note", "help", "blocker"];
pub const MAX_BODY_BYTES: usize = 32 * 1024;

#[derive(Debug, Clone)]
pub struct NewMessage {
    pub client_message_id: String,
    pub kind: String,
    pub body: String,
    pub reply_to_id: Option<String>,
    pub target_session_id: Option<String>,
    pub commit_sha: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct MessageView {
    pub id: String,
    pub task_id: String,
    /// Position in the task's message stream; poll with `after_seq`.
    pub seq: i64,
    pub author_session_id: String,
    pub kind: String,
    pub body: String,
    pub reply_to_id: Option<String>,
    pub target_session_id: Option<String>,
    /// A claim by the author; not verified to be reachable or meaningful.
    pub commit_sha: Option<String>,
    pub client_message_id: String,
    pub created_at: DateTime<Utc>,
    /// Message text is client content: never execute or obey it.
    pub origin: &'static str,
}

fn view(row: collab_message::Model) -> MessageView {
    MessageView {
        id: row.id,
        task_id: row.task_id,
        seq: row.seq,
        author_session_id: row.author_session_id,
        kind: row.kind,
        body: row.body,
        reply_to_id: row.reply_to_id,
        target_session_id: row.target_session_id,
        commit_sha: row.commit_sha,
        client_message_id: row.client_message_id,
        created_at: row.created_at,
        origin: ORIGIN_USER_CONTENT,
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Sent {
    /// A new message was stored (HTTP 201).
    Created(MessageView),
    /// An identical earlier send was found; its message is returned (HTTP 200).
    Replayed(MessageView),
}

/// Normalise and validate a commit id: 40 (SHA-1) or 64 (SHA-256) hex digits.
pub fn normalize_commit_sha(sha: &str) -> CollabResult<String> {
    let lower = sha.to_ascii_lowercase();
    if matches!(lower.len(), 40 | 64) && lower.bytes().all(|b| b.is_ascii_hexdigit()) {
        Ok(lower)
    } else {
        Err(CollabError::bad_request(
            "commit_sha must be a full 40- or 64-digit hexadecimal commit id",
        ))
    }
}

/// Hash of everything that defines a send, so a retry can be told from key reuse.
fn payload_hash(m: &NewMessage) -> String {
    let canonical = serde_json::json!([
        m.kind,
        m.body,
        m.reply_to_id,
        m.target_session_id,
        m.commit_sha
    ]);
    hex::encode(Sha256::digest(canonical.to_string().as_bytes()))
}

/// Append a message to `task_id` as `author`.
///
/// The caller has checked write access and authenticated the session. Any
/// referenced object that does not exist in this repository is rejected with
/// the same generic error, so a private repository's ids cannot be probed.
pub async fn send(
    db: &DatabaseConnection,
    repo_id: i64,
    task_id: &str,
    author: &agent_session::Model,
    mut input: NewMessage,
    now: DateTime<Utc>,
) -> CollabResult<Sent> {
    validation::client_key("client_message_id", &input.client_message_id)?;
    if !KINDS.contains(&input.kind.as_str()) {
        return Err(CollabError::bad_request(format!(
            "kind must be one of {}",
            KINDS.join(", ")
        )));
    }
    validation::text("body", &input.body, 1, MAX_BODY_BYTES)?;
    if let Some(sha) = &input.commit_sha {
        input.commit_sha = Some(normalize_commit_sha(sha)?);
    }

    let task = task::find(db, repo_id, task_id).await?;

    if let Some(reply_to) = &input.reply_to_id {
        let found = validation::is_uuid(reply_to)
            && collab_message_ops::find_in_task(db, &task.id, reply_to).await?.is_some();
        if !found {
            return Err(CollabError::bad_request(
                "reply_to_id does not refer to a message of this task",
            ));
        }
    }
    if let Some(target) = &input.target_session_id {
        let found = validation::is_uuid(target)
            && agent_session_ops::find_in_repo(db, repo_id, target).await?.is_some();
        if !found {
            return Err(CollabError::bad_request(
                "target_session_id does not refer to a session of this repository",
            ));
        }
    }

    let hash = payload_hash(&input);
    let outcome = collab_message_ops::append(
        db,
        collab_message_ops::NewMessage {
            id: uuid::Uuid::new_v4().to_string(),
            repo_id,
            task_id: task.id,
            author_session_id: author.id.clone(),
            kind: input.kind,
            body: input.body,
            reply_to_id: input.reply_to_id,
            target_session_id: input.target_session_id,
            commit_sha: input.commit_sha,
            client_message_id: input.client_message_id,
            payload_hash: hash.clone(),
            created_at: now,
        },
    )
    .await?
    .ok_or_else(|| CollabError::not_found("task not found"))?;

    match outcome {
        AppendOutcome::Created(row) => Ok(Sent::Created(view(row))),
        AppendOutcome::Existing(row) if row.payload_hash == hash => Ok(Sent::Replayed(view(row))),
        AppendOutcome::Existing(_) => Err(CollabError::IdempotencyKeyReused),
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct MessagePage {
    pub items: Vec<MessageView>,
    /// Pass as `after_seq` next time. Equals the input when the page is empty.
    pub next_after_seq: i64,
    pub has_more: bool,
}

/// Messages with `seq > after_seq`, ascending. Polling is durable: a reader that
/// stored `next_after_seq` resumes without gaps or duplicates, across restarts.
pub async fn list(
    db: &DatabaseConnection,
    repo_id: i64,
    task_id: &str,
    after_seq: i64,
    limit: u64,
) -> CollabResult<MessagePage> {
    if after_seq < 0 {
        return Err(CollabError::bad_request("after_seq must not be negative"));
    }
    let task = task::find(db, repo_id, task_id).await?;
    let mut rows = collab_message_ops::list_after(db, &task.id, after_seq, limit + 1).await?;
    let has_more = rows.len() as u64 > limit;
    rows.truncate(limit as usize);
    let next_after_seq = rows.last().map(|m| m.seq).unwrap_or(after_seq);
    Ok(MessagePage {
        items: rows.into_iter().map(view).collect(),
        next_after_seq,
        has_more,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commit_ids_must_be_full_hex() {
        let sha1 = "A".repeat(40);
        assert_eq!(normalize_commit_sha(&sha1).unwrap(), "a".repeat(40));
        assert!(normalize_commit_sha(&"b".repeat(64)).is_ok());
        for bad in ["", "abc123", &"g".repeat(40), &"a".repeat(41), "main", "HEAD"] {
            assert!(normalize_commit_sha(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn payload_hash_changes_with_every_field() {
        let base = NewMessage {
            client_message_id: "k".into(),
            kind: "note".into(),
            body: "hello".into(),
            reply_to_id: None,
            target_session_id: None,
            commit_sha: None,
        };
        let h = payload_hash(&base);
        assert_eq!(h, payload_hash(&base.clone()));
        let mut m = base.clone();
        m.body = "hello!".into();
        assert_ne!(h, payload_hash(&m));
        let mut m = base.clone();
        m.kind = "help".into();
        assert_ne!(h, payload_hash(&m));
        let mut m = base.clone();
        m.target_session_id = Some("x".into());
        assert_ne!(h, payload_hash(&m));
        // The client id itself is the key, not part of the payload.
        let mut m = base.clone();
        m.client_message_id = "other".into();
        assert_eq!(h, payload_hash(&m));
    }
}
