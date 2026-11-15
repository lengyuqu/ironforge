//! The single exit for notifications: one request in, both delivery channels
//! out, each gated by the recipient's preferences (#19 step 5).
//!
//! Before this module a notification was "the thing `notification::notify`
//! writes" and mail was a hand-rolled `look up the address → send` sequence at
//! each call site. `dispatch` is the one place that decides **whether** and
//! **how** a notification reaches a person:
//!
//! | channel | delivery | turned off by |
//! | --- | --- | --- |
//! | `in_app` | a `notifications` row (the bell) | `enabled = false` |
//! | `email` | [`email::send_html_notification`] through the caller's mailer | `enabled = false`, or no mailer |
//!
//! Preference resolution is the ops layer's job
//! ([`notification_preference_ops::effective`]): a repository override beats the
//! global default, and no row at all means **on**.
//!
//! What this is not: an event bus, an outbox, or a retry queue. It does not
//! write webhooks (that is `webhook::service::trigger_event`), it does not push
//! over WebSocket (the HTTP layer does that with the ids in
//! [`DispatchOutcome::created`]), and a failed email is not retried here — the
//! failure is logged and counted, because the in-app row is already durable.
//!
//! ## No mailer, no mail
//!
//! Callers that do not have SMTP configuration at hand pass `None`. The email
//! channel is then silently skipped (no error, not a failure) while the in-app
//! row is still written. This keeps the call-site conversion honest: a function
//! that never had access to a mailer does not grow a mailer parameter just to
//! satisfy this module.
//!
//! ## Audit note
//!
//! Email is **on by default** (only an explicit `enabled = false` turns it
//! off), so wiring a mailer into a previously quiet call site starts sending
//! mail. Do that deliberately, one call site at a time, and prefer the quiet
//! sites to keep passing `None`.

use crate::email;
use crate::error::{CoreError, CoreResult};
use rg_db::entities::notification_preference::{CHANNEL_EMAIL, CHANNEL_IN_APP};
use rg_db::ops::{notification_ops, notification_preference_ops, user_ops};
use sea_orm::DatabaseConnection;

/// The SMTP configuration a dispatch sends through.
///
/// A type alias rather than a wrapper: the mail transport already exists
/// ([`email::SmtpConfig`]) and every caller that can send mail holds one. `None`
/// means "this process has no SMTP configured".
pub type Mailer = email::SmtpConfig;

/// Event types this instance knows how to notify for, in the order a settings
/// page shows them. Used by `GET /user/notification-preferences` to expand the
/// defaults for events the user never touched.
///
/// This list is a **catalogue for display only**: dispatch accepts any
/// `event_type` string, so a new event type works before it is listed here.
pub const EVENT_TYPES: &[&str] = &[
    "push",
    "pull_request",
    "milestone",
    "issue_assigned",
    "issue_reaction",
    "comment_reaction",
    "collab_question",
    "collab_task_blocked",
    "ci_triggered",
];

/// One notification to deliver, independent of where it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotifyRequest {
    /// User ids to notify. One notification row (and possibly one email) per
    /// recipient.
    pub recipients: Vec<i64>,
    /// `notifications.event_type`, and the key preferences are stored under.
    pub event_type: String,
    /// The repository the notification is about, used for the repository
    /// override of a preference. `None` = only the global default applies.
    pub repo_id: Option<i64>,
    /// Notification title; also the email subject.
    pub title: String,
    /// Optional body; also the email body.
    pub body: Option<String>,
    /// Optional link rendered as a button in the email.
    pub action_url: Option<String>,
}

/// What one [`dispatch`] call did, per channel.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DispatchOutcome {
    /// `notifications` rows written.
    pub in_app: usize,
    /// Emails handed to the SMTP transport successfully.
    pub email: usize,
    /// Recipient/channel pairs skipped because the user turned them off.
    pub skipped_by_preference: usize,
    /// Deliveries that were attempted and failed (a row that could not be
    /// written, an email the server refused, a preference that could not be
    /// read). The caller may warn; it must not treat the request as failed.
    pub failed: usize,
    /// `(user_id, notification_id)` for every in-app row written, in recipient
    /// order. The WebSocket push needs the id of the row it announces; keeping
    /// it here is why `dispatch` can replace `notification::notify` at call
    /// sites that push afterwards.
    pub created: Vec<(i64, i64)>,
}

