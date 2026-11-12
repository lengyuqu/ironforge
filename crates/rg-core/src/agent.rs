//! Agent principals: accounts that act for a person.
//!
//! An agent is a row in `users` with `kind = "agent"` and an `owner_user_id`.
//! Keeping it in `users` means everything keyed on a user id (authorship,
//! collaborators, tokens, audit) works for agents unchanged. What makes an
//! agent different is enforced in three places:
//!
//! 1. **No human entrance.** No password (`password_hash` is not a hash),
//!    `auth_provider = "agent"` (no login path accepts it), a synthetic e-mail
//!    under `.invalid` that no identity provider can assert. The only
//!    credential is a personal access token issued by the owner, and that
//!    token may only carry repository scopes ([`validate_agent_token_scopes`]),
//!    so the account, organization-management and administrator APIs stay out
//!    of reach.
//! 2. **Capped by the owner.** [`owner_cap`] is consulted by the repository
//!    permission checks: an agent can do on a repository what it was granted
//!    *and* its owner can currently do, never administer it, and nothing once
//!    the owner is gone or disabled.
//! 3. **Owns nothing.** [`ensure_human`] guards repository and organization
//!    creation, forks and transfers.
//!
//! Not covered here (later phases): review rules for agents, routing an agent's
//! notifications to its owner, an `on_behalf_of` audit column, organization-owned
//! agents.

use rg_db::entities::user::Model as User;
use rg_db::ops::user_ops;
use sea_orm::DatabaseConnection;

use crate::error::{CoreError, CoreResult};

pub const KIND_HUMAN: &str = "human";
pub const KIND_AGENT: &str = "agent";

/// `users.auth_provider` of an agent. No login path accepts it.
pub const AGENT_AUTH_PROVIDER: &str = "agent";

/// Agent usernames are `<owner>-agent-<name>`. People and organizations may not
/// use a name containing this, so a username says who an agent belongs to.
pub const AGENT_USERNAME_INFIX: &str = "-agent-";

/// Most agents one person may own (active or not).
pub const MAX_AGENTS_PER_OWNER: u64 = 50;

const MAX_AGENT_NAME_LEN: usize = 20;
const MAX_DESCRIPTION_LEN: usize = 500;

pub fn is_agent(user: &User) -> bool {
    user.kind == KIND_AGENT
}

/// Whether `username` is in the namespace reserved for agents.
pub fn is_reserved_username(username: &str) -> bool {
    username.to_ascii_lowercase().contains(AGENT_USERNAME_INFIX)
}

/// The short name an owner gives an agent: 1–20 ASCII letters, digits, `_` or
/// `-`, starting with a letter or digit, and not itself containing the infix.
pub fn validate_agent_name(name: &str) -> CoreResult<()> {
    if name.is_empty() || name.len() > MAX_AGENT_NAME_LEN {
        return Err(CoreError::invalid_input(
            "agent name must be between 1 and 20 characters",
        ));
    }
    if !name
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphanumeric())
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err(CoreError::invalid_input(
            "agent name must start with a letter or digit and contain only letters, digits, hyphens and underscores",
        ));
    }
    if is_reserved_username(&format!("x{name}x")) {
        return Err(CoreError::invalid_input(
            "agent name must not contain '-agent-'",
        ));
    }
    Ok(())
}

pub fn agent_username(owner_username: &str, name: &str) -> String {
    format!("{owner_username}{AGENT_USERNAME_INFIX}{name}")
}

