//! LDAP group → team membership planning.
//!
//! The pure half of AUTH-403: given the directory's groups and this instance's
//! teams, decide which memberships to insert. No database, no network, no
//! clock — the same shape as [`super::ldap_sync::plan`], and for the same
//! reason: the interesting cases (a group with no team, a member with no
//! account, someone already in the team) are only cheap to test if deciding is
//! separate from writing.
//!
//! # The three rules this implements
//!
//! See `docs/design/authorization.md`. In short:
//!
//! 1. the caller has already resolved which org to look at (the provider names
//!    exactly one), so this module only sees that org's teams;
//! 2. a group's `cn` must equal a team's name **exactly** — no case folding, no
//!    fuzzy match. A group with no team is reported, never created: creating a
//!    team means choosing a permission, which is an authorization decision;
//! 3. **additions only**. Members the directory does not list are reported as
//!    `extra_members` and left alone — they may have come from a manual grant,
//!    another provider, or history, and removal is not reversible.

use rg_db::entities::sso_provider;
use std::collections::HashMap;

/// A group as the directory reports it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirectoryGroup {
    /// `cn`. Must equal a team name exactly to be synced.
    pub name: String,
    /// Member DNs, as the directory spells them.
    pub member_dns: Vec<String>,
}

impl From<crate::auth::ldap::LdapGroup> for DirectoryGroup {
    fn from(group: crate::auth::ldap::LdapGroup) -> Self {
        Self {
            name: group.name,
            member_dns: group.member_dns,
        }
    }
}

/// A team with its current members.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TeamSnapshot {
    pub id: i64,
    pub name: String,
    /// Everyone already in the team: `(user_id, username)`.
    pub members: Vec<(i64, String)>,
}

/// One membership to insert.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MembershipAdd {
    pub team_id: i64,
    pub team_name: String,
    pub user_id: i64,
    pub username: String,
}

/// What a group sync would do.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct GroupSyncPlan {
    /// Memberships to insert.
    pub add: Vec<MembershipAdd>,
    /// Groups whose name matches no team. Reported, never created.
    pub unmatched_groups: Vec<String>,
    /// Member DNs with no local account.
    pub unmatched_members: Vec<String>,
    /// `(team_name, username)` pairs the group does not list. Reported only —
    /// this is what a future `--prune` would be validated against.
    pub extra_members: Vec<(String, String)>,
}

impl GroupSyncPlan {
    pub fn is_noop(&self) -> bool {
        self.add.is_empty()
    }

    pub fn summary(&self) -> String {
        format!(
            "{} membership(s) to add, {} group(s) with no team, {} unknown member(s), {} extra member(s) reported",
            self.add.len(),
            self.unmatched_groups.len(),
            self.unmatched_members.len(),
            self.extra_members.len()
        )
    }
}

/// An index of local accounts by their directory DN.
///
/// DNs are compared case-insensitively on the whole string as a fallback: LDAP
/// itself treats attribute *types* case-insensitively but values as the schema
/// says, and real directories are not consistent about it. An exact hit always
/// wins, so a directory that is consistent pays nothing for the fallback.
#[derive(Debug, Default)]
pub struct DnIndex {
    exact: HashMap<String, (i64, String)>,
    folded: HashMap<String, (i64, String)>,
}

impl DnIndex {
    pub fn new(accounts: impl IntoIterator<Item = (String, i64, String)>) -> Self {
        let mut index = Self::default();
        for (dn, user_id, username) in accounts {
            let dn = dn.trim().to_string();
            if dn.is_empty() {
                continue;
            }
            index
                .folded
                .insert(dn.to_lowercase(), (user_id, username.clone()));
            index.exact.insert(dn, (user_id, username));
        }
        Self {
            exact: index.exact,
            folded: index.folded,
        }
    }

