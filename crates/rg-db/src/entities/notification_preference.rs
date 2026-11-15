//! Notification preference entity — maps to `notification_preferences`.
//!
//! One row says "this user does **not** want this event on this channel", or
//! explicitly wants it. Absence of a row means the channel is on; see the
//! migration and `notification_preference_ops::effective` for the resolution
//! order (repository override → global default → on).

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// The bell / notification list.
pub const CHANNEL_IN_APP: &str = "in_app";
/// Notification email through the configured SMTP relay.
pub const CHANNEL_EMAIL: &str = "email";

/// Both channels, in the order a settings page shows them.
pub const CHANNELS: &[&str] = &[CHANNEL_IN_APP, CHANNEL_EMAIL];

/// Whether `channel` is one of [`CHANNELS`].
pub fn is_known_channel(channel: &str) -> bool {
    CHANNELS.contains(&channel)
}

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "notification_preferences")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub user_id: i64,
    /// `None` = the global default; `Some` = the override for that repository.
    pub repo_id: Option<i64>,
    /// Same value space as `notifications.event_type`.
    pub event_type: String,
    /// [`CHANNEL_IN_APP`] or [`CHANNEL_EMAIL`].
    pub channel: String,
    pub enabled: bool,
    pub updated_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
