//! The agent draft area: naming rules, matching and lifecycle arithmetic
//! (#18, T11).
//!
//! An *agent draft namespace* is a reserved branch directory
//! `refs/heads/agent/<id>/...`. Branches under it may be pushed and run CI, but
//! their pushes produce no webhooks, no WebSocket fan-out and no mail, and the
//! directory is deleted on its own once `expires_at + grace_secs` has passed.
//!
//! Everything in this module is either pure or a thin read over the table; the
//! REST handlers and the cleanup tick compose it. Keeping the rules here (not in
//! the handlers) is what makes "a push that matches a namespace" and "a
//! reservation a caller may create" the same predicate.
//!
//! # Why `refs/heads/` and not a private namespace
//!
//! The branch allow-list of a personal access token only ever builds
//! `refs/heads/<pattern>`, and `receive-pack` deletes only `refs/heads/*`. A
//! separate `refs/agent/*` namespace would need both to change. A reserved
//! *prefix* under `refs/heads/` inherits branch protection, the branch list, the
//! SSH path and the delete API unchanged.

use chrono::{DateTime, Duration, Utc};
use sea_orm::DatabaseConnection;

use crate::error::{CoreError, CoreResult};
use rg_db::entities::agent_draft_namespace::{
    Model as Namespace, STATE_ACTIVE, STATE_DELETED, STATE_EXPIRING,
};

/// Shortest TTL a caller may ask for. Below a minute a namespace would expire
/// before its first push lands.
pub const MIN_TTL_SECS: i64 = 60;
/// Longest TTL a caller may ask for: a month. The point of the area is that
/// branches disappear, so "forever" is not on offer.
pub const MAX_TTL_SECS: i64 = 30 * 86_400;
/// Grace period when the caller does not ask for one: a day, so a CI run that
/// started just before expiry can finish and be read back.
pub const DEFAULT_GRACE_SECS: i64 = 86_400;
/// `extend` may push the deadline out to at most this many times the original
/// TTL, counted from `created_at`. Four is enough to finish a long job; it also
/// bounds how long a forgotten namespace can outlive its purpose.
pub const EXTEND_CAP_MULTIPLIER: i64 = 4;
/// The literal first segment every draft prefix starts with.
pub const PREFIX_ROOT: &str = "agent";
/// Longest accepted prefix, matching the column and the allow-list rules.
const MAX_PREFIX_CHARS: usize = 200;
/// Longest accepted agent segment (`agent/<this>/`).
const MAX_AGENT_SEGMENT_CHARS: usize = 40;

/// Check a prefix a caller wants reserved (`agent/42/`).
///
/// Three layers: the structural rules that make the segment rules meaningful,
/// the git rules for the names that will live under it (the last segment rule
/// is enforced by `gix` itself on `refs/heads/<prefix>probe`, which is the same
/// check the branch APIs use), and the "no patterns" rule — a prefix is a
/// concrete directory, and `*` here would reserve names the caller did not
/// mean.
pub fn validate_prefix(prefix: &str) -> CoreResult<()> {
    let bad = |why: &str| {
        CoreError::invalid_input(format!("draft prefix '{prefix}' is not valid: {why}"))
    };
    if prefix.is_empty() || prefix.len() > MAX_PREFIX_CHARS {
        return Err(bad("must be 1-200 characters"));
    }
    if !prefix.is_ascii() {
        return Err(bad("must be ASCII"));
    }
    if prefix.contains('*') {
        return Err(bad(
            "is a concrete directory; wildcards belong in a token's branch list, not here",
        ));
    }
    let Some(body) = prefix.strip_suffix('/') else {
        return Err(bad("must end with '/'"));
    };
    let segments: Vec<&str> = body.split('/').collect();
    if segments.len() < 2 {
        return Err(bad("must be 'agent/<id>/' or deeper"));
    }
    if segments[0] != PREFIX_ROOT {
        return Err(bad("must start with 'agent/'"));
    }
    let agent = segments[1];
    if agent.is_empty() || agent.len() > MAX_AGENT_SEGMENT_CHARS {
        return Err(bad("the agent segment must be 1-40 characters"));
    }
    if !agent
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(bad(
            "the agent segment may only use letters, digits, '-' and '_'",
        ));
    }
    for segment in &segments {
        if segment.is_empty() || *segment == "." || *segment == ".." {
            return Err(bad("must not contain empty, '.' or '..' segments"));
        }
        if segment.ends_with(".lock") {
            return Err(bad("no segment may end with '.lock'"));
        }
    }
    // The authoritative check: git must accept a branch under this directory.
    // `probe` stands in for the branch name a caller will pick.
    if gix::refs::FullName::try_from(format!("refs/heads/{prefix}probe")).is_err() {
        return Err(bad("git would refuse a branch name under this directory"));
    }
    Ok(())
}

