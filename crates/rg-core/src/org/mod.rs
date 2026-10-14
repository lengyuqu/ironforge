//! Organization service — business logic for org/team management.

use anyhow::Result;
use sea_orm::DatabaseConnection;

use rg_db::ops::org_ops;

/// Create a new organization.
pub async fn create_org(
    db: &DatabaseConnection,
    name: &str,
    display_name: Option<&str>,
    description: Option<&str>,
    owner_id: i64,
    visibility: &str,
) -> Result<rg_db::entities::organization::Model> {
    // Validate org name (same rules as username)
    crate::validate_username(name)?;

    // Check visibility
    if visibility != "public" && visibility != "private" {
        anyhow::bail!("visibility must be 'public' or 'private'");
    }

    // Check if org name is already taken
    if org_ops::get_org_by_name(db, name).await?.is_some() {
        anyhow::bail!("organization name '{}' is already taken", name);
    }

    org_ops::create_org(db, name, display_name, description, owner_id, visibility).await
}

/// Get an organization by name.
pub async fn get_org_by_name(
    db: &DatabaseConnection,
    name: &str,
) -> Result<Option<rg_db::entities::organization::Model>> {
    org_ops::get_org_by_name(db, name).await
}

/// Get an organization by ID.
pub async fn get_org(
    db: &DatabaseConnection,
    id: i64,
) -> Result<Option<rg_db::entities::organization::Model>> {
    org_ops::get_org(db, id).await
}

/// List organizations for a user.
pub async fn list_user_orgs(
    db: &DatabaseConnection,
    user_id: i64,
) -> Result<Vec<rg_db::entities::organization::Model>> {
    org_ops::list_user_orgs(db, user_id).await
}

/// Update an organization. `actor_id` must be an org owner/admin.
pub async fn update_org(
    db: &DatabaseConnection,
    id: i64,
    actor_id: i64,
    display_name: Option<&str>,
    description: Option<&str>,
    visibility: Option<&str>,
) -> Result<rg_db::entities::organization::Model> {
    if !can_admin_org(db, id, actor_id).await? {
        anyhow::bail!("forbidden: requires organization admin");
    }
    org_ops::update_org(db, id, display_name, description, visibility).await
}

/// Delete an organization (only owner can do this).
pub async fn delete_org(db: &DatabaseConnection, id: i64, requesting_user_id: i64) -> Result<()> {
    let org = org_ops::get_org(db, id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("organization not found"))?;

    if org.owner_id != requesting_user_id {
        anyhow::bail!("only the organization owner can delete it");
    }

    org_ops::delete_org(db, id).await
}

/// Add a member to an organization.
///
/// `actor_id` must be an org owner/admin. Callers that already authorized the
/// actor may use [`add_org_member_unchecked`].
pub async fn add_org_member(
    db: &DatabaseConnection,
    org_id: i64,
    actor_id: i64,
    user_id: i64,
    role: &str,
) -> Result<rg_db::entities::organization_member::Model> {
    if !can_admin_org(db, org_id, actor_id).await? {
        anyhow::bail!("forbidden: requires organization admin");
    }
    add_org_member_unchecked(db, org_id, user_id, role).await
}

/// Add a member without an authorization check.
///
/// Only for callers that performed the `can_admin_org` check themselves (e.g.
/// org creation, where the creator becomes owner).
pub(crate) async fn add_org_member_unchecked(
    db: &DatabaseConnection,
    org_id: i64,
    user_id: i64,
    role: &str,
) -> Result<rg_db::entities::organization_member::Model> {
    if role != "owner" && role != "admin" && role != "member" {
        anyhow::bail!("role must be 'owner', 'admin', or 'member'");
    }
    let member = org_ops::add_org_member(db, org_id, user_id, role).await?;
    // Org membership grants access across all org repos — flush perm cache.
    crate::repo::service::invalidate_perm_cache_all();
    Ok(member)
}

/// Remove a member from an organization. `actor_id` must be an org owner/admin.
pub async fn remove_org_member(
    db: &DatabaseConnection,
    org_id: i64,
    actor_id: i64,
    user_id: i64,
) -> Result<()> {
    if !can_admin_org(db, org_id, actor_id).await? {
        anyhow::bail!("forbidden: requires organization admin");
    }
    remove_org_member_unchecked(db, org_id, user_id).await
}

