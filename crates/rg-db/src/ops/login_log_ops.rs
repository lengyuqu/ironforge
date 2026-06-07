//! Login log operations.
use sea_orm::*;

use crate::entities::login_log;
pub use crate::entities::login_log::Entity;

/// Log a login attempt.
pub async fn log_attempt(
    db: &DatabaseConnection,
    user_id: Option<i64>,
    username: &str,
    auth_provider: &str,
    ip_address: Option<&str>,
    user_agent: Option<&str>,
    success: bool,
    failure_reason: Option<&str>,
) -> Result<login_log::Model, DbErr> {
    let now = chrono::Utc::now();
    let am = login_log::ActiveModel {
        id: NotSet,
        user_id: Set(user_id),
        username: Set(username.to_string()),
        auth_provider: Set(auth_provider.to_string()),
        ip_address: Set(ip_address.map(str::to_string)),
        user_agent: Set(user_agent.map(str::to_string)),
        success: Set(success),
        failure_reason: Set(failure_reason.map(str::to_string)),
        created_at: Set(now),
    };
    am.insert(db).await
}

/// Get recent login logs for a user.
pub async fn recent_for_user(
    db: &DatabaseConnection,
    user_id: i64,
    limit: u64,
) -> Result<Vec<login_log::Model>, DbErr> {
    Entity::find()
        .filter(login_log::Column::UserId.eq(user_id))
        .order_by_desc(login_log::Column::CreatedAt)
        .limit(limit)
        .all(db)
        .await
}

/// Count failed attempts for a username since a given time (brute-force detection).
pub async fn count_failed_since(
    db: &DatabaseConnection,
    username: &str,
    since: chrono::DateTime<chrono::Utc>,
) -> Result<u64, DbErr> {
    use sea_orm::QueryFilter;
    Entity::find()
        .filter(login_log::Column::Username.eq(username))
        .filter(login_log::Column::Success.eq(false))
        .filter(login_log::Column::CreatedAt.gte(since))
        .count(db)
        .await
}
