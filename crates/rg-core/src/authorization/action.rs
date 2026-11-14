//! The platform actions an authorization request can cover.
//!
//! `action` is stored as one of these strings and never taken from client input
//! verbatim (same idea as `agent::validate_agent_token_scopes`): a caller that
//! wants an action it cannot name gets a 400, not a stored row with a typo in it.
//!
//! `ScopeExpand` is deliberately listed but not evaluated in step 1: it is the
//! only action with no execution surface yet, and defining it before "what does
//! the agent actually get" is answered would produce a button that does nothing
//! (issue #19, §三 step 6).

use chrono::Duration;

use crate::error::{CoreError, CoreResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Release a CI job that is waiting on a protected environment.
    CiEnvironmentRelease,
    /// Merge a pull request into a protected branch.
    ProtectedBranchMerge,
    /// Push directly to a protected branch.
    ProtectedBranchPush,
    /// Publish a release.
    ReleasePublish,
    /// Widen an agent session's scope. Not implemented in step 1.
    ScopeExpand,
}

impl Action {
    pub const ALL: [Action; 5] = [
        Action::CiEnvironmentRelease,
        Action::ProtectedBranchMerge,
        Action::ProtectedBranchPush,
        Action::ReleasePublish,
        Action::ScopeExpand,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::CiEnvironmentRelease => "ci_environment_release",
            Self::ProtectedBranchMerge => "protected_branch_merge",
            Self::ProtectedBranchPush => "protected_branch_push",
            Self::ReleasePublish => "release_publish",
            Self::ScopeExpand => "scope_expand",
        }
    }

    /// Reject anything that is not one of the platform's action names.
    pub fn parse(value: &str) -> CoreResult<Self> {
        Self::ALL
            .into_iter()
            .find(|action| action.as_str() == value)
            .ok_or_else(|| {
                CoreError::invalid_input(format!("unknown authorization action: {value}"))
            })
    }

    /// How long an approval stays usable. A ticket that never expires is an
    /// approval that can be redeemed months later, after the world moved.
    ///
    /// - environment release / direct push: half an hour, matching how long a
    ///   waiting job or a push attempt stays meaningful;
    /// - PR merge / release publish: a day, the usual review cycle, and the
    ///   merge ticket is additionally pinned to `head_sha`;
    /// - scope expansion: a week, the number in the design.
    pub fn ttl(self) -> Duration {
        match self {
            Self::CiEnvironmentRelease | Self::ProtectedBranchPush => Duration::minutes(30),
            Self::ProtectedBranchMerge | Self::ReleasePublish => Duration::hours(24),
            Self::ScopeExpand => Duration::days(7),
        }
    }

    /// Whether running the action twice is harmless. A retryable failure returns
    /// the ticket to `pending`; a non-idempotent one revokes it.
    pub fn is_retryable(self) -> bool {
        matches!(
            self,
            Self::ProtectedBranchMerge | Self::ProtectedBranchPush | Self::ScopeExpand
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_platform_names_parse() {
        for action in Action::ALL {
            assert_eq!(Action::parse(action.as_str()).unwrap(), action);
        }
        for bad in [
            "",
            "merge",
            "PROTECTED_BRANCH_MERGE",
            "protected-branch-merge",
            " ",
            "protected_branch_merge; DROP TABLE authorization_requests",
        ] {
            assert!(Action::parse(bad).is_err(), "{bad} must be rejected");
        }
    }

    #[test]
    fn ttl_is_graded_and_never_forever() {
        assert_eq!(Action::CiEnvironmentRelease.ttl(), Duration::minutes(30));
        assert_eq!(Action::ScopeExpand.ttl(), Duration::days(7));
        for action in Action::ALL {
            assert!(action.ttl() > Duration::zero());
        }
    }

    #[test]
    fn publishing_and_releasing_are_not_retryable() {
        assert!(!Action::CiEnvironmentRelease.is_retryable());
        assert!(!Action::ReleasePublish.is_retryable());
        assert!(Action::ProtectedBranchMerge.is_retryable());
    }
}