/// Remove a member without an authorization check.
pub(crate) async fn remove_org_member_unchecked(
    db: &DatabaseConnection,
    org_id: i64,
    user_id: i64,
) -> Result<()> {
    org_ops::remove_org_member(db, org_id, user_id).await?;
    crate::repo::service::invalidate_perm_cache_all();
    Ok(())
}

/// List organization members.
pub async fn list_org_members(
    db: &DatabaseConnection,
    org_id: i64,
) -> Result<Vec<rg_db::entities::organization_member::Model>> {
    org_ops::list_org_members(db, org_id).await
}

/// Check if user is a member of the org.
pub async fn is_org_member(db: &DatabaseConnection, org_id: i64, user_id: i64) -> Result<bool> {
    org_ops::is_org_member(db, org_id, user_id).await
}

/// Find a specific org member.
pub async fn find_org_member(
    db: &DatabaseConnection,
    org_id: i64,
    user_id: i64,
) -> Result<Option<rg_db::entities::organization_member::Model>> {
    org_ops::find_org_member(db, org_id, user_id).await
}

/// Check if a user is a member of a team.
pub async fn is_team_member(db: &DatabaseConnection, team_id: i64, user_id: i64) -> Result<bool> {
    org_ops::is_team_member(db, team_id, user_id).await
}

// ── Authorization ────────────────────────────────────────────

/// Whether `actor_id` may administer `org_id`.
///
/// This is the org-scoped counterpart of `repo::service::can_admin_repo`: the
/// org creator (`owner_id`), an org member whose role is `owner` or `admin`,
/// or a member of an admin team. Team and membership mutations MUST go through
/// this check — the org/team service functions themselves take no actor and so
/// cannot enforce it.
pub async fn can_admin_org(
    db: &DatabaseConnection,
    org_id: i64,
    actor_id: i64,
) -> Result<bool> {
    let Some(org) = org_ops::get_org(db, org_id).await? else {
        return Ok(false);
    };
    if org.owner_id == actor_id {
        return Ok(true);
    }
    if let Some(member) = org_ops::find_org_member(db, org_id, actor_id).await? {
        if matches!(member.role.as_str(), "owner" | "admin") {
            return Ok(true);
        }
    }
    org_ops::is_member_of_admin_team(db, org_id, actor_id).await
}

/// Whether `actor_id` may read the org and its teams/members.
///
/// Public organizations are readable by anyone (including anonymous callers);
/// private organizations are visible only to members. `actor_id` is `None`
/// for unauthenticated requests.
pub async fn can_read_org(
    db: &DatabaseConnection,
    org_id: i64,
    actor_id: Option<i64>,
) -> Result<bool> {
    let Some(org) = org_ops::get_org(db, org_id).await? else {
        return Ok(false);
    };
    if org.visibility != "private" {
        return Ok(true);
    }
    match actor_id {
        Some(uid) => org_ops::is_org_member(db, org_id, uid).await,
        None => Ok(false),
    }
}

/// Resolve an org by name and authorize `actor_id` to read it.
///
/// Used by the org/team read endpoints, which must not leak private-org team
/// and member lists to anonymous or non-member callers.
pub async fn require_org_read(
    db: &DatabaseConnection,
    org_name: &str,
    actor_id: Option<i64>,
) -> Result<rg_db::entities::organization::Model> {
    let org = org_ops::get_org_by_name(db, org_name)
        .await?
        .ok_or_else(|| anyhow::anyhow!("organization not found"))?;
    if !can_read_org(db, org.id, actor_id).await? {
        anyhow::bail!("forbidden: organization is private");
    }
    Ok(org)
}

/// Resolve an org by name and authorize `actor_id` to administer it.
///
/// Returns the org model on success so callers do not repeat the lookup.
pub async fn require_org_admin(
    db: &DatabaseConnection,
    org_name: &str,
    actor_id: i64,
) -> Result<rg_db::entities::organization::Model> {
    let org = org_ops::get_org_by_name(db, org_name)
        .await?
        .ok_or_else(|| anyhow::anyhow!("organization not found"))?;
    if !can_admin_org(db, org.id, actor_id).await? {
        anyhow::bail!("forbidden: requires organization admin");
    }
    Ok(org)
}

// ── Team service ─────────────────────────────────────────────

