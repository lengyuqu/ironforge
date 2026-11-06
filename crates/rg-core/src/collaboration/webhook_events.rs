//! Collaboration events for repository webhooks.
//!
//! A repository admin can point a webhook at a chat bridge or any other
//! receiver and subscribe it to these events, so a question or a blocked task
//! reaches them without anybody opening the collaboration page. This is the
//! only way these events leave the instance; nothing is sent unless a writer of
//! the repository configured a webhook for the event.
//!
//! What the payload is and is not:
//!
//! * It describes a **declaration**. A client said a question was asked or a
//!   task is blocked; the platform did not verify either.
//! * Everything an agent client wrote (question title, option labels, task
//!   title) sits under `user_content` and is not instructions for the receiver.
//! * The question body and the task description are **not** included: the
//!   receiver gets enough to notify a person and a link to read the rest where
//!   access is checked.
//! * Delivery uses the existing webhook machinery: HMAC signature when a secret
//!   is set, the outbound URL guard, durable retry, and a delivery record.
//! * Best effort after the business write: a failure to hand an event over is
//!   logged and never fails the question or the task update.

use sea_orm::DatabaseConnection;
use serde_json::{json, Value};

use super::question::QuestionView;
use super::task::TaskView;

/// A question was created.
pub const EVENT_QUESTION_ASKED: &str = "collab.question.asked";
/// A task was declared blocked (a real move into `blocked`, not a repeat).
pub const EVENT_TASK_BLOCKED: &str = "collab.task.blocked";

/// Link to a page of the repository's collaboration area, when the instance
/// knows its public address.
fn web_url(external_url: Option<&str>, repo_full_name: &str, suffix: &str) -> Option<String> {
    let base = external_url?.trim().trim_end_matches('/');
    if base.is_empty() {
        return None;
    }
    Some(format!("{base}/{repo_full_name}/collaboration{suffix}"))
}

/// Payload for [`EVENT_QUESTION_ASKED`].
pub fn question_asked_payload(
    repo_full_name: &str,
    external_url: Option<&str>,
    question: &QuestionView,
) -> Value {
    json!({
        "event": EVENT_QUESTION_ASKED,
        "authority": "declaration",
        "repository": repo_full_name,
        "task_id": question.task_id,
        "question": {
            "id": question.id,
            "status": question.status,
            "recipient": {
                "kind": question.recipient.kind,
                "id": question.recipient.id,
            },
            "created_at": question.created_at,
            // Written by an agent client; shown as text, never an instruction.
            "user_content": {
                "title": question.title,
                "options": question.options.iter().map(|o| o.label.as_str()).collect::<Vec<_>>(),
            },
        },
        "url": web_url(external_url, repo_full_name, ""),
    })
}

/// Payload for [`EVENT_TASK_BLOCKED`].
pub fn task_blocked_payload(
    repo_full_name: &str,
    external_url: Option<&str>,
    task: &TaskView,
) -> Value {
    json!({
        "event": EVENT_TASK_BLOCKED,
        "authority": "declaration",
        "repository": repo_full_name,
        "task": {
            "id": task.id,
            "status": task.status,
            "version": task.version,
            "owner_session_id": task.owner_session_id,
            "user_content": { "title": task.title },
        },
        "url": web_url(external_url, repo_full_name, &format!("/tasks/{}", task.id)),
    })
}

async fn send(db: &DatabaseConnection, repo_id: i64, event: &str, payload: Value) {
    if let Err(err) = crate::webhook::service::trigger_event(db, repo_id, event, &payload).await {
        tracing::warn!(
            event,
            "collab webhook: could not hand the event over: {err:#}"
        );
    }
}

/// Hand a freshly created question to the repository's webhooks.
pub async fn question_asked(
    db: &DatabaseConnection,
    repo_id: i64,
    repo_full_name: &str,
    external_url: Option<&str>,
    question: &QuestionView,
) {
    send(
        db,
        repo_id,
        EVENT_QUESTION_ASKED,
        question_asked_payload(repo_full_name, external_url, question),
    )
    .await;
}

/// Hand a task that was just declared blocked to the repository's webhooks.
pub async fn task_blocked(
    db: &DatabaseConnection,
    repo_id: i64,
    repo_full_name: &str,
    external_url: Option<&str>,
    task: &TaskView,
) {
    send(
        db,
        repo_id,
        EVENT_TASK_BLOCKED,
        task_blocked_payload(repo_full_name, external_url, task),
    )
    .await;
}

#[cfg(test)]
mod tests {
    use super::web_url;

    #[test]
    fn links_are_built_only_when_the_public_address_is_known() {
        assert_eq!(web_url(None, "a/b", ""), None);
        assert_eq!(web_url(Some("  "), "a/b", ""), None);
        assert_eq!(
            web_url(Some("https://forge.example/"), "a/b", "/tasks/1").as_deref(),
            Some("https://forge.example/a/b/collaboration/tasks/1")
        );
    }
}