/// What an owner sees about one of their agents.
#[derive(Debug, serde::Serialize)]
pub struct AgentInfo {
    pub id: i64,
    pub username: String,
    pub kind: &'static str,
    pub owner_user_id: i64,
    pub description: Option<String>,
    pub is_active: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl From<User> for AgentInfo {
    fn from(user: User) -> Self {
        Self {
            id: user.id,
            username: user.username,
            kind: KIND_AGENT,
            owner_user_id: user.owner_user_id.unwrap_or_default(),
            description: user.agent_description,
            is_active: user.is_active,
            created_at: user.created_at,
        }
    }
}

fn validate_description(description: Option<&str>) -> CoreResult<()> {
    if description.is_some_and(|d| d.chars().count() > MAX_DESCRIPTION_LEN) {
        return Err(CoreError::invalid_input(
            "agent description must be at most 500 characters",
        ));
    }
    Ok(())
}

/// The account, required to be an active person. Used wherever only a person
/// may act (owning things, managing agents).
pub async fn ensure_human(db: &DatabaseConnection, user_id: i64) -> CoreResult<User> {
    let user = user_ops::find_by_id(db, user_id)
        .await?
        .ok_or_else(|| CoreError::not_found("user not found"))?;
    if is_agent(&user) {
        return Err(CoreError::forbidden(
            "an agent account may not do this; ask its owner",
        ));
    }
    Ok(user)
}

/// Create an agent owned by `owner_id`.
pub async fn create_agent(
    db: &DatabaseConnection,
    owner_id: i64,
    name: &str,
    description: Option<&str>,
) -> CoreResult<AgentInfo> {
    let owner = ensure_human(db, owner_id).await?;
    if !owner.is_active {
        return Err(CoreError::forbidden("account is disabled"));
    }
    validate_agent_name(name)?;
    validate_description(description)?;
    if user_ops::count_agents_by_owner(db, owner.id).await? >= MAX_AGENTS_PER_OWNER {
        return Err(CoreError::conflict(format!(
            "an account may own at most {MAX_AGENTS_PER_OWNER} agents"
        )));
    }
    let username = agent_username(&owner.username, name);
    // Owners are resolved "user first, then organization": an agent named like
    // an organization that predates the reserved namespace would shadow it.
    if rg_db::ops::org_ops::get_org_by_name(db, &username)
        .await?
        .is_some()
    {
        return Err(CoreError::conflict(format!(
            "the name '{username}' is taken by an organization"
        )));
    }
    // `.invalid` is reserved (RFC 2606): nothing can be delivered to it and no
    // identity provider has a reason to assert it.
    let email = format!("agent-{}@agents.invalid", uuid::Uuid::new_v4());
    match user_ops::create_agent(db, &username, &email, owner.id, description).await {
        Ok(agent) => Ok(agent.into()),
        Err(error) if crate::user::service::is_unique_violation(&error) => Err(
            CoreError::conflict(format!("agent '{username}' already exists")),
        ),
        Err(error) => Err(error.into()),
    }
}

pub async fn list_agents(db: &DatabaseConnection, owner_id: i64) -> CoreResult<Vec<AgentInfo>> {
    Ok(user_ops::list_agents_by_owner(db, owner_id)
        .await?
        .into_iter()
        .map(Into::into)
        .collect())
}

/// The agent `username`, if `owner_id` owns it. Anything else — unknown name, a
/// person, someone else's agent — is the same "not found".
pub async fn find_owned_agent(
    db: &DatabaseConnection,
    owner_id: i64,
    username: &str,
) -> CoreResult<User> {
    match user_ops::find_by_username(db, username).await? {
        Some(user) if is_agent(&user) && user.owner_user_id == Some(owner_id) => Ok(user),
        _ => Err(CoreError::not_found("agent not found")),
    }
}

/// Change an agent's description and/or active flag.
///
/// Deactivating also bumps `token_version`, so sessions already minted from the
/// agent's tokens stop working at once; the tokens themselves are refused while
/// the account is inactive and work again if it is reactivated.
pub async fn update_agent(
    db: &DatabaseConnection,
    owner_id: i64,
    username: &str,
    description: Option<Option<String>>,
    is_active: Option<bool>,
) -> CoreResult<AgentInfo> {
    let agent = find_owned_agent(db, owner_id, username).await?;
    if let Some(description) = &description {
        validate_description(description.as_deref())?;
    }
    let updated = user_ops::update_agent(db, agent.id, description, is_active).await?;
    if is_active == Some(false) && agent.is_active {
        user_ops::bump_token_version(db, agent.id).await?;
    }
    Ok(updated.into())
}

/// An agent's token may only carry repository scopes: `user` and `admin` would
/// let it manage the account it runs as, or the instance.
pub fn validate_agent_token_scopes(normalized_scopes: &str) -> CoreResult<()> {
    if normalized_scopes
        .split(',')
        .any(|scope| !scope.trim().starts_with("repo"))
    {
        return Err(CoreError::invalid_input(
            "an agent token may only have repository scopes (repo, repo:write, repo:collab, repo:read)",
        ));
    }
    Ok(())
}

/// How an account's repository permissions are limited by who it acts for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnerCap {
    /// A person: no cap.
    None,
    /// An agent: at most what this (active, human) owner can do.
    Owner(i64),
    /// An agent whose owner is missing, disabled or not a person: nothing.
    Denied,
}

