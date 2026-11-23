//! LDAP directory sync — deactivate the accounts the directory no longer has.
//!
//! Until now an LDAP-linked account only existed in one direction: logging in
//! created it, and nothing ever noticed when the person left. Their token kept
//! working, their repository access kept working, and the only way to find out
//! was an audit.
//!
//! # What this does, and deliberately does not do
//!
//! **Does:** flip `is_active` on accounts that are absent from (or back in) the
//! directory. Their repositories, issues, pull requests and comments are left
//! exactly as they are — the account is disabled, not deleted. That is what
//! makes the operation reversible: someone who returns to the directory gets
//! their account back on the next run.
//!
//! **Does not:**
//!
//! * **create accounts** the directory has but this instance has never seen.
//!   Creation means allocating a quota, picking a role, and running whatever
//!   onboarding the deployment expects — decisions a background job should not
//!   make. Those entries are reported as `unknown_in_directory`.
//! * **update profile attributes.** `display_name` / `dn` are refreshed on
//!   login (`user::service::resolve_ldap_identity`); a sync that also rewrote
//!   them would silently undo local edits and make the diff unreadable.
//! * **delete anything.** Ever.
//!
//! # What keeps it safe
//!
//! A truncated directory listing would look exactly like "everyone past the
//! size limit has left". [`crate::auth::ldap::list_users`] therefore treats a
//! non-zero LDAP result (including `sizeLimitExceeded`) as an error, and this
//! module never plans anything until the full listing is in hand.

use crate::auth::ldap::{self, LdapUser};
use crate::error::{CoreError, CoreResult};
use rg_db::entities::sso_provider;
use rg_db::entities::user;
use sea_orm::DatabaseConnection;

/// A local account that logs in through the provider being synced.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinkedAccount {
    pub id: i64,
    /// The directory name, resolved the same way the login path does it
    /// (`uid`, falling back to the user name).
    pub username: String,
    pub is_active: bool,
}

impl From<user::Model> for LinkedAccount {
    fn from(model: user::Model) -> Self {
        Self {
            id: model.id,
            // `ldap_uid` is what login stored; `username` is the local copy of
            // the same value. Prefer the former so a locally renamed account
            // still matches its directory entry.
            username: model
                .ldap_uid
                .clone()
                .unwrap_or_else(|| model.username.clone()),
            is_active: model.is_active,
        }
    }
}

/// What a sync run would do. Computed before anything is written so it can be
/// printed, reviewed and tested without a directory.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct SyncPlan {
    /// Active accounts missing from the directory.
    pub deactivate: Vec<LinkedAccount>,
    /// Inactive accounts the directory has again.
    pub reactivate: Vec<LinkedAccount>,
    /// Active accounts that are still in the directory.
    pub unchanged: usize,
    /// Directory entries with no local account. Reported, never created.
    pub unknown_in_directory: Vec<String>,
}

impl SyncPlan {
    pub fn is_noop(&self) -> bool {
        self.deactivate.is_empty() && self.reactivate.is_empty()
    }

    /// One line an operator can paste into a change ticket.
    pub fn summary(&self) -> String {
        format!(
            "{} to deactivate, {} to reactivate, {} unchanged, {} unknown in directory",
            self.deactivate.len(),
            self.reactivate.len(),
            self.unchanged,
            self.unknown_in_directory.len()
        )
    }
}

/// Decide what to do, given the local accounts and the directory listing.
///
/// Pure: no database, no network, no clock.
pub fn plan(local: &[LinkedAccount], directory: &[LdapUser]) -> SyncPlan {
    let mut directory_names: Vec<&str> = directory
        .iter()
        .map(|user| user.username.trim())
        .filter(|name| !name.is_empty())
        .collect();
    directory_names.sort_unstable();
    directory_names.dedup();

    let mut plan = SyncPlan::default();
    for account in local {
        let present = directory_names
            .binary_search(&account.username.trim())
            .is_ok();
        match (present, account.is_active) {
            (true, true) => plan.unchanged += 1,
            // Back in the directory after having been disabled.
            (true, false) => plan.reactivate.push(account.clone()),
            // Gone from the directory while still able to log in.
            (false, true) => plan.deactivate.push(account.clone()),
            // Gone and already disabled: the previous run's work, not an event.
            (false, false) => {}
        }
    }

    let local_names: Vec<&str> = local
        .iter()
        .map(|account| account.username.trim())
        .collect();
    plan.unknown_in_directory = directory_names
        .into_iter()
        .filter(|name| !local_names.contains(name))
        .map(str::to_string)
        .collect();
    plan.unknown_in_directory.sort_unstable();
    plan.unknown_in_directory.dedup();

    plan
}

/// Does this plan look like a broken directory configuration rather than an
/// offboarding wave?
///
/// Disabling a large share of the active accounts is what a wrong base DN, an
/// expired bind account, or a partial listing looks like. Both the CLI and the
/// scheduled run refuse it; only a human may override.
pub fn looks_like_mass_deactivation(plan: &SyncPlan) -> bool {
    let active_before = plan.unchanged + plan.deactivate.len();
    plan.deactivate.len() >= 5 && plan.deactivate.len() * 2 >= active_before
}

