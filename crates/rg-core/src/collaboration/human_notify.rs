//! Telling people that something is waiting for them.
//!
//! Until now a question an agent asked "to the repository writers" was only
//! visible to someone who opened the collaboration page. This module turns the
//! moment a question is created into an ordinary in-app notification for the
//! people who may answer it, so the existing bell, unread count and WebSocket
//! push carry it.
//!
//! What this is and is not:
//!
//! * It is a nudge, sent best effort **after** the question was committed. A
//!   failure here is logged and never fails or rolls back the question.
//! * It is sent once per created question: an idempotent replay does not call
//!   in here, and a lost race against the unique key returns `replayed`.
//! * Recipients are re-checked against the *current* repository permissions at
//!   send time (the permission cache is bypassed for each candidate), so a user
//!   who just lost access is not told about a question.
//! * The notification title is the question title; the body is a fixed
//!   sentence naming the repository and task. The title is written
//!   by a client and is shown as plain text; the question body is not copied
//!   into the notification.
//! * There is no email and no webhook yet; nothing here reaches outside the
//!   instance.

use rg_db::entities::repository;
use rg_db::ops::{agent_session_ops, notification_ops, org_ops, repo_collaborator_ops, user_ops};
use sea_orm::DatabaseConnection;

use super::question::{QuestionView, RECIPIENT_REPO_WRITERS, RECIPIENT_USER};
use super::task::TaskView;
use crate::repo::service::{can_read_repo, can_write_repo, invalidate_perm_cache_user};

/// `notifications.event_type` of a question that is waiting for a person.
pub const EVENT_QUESTION_ASKED: &str = "collab_question";

/// `notifications.event_type` of a task its own session declared blocked.
pub const EVENT_TASK_BLOCKED: &str = "collab_task_blocked";

/// Most people notified for one question. A repository with more writers than
/// this still gets the question in everyone's "waiting for you" list; only the
/// bell is capped, so one agent cannot fill hundreds of inboxes.
pub const MAX_RECIPIENTS_PER_QUESTION: usize = 50;

/// Candidates examined before giving up on finding writers (owner,
/// collaborators and organization members together).
const MAX_CANDIDATES: usize = 500;

/// Longest question title copied into a notification title, in characters.
const MAX_TITLE_CHARS: usize = 120;

/// One person who was notified, for the caller to push over WebSocket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notified {
    pub user_id: i64,
    pub notification_id: i64,
}

fn truncate_chars(text: &str, max: usize) -> String {
    let mut out: String = text.chars().take(max).collect();
    if text.chars().count() > max {
        out.push('…');
    }
    out
}

/// Who may be told about a question addressed to `recipient`, in a stable
/// order and without duplicates. Permissions are checked here, not trusted from
/// the question.
async fn recipients(
    db: &DatabaseConnection,
    repo: &repository::Model,
    question: &QuestionView,
) -> anyhow::Result<Vec<i64>> {
    let mut out: Vec<i64> = Vec::new();
    match question.recipient.kind.as_str() {
        RECIPIENT_USER => {
            let Some(id) = question
                .recipient
                .id
                .as_deref()
                .and_then(|id| id.parse::<i64>().ok())
            else {
                return Ok(out);
            };
            // A user id that does not exist, or that cannot read the repository,
            // is told nothing: that would confirm the id and leak a title.
            if user_ops::find_by_id(db, id).await?.is_some() {
                invalidate_perm_cache_user(repo.id, id);
                if can_read_repo(db, repo, Some(id)).await? {
                    out.push(id);
                }
            }
        }
        RECIPIENT_REPO_WRITERS => {
            let mut candidates: Vec<i64> = vec![repo.owner_id];
            for c in repo_collaborator_ops::list_by_repo(db, repo.id).await? {
                if matches!(c.permission.as_str(), "write" | "admin") {
                    candidates.push(c.user_id);
                }
            }
            if let Some(org_id) = repo.org_id {
                for m in org_ops::list_org_members(db, org_id).await? {
                    candidates.push(m.user_id);
                }
            }
            let mut seen = std::collections::HashSet::new();
            for id in candidates.into_iter().take(MAX_CANDIDATES) {
                if out.len() >= MAX_RECIPIENTS_PER_QUESTION {
                    break;
                }
                if !seen.insert(id) {
                    continue;
                }
                invalidate_perm_cache_user(repo.id, id);
                if can_write_repo(db, repo, Some(id)).await? {
                    out.push(id);
                }
            }
        }
        // A session recipient already gets a durable notice (see `notice`); a
        // session is a label, not a person.
        _ => {}
    }
    Ok(out)
}