/// The cap that applies to `actor_id`. An unknown account is `Denied`.
pub async fn owner_cap(db: &DatabaseConnection, actor_id: i64) -> CoreResult<OwnerCap> {
    let Some(actor) = user_ops::find_by_id(db, actor_id).await? else {
        return Ok(OwnerCap::Denied);
    };
    if !is_agent(&actor) {
        return Ok(OwnerCap::None);
    }
    let Some(owner_id) = actor.owner_user_id else {
        return Ok(OwnerCap::Denied);
    };
    match user_ops::find_by_id(db, owner_id).await? {
        Some(owner) if owner.is_active && !is_agent(&owner) => Ok(OwnerCap::Owner(owner.id)),
        _ => Ok(OwnerCap::Denied),
    }
}

// ---------------------------------------------------------------------------
// T24: who to tell on behalf of an account (separate block, append-only)
// ---------------------------------------------------------------------------

/// The person accountable for `user_id`, i.e. who a notification meant for this
/// account should reach: the owner of an agent, or the account itself for a
/// person. `None` when there is nobody to tell: the account does not exist, is
/// disabled, or is an agent whose owner is missing, disabled or not a person.
///
/// Same rule as [`owner_cap`] (an agent is held to its owner), so the two
/// cannot disagree about who stands behind an agent. The result is never an
/// agent, so it is safe to use as a recipient of anything that leaves the
/// instance.
pub async fn accountable_user(db: &DatabaseConnection, user_id: i64) -> CoreResult<Option<i64>> {
    let Some(user) = user_ops::find_by_id(db, user_id).await? else {
        return Ok(None);
    };
    if !is_agent(&user) {
        return Ok(user.is_active.then_some(user.id));
    }
    match owner_cap(db, user_id).await? {
        OwnerCap::Owner(owner_id) => Ok(Some(owner_id)),
        OwnerCap::None | OwnerCap::Denied => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_names_are_short_plain_and_outside_the_reserved_infix() {
        for ok in ["ci", "reviewer-1", "a", "x_y", "0day"] {
            assert!(validate_agent_name(ok).is_ok(), "{ok}");
        }
        for bad in [
            "",
            "-x",
            "_x",
            "a b",
            "a/b",
            "名字",
            &"x".repeat(21),
            "a-agent-b",
        ] {
            assert!(validate_agent_name(bad).is_err(), "{bad}");
        }
        // A name may not complete the infix at either edge of the username.
        assert!(validate_agent_name("agent-").is_ok());
    }

    #[test]
    fn the_reserved_namespace_is_case_insensitive() {
        assert!(is_reserved_username("bob-agent-ci"));
        assert!(is_reserved_username("Bob-AGENT-ci"));
        assert!(!is_reserved_username("agent"));
        assert!(!is_reserved_username("secret-agent"));
        assert_eq!(agent_username("bob", "ci"), "bob-agent-ci");
    }

    #[test]
    fn agent_tokens_take_repository_scopes_only() {
        assert!(validate_agent_token_scopes("repo:write,repo:read").is_ok());
        assert!(validate_agent_token_scopes("repo").is_ok());
        assert!(validate_agent_token_scopes("repo,user").is_err());
        assert!(validate_agent_token_scopes("admin").is_err());
    }
}