impl DispatchOutcome {
    /// Total deliveries attempted, for a one-line log.
    pub fn delivered(&self) -> usize {
        self.in_app + self.email
    }
}

/// Deliver one notification to every recipient, honouring their preferences.
///
/// Never returns an error: a notification is best effort once the underlying
/// business action has been committed, and the caller should not fail because a
/// bell row or an email did not go out. Everything that went wrong is in
/// [`DispatchOutcome::failed`] and in the logs.
pub async fn dispatch(
    db: &DatabaseConnection,
    mailer: Option<&Mailer>,
    req: NotifyRequest,
) -> DispatchOutcome {
    run(db, mailer, req, Channels::BOTH).await
}

/// Like [`dispatch`], but only the email channel: no `notifications` row is
/// written and the `in_app` preference is not consulted.
///
/// For converging a call site that already only sent mail (a CI-run notice, for
/// example): routing it through [`dispatch`] would add a bell row per event,
/// which is a behaviour change the caller never asked for. The email is gated by
/// the `email` preference exactly as in [`dispatch`].
pub async fn dispatch_email_only(
    db: &DatabaseConnection,
    mailer: Option<&Mailer>,
    req: NotifyRequest,
) -> DispatchOutcome {
    run(db, mailer, req, Channels::EMAIL_ONLY).await
}

#[derive(Debug, Clone, Copy)]
struct Channels {
    in_app: bool,
    email: bool,
}

impl Channels {
    const BOTH: Self = Self {
        in_app: true,
        email: true,
    };
    const EMAIL_ONLY: Self = Self {
        in_app: false,
        email: true,
    };
}

async fn run(
    db: &DatabaseConnection,
    mailer: Option<&Mailer>,
    req: NotifyRequest,
    channels: Channels,
) -> DispatchOutcome {
    let mut outcome = DispatchOutcome::default();
    for user_id in req.recipients.iter().copied() {
        deliver_one(db, mailer, &req, channels, user_id, &mut outcome).await;
    }
    if req.recipients.len() > 1 && (outcome.failed > 0 || outcome.skipped_by_preference > 0) {
        tracing::debug!(
            event_type = %req.event_type,
            recipients = req.recipients.len(),
            in_app = outcome.in_app,
            email = outcome.email,
            skipped = outcome.skipped_by_preference,
            failed = outcome.failed,
            "notification dispatch finished with skips or failures"
        );
    }
    outcome
}

async fn deliver_one(
    db: &DatabaseConnection,
    mailer: Option<&Mailer>,
    req: &NotifyRequest,
    channels: Channels,
    user_id: i64,
    outcome: &mut DispatchOutcome,
) {
    // A preference that cannot be read must not drop the in-app notification
    // (a lost bell entry is worse than one the user had muted) but must never
    // cause a mail: the email branch requires an affirmative answer.
    let prefs =
        match notification_preference_ops::effective(db, user_id, &req.event_type, req.repo_id)
            .await
        {
            Ok(prefs) => Some(prefs),
            Err(error) => {
                tracing::warn!(
                    user_id,
                    event_type = %req.event_type,
                    %error,
                    "notification preferences unreadable; delivering in-app only"
                );
                outcome.failed += 1;
                None
            }
        };

    // ── in_app ────────────────────────────────────────────────────────────
    if channels.in_app {
        let in_app_enabled = prefs.as_ref().map(|p| p.in_app.enabled).unwrap_or(true);
        if in_app_enabled {
            match notification_ops::create_notification(
                db,
                user_id,
                &req.event_type,
                &req.title,
                req.body.as_deref(),
                req.repo_id,
            )
            .await
            {
                Ok(row) => {
                    outcome.in_app += 1;
                    outcome.created.push((user_id, row.id));
                }
                Err(error) => {
                    tracing::warn!(
                        user_id,
                        event_type = %req.event_type,
                        %error,
                        "failed to store notification"
                    );
                    outcome.failed += 1;
                }
            }
        } else {
            outcome.skipped_by_preference += 1;
        }
    }

    // ── email ─────────────────────────────────────────────────────────────
    if !channels.email {
        return;
    }
    let Some(prefs) = prefs else {
        return;
    };
    if !prefs.email.enabled {
        outcome.skipped_by_preference += 1;
        return;
    }
    let Some(mailer) = mailer else {
        // No SMTP configured in this process: the channel is on but cannot
        // deliver. Not a failure — the same "silently skipped" contract the
        // email service has always had.
        return;
    };
    let user = match user_ops::find_by_id(db, user_id).await {
        Ok(Some(user)) => user,
        Ok(None) => {
            // The account vanished between the business action and delivery.
            // Nothing to send to, and nothing to retry.
            tracing::debug!(user_id, "no recipient account for notification email");
            return;
        }
        Err(error) => {
            tracing::warn!(user_id, %error, "failed to load notification email recipient");
            outcome.failed += 1;
            return;
        }
    };
    let to = user.email.trim();
    if to.is_empty() {
        tracing::debug!(
            user_id,
            "recipient has no email address; skipping notification email"
        );
        return;
    }
    let subject = if req.title.trim().is_empty() {
        req.event_type.clone()
    } else {
        req.title.clone()
    };
    let body = req.body.clone().unwrap_or_default();
    match email::send_html_notification(mailer, to, &subject, &body, req.action_url.as_deref())
        .await
    {
        Ok(()) => outcome.email += 1,
        Err(error) => {
            // The in-app row is already committed; a mail failure is logged and
            // counted, never propagated (task T12).
            tracing::warn!(
                user_id,
                event_type = %req.event_type,
                %error,
                "notification email failed; in-app notification kept"
            );
            outcome.failed += 1;
        }
    }
}

