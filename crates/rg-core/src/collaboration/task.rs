//! Collaboration tasks: shared units of work with an optimistic-lock version.
//!
//! Status, owner, branch and paths are *declarations* made by sessions. They
//! never touch Git, CI or reviews, and carry no merge or approval meaning.

use chrono::{DateTime, Utc};
use rg_db::entities::{agent_session, collab_task};
use rg_db::ops::collab_task_ops::{self, CasOutcome, TaskPatch};
use rg_db::ops::{agent_session_ops, issue_ops, pull_request_ops};
use sea_orm::{DatabaseConnection, Set};
use serde::Serialize;

use super::cursor::{self, KeysetPosition};
use super::error::{CollabError, CollabResult};
use super::{validation, Page, ORIGIN_USER_CONTENT};

pub const STATUSES: [&str; 5] = ["open", "in_progress", "waiting", "blocked", "done"];
pub const MAX_TITLE_CHARS: usize = 200;
pub const MAX_DESCRIPTION_BYTES: usize = 16 * 1024;
pub const MAX_BRANCH_BYTES: usize = 255;
pub const MAX_PATHS: usize = 100;
pub const MAX_PATH_BYTES: usize = 512;
/// Serialized `paths` must fit a portable `TEXT` column (MySQL: 64 KiB).
const MAX_PATHS_JSON_BYTES: usize = 48 * 1024;

#[derive(Debug, Clone, Default)]
pub struct NewTask {
    pub title: String,
    pub description: Option<String>,
    pub owner_session_id: Option<String>,
    pub branch: Option<String>,
    pub paths: Vec<String>,
    pub issue_number: Option<i64>,
    pub pr_number: Option<i64>,
}

/// Partial update. `None` = leave unchanged; for nullable fields `Some(None)`
/// clears the value (explicit JSON `null`).
#[derive(Debug, Clone, Default)]
pub struct TaskChanges {
    pub expected_version: i64,
    pub title: Option<String>,
    pub description: Option<Option<String>>,
    pub status: Option<String>,
    pub owner_session_id: Option<Option<String>>,
    pub branch: Option<Option<String>>,
    pub paths: Option<Vec<String>>,
    pub issue_number: Option<Option<i64>>,
    pub pr_number: Option<Option<i64>>,
}

