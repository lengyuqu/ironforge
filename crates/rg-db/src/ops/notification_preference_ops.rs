//! Database operations for `notification_preferences`.
//!
//! The reader ([`effective`]) is the authority on what "not set" means: a
//! missing row is **on**, a repository row beats a global row, a global row
//! beats the default. The writer ([`upsert`]) finds before it writes so that a
//! second write for the same scope updates the existing row instead of adding a
//! duplicate — necessary because SQL treats two `NULL` `repo_id`s as distinct,
//! so the unique key cannot deduplicate global rows (see the migration).

use anyhow::{Context, Result};
use sea_orm::*;

use crate::entities::notification_preference::{self, CHANNELS, CHANNEL_EMAIL, CHANNEL_IN_APP};

/// What applies to one channel: the answer plus where it came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChannelPreference {
    /// Whether the channel is on.
    pub enabled: bool,
    /// Whether an explicit row produced this answer. `false` = the default.
    pub explicit: bool,
}

/// The effective preference of one event type, per channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EffectivePreference {
    pub in_app: ChannelPreference,
    pub email: ChannelPreference,
}

impl Default for EffectivePreference {
    /// Nothing set: both channels on, neither explicit.
    fn default() -> Self {
        Self {
            in_app: ChannelPreference {
                enabled: true,
                explicit: false,
            },
            email: ChannelPreference {
                enabled: true,
                explicit: false,
            },
        }
    }
}

impl EffectivePreference {
    /// Whether `channel` is on. An unknown channel is treated as off, so a
    /// caller that passes a typo cannot accidentally deliver.
    pub fn enabled(&self, channel: &str) -> bool {
        match channel {
            CHANNEL_IN_APP => self.in_app.enabled,
            CHANNEL_EMAIL => self.email.enabled,
            _ => false,
        }
    }
}

/// The scope filter shared by [`find`] and [`effective`].
fn scope_query(
    user_id: i64,
    event_type: &str,
    repo_id: Option<i64>,
    include_global: bool,
) -> Select<notification_preference::Entity> {
    let mut query = notification_preference::Entity::find()
        .filter(notification_preference::Column::UserId.eq(user_id))
        .filter(notification_preference::Column::EventType.eq(event_type));
    query = match repo_id {
        Some(id) if include_global => query.filter(
            notification_preference::Column::RepoId
                .is_null()
                .or(notification_preference::Column::RepoId.eq(id)),
        ),
        Some(id) => query.filter(notification_preference::Column::RepoId.eq(id)),
        None => query.filter(notification_preference::Column::RepoId.is_null()),
    };
    query
}

/// All preference rows of a user, newest first.
pub async fn list_for_user(
    db: &DatabaseConnection,
    user_id: i64,
) -> Result<Vec<notification_preference::Model>> {
    notification_preference::Entity::find()
        .filter(notification_preference::Column::UserId.eq(user_id))
        .order_by_desc(notification_preference::Column::UpdatedAt)
        .order_by_desc(notification_preference::Column::Id)
        .all(db)
        .await
        .context("db: list notification preferences")
}

/// Resolve the effective preference for one event type.
///
/// Order: repository override → global default → on. `repo_id = None` reads the
/// global default only. A failure is returned, not swallowed: the caller
/// decides whether "could not read" means "deliver" or "hold".
pub async fn effective(
    db: &DatabaseConnection,
    user_id: i64,
    event_type: &str,
    repo_id: Option<i64>,
) -> Result<EffectivePreference> {
    let rows = scope_query(user_id, event_type, repo_id, true)
        .order_by_desc(notification_preference::Column::UpdatedAt)
        .order_by_desc(notification_preference::Column::Id)
        .all(db)
        .await
        .context("db: read notification preferences")?;

    let mut out = EffectivePreference::default();
    // Newest first, so the first hit per (channel, scope) wins and a duplicate
    // global row cannot make the answer depend on the backend's row order.
    for channel in CHANNELS {
        let scoped = repo_id.map(|id| {
            rows.iter()
                .find(|row| row.channel == *channel && row.repo_id == Some(id))
        });
        let global = rows
            .iter()
            .find(|row| row.channel == *channel && row.repo_id.is_none());
        if let Some(row) = scoped.flatten().or(global) {
            let answer = ChannelPreference {
                enabled: row.enabled,
                explicit: true,
            };
            match *channel {
                CHANNEL_IN_APP => out.in_app = answer,
                CHANNEL_EMAIL => out.email = answer,
                _ => {}
            }
        }
    }
    Ok(out)
}

/// Find one row for an exact scope.
pub async fn find(
    db: &DatabaseConnection,
    user_id: i64,
    repo_id: Option<i64>,
    event_type: &str,
    channel: &str,
) -> Result<Option<notification_preference::Model>> {
    scope_query(user_id, event_type, repo_id, false)
        .filter(notification_preference::Column::Channel.eq(channel))
        .order_by_desc(notification_preference::Column::UpdatedAt)
        .order_by_desc(notification_preference::Column::Id)
        .one(db)
        .await
        .context("db: find notification preference")
}

