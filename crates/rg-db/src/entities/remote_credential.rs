//! Remote credential entity — maps to the `remote_credentials` table.
//!
//! One row is one token for one host, owned by a user or an organization. See
//! `migrations::m20261123_000001_create_remote_credentials` for why the owner
//! is a `(kind, id)` pair and why the host is the lookup key.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// Owner discriminators. Anything else is rejected at the service boundary.
pub const OWNER_KIND_USER: &str = "user";
pub const OWNER_KIND_ORG: &str = "org";

/// How the token is presented to the remote host.
///
/// `basic` covers GitHub / GitLab / Gitea / Bitbucket and any other host that
/// takes HTTP basic auth with a personal access token as the password.
/// `bearer` covers hosts that want `Authorization: Bearer <token>` instead —
/// the same token, a different header shape.
pub const AUTH_SCHEME_BASIC: &str = "basic";
pub const AUTH_SCHEME_BEARER: &str = "bearer";

/// Basic-auth user used when the row leaves `username` empty.
///
/// GitHub requires a non-empty user and ignores its value; GitLab accepts
/// `oauth2` or anything else; Gitea ignores it. `x-access-token` is the spelling
/// every one of them accepts, so it is the safe default rather than a guess at
/// the end user's account name.
pub const DEFAULT_BASIC_USERNAME: &str = "x-access-token";

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "remote_credentials")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    /// [`OWNER_KIND_USER`] or [`OWNER_KIND_ORG`].
    pub owner_kind: String,
    /// `users.id` or `organizations.id`, selected by `owner_kind`.
    pub owner_id: i64,
    /// Normalised host key (see `rg_core::net_guard::url_host_key`) —
    /// lowercase, default port dropped. Never a URL, never a path.
    pub host: String,
    /// Basic-auth user; `None` means [`DEFAULT_BASIC_USERNAME`].
    pub username: Option<String>,
    /// AES-256-GCM ciphertext. Never serialised to an API response.
    #[serde(skip_serializing)]
    pub token_encrypted: String,
    /// [`AUTH_SCHEME_BASIC`] or [`AUTH_SCHEME_BEARER`].
    pub auth_scheme: String,
    /// Human-readable name shown in the UI.
    pub label: Option<String>,
    /// Set the first time a git call actually uses this token.
    pub last_used_at: Option<DateTimeUtc>,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