    fn lookup(&self, dn: &str) -> Option<&(i64, String)> {
        let dn = dn.trim();
        self.exact
            .get(dn)
            .or_else(|| self.folded.get(&dn.to_lowercase()))
    }
}

/// Decide which memberships to insert.
pub fn plan_groups(
    teams: &[TeamSnapshot],
    groups: &[DirectoryGroup],
    accounts: &DnIndex,
) -> GroupSyncPlan {
    let mut plan = GroupSyncPlan::default();

    for group in groups {
        // Rule 2: exact name, no folding.
        let Some(team) = teams.iter().find(|team| team.name == group.name) else {
            plan.unmatched_groups.push(group.name.clone());
            continue;
        };

        for dn in &group.member_dns {
            let Some((user_id, username)) = accounts.lookup(dn) else {
                plan.unmatched_members.push(dn.trim().to_string());
                continue;
            };
            // Already in the team: nothing to do, and not an "extra" either —
            // the directory does list this person.
            if team.members.iter().any(|(id, _)| id == user_id) {
                continue;
            }
            plan.add.push(MembershipAdd {
                team_id: team.id,
                team_name: team.name.clone(),
                user_id: *user_id,
                username: username.clone(),
            });
        }
    }

    // Rule 3: the other direction is reported, never acted on. Computed by user
    // id — the only stable key across a rename.
    for group in groups {
        let Some(team) = teams.iter().find(|team| team.name == group.name) else {
            continue;
        };
        let directory_ids: Vec<i64> = group
            .member_dns
            .iter()
            .filter_map(|dn| accounts.lookup(dn).map(|(id, _)| *id))
            .collect();
        for (user_id, username) in &team.members {
            if !directory_ids.contains(user_id) {
                plan.extra_members
                    .push((team.name.clone(), username.clone()));
            }
        }
    }

    plan.add.sort_by(|a, b| {
        a.team_name
            .cmp(&b.team_name)
            .then_with(|| a.username.cmp(&b.username))
    });
    plan.unmatched_groups.sort();
    plan.unmatched_members.sort();
    plan.unmatched_members.dedup();
    plan.extra_members.sort();

    plan
}

// ── Database layer ─────────────────────────────────────────────

/// The teams of one org, each with its current members.
async fn team_snapshots(
    db: &sea_orm::DatabaseConnection,
    org_id: i64,
) -> crate::error::CoreResult<Vec<TeamSnapshot>> {
    let teams = rg_db::ops::org_ops::list_org_teams(db, org_id).await?;
    let mut snapshots = Vec::with_capacity(teams.len());
    for team in teams {
        let members = rg_db::ops::org_ops::list_team_members(db, team.id).await?;
        snapshots.push(TeamSnapshot {
            id: team.id,
            name: team.name,
            // The username is only used for the human-readable `extra_members`
            // report; `team_members` stores the id, so the report for these
            // entries reads `#<id>` until a join is worth the query.
            members: members
                .into_iter()
                .map(|member| (member.user_id, format!("#{}", member.user_id)))
                .collect(),
        });
    }
    Ok(snapshots)
}

/// Directory DN → local account, for one provider.
///
/// `ldap_dn` is the identity the login path stored; accounts without one cannot
/// be matched by a group listing and are simply absent from the index.
async fn account_index(
    db: &sea_orm::DatabaseConnection,
    provider_id: i64,
) -> crate::error::CoreResult<DnIndex> {
    let accounts = rg_db::ops::user_ops::list_by_ldap_provider(db, provider_id).await?;
    Ok(DnIndex::new(accounts.into_iter().filter_map(|user| {
        let dn = user.ldap_dn.clone()?;
        Some((dn, user.id, user.username))
    })))
}