/// Create a team. `actor_id` must be an org owner/admin.
pub async fn create_team(
    db: &DatabaseConnection,
    org_id: i64,
    actor_id: i64,
    name: &str,
    description: Option<&str>,
    permission: &str,
) -> Result<rg_db::entities::team::Model> {
    if !can_admin_org(db, org_id, actor_id).await? {
        anyhow::bail!("forbidden: requires organization admin");
    }
    create_team_unchecked(db, org_id, name, description, permission).await
}

/// Create a team without an authorization check.
pub(crate) async fn create_team_unchecked(
    db: &DatabaseConnection,
    org_id: i64,
    name: &str,
    description: Option<&str>,
    permission: &str,
) -> Result<rg_db::entities::team::Model> {
    if permission != "read" && permission != "write" && permission != "admin" {
        anyhow::bail!("permission must be 'read', 'write', or 'admin'");
    }
    org_ops::create_team(db, org_id, name, description, permission).await
}

/// List teams for an organization.
pub async fn list_org_teams(
    db: &DatabaseConnection,
    org_id: i64,
) -> Result<Vec<rg_db::entities::team::Model>> {
    org_ops::list_org_teams(db, org_id).await
}

/// Get a team by ID.
pub async fn get_team(
    db: &DatabaseConnection,
    id: i64,
) -> Result<Option<rg_db::entities::team::Model>> {
    org_ops::get_team(db, id).await
}

/// Delete a team. `actor_id` must be an org owner/admin of the team's org.
pub async fn delete_team(db: &DatabaseConnection, id: i64, actor_id: i64) -> Result<()> {
    let team = org_ops::get_team(db, id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("team not found"))?;
    if !can_admin_org(db, team.org_id, actor_id).await? {
        anyhow::bail!("forbidden: requires organization admin");
    }
    delete_team_unchecked(db, id).await
}

/// Delete a team without an authorization check.
pub(crate) async fn delete_team_unchecked(db: &DatabaseConnection, id: i64) -> Result<()> {
    org_ops::delete_team(db, id).await?;
    crate::repo::service::invalidate_perm_cache_all();
    Ok(())
}

/// Add a member to a team. `actor_id` must be an org owner/admin of the team's org.
pub async fn add_team_member(
    db: &DatabaseConnection,
    team_id: i64,
    actor_id: i64,
    user_id: i64,
    role: &str,
) -> Result<rg_db::entities::team_member::Model> {
    let team = org_ops::get_team(db, team_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("team not found"))?;
    if !can_admin_org(db, team.org_id, actor_id).await? {
        anyhow::bail!("forbidden: requires organization admin");
    }
    add_team_member_unchecked(db, team_id, user_id, role).await
}

/// Add a member to a team without an authorization check.
pub(crate) async fn add_team_member_unchecked(
    db: &DatabaseConnection,
    team_id: i64,
    user_id: i64,
    role: &str,
) -> Result<rg_db::entities::team_member::Model> {
    if role != "member" && role != "maintainer" {
        anyhow::bail!("role must be 'member' or 'maintainer'");
    }
    let member = org_ops::add_team_member(db, team_id, user_id, role).await?;
    // Team membership can grant repo access — flush perm cache.
    crate::repo::service::invalidate_perm_cache_all();
    Ok(member)
}

/// Remove a member from a team. `actor_id` must be an org owner/admin of the team's org.
pub async fn remove_team_member(
    db: &DatabaseConnection,
    team_id: i64,
    actor_id: i64,
    user_id: i64,
) -> Result<()> {
    let team = org_ops::get_team(db, team_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("team not found"))?;
    if !can_admin_org(db, team.org_id, actor_id).await? {
        anyhow::bail!("forbidden: requires organization admin");
    }
    remove_team_member_unchecked(db, team_id, user_id).await
}

/// Remove a member from a team without an authorization check.
pub(crate) async fn remove_team_member_unchecked(
    db: &DatabaseConnection,
    team_id: i64,
    user_id: i64,
) -> Result<()> {
    org_ops::remove_team_member(db, team_id, user_id).await?;
    crate::repo::service::invalidate_perm_cache_all();
    Ok(())
}

/// List team members.
pub async fn list_team_members(
    db: &DatabaseConnection,
    team_id: i64,
) -> Result<Vec<rg_db::entities::team_member::Model>> {
    org_ops::list_team_members(db, team_id).await
}