/// The channels a notification can travel on, for callers that build a
/// preference payload.
pub fn channels() -> &'static [&'static str] {
    rg_db::entities::notification_preference::CHANNELS
}

/// One line of the settings page: what applies at one scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreferenceRow {
    /// `None` = the global default; `Some` = a repository override.
    pub repo_id: Option<i64>,
    pub event_type: String,
    pub channel: String,
    pub enabled: bool,
    /// `false` = the default (on, because nothing was stored).
    pub explicit: bool,
}

/// Event types split by channel, for the settings page: the global default of
/// every catalogue event type (derived, `explicit = false` when untouched) plus
/// every explicit repository override the user has stored.
pub async fn effective_matrix(
    db: &DatabaseConnection,
    user_id: i64,
) -> CoreResult<Vec<PreferenceRow>> {
    let overrides = notification_preference_ops::list_for_user(db, user_id).await?;
    let mut out = Vec::with_capacity(EVENT_TYPES.len() * channels().len() + overrides.len());
    for event_type in EVENT_TYPES {
        let prefs = notification_preference_ops::effective(db, user_id, event_type, None).await?;
        for channel in channels() {
            let (enabled, explicit) = match *channel {
                CHANNEL_IN_APP => (prefs.in_app.enabled, prefs.in_app.explicit),
                CHANNEL_EMAIL => (prefs.email.enabled, prefs.email.explicit),
                _ => continue,
            };
            out.push(PreferenceRow {
                repo_id: None,
                event_type: (*event_type).to_string(),
                channel: (*channel).to_string(),
                enabled,
                explicit,
            });
        }
    }
    for row in overrides {
        if row.repo_id.is_some() {
            out.push(PreferenceRow {
                repo_id: row.repo_id,
                event_type: row.event_type,
                channel: row.channel,
                enabled: row.enabled,
                explicit: true,
            });
        }
    }
    Ok(out)
}