/// Work out the group plan for one provider, without writing anything.
///
/// Errors if the provider is not fully configured for group sync — a partial
/// configuration would otherwise plan against an empty directory silently.
pub async fn plan_provider_groups(
    db: &sea_orm::DatabaseConnection,
    provider: &sso_provider::Model,
    jwt_secret: &str,
) -> crate::error::CoreResult<Option<GroupSyncPlan>> {
    if provider.provider_type != "ldap" {
        return Err(crate::error::CoreError::invalid_input(
            "provider is not LDAP",
        ));
    }
    let (Some(base_dn), Some(filter), Some(org_id)) = (
        provider.ldap_group_base_dn.as_deref(),
        provider.ldap_group_filter.as_deref(),
        provider.ldap_group_team_org_id,
    ) else {
        // Not configured: nothing to do, and not an error.
        return Ok(None);
    };

    let config = crate::user::service::ldap_config_from_provider(provider, jwt_secret)?;
    let groups: Vec<DirectoryGroup> = crate::auth::ldap::list_groups(&config, base_dn, filter)
        .await?
        .into_iter()
        .map(DirectoryGroup::from)
        .collect();
    let teams = team_snapshots(db, org_id).await?;
    let accounts = account_index(db, provider.id).await?;
    Ok(Some(plan_groups(&teams, &groups, &accounts)))
}

/// Insert the planned memberships. Returns how many were inserted.
///
/// Additions only: [`plan_groups`] never produces a removal, so there is
/// nothing here that could drop someone from a team.
pub async fn apply_groups(
    db: &sea_orm::DatabaseConnection,
    plan: &GroupSyncPlan,
) -> crate::error::CoreResult<usize> {
    let mut inserted = 0usize;
    for add in &plan.add {
        // `member` is the conservative role: maintainer would also allow the
        // person to add others, which a directory group says nothing about.
        rg_db::ops::org_ops::add_team_member(db, add.team_id, add.user_id, "member").await?;
        inserted += 1;
    }
    Ok(inserted)
}

/// What a scheduled group sync did for one provider.
#[derive(Debug)]
pub enum GroupOutcome {
    /// Not configured for this provider.
    NotConfigured {
        provider_id: i64,
    },
    /// Configured, nothing to add.
    Unchanged {
        provider_id: i64,
    },
    /// Memberships inserted.
    Applied {
        provider_id: i64,
        inserted: usize,
        plan: GroupSyncPlan,
    },
    Failed {
        provider_id: i64,
        error: String,
    },
}