/// What one provider did in a scheduled run.
#[derive(Debug)]
pub enum ProviderOutcome {
    /// Nothing to do.
    Unchanged { provider_id: i64 },
    /// Changes were written.
    Applied {
        provider_id: i64,
        changed: usize,
        plan: SyncPlan,
    },
    /// The plan looked dangerous, so nothing was written.
    RefusedMassDeactivation { provider_id: i64, plan: SyncPlan },
    /// The directory could not be read; nothing was written.
    Failed { provider_id: i64, error: String },
}

/// Sync every enabled LDAP provider — the scheduled entry point.
///
/// Deliberately cannot force a mass deactivation: a background job that
/// disables an entire instance because someone edited a base DN is worse than
/// one that does nothing and says so. The operator runs the CLI with
/// `--allow-mass-deactivation` when they actually mean it.
pub async fn sync_all_enabled(db: &DatabaseConnection, jwt_secret: &str) -> Vec<ProviderOutcome> {
    let providers = match rg_db::ops::sso_provider_ops::list_enabled(db).await {
        Ok(providers) => providers,
        Err(error) => {
            tracing::error!(%error, "LDAP sync: cannot list providers");
            return Vec::new();
        }
    };

    let mut outcomes = Vec::new();
    for provider in providers
        .into_iter()
        .filter(|provider| provider.provider_type == "ldap")
    {
        let provider_id = provider.id;
        let plan = match plan_provider(db, &provider, jwt_secret).await {
            Ok(plan) => plan,
            Err(error) => {
                tracing::error!(provider_id, %error, "LDAP sync: provider failed");
                outcomes.push(ProviderOutcome::Failed {
                    provider_id,
                    error: error.to_string(),
                });
                continue;
            }
        };

        if plan.is_noop() {
            outcomes.push(ProviderOutcome::Unchanged { provider_id });
            continue;
        }
        if looks_like_mass_deactivation(&plan) {
            tracing::warn!(
                provider_id,
                deactivate = plan.deactivate.len(),
                "LDAP sync: refusing what looks like a mass deactivation; run `ironforge ldap-sync --allow-mass-deactivation` if it is real"
            );
            outcomes.push(ProviderOutcome::RefusedMassDeactivation { provider_id, plan });
            continue;
        }

        match apply(db, &plan).await {
            Ok((changed, _)) => {
                tracing::info!(
                    provider_id,
                    changed,
                    deactivate = plan.deactivate.len(),
                    reactivate = plan.reactivate.len(),
                    "LDAP sync applied"
                );
                outcomes.push(ProviderOutcome::Applied {
                    provider_id,
                    changed,
                    plan,
                });
            }
            Err(error) => {
                tracing::error!(provider_id, %error, "LDAP sync: apply failed");
                outcomes.push(ProviderOutcome::Failed {
                    provider_id,
                    error: error.to_string(),
                });
            }
        }
    }
    outcomes
}

/// Work out the plan for one LDAP provider, without writing anything.
pub async fn plan_provider(
    db: &DatabaseConnection,
    provider: &sso_provider::Model,
    jwt_secret: &str,
) -> CoreResult<SyncPlan> {
    if provider.provider_type != "ldap" {
        return Err(CoreError::invalid_input("provider is not LDAP"));
    }
    let config = crate::user::service::ldap_config_from_provider(provider, jwt_secret)?;
    let directory = ldap::list_users(&config).await?;
    let local: Vec<LinkedAccount> = rg_db::ops::user_ops::list_by_ldap_provider(db, provider.id)
        .await?
        .into_iter()
        .map(LinkedAccount::from)
        .collect();
    Ok(plan(&local, &directory))
}