/// Notify the people a freshly created question is addressed to.
///
/// Best effort: every failure is logged and swallowed. Returns who was actually
/// notified so the HTTP layer can push the same event over WebSocket.
pub async fn question_asked(
    db: &DatabaseConnection,
    repo: &repository::Model,
    repo_full_name: &str,
    question: &QuestionView,
) -> Vec<Notified> {
    let people = match recipients(db, repo, question).await {
        Ok(people) => people,
        Err(err) => {
            tracing::warn!(question = %question.id, "collab notify: could not resolve recipients: {err:#}");
            return Vec::new();
        }
    };
    // The title is the question's own title and nothing else, so a client can
    // put its own localized wording around it (the web UI does).
    let title = truncate_chars(&question.title, MAX_TITLE_CHARS);
    let body = format!(
        "In {repo_full_name}, on task {}. The title was written by an agent client and is not verified. Open the repository's collaboration page to answer.",
        question.task_id
    );
    let mut notified = Vec::new();
    for user_id in people {
        match crate::notification::notify(
            db,
            user_id,
            EVENT_QUESTION_ASKED,
            &title,
            Some(&body),
            Some(repo.id),
        )
        .await
        {
            Ok(row) => notified.push(Notified {
                user_id,
                notification_id: row.id,
            }),
            Err(err) => {
                tracing::warn!(question = %question.id, user_id, "collab notify: could not store notification: {err}");
            }
        }
    }
    notified
}

/// Tell the account behind a task's owner session that the task was just
/// declared `blocked`.
///
/// Who: only the account that owns the task's owner session, not every writer.
/// Sessions of one account all belong to the same person, so this is "your agent
/// is stuck", and it stays quiet in a repository with many writers.
///
/// When: the caller invokes this only on a real transition into `blocked`, not
/// when a blocked task is updated again. While the earlier notification about
/// the same task is still unread, a new one is not added, so a task that flaps
/// between states does not fill the bell.
///
/// The status is a declaration by a client; nothing here says the task is
/// really stuck.
pub async fn task_blocked(
    db: &DatabaseConnection,
    repo: &repository::Model,
    repo_full_name: &str,
    task: &TaskView,
) -> Vec<Notified> {
    let Some(session_id) = task.owner_session_id.as_deref() else {
        return Vec::new();
    };
    let session = match agent_session_ops::find_in_repo(db, repo.id, session_id).await {
        Ok(Some(session)) => session,
        Ok(None) => return Vec::new(),
        Err(err) => {
            tracing::warn!(task = %task.id, "collab notify: could not load the owner session: {err:#}");
            return Vec::new();
        }
    };
    let user_id = session.user_id;
    invalidate_perm_cache_user(repo.id, user_id);
    match can_read_repo(db, repo, Some(user_id)).await {
        Ok(true) => {}
        Ok(false) => return Vec::new(),
        Err(err) => {
            tracing::warn!(task = %task.id, "collab notify: permission check failed: {err:#}");
            return Vec::new();
        }
    }
    let body = format!(
        "In {repo_full_name}, task {}. A client declared it blocked; that is a declaration, not verified. Open the repository's collaboration page to look.",
        task.id
    );
    match notification_ops::has_unread_with_body(db, user_id, EVENT_TASK_BLOCKED, &body).await {
        Ok(false) => {}
        Ok(true) => return Vec::new(),
        Err(err) => {
            tracing::warn!(task = %task.id, "collab notify: duplicate check failed: {err:#}");
            return Vec::new();
        }
    }
    match crate::notification::notify(
        db,
        user_id,
        EVENT_TASK_BLOCKED,
        &truncate_chars(&task.title, MAX_TITLE_CHARS),
        Some(&body),
        Some(repo.id),
    )
    .await
    {
        Ok(row) => vec![Notified {
            user_id,
            notification_id: row.id,
        }],
        Err(err) => {
            tracing::warn!(task = %task.id, "collab notify: could not store notification: {err}");
            Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::truncate_chars;

    #[test]
    fn titles_are_cut_on_character_boundaries() {
        assert_eq!(truncate_chars("short", 10), "short");
        assert_eq!(truncate_chars("abcdef", 3), "abc…");
        assert_eq!(truncate_chars("你好世界", 2), "你好…");
    }
}