/// Sync group memberships for every enabled LDAP provider that has group sync
/// configured.
///
/// A provider with the three columns empty is skipped silently — that is the
/// off state, not a failure. A provider that *is* configured but whose directory
/// cannot be read fails on its own without affecting the others.
pub async fn sync_all_enabled(
    db: &sea_orm::DatabaseConnection,
    jwt_secret: &str,
) -> Vec<GroupOutcome> {
    let providers = match rg_db::ops::sso_provider_ops::list_enabled(db).await {
        Ok(providers) => providers,
        Err(error) => {
            tracing::error!(%error, "LDAP group sync: cannot list providers");
            return Vec::new();
        }
    };

    let mut outcomes = Vec::new();
    for provider in providers
        .into_iter()
        .filter(|provider| provider.provider_type == "ldap")
    {
        let provider_id = provider.id;
        let plan = match plan_provider_groups(db, &provider, jwt_secret).await {
            Ok(Some(plan)) => plan,
            Ok(None) => {
                outcomes.push(GroupOutcome::NotConfigured { provider_id });
                continue;
            }
            Err(error) => {
                tracing::error!(provider_id, %error, "LDAP group sync: planning failed");
                outcomes.push(GroupOutcome::Failed {
                    provider_id,
                    error: error.to_string(),
                });
                continue;
            }
        };

        if plan.is_noop() {
            outcomes.push(GroupOutcome::Unchanged { provider_id });
            continue;
        }

        match apply_groups(db, &plan).await {
            Ok(inserted) => {
                tracing::info!(
                    provider_id,
                    inserted,
                    unmatched_groups = plan.unmatched_groups.len(),
                    unmatched_members = plan.unmatched_members.len(),
                    "LDAP group sync applied"
                );
                outcomes.push(GroupOutcome::Applied {
                    provider_id,
                    inserted,
                    plan,
                });
            }
            Err(error) => {
                tracing::error!(provider_id, %error, "LDAP group sync: apply failed");
                outcomes.push(GroupOutcome::Failed {
                    provider_id,
                    error: error.to_string(),
                });
            }
        }
    }
    outcomes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn group(name: &str, members: &[&str]) -> DirectoryGroup {
        DirectoryGroup {
            name: name.to_string(),
            member_dns: members.iter().map(|dn| (*dn).to_string()).collect(),
        }
    }

    fn team(id: i64, name: &str, members: &[(i64, &str)]) -> TeamSnapshot {
        TeamSnapshot {
            id,
            name: name.to_string(),
            members: members
                .iter()
                .map(|(id, name)| (*id, (*name).to_string()))
                .collect(),
        }
    }

    fn index(entries: &[(&str, i64, &str)]) -> DnIndex {
        DnIndex::new(
            entries
                .iter()
                .map(|(dn, id, name)| ((*dn).to_string(), *id, (*name).to_string())),
        )
    }

    const ALICE: &str = "uid=alice,dc=example,dc=com";
    const BOB: &str = "uid=bob,dc=example,dc=com";

    #[test]
    fn a_group_adds_the_members_the_team_does_not_have() {
        let plan = plan_groups(
            &[team(1, "backend", &[])],
            &[group("backend", &[ALICE, BOB])],
            &index(&[(ALICE, 10, "alice"), (BOB, 11, "bob")]),
        );
        assert_eq!(plan.add.len(), 2);
        assert_eq!(plan.add[0].username, "alice");
        assert_eq!(plan.add[0].team_id, 1);
        assert!(plan.unmatched_groups.is_empty());
        assert!(plan.unmatched_members.is_empty());
        assert!(plan.extra_members.is_empty());
    }

    /// Rule 2: `Backend` and `backend` are different names. Folding them would
    /// make a sync look successful while nothing was written.
    #[test]
    fn a_group_name_that_differs_only_in_case_matches_nothing() {
        let plan = plan_groups(
            &[team(1, "backend", &[])],
            &[group("Backend", &[ALICE])],
            &index(&[(ALICE, 10, "alice")]),
        );
        assert!(plan.add.is_empty());
        assert_eq!(plan.unmatched_groups, vec!["Backend".to_string()]);
    }

    /// Rule 2: no team, no creation — a team carries a permission.
    #[test]
    fn a_group_with_no_team_is_reported_and_nothing_is_created() {
        let plan = plan_groups(
            &[team(1, "backend", &[])],
            &[group("frontend", &[ALICE])],
            &index(&[(ALICE, 10, "alice")]),
        );
        assert!(plan.add.is_empty());
        assert_eq!(plan.unmatched_groups, vec!["frontend".to_string()]);
    }

    #[test]
    fn a_member_dn_with_no_account_is_reported() {
        let plan = plan_groups(
            &[team(1, "backend", &[])],
            &[group("backend", &[ALICE, "uid=ghost,dc=example,dc=com"])],
            &index(&[(ALICE, 10, "alice")]),
        );
        assert_eq!(plan.add.len(), 1);
        assert_eq!(
            plan.unmatched_members,
            vec!["uid=ghost,dc=example,dc=com".to_string()]
        );
    }

    /// Rule 3: the direction that must never be acted on.
    #[test]
    fn team_members_the_group_does_not_list_are_reported_not_removed() {
        let plan = plan_groups(
            &[team(1, "backend", &[(11, "bob"), (10, "alice")])],
            &[group("backend", &[ALICE])],
            &index(&[(ALICE, 10, "alice"), (BOB, 11, "bob")]),
        );
        assert!(plan.add.is_empty(), "alice is already in the team");
        assert_eq!(
            plan.extra_members,
            vec![("backend".to_string(), "bob".to_string())],
            "bob is in the team but not in the group: report, never remove"
        );
    }

    #[test]
    fn someone_already_in_the_team_is_not_added_again() {
        let plan = plan_groups(
            &[team(1, "backend", &[(10, "alice")])],
            &[group("backend", &[ALICE])],
            &index(&[(ALICE, 10, "alice")]),
        );
        assert!(plan.is_noop());
        assert!(plan.extra_members.is_empty(), "she is in both: not extra");
    }

    /// A DN the directory spells in another case still matches the account, so
    /// a casing difference does not silently drop people from a team.
    #[test]
    fn dn_casing_is_a_fallback_not_a_mismatch() {
        let plan = plan_groups(
            &[team(1, "backend", &[])],
            &[group("backend", &["UID=Alice,DC=Example,DC=Com"])],
            &index(&[(ALICE, 10, "alice")]),
        );
        assert_eq!(plan.add.len(), 1, "case-insensitive fallback must match");
        assert_eq!(plan.add[0].username, "alice");
    }

    /// End-to-end against a real directory.
    ///
    /// Set `LDAP_TEST_HOST` / `LDAP_TEST_PORT` (and optionally
    /// `LDAP_TEST_BASE_DN`) to run it; otherwise it returns immediately so the
    /// default suite needs no container. This is the only place the LDAP
    /// attribute handling meets a server that actually speaks LDAP.
    ///
    /// Seeded directory expectation: groups `backend` (alice, bob) and
    /// `frontend` (bob) under the base DN, and local teams of the same names.
    #[tokio::test]
    async fn a_real_directory_fills_the_teams_it_names() {
        use rg_db::sea_orm::{ConnectionTrait, Database, DbBackend, Statement};

        let Ok(host) = std::env::var("LDAP_TEST_HOST") else {
            return;
        };
        let port: u16 = std::env::var("LDAP_TEST_PORT")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(1389);
        let base_dn =
            std::env::var("LDAP_TEST_BASE_DN").unwrap_or_else(|_| "dc=example,dc=org".to_string());

        let db = Database::connect("sqlite::memory:").await.unwrap();
        rg_db::run_migrations(&db).await.unwrap();

        let key = crate::auth::encryption::derive_key("ldap-group-live-test");

        // The provider first: an LDAP account cannot exist without one, and the
        // org needs an owner id.
        let provider = rg_db::ops::sso_provider_ops::upsert(
            &db,
            None,
            "Corp",
            "corp",
            "ldap",
            None,
            None,
            None,
            None,
            Some(&host),
            Some(i32::from(port)),
            Some("cn=admin,dc=example,dc=org"),
            Some(&crate::auth::encryption::encrypt("admin", &key).unwrap()),
            Some(&base_dn),
            Some("(uid={username})"),
            true,
            None,
        )
        .await
        .unwrap();
        let owner = rg_db::ops::user_ops::create_ldap_user(
            &db,
            provider.id,
            "owner",
            "owner@example.org",
            None,
            "uid=owner,dc=example,dc=org",
            Some("owner"),
        )
        .await
        .unwrap();

        let org =
            rg_db::ops::org_ops::create_org(&db, "acme", Some("Acme"), None, owner.id, "public")
                .await
                .unwrap();
        let backend_team = rg_db::ops::org_ops::create_team(&db, org.id, "backend", None, "write")
            .await
            .unwrap();
        let frontend_team = rg_db::ops::org_ops::create_team(&db, org.id, "frontend", None, "read")
            .await
            .unwrap();

        let provider = rg_db::ops::sso_provider_ops::set_ldap_group_sync(
            &db,
            provider.id,
            Some(&base_dn),
            Some("(objectClass=groupOfNames)"),
            Some(org.id),
        )
        .await
        .unwrap();

        // Two accounts, both already known to this instance.
        for (username, dn) in [
            ("alice", "uid=alice,dc=example,dc=org"),
            ("bob", "uid=bob,dc=example,dc=org"),
        ] {
            rg_db::ops::user_ops::create_ldap_user(
                &db,
                provider.id,
                username,
                &format!("{username}@example.org"),
                None,
                dn,
                Some(username),
            )
            .await
            .unwrap();
        }

        let plan = plan_provider_groups(&db, &provider, "ldap-group-live-test")
            .await
            .expect("planning must succeed")
            .expect("group sync is configured");
        assert_eq!(
            plan.add.len(),
            3,
            "backend has 2 members, frontend has 1. add={:?} unmatched_groups={:?} unmatched_members={:?} extra={:?}",
            plan.add,
            plan.unmatched_groups,
            plan.unmatched_members,
            plan.extra_members
        );
        assert!(
            plan.unmatched_groups.is_empty(),
            "{:?}",
            plan.unmatched_groups
        );
        assert!(
            plan.unmatched_members.is_empty(),
            "{:?}",
            plan.unmatched_members
        );
        assert_eq!(plan.extra_members.len(), 0);

        let inserted = apply_groups(&db, &plan).await.expect("apply");
        assert_eq!(inserted, 3);

        let backend_members = rg_db::ops::org_ops::list_team_members(&db, backend_team.id)
            .await
            .unwrap();
        let frontend_members = rg_db::ops::org_ops::list_team_members(&db, frontend_team.id)
            .await
            .unwrap();
        assert_eq!(backend_members.len(), 2);
        assert_eq!(frontend_members.len(), 1);

        // Idempotent: the second plan sees the memberships it just wrote.
        let again = plan_provider_groups(&db, &provider, "ldap-group-live-test")
            .await
            .unwrap()
            .unwrap();
        assert!(again.is_noop(), "second run must add nothing: {:?}", again);

        // Additions only: drop alice from the group on the server side is out of
        // scope here, but a team member the group never listed must be reported
        // and left alone. `frontend` has a member the directory does not list.
        let extra = rg_db::ops::org_ops::add_team_member(&db, frontend_team.id, owner.id, "member")
            .await
            .unwrap();
        assert!(extra.id > 0);
        let with_extra = plan_provider_groups(&db, &provider, "ldap-group-live-test")
            .await
            .unwrap()
            .unwrap();
        assert!(with_extra.is_noop());
        assert_eq!(with_extra.extra_members.len(), 1);
        let rows = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                format!(
                    "SELECT COUNT(*) FROM team_members WHERE team_id = {}",
                    frontend_team.id
                ),
            ))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            rows.try_get_by_index::<i64>(0).unwrap(),
            2,
            "the extra member must still be there"
        );
    }

    #[test]
    fn an_empty_group_adds_nothing_and_reports_every_team_member_as_extra() {
        let plan = plan_groups(
            &[team(1, "backend", &[(10, "alice")])],
            &[group("backend", &[])],
            &index(&[(ALICE, 10, "alice")]),
        );
        assert!(plan.is_noop());
        assert_eq!(
            plan.extra_members,
            vec![("backend".to_string(), "alice".to_string())]
        );
    }

    /// Running the plan twice must not produce a second set of inserts — the
    /// second run sees the members it just added.
    #[test]
    fn a_second_plan_after_applying_is_empty() {
        let accounts = index(&[(ALICE, 10, "alice")]);
        let directory = [group("backend", &[ALICE])];
        let first = plan_groups(&[team(1, "backend", &[])], &directory, &accounts);
        assert_eq!(first.add.len(), 1);

        // What the database looks like after applying the first plan.
        let after = [team(1, "backend", &[(10, "alice")])];
        let second = plan_groups(&after, &directory, &accounts);
        assert!(second.is_noop());
        assert!(second.extra_members.is_empty());
    }
}
