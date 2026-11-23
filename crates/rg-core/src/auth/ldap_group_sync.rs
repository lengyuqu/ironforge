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
//! See `docs/plans/AUTH-403-ldap-group-team-sync.md`. In short:
//!
//! 1. the caller has already resolved which org to look at (the provider names
//!    exactly one), so this module only sees that org's teams;
//! 2. a group's `cn` must equal a team's name **exactly** — no case folding, no
//!    fuzzy match. A group with no team is reported, never created: creating a
//!    team means choosing a permission, which is an authorization decision;
//! 3. **additions only**. Members the directory does not list are reported as
//!    `extra_members` and left alone — they may have come from a manual grant,
//!    another provider, or history, and removal is not reversible.

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