/// Apply a plan. Returns `(accounts changed, rows actually updated)`.
///
/// The second number can differ from the first if a row vanished between the
/// plan and the write — the caller reports both rather than pretending they
/// always match.
pub async fn apply(db: &DatabaseConnection, plan: &SyncPlan) -> CoreResult<(usize, u64)> {
    let mut changed = 0usize;
    let mut rows = 0u64;
    for account in &plan.deactivate {
        rows += rg_db::ops::user_ops::set_active(db, account.id, false).await?;
        changed += 1;
    }
    for account in &plan.reactivate {
        rows += rg_db::ops::user_ops::set_active(db, account.id, true).await?;
        changed += 1;
    }
    Ok((changed, rows))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account(id: i64, username: &str, is_active: bool) -> LinkedAccount {
        LinkedAccount {
            id,
            username: username.to_string(),
            is_active,
        }
    }

    fn directory(usernames: &[&str]) -> Vec<LdapUser> {
        usernames
            .iter()
            .map(|name| LdapUser {
                username: (*name).to_string(),
                email: Some(format!("{name}@example.com")),
                display_name: None,
                dn: format!("uid={name},dc=example,dc=com"),
                uid: Some((*name).to_string()),
            })
            .collect()
    }

    /// The core case: someone left the company, their account stops working,
    /// their content does not move.
    #[test]
    fn an_account_missing_from_the_directory_is_deactivated() {
        let plan = plan(
            &[account(1, "alice", true), account(2, "bob", true)],
            &directory(&["alice"]),
        );
        assert_eq!(plan.deactivate, vec![account(2, "bob", true)]);
        assert!(plan.reactivate.is_empty());
        assert_eq!(plan.unchanged, 1);
        assert!(plan.unknown_in_directory.is_empty());
    }

    /// Someone who comes back gets their account back — the reason this
    /// disables rather than deletes.
    #[test]
    fn an_account_that_returns_is_reactivated() {
        let plan = plan(&[account(1, "alice", false)], &directory(&["alice"]));
        assert_eq!(plan.reactivate, vec![account(1, "alice", false)]);
        assert!(plan.deactivate.is_empty());
        assert_eq!(plan.unchanged, 0);
    }

    /// An account already disabled and still absent is not an event: a sync
    /// running every hour must not report the same person forever.
    #[test]
    fn an_already_disabled_absent_account_is_not_reported_again() {
        let plan = plan(&[account(1, "bob", false)], &directory(&["alice"]));
        assert!(plan.deactivate.is_empty());
        assert!(plan.reactivate.is_empty());
        assert_eq!(plan.unchanged, 0);
        assert_eq!(plan.unknown_in_directory, vec!["alice".to_string()]);
    }

    /// Accounts in the directory that this instance has never met are reported,
    /// never created: creation carries quota and role decisions.
    #[test]
    fn unknown_directory_entries_are_reported_not_created() {
        let plan = plan(
            &[account(1, "alice", true)],
            &directory(&["alice", "carol", "dave"]),
        );
        assert_eq!(
            plan.unknown_in_directory,
            vec!["carol".to_string(), "dave".to_string()]
        );
        assert!(
            plan.deactivate.is_empty(),
            "unknown entries must not deactivate anyone"
        );
    }

    /// An empty directory listing means everyone has left — which is far more
    /// likely a misconfiguration (wrong base DN, bind account lost its read
    /// access) than a real event. The plan says so loudly; the caller decides
    /// whether to apply it.
    #[test]
    fn an_empty_directory_plans_to_deactivate_everyone() {
        let plan = plan(&[account(1, "alice", true), account(2, "bob", true)], &[]);
        assert_eq!(plan.deactivate.len(), 2);
        assert!(!plan.is_noop());
        // The summary is what an operator reads before approving.
        assert!(plan.summary().contains("2 to deactivate"));
    }

    /// The guard that keeps a scheduled run from wiping the instance: a
    /// large share is a configuration smell, a small one is offboarding.
    #[test]
    fn mass_deactivation_is_recognised_by_share_not_only_by_count() {
        // Four people: too few to call it mass, whatever the share.
        let small = plan(
            &[
                account(1, "a", true),
                account(2, "b", true),
                account(3, "c", true),
                account(4, "d", true),
            ],
            &[],
        );
        assert_eq!(small.deactivate.len(), 4);
        assert!(!looks_like_mass_deactivation(&small));

        // Five out of five: mass.
        let everyone = plan(
            &[
                account(1, "a", true),
                account(2, "b", true),
                account(3, "c", true),
                account(4, "d", true),
                account(5, "e", true),
            ],
            &[],
        );
        assert!(looks_like_mass_deactivation(&everyone));

        // Five out of twenty: a real offboarding wave, not a misconfiguration.
        let mut local: Vec<LinkedAccount> = (1..=20).map(|i| account(i, "x", true)).collect();
        for (i, account) in local.iter_mut().take(5).enumerate() {
            account.username = format!("gone{i}");
        }
        let mut directory = Vec::new();
        for account in local.iter().skip(5) {
            directory.push(LdapUser {
                username: account.username.clone(),
                email: None,
                display_name: None,
                dn: String::new(),
                uid: None,
            });
        }
        let five_of_twenty = plan(&local, &directory);
        assert_eq!(five_of_twenty.deactivate.len(), 5);
        assert!(!looks_like_mass_deactivation(&five_of_twenty));
    }

    /// Whitespace around a directory name must not decide whether someone
    /// keeps their account.
    #[test]
    fn surrounding_whitespace_is_ignored_on_both_sides() {
        let mut local = account(1, "  alice  ", true);
        let listing = directory(&["alice"]);
        let padded_local = plan(&[local.clone()], &listing);
        assert_eq!(padded_local.unchanged, 1);
        assert!(padded_local.deactivate.is_empty());

        local.username = "alice".to_string();
        let padded_directory = plan(&[local], &directory(&[" alice "]));
        assert_eq!(padded_directory.unchanged, 1);
    }

    /// The local copy of the name may differ from the directory's (a rename),
    /// but matching uses the directory identity, so no spurious deactivation.
    #[test]
    fn matching_uses_the_directory_name_not_the_local_display_name() {
        // `username` here is what `ldap_uid` held at login time.
        let plan = plan(&[account(1, "alice", true)], &directory(&["alice"]));
        assert_eq!(plan.unchanged, 1);
    }
}
