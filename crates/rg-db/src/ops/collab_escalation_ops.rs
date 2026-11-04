//! Database operations for `collab_escalation_settings` and the read queries
//! escalation evaluation needs.

use anyhow::{Context, Result};
use sea_orm::sea_query::Expr;
use sea_orm::*;

use crate::entities::collab_escalation_setting::{
    self, ActiveModel, Entity as SettingEntity, Model as Setting,
};
use crate::entities::collab_handoff::{self, Entity as HandoffEntity, Model as Handoff};
use crate::entities::collab_task::{self, Entity as TaskEntity, Model as Task};
use super::collab_handoff_ops::status as handoff_status;

pub async fn get<C: ConnectionTrait>(db: &C, repo_id: i64) -> Result<Option<Setting>> {
    SettingEntity::find_by_id(repo_id)
        .one(db)
        .await
        .context("db: find escalation settings")
}

#[derive(Debug, Clone, PartialEq)]
pub enum SaveOutcome {
    Saved(Setting),
    /// The stored version differs from `expected_version` (0 = none stored);
    /// `current` is the winner's row if there is one.
    Conflict { current: Option<Setting> },
}

/// Store `settings` iff the stored version is `expected_version` (0 = no row
/// yet). A first save is a plain insert whose unique violation means another
/// admin saved first; later saves are one conditional `UPDATE`.
pub async fn save_cas(
    db: &DatabaseConnection,
    expected_version: i64,
    settings: Setting,
) -> Result<SaveOutcome> {
    if expected_version == 0 {
        let row = ActiveModel {
            repo_id: Set(settings.repo_id),
            enabled: Set(settings.enabled),
            blocked_after_secs: Set(settings.blocked_after_secs),
            question_after_secs: Set(settings.question_after_secs),
            handoff_after_secs: Set(settings.handoff_after_secs),
            repeat_after_secs: Set(settings.repeat_after_secs),
            quiet_start_minute: Set(settings.quiet_start_minute),
            quiet_end_minute: Set(settings.quiet_end_minute),
            version: Set(1),
            updated_by_user_id: Set(settings.updated_by_user_id),
            updated_at: Set(settings.updated_at),
        };
        return match row.insert(db).await {
            Ok(saved) => Ok(SaveOutcome::Saved(saved)),
            Err(err) if matches!(err.sql_err(), Some(SqlErr::UniqueConstraintViolation(_))) => {
                Ok(SaveOutcome::Conflict { current: get(db, settings.repo_id).await? })
            }
            Err(err) => Err(err).context("db: insert escalation settings"),
        };
    }
    let res = SettingEntity::update_many()
        .col_expr(collab_escalation_setting::Column::Enabled, Expr::value(settings.enabled))
        .col_expr(
            collab_escalation_setting::Column::BlockedAfterSecs,
            Expr::value(settings.blocked_after_secs),
        )
        .col_expr(
            collab_escalation_setting::Column::QuestionAfterSecs,
            Expr::value(settings.question_after_secs),
        )
        .col_expr(
            collab_escalation_setting::Column::HandoffAfterSecs,
            Expr::value(settings.handoff_after_secs),
        )
        .col_expr(
            collab_escalation_setting::Column::RepeatAfterSecs,
            Expr::value(settings.repeat_after_secs),
        )
        .col_expr(
            collab_escalation_setting::Column::QuietStartMinute,
            Expr::value(settings.quiet_start_minute),
        )
        .col_expr(
            collab_escalation_setting::Column::QuietEndMinute,
            Expr::value(settings.quiet_end_minute),
        )
        .col_expr(
            collab_escalation_setting::Column::Version,
            Expr::col(collab_escalation_setting::Column::Version).add(1),
        )
        .col_expr(
            collab_escalation_setting::Column::UpdatedByUserId,
            Expr::value(settings.updated_by_user_id),
        )
        .col_expr(collab_escalation_setting::Column::UpdatedAt, Expr::value(settings.updated_at))
        .filter(collab_escalation_setting::Column::RepoId.eq(settings.repo_id))
        .filter(collab_escalation_setting::Column::Version.eq(expected_version))
        .exec(db)
        .await
        .context("db: update escalation settings")?;
    let current = get(db, settings.repo_id).await?;
    Ok(match (res.rows_affected, current) {
        (0, current) => SaveOutcome::Conflict { current },
        (_, Some(saved)) => SaveOutcome::Saved(saved),
        (_, None) => SaveOutcome::Conflict { current: None },
    })
}

/// Tasks that declare themselves `blocked` or `waiting` and have an owner,
/// least recently updated first, optionally only those owned by one session.
pub async fn list_stalled_tasks<C: ConnectionTrait>(
    db: &C,
    repo_id: i64,
    owner_session_id: Option<&str>,
    limit: u64,
) -> Result<Vec<Task>> {
    let mut query = TaskEntity::find()
        .filter(collab_task::Column::RepoId.eq(repo_id))
        .filter(collab_task::Column::Status.is_in(["blocked", "waiting"]))
        .filter(collab_task::Column::OwnerSessionId.is_not_null());
    if let Some(owner) = owner_session_id {
        query = query.filter(collab_task::Column::OwnerSessionId.eq(owner));
    }
    query
        .order_by_asc(collab_task::Column::UpdatedAt)
        .order_by_asc(collab_task::Column::Id)
        .limit(limit)
        .all(db)
        .await
        .context("db: list stalled tasks")
}

/// Pending handoffs of a repository, oldest first, optionally only those
/// created by one session.
pub async fn list_pending_handoffs<C: ConnectionTrait>(
    db: &C,
    repo_id: i64,
    from_session_id: Option<&str>,
    limit: u64,
) -> Result<Vec<Handoff>> {
    let mut query = HandoffEntity::find()
        .filter(collab_handoff::Column::RepoId.eq(repo_id))
        .filter(collab_handoff::Column::Status.eq(handoff_status::PENDING));
    if let Some(from) = from_session_id {
        query = query.filter(collab_handoff::Column::FromSessionId.eq(from));
    }
    query
        .order_by_asc(collab_handoff::Column::CreatedAt)
        .order_by_asc(collab_handoff::Column::Id)
        .limit(limit)
        .all(db)
        .await
        .context("db: list pending handoffs for escalation")
}

/// Repositories that have escalation switched on.
pub async fn list_enabled_repo_ids<C: ConnectionTrait>(db: &C) -> Result<Vec<i64>> {
    Ok(SettingEntity::find()
        .filter(collab_escalation_setting::Column::Enabled.eq(true))
        .all(db)
        .await
        .context("db: list repos with escalation")?
        .into_iter()
        .map(|s| s.repo_id)
        .collect())
}
