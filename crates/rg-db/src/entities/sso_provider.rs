//! SsoProvider entity — maps to `sso_providers` table.
use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "sso_providers")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    /// Display name shown on login page
    pub name: String,
    /// URL-friendly slug: "github", "google", "oidc-corp"
    #[sea_orm(unique)]
    pub slug: String,
    /// "oauth2" | "oidc" | "ldap"
    pub provider_type: String,
    /// OAuth2 client ID (encrypted at rest)
    pub client_id: Option<String>,
    /// OAuth2 client secret (AES-GCM encrypted)
    pub client_secret_enc: Option<String>,
    /// OIDC discovery document URL (for OIDC providers)
    pub discovery_url: Option<String>,
    /// Space-separated scopes
    pub scopes: Option<String>,
    /// LDAP host (for LDAP providers)
    pub ldap_host: Option<String>,
    pub ldap_port: Option<i32>,
    /// LDAP bind DN (for binding)
    pub ldap_bind_dn: Option<String>,
    /// LDAP bind password (AES-GCM encrypted)
    pub ldap_bind_password_enc: Option<String>,
    /// LDAP base DN for user search
    pub ldap_base_dn: Option<String>,
    /// LDAP user filter template, e.g. "(uid={username})"
    pub ldap_user_filter: Option<String>,
    /// Group search base. Group sync is enabled only when this, the filter and
    /// the target org are all set (see `m20261123_000003_add_ldap_group_sync`).
    pub ldap_group_base_dn: Option<String>,
    /// Group filter, e.g. `(objectClass=groupOfNames)`.
    pub ldap_group_filter: Option<String>,
    /// Organization whose teams the group names refer to.
    pub ldap_group_team_org_id: Option<i64>,
    pub enabled: bool,
    /// Icon URL for login button (optional)
    pub icon_url: Option<String>,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