/// Write (or overwrite) the preference for an exact scope.
///
/// Find-then-write rather than insert-on-conflict: the conflict target would
/// not match a global row on any supported backend, because `NULL <> NULL`.
pub async fn upsert(
    db: &DatabaseConnection,
    user_id: i64,
    repo_id: Option<i64>,
    event_type: &str,
    channel: &str,
    enabled: bool,
) -> Result<notification_preference::Model> {
    let now = chrono::Utc::now();
    if let Some(existing) = find(db, user_id, repo_id, event_type, channel).await? {
        let mut active: notification_preference::ActiveModel = existing.into();
        active.enabled = Set(enabled);
        active.updated_at = Set(now);
        return active
            .update(db)
            .await
            .context("db: update notification preference");
    }
    notification_preference::ActiveModel {
        user_id: Set(user_id),
        repo_id: Set(repo_id),
        event_type: Set(event_type.to_string()),
        channel: Set(channel.to_string()),
        enabled: Set(enabled),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(db)
    .await
    .context("db: create notification preference")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::notification_preference::CHANNEL_IN_APP;

    async fn memory_db() -> DatabaseConnection {
        let db = crate::connect("sqlite::memory:").await.unwrap();
        crate::run_migrations(&db).await.unwrap();
        db
    }

    /// A real user row: the preference table has a cascading foreign key.
    async fn user(db: &DatabaseConnection, name: &str) -> i64 {
        crate::ops::user_ops::create_user(
            db,
            name,
            &format!("{name}@example.com"),
            "hash",
            "Test User",
        )
        .await
        .unwrap()
        .id
    }

    async fn repo(db: &DatabaseConnection, owner_id: i64, name: &str) -> i64 {
        use crate::entities::repository;
        let now = chrono::Utc::now();
        repository::ActiveModel {
            owner_id: Set(owner_id),
            name: Set(name.to_string()),
            description: Set(None),
            is_private: Set(false),
            default_branch: Set("main".into()),
            fork_id: Set(None),
            stars_count: Set(0),
            forks_count: Set(0),
            org_id: Set(None),
            created_at: Set(now),
            updated_at: Set(now),
            deleted_at: Set(None),
            origin_repo_id: Set(None),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
        .id
    }

    #[tokio::test]
    async fn nothing_set_means_both_channels_are_on_and_not_explicit() {
        let db = memory_db().await;
        let user_id = user(&db, "prefdefault").await;
        let pref = effective(&db, user_id, "push", None).await.unwrap();
        assert!(pref.in_app.enabled);
        assert!(pref.email.enabled);
        assert!(!pref.in_app.explicit);
        assert!(!pref.email.explicit);
    }

    #[tokio::test]
    async fn a_global_opt_out_is_used_when_no_repository_override_exists() {
        let db = memory_db().await;
        let user_id = user(&db, "prefglobal").await;
        upsert(&db, user_id, None, "push", CHANNEL_EMAIL, false)
            .await
            .unwrap();
        let pref = effective(&db, user_id, "push", None).await.unwrap();
        assert!(!pref.email.enabled);
        assert!(pref.email.explicit);
        assert!(pref.in_app.enabled, "the other channel is untouched");
    }

    #[tokio::test]
    async fn a_repository_override_beats_the_global_default() {
        let db = memory_db().await;
        let user_id = user(&db, "prefscoped").await;
        let repo_id = repo(&db, user_id, "scoped-repo").await;
        // Globally off, but this one repository is explicitly on again.
        upsert(&db, user_id, None, "push", CHANNEL_IN_APP, false)
            .await
            .unwrap();
        upsert(&db, user_id, Some(repo_id), "push", CHANNEL_IN_APP, true)
            .await
            .unwrap();

        assert!(
            effective(&db, user_id, "push", Some(repo_id))
                .await
                .unwrap()
                .in_app
                .enabled,
            "the repository override wins"
        );
        assert!(
            !effective(&db, user_id, "push", None)
                .await
                .unwrap()
                .in_app
                .enabled,
            "the global default is unchanged"
        );
        let other = repo(&db, user_id, "other-repo").await;
        assert!(
            !effective(&db, user_id, "push", Some(other))
                .await
                .unwrap()
                .in_app
                .enabled,
            "a repository without an override falls back to the global default"
        );
    }

    #[tokio::test]
    async fn preferences_are_per_event_type() {
        let db = memory_db().await;
        let user_id = user(&db, "prefevent").await;
        upsert(&db, user_id, None, "push", CHANNEL_EMAIL, false)
            .await
            .unwrap();
        assert!(
            !effective(&db, user_id, "push", None)
                .await
                .unwrap()
                .email
                .enabled
        );
        assert!(
            effective(&db, user_id, "milestone", None)
                .await
                .unwrap()
                .email
                .enabled,
            "another event type keeps the default"
        );
    }

    #[tokio::test]
    async fn upsert_updates_instead_of_duplicating() {
        let db = memory_db().await;
        let user_id = user(&db, "prefupsert").await;
        upsert(&db, user_id, None, "push", CHANNEL_EMAIL, false)
            .await
            .unwrap();
        upsert(&db, user_id, None, "push", CHANNEL_EMAIL, true)
            .await
            .unwrap();
        let rows = list_for_user(&db, user_id).await.unwrap();
        assert_eq!(rows.len(), 1, "one row per scope: {rows:?}");
        assert!(rows[0].enabled);
    }

    #[tokio::test]
    async fn a_global_and_a_repository_row_coexist() {
        let db = memory_db().await;
        let user_id = user(&db, "prefboth").await;
        let repo_id = repo(&db, user_id, "both-repo").await;
        upsert(&db, user_id, None, "push", CHANNEL_IN_APP, false)
            .await
            .unwrap();
        upsert(&db, user_id, Some(repo_id), "push", CHANNEL_IN_APP, true)
            .await
            .unwrap();
        assert_eq!(list_for_user(&db, user_id).await.unwrap().len(), 2);
    }
}