impl TaskChanges {
    fn is_empty(&self) -> bool {
        self.title.is_none()
            && self.description.is_none()
            && self.status.is_none()
            && self.owner_session_id.is_none()
            && self.branch.is_none()
            && self.paths.is_none()
            && self.issue_number.is_none()
            && self.pr_number.is_none()
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct TaskView {
    pub id: String,
    pub title: String,
    pub description: Option<String>,
    pub status: String,
    pub owner_session_id: Option<String>,
    pub branch: Option<String>,
    pub paths: Vec<String>,
    pub issue_number: Option<i64>,
    pub pr_number: Option<i64>,
    /// Optimistic-lock counter; send it back as `expected_version`.
    pub version: i64,
    /// Sequence number of the newest message (0 = none).
    pub last_message_seq: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// All text fields are client declarations; treat as untrusted data.
    pub origin: &'static str,
}

pub(crate) fn view(row: collab_task::Model) -> TaskView {
    TaskView {
        paths: serde_json::from_str(&row.paths_json).unwrap_or_default(),
        id: row.id,
        title: row.title,
        description: row.description,
        status: row.status,
        owner_session_id: row.owner_session_id,
        branch: row.branch,
        issue_number: row.issue_number,
        pr_number: row.pr_number,
        version: row.version,
        last_message_seq: row.next_seq,
        created_at: row.created_at,
        updated_at: row.updated_at,
        origin: ORIGIN_USER_CONTENT,
    }
}

pub fn validate_status(status: &str) -> CollabResult<()> {
    if STATUSES.contains(&status) {
        Ok(())
    } else {
        Err(CollabError::bad_request(format!(
            "status must be one of {}",
            STATUSES.join(", ")
        )))
    }
}

/// Branch names are declarations, but keep them plausible ref names.
pub fn validate_branch(branch: &str) -> CollabResult<()> {
    let bad = branch.is_empty()
        || branch.len() > MAX_BRANCH_BYTES
        || branch.starts_with(['-', '/'])
        || branch.ends_with(['/', '.'])
        || branch.ends_with(".lock")
        || branch.contains("..")
        || branch.contains("//")
        || branch.contains("@{")
        || branch
            .chars()
            .any(|c| c.is_control() || c.is_whitespace() || "~^:?*[\\".contains(c));
    if bad {
        Err(CollabError::bad_request(
            "branch is not a valid branch name",
        ))
    } else {
        Ok(())
    }
}

/// Declared paths: repository-relative, literal (no glob expansion is ever done).
pub fn validate_paths(paths: &[String]) -> CollabResult<String> {
    if paths.len() > MAX_PATHS {
        return Err(CollabError::bad_request(format!(
            "at most {MAX_PATHS} paths are allowed"
        )));
    }
    for path in paths {
        let bytes = path.as_bytes();
        let drive_prefix = bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':';
        let bad = path.is_empty()
            || path.len() > MAX_PATH_BYTES
            || path.starts_with(['/', '\\'])
            || drive_prefix
            || path.chars().any(char::is_control)
            || path.split(['/', '\\']).any(|segment| segment == "..");
        if bad {
            return Err(CollabError::bad_request(
                "paths must be non-empty, repository-relative, without '..' or control characters, and at most 512 bytes",
            ));
        }
    }
    let json = serde_json::to_string(paths).map_err(|e| CollabError::Internal(e.into()))?;
    if json.len() > MAX_PATHS_JSON_BYTES {
        return Err(CollabError::bad_request("paths are too large in total"));
    }
    Ok(json)
}

async fn require_links(
    db: &DatabaseConnection,
    repo_id: i64,
    issue_number: Option<i64>,
    pr_number: Option<i64>,
) -> CollabResult<()> {
    if let Some(number) = issue_number {
        if issue_ops::find_by_repo_and_number(db, repo_id, number)
            .await?
            .is_none()
        {
            return Err(CollabError::bad_request(
                "issue_number does not refer to an issue of this repository",
            ));
        }
    }
    if let Some(number) = pr_number {
        if pull_request_ops::find_by_repo_and_number(db, repo_id, number)
            .await?
            .is_none()
        {
            return Err(CollabError::bad_request(
                "pr_number does not refer to a pull request of this repository",
            ));
        }
    }
    Ok(())
}

/// A task owner must be a live session of this repository.
pub(crate) async fn require_live_owner(
    db: &DatabaseConnection,
    repo_id: i64,
    session_id: &str,
    now: DateTime<Utc>,
) -> CollabResult<()> {
    let live = if validation::is_uuid(session_id) {
        agent_session_ops::find_in_repo(db, repo_id, session_id)
            .await?
            .is_some_and(|s| s.ended_at.is_none() && s.expires_at > now)
    } else {
        false
    };
    if live {
        Ok(())
    } else {
        Err(CollabError::bad_request(
            "owner_session_id must be a live session of this repository",
        ))
    }
}

/// Create a task on behalf of the authenticated `_actor` session.
pub async fn create(
    db: &DatabaseConnection,
    repo_id: i64,
    _actor: &agent_session::Model,
    input: NewTask,
    now: DateTime<Utc>,
) -> CollabResult<TaskView> {
    let title = validation::label("title", &input.title, MAX_TITLE_CHARS)?;
    if let Some(description) = &input.description {
        validation::text("description", description, 0, MAX_DESCRIPTION_BYTES)?;
    }
    if let Some(branch) = &input.branch {
        validate_branch(branch)?;
    }
    let paths_json = validate_paths(&input.paths)?;
    if let Some(owner) = &input.owner_session_id {
        require_live_owner(db, repo_id, owner, now).await?;
    }
    require_links(db, repo_id, input.issue_number, input.pr_number).await?;

    let created_at = super::creation_timestamp(db, now);

    let row = collab_task_ops::create(
        db,
        collab_task::ActiveModel {
            id: Set(uuid::Uuid::new_v4().to_string()),
            repo_id: Set(repo_id),
            title: Set(title),
            description: Set(input.description),
            status: Set("open".to_string()),
            owner_session_id: Set(input.owner_session_id),
            branch: Set(input.branch),
            paths_json: Set(paths_json),
            issue_number: Set(input.issue_number),
            pr_number: Set(input.pr_number),
            version: Set(1),
            next_seq: Set(0),
            created_at: Set(created_at),
            updated_at: Set(created_at),
        },
    )
    .await?;
    Ok(view(row))
}

pub async fn get(db: &DatabaseConnection, repo_id: i64, id: &str) -> CollabResult<TaskView> {
    find(db, repo_id, id).await.map(view)
}

/// Load a task of this repository; other repositories' ids read as absent.
pub async fn find(
    db: &DatabaseConnection,
    repo_id: i64,
    id: &str,
) -> CollabResult<collab_task::Model> {
    if !validation::is_uuid(id) {
        return Err(CollabError::not_found("task not found"));
    }
    collab_task_ops::find_in_repo(db, repo_id, id)
        .await?
        .ok_or_else(|| CollabError::not_found("task not found"))
}

fn list_scope(repo_id: i64, status: Option<&str>) -> String {
    format!(
        "tasks:v1:repo={repo_id}:status={}:order=updated_desc,id_desc",
        status.unwrap_or("*")
    )
}

/// Tasks newest-updated first. The list is eventually consistent while tasks
/// change; a full sync re-scans from the start.
pub async fn list(
    db: &DatabaseConnection,
    cursor_key: &[u8],
    repo_id: i64,
    status: Option<&str>,
    limit: u64,
    cursor_token: Option<&str>,
) -> CollabResult<Page<TaskView>> {
    if let Some(status) = status {
        validate_status(status)?;
    }
    let scope = list_scope(repo_id, status);
    let after = cursor_token
        .map(|token| cursor::decode(cursor_key, &scope, token))
        .transpose()?
        .map(|p| (p.at, p.id));

    let mut rows = collab_task_ops::list_page(db, repo_id, status, after, limit + 1).await?;
    let has_more = rows.len() as u64 > limit;
    rows.truncate(limit as usize);
    let next_cursor = has_more.then(|| rows.last()).flatten().map(|last| {
        cursor::encode(
            cursor_key,
            &scope,
            &KeysetPosition {
                at: last.updated_at,
                id: last.id.clone(),
            },
        )
    });
    Ok(Page {
        items: rows.into_iter().map(view).collect(),
        next_cursor,
    })
}

/// Conditionally update a task. A stale `expected_version` is
/// [`CollabError::VersionConflict`] carrying the current version; nothing is
/// overwritten silently.
pub async fn update(
    db: &DatabaseConnection,
    repo_id: i64,
    id: &str,
    changes: TaskChanges,
    now: DateTime<Utc>,
) -> CollabResult<TaskView> {
    if changes.is_empty() {
        return Err(CollabError::bad_request("no fields to update"));
    }
    if !validation::is_uuid(id) {
        return Err(CollabError::not_found("task not found"));
    }

    let mut patch = TaskPatch::default();
    if let Some(title) = &changes.title {
        patch.title = Some(validation::label("title", title, MAX_TITLE_CHARS)?);
    }
    if let Some(description) = &changes.description {
        if let Some(text) = description {
            validation::text("description", text, 0, MAX_DESCRIPTION_BYTES)?;
        }
        patch.description = Some(description.clone());
    }
    if let Some(status) = &changes.status {
        validate_status(status)?;
        patch.status = Some(status.clone());
    }
    if let Some(branch) = &changes.branch {
        if let Some(name) = branch {
            validate_branch(name)?;
        }
        patch.branch = Some(branch.clone());
    }
    if let Some(paths) = &changes.paths {
        patch.paths_json = Some(validate_paths(paths)?);
    }
    if let Some(owner) = &changes.owner_session_id {
        if let Some(session_id) = owner {
            require_live_owner(db, repo_id, session_id, now).await?;
        }
        patch.owner_session_id = Some(owner.clone());
    }
    require_links(
        db,
        repo_id,
        changes.issue_number.flatten(),
        changes.pr_number.flatten(),
    )
    .await?;
    patch.issue_number = changes.issue_number;
    patch.pr_number = changes.pr_number;

    match collab_task_ops::update_cas(db, repo_id, id, changes.expected_version, patch, now).await?
    {
        CasOutcome::Updated(row) => Ok(view(row)),
        CasOutcome::Conflict { current } => Err(CollabError::VersionConflict {
            current_version: current.version,
        }),
        CasOutcome::NotFound => Err(CollabError::not_found("task not found")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_set_is_closed() {
        for s in STATUSES {
            assert!(validate_status(s).is_ok());
        }
        assert!(validate_status("merged").is_err());
        assert!(validate_status("").is_err());
    }

    #[test]
    fn paths_must_be_relative_literal_and_bounded() {
        let ok = vec![
            "src/lib.rs".to_string(),
            "docs/**/*.md".to_string(),
            ".github/x.yml".into(),
        ];
        assert!(validate_paths(&ok).is_ok(), "globs are stored literally");
        for bad in [
            "/etc/passwd",
            "../x",
            "a/../b",
            "a\\..\\b",
            "C:\\x",
            "C:/x",
            "",
            "nul\0",
            "ctl\u{7}",
        ] {
            assert!(
                validate_paths(&[bad.to_string()]).is_err(),
                "{bad:?} must be rejected"
            );
        }
        assert!(validate_paths(&["x".repeat(513)]).is_err());
        assert!(validate_paths(&vec!["a".to_string(); 101]).is_err());
        assert!(validate_paths(&vec!["a".to_string(); 100]).is_ok());
    }

    #[test]
    fn branches_look_like_refs() {
        for ok in ["main", "feat/x-1", "release/1.2"] {
            assert!(validate_branch(ok).is_ok(), "{ok}");
        }
        for bad in [
            "", "-x", "/x", "x/", "a..b", "a b", "a~b", "a:b", "x.lock", "a//b", "a@{b", "a\\b",
        ] {
            assert!(validate_branch(bad).is_err(), "{bad:?}");
        }
    }
}