/// Convenience for callers that only need "was anything attempted".
pub fn failure_error(outcome: &DispatchOutcome) -> Option<CoreError> {
    (outcome.failed > 0).then(|| {
        CoreError::internal(format!(
            "notification dispatch recorded {} failure(s)",
            outcome.failed
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rg_db::entities::notification_preference::{CHANNEL_EMAIL, CHANNEL_IN_APP};
    use rg_db::ops::notification_preference_ops::upsert;
    use sea_orm::{ActiveModelTrait, DatabaseConnection, Set};

    async fn memory_db() -> DatabaseConnection {
        let db = rg_db::connect("sqlite::memory:").await.unwrap();
        rg_db::run_migrations(&db).await.unwrap();
        db
    }

    async fn user(db: &DatabaseConnection, name: &str) -> i64 {
        rg_db::ops::user_ops::create_user(
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
        use rg_db::entities::repository;
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

    fn request(recipients: Vec<i64>, repo_id: Option<i64>) -> NotifyRequest {
        NotifyRequest {
            recipients,
            event_type: "push".to_string(),
            repo_id,
            title: "New push to demo".to_string(),
            body: Some("someone pushed to main".to_string()),
            action_url: None,
        }
    }

    async fn rows(
        db: &DatabaseConnection,
        user_id: i64,
    ) -> Vec<rg_db::entities::notification::Model> {
        rg_db::ops::notification_ops::list_notifications(db, user_id, false)
            .await
            .unwrap()
    }

    /// A mailer that cannot work: `127.0.0.1:1` refuses immediately, so a test
    /// can tell "the mail path ran and failed" from "the mail path was skipped".
    fn unreachable_mailer() -> Mailer {
        Mailer::new("127.0.0.1", 1, "user", "pass", "ironforge@example.com")
    }

    #[tokio::test]
    async fn no_preferences_means_both_channels_are_on_and_the_row_is_written() {
        let db = memory_db().await;
        let user_id = user(&db, "dispatchdefault").await;
        let outcome = dispatch(&db, None, request(vec![user_id], None)).await;
        assert_eq!(outcome.in_app, 1);
        assert_eq!(outcome.skipped_by_preference, 0, "nothing was turned off");
        assert_eq!(outcome.failed, 0);
        assert_eq!(outcome.created.len(), 1);
        assert_eq!(rows(&db, user_id).await.len(), 1);
    }

    #[tokio::test]
    async fn a_muted_in_app_channel_writes_no_row() {
        let db = memory_db().await;
        let user_id = user(&db, "dispatchmute").await;
        upsert(&db, user_id, None, "push", CHANNEL_IN_APP, false)
            .await
            .unwrap();
        let outcome = dispatch(&db, None, request(vec![user_id], None)).await;
        assert_eq!(outcome.in_app, 0);
        assert_eq!(outcome.skipped_by_preference, 1);
        assert!(outcome.created.is_empty());
        assert!(rows(&db, user_id).await.is_empty());
    }

    #[tokio::test]
    async fn a_repository_override_beats_the_global_preference() {
        let db = memory_db().await;
        let user_id = user(&db, "dispatchscope").await;
        let repo_id = repo(&db, user_id, "dispatch-scoped").await;
        upsert(&db, user_id, None, "push", CHANNEL_IN_APP, false)
            .await
            .unwrap();
        upsert(&db, user_id, Some(repo_id), "push", CHANNEL_IN_APP, true)
            .await
            .unwrap();

        let outcome = dispatch(&db, None, request(vec![user_id], Some(repo_id))).await;
        assert_eq!(outcome.in_app, 1, "the repository override re-enables it");

        let other_repo = repo(&db, user_id, "dispatch-other").await;
        let outcome = dispatch(&db, None, request(vec![user_id], Some(other_repo))).await;
        assert_eq!(outcome.in_app, 0, "no override: the global mute applies");
        assert_eq!(outcome.skipped_by_preference, 1);
    }

    #[tokio::test]
    async fn a_muted_email_channel_still_writes_the_in_app_row() {
        let db = memory_db().await;
        let user_id = user(&db, "dispatchmuteemail").await;
        upsert(&db, user_id, None, "push", CHANNEL_EMAIL, false)
            .await
            .unwrap();
        let outcome = dispatch(
            &db,
            Some(&unreachable_mailer()),
            request(vec![user_id], None),
        )
        .await;
        assert_eq!(outcome.in_app, 1, "in-app is independent of the email mute");
        assert_eq!(outcome.email, 0);
        assert_eq!(
            outcome.failed, 0,
            "no send was attempted, so nothing could fail"
        );
        assert_eq!(outcome.skipped_by_preference, 1);
        assert_eq!(rows(&db, user_id).await.len(), 1);
    }

    #[tokio::test]
    async fn an_enabled_email_is_attempted() {
        let db = memory_db().await;
        let user_id = user(&db, "dispatchsend").await;
        let outcome = dispatch(
            &db,
            Some(&unreachable_mailer()),
            request(vec![user_id], None),
        )
        .await;
        assert_eq!(outcome.in_app, 1);
        assert_eq!(outcome.email, 0);
        assert_eq!(
            outcome.failed, 1,
            "the mail path ran and reported a transport failure"
        );
    }

    #[tokio::test]
    async fn a_failed_email_does_not_lose_the_in_app_row() {
        let db = memory_db().await;
        let user_id = user(&db, "dispatchkeepsrow").await;
        let outcome = dispatch(
            &db,
            Some(&unreachable_mailer()),
            request(vec![user_id], None),
        )
        .await;
        assert_eq!(outcome.failed, 1);
        assert_eq!(
            rows(&db, user_id).await.len(),
            1,
            "the bell row must survive a mail failure"
        );
    }

    #[tokio::test]
    async fn a_missing_mailer_is_silent() {
        let db = memory_db().await;
        let user_id = user(&db, "dispatchnomailer").await;
        let outcome = dispatch(&db, None, request(vec![user_id], None)).await;
        assert_eq!(outcome.email, 0);
        assert_eq!(outcome.failed, 0, "no mailer is not a failure");
        assert_eq!(outcome.skipped_by_preference, 0, "the channel was on");
        assert_eq!(outcome.in_app, 1);
    }

    #[tokio::test]
    async fn every_recipient_gets_its_own_row_and_id() {
        let db = memory_db().await;
        let first = user(&db, "dispatchone").await;
        let second = user(&db, "dispatchtwo").await;
        let outcome = dispatch(&db, None, request(vec![first, second], None)).await;
        assert_eq!(outcome.in_app, 2);
        assert_eq!(outcome.created.len(), 2);
        assert_eq!(outcome.created[0].0, first);
        assert_eq!(outcome.created[1].0, second);
        assert_eq!(rows(&db, first).await.len(), 1);
        assert_eq!(rows(&db, second).await.len(), 1);
    }

    /// The email-only entry point must not add a bell row: it converges a call
    /// site that only ever sent mail.
    #[tokio::test]
    async fn email_only_writes_no_row_and_still_obeys_the_email_preference() {
        let db = memory_db().await;
        let user_id = user(&db, "dispatchmailonly").await;
        let outcome = dispatch_email_only(
            &db,
            Some(&unreachable_mailer()),
            request(vec![user_id], None),
        )
        .await;
        assert_eq!(outcome.in_app, 0);
        assert!(outcome.created.is_empty());
        assert!(
            rows(&db, user_id).await.is_empty(),
            "email-only must not create a notification row"
        );
        assert_eq!(outcome.failed, 1, "the mail path was attempted");

        upsert(&db, user_id, None, "push", CHANNEL_EMAIL, false)
            .await
            .unwrap();
        let outcome = dispatch_email_only(
            &db,
            Some(&unreachable_mailer()),
            request(vec![user_id], None),
        )
        .await;
        assert_eq!(
            outcome.failed, 0,
            "the mute stops the send before it starts"
        );
        assert_eq!(outcome.skipped_by_preference, 1);
        assert_eq!(outcome.email, 0);
    }

    #[tokio::test]
    async fn email_only_without_a_mailer_is_silent() {
        let db = memory_db().await;
        let user_id = user(&db, "dispatchmailonlynone").await;
        let outcome = dispatch_email_only(&db, None, request(vec![user_id], None)).await;
        assert_eq!(outcome.in_app, 0);
        assert_eq!(outcome.email, 0);
        assert_eq!(outcome.failed, 0);
    }

    #[tokio::test]
    async fn the_matrix_expands_the_defaults_and_lists_repository_overrides() {
        let db = memory_db().await;
        let user_id = user(&db, "dispatchmatrix").await;
        let repo_id = repo(&db, user_id, "matrix-repo").await;
        upsert(&db, user_id, None, "push", CHANNEL_EMAIL, false)
            .await
            .unwrap();
        upsert(&db, user_id, Some(repo_id), "push", CHANNEL_EMAIL, true)
            .await
            .unwrap();
        let matrix = effective_matrix(&db, user_id).await.unwrap();
        let global = matrix
            .iter()
            .find(|row| {
                row.repo_id.is_none() && row.event_type == "push" && row.channel == CHANNEL_EMAIL
            })
            .expect("the global push/email entry");
        assert!(!global.enabled, "the global mute");
        assert!(global.explicit, "and it is explicit");
        let scoped = matrix
            .iter()
            .find(|row| row.repo_id == Some(repo_id))
            .expect("the repository override");
        assert_eq!(scoped.event_type, "push");
        assert_eq!(scoped.channel, CHANNEL_EMAIL);
        assert!(scoped.enabled);
        let untouched = matrix
            .iter()
            .find(|row| {
                row.repo_id.is_none()
                    && row.event_type == "milestone"
                    && row.channel == CHANNEL_IN_APP
            })
            .expect("a default entry");
        assert!(untouched.enabled);
        assert!(!untouched.explicit, "a default is not explicit");
    }
}