/// The branch part of a full ref (`refs/heads/a/b` → `a/b`), or `None` for a
/// ref outside `refs/heads/`.
pub fn branch_of(refname: &str) -> Option<&str> {
    refname
        .strip_prefix("refs/heads/")
        .filter(|branch| !branch.is_empty())
}

/// Whether `branch` (without `refs/heads/`) lives under `prefix`. The prefix
/// itself is not a branch: `agent/42` is the directory, `agent/42/x` is in it.
pub fn namespace_covers(prefix: &str, branch: &str) -> bool {
    branch.len() > prefix.len() && branch.starts_with(prefix)
}

/// The most specific namespace covering `branch`, if any.
///
/// "Most specific" matters when a caller reserves both `agent/42/` and a
/// narrower `agent/42/experiments/`: the narrower one owns the branch, so its
/// TTL and its gate are the ones that apply.
pub fn longest_match<'a>(namespaces: &'a [Namespace], branch: &str) -> Option<&'a Namespace> {
    namespaces
        .iter()
        .filter(|ns| namespace_covers(&ns.prefix, branch))
        .max_by_key(|ns| ns.prefix.len())
}

/// The namespace governing `refname`, if the repository has one.
///
/// Loads the repository's namespaces (a handful) and matches in memory: the
/// alternative is one query per ref with a `LIKE` and a backend-specific
/// string function.
pub async fn find_namespace(
    db: &DatabaseConnection,
    repo_id: i64,
    refname: &str,
) -> CoreResult<Option<Namespace>> {
    let Some(branch) = branch_of(refname) else {
        return Ok(None);
    };
    let namespaces = rg_db::ops::agent_draft_ops::list_by_repo(db, repo_id)
        .await
        .map_err(|e| CoreError::internal(e.to_string()))?;
    Ok(longest_match(&namespaces, branch).cloned())
}

/// Is `refname` inside a draft namespace? Used by the push hooks (suppress the
/// human-facing channels) and by CI (no secrets, no host execution).
///
/// A closed namespace counts as a draft too: its pushes are refused, but if one
/// ever slips through it must still not notify anyone.
pub async fn is_draft_ref(db: &DatabaseConnection, repo_id: i64, refname: &str) -> bool {
    find_namespace(db, repo_id, refname)
        .await
        .ok()
        .flatten()
        .is_some()
}

/// The moment `namespace` stops accepting pushes.
pub fn gate_closed_at(namespace: &Namespace) -> DateTime<Utc> {
    namespace.expires_at
}

/// The moment its branches are deleted: the gate closes, then the grace period
/// runs.
pub fn delete_due_at(namespace: &Namespace) -> DateTime<Utc> {
    namespace.expires_at + Duration::seconds(namespace.grace_secs)
}

/// Is the gate closed (pushes refused) at `now`?
pub fn gate_is_closed(namespace: &Namespace, now: DateTime<Utc>) -> bool {
    namespace.state != STATE_ACTIVE
        || namespace.state == STATE_DELETED
        || namespace.expires_at <= now
}

/// May the cleanup tick delete this namespace's branches at `now`?
pub fn deletion_is_due(namespace: &Namespace, now: DateTime<Utc>) -> bool {
    namespace.state == STATE_EXPIRING && delete_due_at(namespace) <= now
}

/// Refuse patterns for the pending ref updates of a push.
///
/// Returned as `(pattern, message)` for `receive-pack`'s `rejected_refs`, whose
/// matcher accepts `*`. Two distinct refusals:
///
/// * a closed namespace refuses its whole directory (the agent should register
///   a new draft, not resurrect a dead one);
/// * an open namespace refuses the directory *itself* (`refs/heads/agent/42`),
///   which is a directory, not a branch — pushing it would make the prefix a
///   branch and break the "everything under it" reading.
pub fn push_rejections(namespaces: &[Namespace], now: DateTime<Utc>) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for ns in namespaces {
        let directory = format!("refs/heads/{}", ns.prefix);
        let body = ns.prefix.trim_end_matches('/');
        let closed = ns.state == STATE_DELETED || ns.expires_at <= now;
        if closed {
            let why = if ns.state == STATE_DELETED {
                "was cleaned up".to_string()
            } else {
                format!(
                    "expired at {}",
                    ns.expires_at
                        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
                )
            };
            let message = format!(
                "the draft namespace {body}/ {why}; register a new draft (POST /repos/{{owner}}/{{name}}/agent-drafts) to push again"
            );
            // Both the directory itself and everything under it.
            out.push((
                format!("refs/heads/{body}"),
                format!("the draft namespace {body}/ {why}"),
            ));
            out.push((format!("{directory}*"), message));
        } else {
            out.push((
                format!("refs/heads/{body}"),
                format!(
                    "the draft namespace {body}/ is a directory, not a branch; push to a branch inside it"
                ),
            ));
        }
    }
    out
}

