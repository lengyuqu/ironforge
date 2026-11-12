//! Who belongs to a repository's collaboration working group.
//!
//! Collaboration data is not public content (see `require_collab_read` in the
//! REST layer): readers are the owner, collaborators of any level, organization
//! members and anyone who can write. This is the one definition; the REST
//! layer and background evaluation (escalation) both use it, so a reminder is
//! never produced for an account that could not read the record itself.

use rg_db::entities::repository;
use rg_db::ops::{org_ops, repo_collaborator_ops};
use sea_orm::DatabaseConnection;

use super::error::{CollabError, CollabResult};

/// Whether `user_id` is in the working group of `repo`. Decided from the
/// database, not the 30s process-local permission cache, so a revocation takes
/// effect on the very next call — at the price of a global perm-cache write lock per call (`require_collab_read` pays it twice per request: its own clear, then this one; `require_collab_write` pays it once, then calls `can_write_repo` directly).
pub async fn is_collab_member(
    db: &DatabaseConnection,
    repo: &repository::Model,
    user_id: i64,
) -> CollabResult<bool> {
    crate::repo::service::invalidate_perm_cache_user(repo.id, user_id);
    // An agent is in the working group only if its owner is too: the checks
    // below read the collaborator and organization tables directly, which
    // would otherwise bypass the owner cap that `can_read_repo`/`can_write_repo`
    // apply.
    match crate::agent::owner_cap(db, user_id)
        .await
        .map_err(|e| CollabError::Internal(anyhow::anyhow!("{e}")))?
    {
        crate::agent::OwnerCap::None => {}
        crate::agent::OwnerCap::Denied => return Ok(false),
        crate::agent::OwnerCap::Owner(owner_id) => {
            // The owner is a person, so this recurses at most once.
            if !Box::pin(is_collab_member(db, repo, owner_id)).await? {
                return Ok(false);
            }
        }
    }
    if repo.owner_id == user_id {
        return Ok(true);
    }
    if repo_collaborator_ops::get_permission(db, repo.id, user_id)
        .await?
        .is_some()
    {
        return Ok(true);
    }
    if let Some(org_id) = repo.org_id {
        if org_ops::is_org_member(db, org_id, user_id).await? {
            return Ok(true);
        }
    }
    // Writers through other routes (e.g. organization teams) count as well.
    crate::repo::service::can_write_repo(db, repo, Some(user_id))
        .await
        .map_err(|e| CollabError::Internal(anyhow::anyhow!("{e}")))
}
