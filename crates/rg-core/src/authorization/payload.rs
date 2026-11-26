//! What a request is bound to, and how that binding is hashed.
//!
//! Two digests come out of this module:
//!
//! - [`payload_hash`]: `(repo, action, subject, head_sha)`. Stored on the
//!   request, re-computed at approval time and compared, so a row edited between
//!   "ask" and "approve" (the classic TOCTOU: SHA was A, now it is B) is refused
//!   instead of approved.
//! - [`authorization_set_id`]: the rule in force (who may approve, how many
//!   votes). Requests store it so "who was allowed to approve this, back then"
//!   stays answerable after the rule changed.
//!
//! Step 1 has no `parameters` column; see `docs/design/authorization.md` §5. Adding
//! one later means adding a column and a migration, and extending the canonical
//! form here so old hashes cannot be forged by dropping a field.

use sha2::{Digest, Sha256};

use super::action::Action;

/// Digest of everything a request is bound to.
pub fn payload_hash(repo_id: i64, action: Action, subject: &str, head_sha: Option<&str>) -> String {
    let canonical = serde_json::json!([repo_id, action.as_str(), subject, head_sha]);
    hex::encode(Sha256::digest(canonical.to_string().as_bytes()))
}

/// Digest of the authorization rule. Identical inputs give the same id on every
/// process and every backend; any change in the rule gives a different one.
///
/// `explicit` is sorted first so two callers that observed the same set in a
/// different order agree on the version.
#[allow(clippy::too_many_arguments)]
pub fn authorization_set_id(
    repo_id: i64,
    action: Action,
    subject: &str,
    explicit: &[i64],
    admins: bool,
    writers: bool,
    collaborators: bool,
    required_approvals: u32,
) -> i64 {
    let mut ids = explicit.to_vec();
    ids.sort_unstable();
    ids.dedup();
    let canonical = serde_json::json!([
        repo_id,
        action.as_str(),
        subject,
        ids,
        admins,
        writers,
        collaborators,
        required_approvals
    ]);
    let digest = Sha256::digest(canonical.to_string().as_bytes());
    let mut head = [0u8; 8];
    head.copy_from_slice(&digest[..8]);
    (u64::from_be_bytes(head) & i64::MAX as u64) as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payload_hash_is_stable_and_case_sensitive() {
        let a = payload_hash(1, Action::ProtectedBranchMerge, "7", Some("abc"));
        let b = payload_hash(1, Action::ProtectedBranchMerge, "7", Some("abc"));
        assert_eq!(a, b);
        assert_eq!(a.len(), 64);
        assert_ne!(
            a,
            payload_hash(1, Action::ProtectedBranchMerge, "7", Some("abd"))
        );
        assert_ne!(
            a,
            payload_hash(2, Action::ProtectedBranchMerge, "7", Some("abc"))
        );
        assert_ne!(
            a,
            payload_hash(1, Action::ProtectedBranchPush, "7", Some("abc"))
        );
        assert_ne!(a, payload_hash(1, Action::ProtectedBranchMerge, "7", None));
    }

    #[test]
    fn set_id_depends_on_the_rule_not_the_binding() {
        let base = authorization_set_id(
            1,
            Action::ProtectedBranchMerge,
            "7",
            &[3, 4],
            true,
            true,
            false,
            1,
        );
        // Order of the explicit list is not part of the rule.
        assert_eq!(
            base,
            authorization_set_id(
                1,
                Action::ProtectedBranchMerge,
                "7",
                &[4, 3],
                true,
                true,
                false,
                1
            )
        );
        // Every field that changes who may approve or how many votes is.
        assert_ne!(
            base,
            authorization_set_id(
                1,
                Action::ProtectedBranchMerge,
                "7",
                &[3, 4],
                true,
                true,
                false,
                2
            )
        );
        assert_ne!(
            base,
            authorization_set_id(
                1,
                Action::ProtectedBranchMerge,
                "7",
                &[3],
                true,
                true,
                false,
                1
            )
        );
        assert_ne!(
            base,
            authorization_set_id(
                1,
                Action::ProtectedBranchMerge,
                "7",
                &[3, 4],
                false,
                true,
                false,
                1
            )
        );
        assert!(base >= 0, "the id must be a positive bigint");
    }

    #[test]
    fn duplicate_names_in_the_explicit_list_do_not_change_the_version() {
        assert_eq!(
            authorization_set_id(
                1,
                Action::ReleasePublish,
                "*",
                &[2, 2, 3],
                false,
                false,
                false,
                1
            ),
            authorization_set_id(
                1,
                Action::ReleasePublish,
                "*",
                &[3, 2],
                false,
                false,
                false,
                1
            )
        );
    }
}