/// Compute the new deadline for `extend`.
///
/// Extending never shortens: the base is the later of "now" and the current
/// deadline, so a caller extending early does not lose time. The result is
/// clamped to `created_at + 4 × ttl_secs`; when the namespace is already at that
/// ceiling the caller gets an error instead of a silently unchanged answer.
pub fn extend_deadline(
    namespace: &Namespace,
    now: DateTime<Utc>,
    requested_secs: i64,
) -> CoreResult<DateTime<Utc>> {
    if requested_secs <= 0 {
        return Err(CoreError::invalid_input("ttl_secs must be positive"));
    }
    let cap = namespace.created_at + Duration::seconds(namespace.ttl_secs * EXTEND_CAP_MULTIPLIER);
    let base = namespace.expires_at.max(now);
    if base >= cap {
        return Err(CoreError::invalid_input(format!(
            "draft namespace '{}' is already at its maximum lifetime ({} x its TTL)",
            namespace.prefix, EXTEND_CAP_MULTIPLIER
        )));
    }
    Ok((base + Duration::seconds(requested_secs)).min(cap))
}

/// The `state` a freshly created namespace is in.
pub fn initial_state() -> &'static str {
    STATE_ACTIVE
}

/// Human-readable list of the states, for error messages.
pub fn state_name(state: &str) -> &str {
    match state {
        STATE_ACTIVE => "active",
        STATE_EXPIRING => "expiring",
        STATE_DELETED => "deleted",
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rg_db::entities::agent_draft_namespace::Model as Namespace;

    fn ns(prefix: &str, ttl_secs: i64, expires_in_secs: i64, state: &str) -> Namespace {
        let now = Utc::now();
        Namespace {
            id: 1,
            repo_id: 1,
            prefix: prefix.to_string(),
            owner_user_id: 1,
            ttl_secs,
            expires_at: now + Duration::seconds(expires_in_secs),
            grace_secs: 60,
            state: state.to_string(),
            extended_count: 0,
            created_at: now,
        }
    }

    #[test]
    fn valid_prefixes_are_accepted() {
        for prefix in [
            "agent/42/",
            "agent/a1/",
            "agent/my-agent/",
            "agent/a/experiments/",
        ] {
            assert!(
                validate_prefix(prefix).is_ok(),
                "{prefix} should be valid: {:?}",
                validate_prefix(prefix)
            );
        }
    }

    #[test]
    fn the_prefix_rule_table_refuses_every_shape_that_could_escape_the_area() {
        let too_long = format!("agent/{}/", "a".repeat(MAX_AGENT_SEGMENT_CHARS + 1));
        let cases: &[(&str, &str)] = &[
            ("", "empty"),
            ("/", "only a slash"),
            ("agent/42", "no trailing slash"),
            ("agent/", "empty agent segment"),
            ("sub/42/", "wrong root"),
            ("agent/42/*", "a wildcard"),
            ("agent/4 2/", "a space"),
            ("agent/4-2-/", "valid, see the accepted table"),
            ("agent/../", "traversal"),
            ("agent/42//", "an empty segment"),
            ("agent/42/x.", "no trailing slash"),
            ("agent/42/x.lock/", "a .lock segment"),
            ("agent/42/é/", "non-ASCII"),
            ("agent/42/..", "traversal without slash"),
        ];
        for (prefix, why) in cases {
            if *prefix == "agent/4-2-/" {
                assert!(validate_prefix(prefix).is_ok(), "{why}");
                continue;
            }
            assert!(
                validate_prefix(prefix).is_err(),
                "{prefix} ({why}) must be refused"
            );
        }
        assert!(validate_prefix(&too_long).is_err(), "an over-long segment");
        let over_long = format!("agent/42/{}", "a".repeat(MAX_PREFIX_CHARS));
        assert!(validate_prefix(&over_long).is_err(), "an over-long prefix");
    }

    #[test]
    fn a_branch_is_covered_only_below_the_prefix_and_the_longest_match_wins() {
        assert!(namespace_covers("agent/42/", "agent/42/fix"));
        assert!(!namespace_covers("agent/42/", "agent/42"));
        assert!(!namespace_covers("agent/42/", "agent/43/fix"));
        assert!(!namespace_covers("agent/42/", "agent/4/fix"));

        let wide = ns("agent/42/", 3600, 3600, STATE_ACTIVE);
        let narrow = Namespace {
            id: 2,
            prefix: "agent/42/experiments/".into(),
            ..wide.clone()
        };
        let namespaces = vec![wide.clone(), narrow.clone()];
        assert_eq!(
            longest_match(&namespaces, "agent/42/experiments/x")
                .map(|n| n.id)
                .unwrap(),
            narrow.id
        );
        assert_eq!(
            longest_match(&namespaces, "agent/42/fix")
                .map(|n| n.id)
                .unwrap(),
            wide.id
        );
        assert!(longest_match(&namespaces, "main").is_none());
        assert!(longest_match(&namespaces, "agent/43/x").is_none());
    }

    #[test]
    fn only_refs_heads_branches_take_part() {
        assert_eq!(branch_of("refs/heads/agent/42/x"), Some("agent/42/x"));
        assert_eq!(branch_of("refs/tags/v1"), None);
        assert_eq!(branch_of("refs/heads/"), None);
    }

    #[test]
    fn a_closed_namespace_refuses_its_directory_and_everything_below_it() {
        let active = ns("agent/42/", 3600, 3600, STATE_ACTIVE);
        let expired = ns("agent/43/", 3600, -10, STATE_ACTIVE);
        let deleted = ns("agent/44/", 3600, -10, STATE_DELETED);
        let now = Utc::now();
        let patterns = push_rejections(&[active, expired, deleted], now);

        let hit = |refname: &str| -> Option<String> {
            patterns
                .iter()
                .find(|(pattern, _)| {
                    crate::branch_protection::service::ref_matches_pattern(refname, pattern)
                })
                .map(|(_, message)| message.clone())
        };
        // The open namespace refuses only its own directory.
        assert!(hit("refs/heads/agent/42").is_some());
        assert!(hit("refs/heads/agent/42/fix").is_none());
        // A closed one refuses the directory and its contents, with a reason
        // that says how to push again.
        assert!(hit("refs/heads/agent/43").is_some());
        let message = hit("refs/heads/agent/43/fix").expect("refused");
        assert!(message.contains("expired at"), "{message}");
        assert!(message.contains("agent-drafts"), "{message}");
        assert!(hit("refs/heads/agent/44/fix")
            .unwrap()
            .contains("cleaned up"));
        // Unrelated refs are untouched.
        assert!(hit("refs/heads/main").is_none());
        assert!(hit("refs/heads/agent/4/fix").is_none());
    }

    #[test]
    fn extend_never_shortens_and_stops_at_four_times_the_ttl() {
        let mut namespace = ns("agent/42/", 3600, 1800, STATE_ACTIVE);
        namespace.created_at = Utc::now();
        namespace.expires_at = namespace.created_at + Duration::seconds(1800);

        // Inside the cap: the requested time is added to the current deadline.
        let extended = extend_deadline(&namespace, Utc::now(), 3600).unwrap();
        assert_eq!(extended, namespace.expires_at + Duration::seconds(3600));

        // At the cap: refused rather than silently unchanged.
        let cap = namespace.created_at + Duration::seconds(4 * 3600);
        namespace.expires_at = cap;
        assert!(extend_deadline(&namespace, Utc::now(), 60).is_err());

        // Past what the cap allows: clamped to the cap, never beyond it.
        namespace.expires_at = cap - Duration::seconds(60);
        let clamped = extend_deadline(&namespace, Utc::now(), 3600).unwrap();
        assert_eq!(clamped, cap);

        // A namespace whose deadline already passed does not shorten: the base
        // is "now", not the stale deadline.
        let now = Utc::now();
        namespace.created_at = now;
        namespace.expires_at = now - Duration::seconds(600);
        let rescued = extend_deadline(&namespace, now, 60).unwrap();
        assert_eq!(rescued, now + Duration::seconds(60));
    }

    #[test]
    fn the_lifecycle_boundaries_are_expiry_then_grace() {
        let now = Utc::now();
        let mut namespace = ns("agent/42/", 3600, 60, STATE_ACTIVE);
        namespace.expires_at = now;
        assert!(gate_is_closed(&namespace, now), "expiry closes the gate");
        assert!(!deletion_is_due(&namespace, now), "and starts the grace");

        namespace.state = STATE_EXPIRING.to_string();
        assert!(!deletion_is_due(&namespace, now + Duration::seconds(59)));
        assert!(deletion_is_due(&namespace, now + Duration::seconds(60)));
        assert!(gate_is_closed(&namespace, now));

        namespace.state = STATE_DELETED.to_string();
        assert!(!deletion_is_due(&namespace, now + Duration::seconds(3600)));
    }
}
