//! What deleting a repository or an account does to collaboration records.
//!
//! Collaboration records have **no retention policy**: they live exactly as
//! long as the rows they hang from.
//!
//! * Deleting a repository through the application is a *soft* delete: the row
//!   stays, so every collaboration record stays too. Only the Git data is
//!   purged (after `IRONFORGE_DELETED_REPO_RETENTION_DAYS`, default 30), which
//!   leaves commit ids in tasks, questions and work packages unresolvable.
//!   Removing the repository *row* (database administration) cascades through
//!   foreign keys to every collaboration table.
//! * Deleting an account removes its sessions, and with them the tasks those
//!   sessions own and the messages they wrote — including other participants'
//!   messages inside those tasks.
//!
//! These previews only *count*; they authorize nothing and delete nothing, and
//! they are a point-in-time read (counts can change before a deletion runs, so
//! re-read right before acting).

use chrono::{DateTime, Utc};
use rg_db::entities::{
    agent_session, collab_answer, collab_handoff, collab_message, collab_question, collab_takeover,
    collab_task, collab_work_package,
};
use sea_orm::{
    ColumnTrait, Condition, DatabaseConnection, EntityTrait, PaginatorTrait, QueryFilter,
    QuerySelect,
};
use serde::Serialize;

use super::error::CollabResult;

#[derive(Debug, Clone, Serialize, PartialEq, Eq, Default)]
pub struct Counts {
    pub sessions: u64,
    pub tasks: u64,
    pub messages: u64,
    pub handoffs: u64,
    pub takeovers: u64,
    pub questions: u64,
    pub answers: u64,
    pub work_packages: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct RepoImpact {
    pub repo_id: i64,
    /// Collaboration records of the repository today.
    pub records: Counts,
    /// What the application's repository delete does to `records`: `kept`.
    pub on_repository_delete: &'static str,
    /// What removing the repository row does to `records`: `cascade`.
    pub on_repository_row_removal: &'static str,
    /// Days the Git data survives a repository delete before it is purged.
    pub git_data_retention_days: i64,
    pub retention_policy: &'static str,
    pub as_of: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
pub struct UserImpact {
    pub user_id: i64,
    pub sessions_removed: u64,
    /// Tasks owned by the account's sessions: removed with their sessions.
    pub owned_tasks_removed: u64,
    /// Children of those tasks (all participants' messages, handoffs, ...).
    pub removed_with_owned_tasks: Counts,
    /// Messages the account's sessions wrote in tasks owned by *other* sessions;
    /// they are removed too.
    pub messages_in_other_tasks_removed: u64,
    pub deletion_semantics: &'static str,
    pub retention_policy: &'static str,
    pub as_of: DateTime<Utc>,
}

pub async fn repo_impact(
    db: &DatabaseConnection,
    repo_id: i64,
    now: DateTime<Utc>,
) -> CollabResult<RepoImpact> {
    let counts = Counts {
        sessions: agent_session::Entity::find()
            .filter(agent_session::Column::RepoId.eq(repo_id))
            .count(db)
            .await?,
        tasks: collab_task::Entity::find()
            .filter(collab_task::Column::RepoId.eq(repo_id))
            .count(db)
            .await?,
        messages: collab_message::Entity::find()
            .filter(collab_message::Column::RepoId.eq(repo_id))
            .count(db)
            .await?,
        handoffs: collab_handoff::Entity::find()
            .filter(collab_handoff::Column::RepoId.eq(repo_id))
            .count(db)
            .await?,
        takeovers: collab_takeover::Entity::find()
            .filter(collab_takeover::Column::RepoId.eq(repo_id))
            .count(db)
            .await?,
        questions: collab_question::Entity::find()
            .filter(collab_question::Column::RepoId.eq(repo_id))
            .count(db)
            .await?,
        answers: collab_answer::Entity::find()
            .filter(collab_answer::Column::RepoId.eq(repo_id))
            .count(db)
            .await?,
        work_packages: collab_work_package::Entity::find()
            .filter(collab_work_package::Column::RepoId.eq(repo_id))
            .count(db)
            .await?,
    };
    Ok(RepoImpact {
        repo_id,
        records: counts,
        on_repository_delete: "kept",
        on_repository_row_removal: "cascade",
        git_data_retention_days: crate::repo::service::deleted_repo_retention().num_days(),
        retention_policy: "none",
        as_of: now,
    })
}

pub async fn user_impact(
    db: &DatabaseConnection,
    user_id: i64,
    now: DateTime<Utc>,
) -> CollabResult<UserImpact> {
    let sessions: Vec<String> = agent_session::Entity::find()
        .filter(agent_session::Column::UserId.eq(user_id))
        .select_only()
        .column(agent_session::Column::Id)
        .into_tuple()
        .all(db)
        .await?;
    let sessions_removed = sessions.len() as u64;

    let owned_tasks: Vec<String> = if sessions.is_empty() {
        Vec::new()
    } else {
        collab_task::Entity::find()
            .filter(collab_task::Column::OwnerSessionId.is_in(sessions.clone()))
            .select_only()
            .column(collab_task::Column::Id)
            .into_tuple()
            .all(db)
            .await?
    };

    let mut children = Counts {
        tasks: owned_tasks.len() as u64,
        ..Counts::default()
    };
    let mut other_messages = 0;
    if !owned_tasks.is_empty() {
        children.messages = collab_message::Entity::find()
            .filter(collab_message::Column::TaskId.is_in(owned_tasks.clone()))
            .count(db)
            .await?;
        children.handoffs = collab_handoff::Entity::find()
            .filter(collab_handoff::Column::TaskId.is_in(owned_tasks.clone()))
            .count(db)
            .await?;
        children.takeovers = collab_takeover::Entity::find()
            .filter(collab_takeover::Column::TaskId.is_in(owned_tasks.clone()))
            .count(db)
            .await?;
        let questions: Vec<String> = collab_question::Entity::find()
            .filter(collab_question::Column::TaskId.is_in(owned_tasks.clone()))
            .select_only()
            .column(collab_question::Column::Id)
            .into_tuple()
            .all(db)
            .await?;
        children.questions = questions.len() as u64;
        if !questions.is_empty() {
            children.answers = collab_answer::Entity::find()
                .filter(collab_answer::Column::QuestionId.is_in(questions))
                .count(db)
                .await?;
        }
        children.work_packages = collab_work_package::Entity::find()
            .filter(collab_work_package::Column::TaskId.is_in(owned_tasks.clone()))
            .count(db)
            .await?;
    }
    if !sessions.is_empty() {
        let mut authored =
            Condition::all().add(collab_message::Column::AuthorSessionId.is_in(sessions));
        if !owned_tasks.is_empty() {
            authored = authored.add(collab_message::Column::TaskId.is_not_in(owned_tasks));
        }
        other_messages = collab_message::Entity::find()
            .filter(authored)
            .count(db)
            .await?;
    }

    Ok(UserImpact {
        user_id,
        sessions_removed,
        owned_tasks_removed: children.tasks,
        removed_with_owned_tasks: children,
        messages_in_other_tasks_removed: other_messages,
        deletion_semantics: "foreign_key_cascade",
        retention_policy: "none",
        as_of: now,
    })
}
