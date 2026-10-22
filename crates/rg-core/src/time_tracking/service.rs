//! Time tracking service — track time spent on issues.

use crate::error::{CoreError, CoreResult};
use chrono::Utc;
use rg_db::entities::time_entry::{ActiveModel, Model as TimeEntry};
use sea_orm::{ActiveValue::Set, DatabaseConnection};

/// Add a time entry to an issue.
pub async fn add_time(
    db: &DatabaseConnection,
    issue_id: i64,
    user_id: i64,
    duration_minutes: i64,
    description: Option<String>,
) -> CoreResult<TimeEntry> {
    if duration_minutes <= 0 {
        return Err(CoreError::invalid_input("duration must be positive"));
    }

    let now = Utc::now();
    let model = ActiveModel {
        issue_id: Set(issue_id),
        user_id: Set(user_id),
        duration_minutes: Set(duration_minutes),
        description: Set(description),
        created_at: Set(now),
        ..Default::default()
    };

    Ok(rg_db::ops::time_entry_ops::create(db, model).await?)
}

/// List time entries for an issue (paginated).
pub async fn list_time_entries(
    db: &DatabaseConnection,
    issue_id: i64,
    offset: u64,
    limit: u64,
) -> CoreResult<(Vec<TimeEntry>, i64)> {
    Ok(rg_db::ops::time_entry_ops::list_by_issue(db, issue_id, offset, limit).await?)
}

/// Get total tracked time for an issue (in minutes).
pub async fn total_time_minutes(db: &DatabaseConnection, issue_id: i64) -> CoreResult<i64> {
    Ok(rg_db::ops::time_entry_ops::total_minutes_by_issue(db, issue_id).await?)
}

/// Delete a time entry of the given issue.
///
/// An entry that does not exist, or that belongs to a different issue, is
/// reported as not found so one issue's URL can never delete another's data.
pub async fn delete_time_entry(
    db: &DatabaseConnection,
    issue_id: i64,
    id: i64,
) -> CoreResult<()> {
    match rg_db::ops::time_entry_ops::find_by_id(db, id).await? {
        Some(entry) if entry.issue_id == issue_id => {}
        _ => return Err(CoreError::not_found("time entry not found")),
    }
    Ok(rg_db::ops::time_entry_ops::delete_by_id(db, id).await?)
}

/// Format minutes into a human-readable string (e.g. "2h 15m").
pub fn format_duration(minutes: i64) -> String {
    if minutes < 60 {
        return format!("{minutes}m");
    }
    let hours = minutes / 60;
    let mins = minutes % 60;
    if mins == 0 {
        format!("{hours}h")
    } else {
        format!("{hours}h {mins}m")
    }
}
