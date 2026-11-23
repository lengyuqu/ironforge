//! SSO provider operations.
use sea_orm::*;

use crate::entities::sso_provider;
pub use crate::entities::sso_provider::Entity;

/// List all configured SSO providers (admin use).
pub async fn list_all(db: &DatabaseConnection) -> Result<Vec<sso_provider::Model>, DbErr> {
    Entity::find()
        .order_by_asc(sso_provider::Column::Id)
        .all(db)
        .await
}

/// List only enabled providers (for login page).
pub async fn list_enabled(db: &DatabaseConnection) -> Result<Vec<sso_provider::Model>, DbErr> {
    Entity::find()
        .filter(sso_provider::Column::Enabled.eq(true))
        .order_by_asc(sso_provider::Column::Id)
        .all(db)
        .await
}

/// Find provider by slug (e.g. "github", "google").
pub async fn find_by_slug(
    db: &DatabaseConnection,
    slug: &str,
) -> Result<Option<sso_provider::Model>, DbErr> {
    Entity::find()
        .filter(sso_provider::Column::Slug.eq(slug))
        .one(db)
        .await
}

/// Find provider by id.
pub async fn find_by_id(
    db: &DatabaseConnection,
    id: i64,
) -> Result<Option<sso_provider::Model>, DbErr> {
    Entity::find_by_id(id).one(db).await
}

/// Upsert a provider from admin settings.
#[allow(clippy::too_many_arguments)]
pub async fn upsert(
    db: &DatabaseConnection,
    id: Option<i64>,
    name: &str,
    slug: &str,
    provider_type: &str,
    client_id: Option<&str>,
    client_secret_enc: Option<&str>,
    discovery_url: Option<&str>,
    scopes: Option<&str>,
    ldap_host: Option<&str>,
    ldap_port: Option<i32>,
    ldap_bind_dn: Option<&str>,
    ldap_bind_password_enc: Option<&str>,
    ldap_base_dn: Option<&str>,
    ldap_user_filter: Option<&str>,
    enabled: bool,
    icon_url: Option<&str>,
) -> Result<sso_provider::Model, DbErr> {
    let now = chrono::Utc::now();
    if let Some(existing_id) = id {
        let some = Entity::find_by_id(existing_id).one(db).await?;
        if let Some(m) = some {
            let mut am: sso_provider::ActiveModel = m.into();
            am.name = Set(name.to_string());
            am.slug = Set(slug.to_string());
            am.provider_type = Set(provider_type.to_string());
            am.client_id = Set(client_id.map(str::to_string));
            am.client_secret_enc = Set(client_secret_enc.map(str::to_string));
            am.discovery_url = Set(discovery_url.map(str::to_string));
            am.scopes = Set(scopes.map(str::to_string));
            am.ldap_host = Set(ldap_host.map(str::to_string));
            am.ldap_port = Set(ldap_port);
            am.ldap_bind_dn = Set(ldap_bind_dn.map(str::to_string));
            am.ldap_bind_password_enc = Set(ldap_bind_password_enc.map(str::to_string));
            am.ldap_base_dn = Set(ldap_base_dn.map(str::to_string));
            am.ldap_user_filter = Set(ldap_user_filter.map(str::to_string));
            am.enabled = Set(enabled);
            am.icon_url = Set(icon_url.map(str::to_string));
            am.updated_at = Set(now);
            return am.update(db).await;
        }
    }

    let am = sso_provider::ActiveModel {
        id: NotSet,
        name: Set(name.to_string()),
        slug: Set(slug.to_string()),
        provider_type: Set(provider_type.to_string()),
        client_id: Set(client_id.map(str::to_string)),
        client_secret_enc: Set(client_secret_enc.map(str::to_string)),
        discovery_url: Set(discovery_url.map(str::to_string)),
        scopes: Set(scopes.map(str::to_string)),
        ldap_host: Set(ldap_host.map(str::to_string)),
        ldap_port: Set(ldap_port),
        ldap_bind_dn: Set(ldap_bind_dn.map(str::to_string)),
        ldap_bind_password_enc: Set(ldap_bind_password_enc.map(str::to_string)),
        ldap_base_dn: Set(ldap_base_dn.map(str::to_string)),
        ldap_user_filter: Set(ldap_user_filter.map(str::to_string)),
        // Group sync is a second, optional configuration; creation never sets
        // it, so an edit to an existing provider cannot clear it either.
        ldap_group_base_dn: Set(None),
        ldap_group_filter: Set(None),
        ldap_group_team_org_id: Set(None),
        enabled: Set(enabled),
        icon_url: Set(icon_url.map(str::to_string)),
        created_at: Set(now),
        updated_at: Set(now),
    };
    am.insert(db).await
}

/// Delete a provider by id.
/// Set or clear the LDAP group → team configuration of a provider.
///
/// All three values move together: passing `None` for all of them disables
/// group sync, and the endpoint above refuses a partial configuration. Keeping
/// this out of [`upsert`] means a provider edit (name, host, bind password)
/// can never silently drop the group settings.
pub async fn set_ldap_group_sync(
    db: &DatabaseConnection,
    provider_id: i64,
    base_dn: Option<&str>,
    filter: Option<&str>,
    team_org_id: Option<i64>,
) -> Result<sso_provider::Model, DbErr> {
    let existing = Entity::find_by_id(provider_id).one(db).await?;
    let Some(existing) = existing else {
        return Err(DbErr::RecordNotFound("sso provider".to_string()));
    };
    let mut am: sso_provider::ActiveModel = existing.into();
    am.ldap_group_base_dn = Set(base_dn.map(str::to_string));
    am.ldap_group_filter = Set(filter.map(str::to_string));
    am.ldap_group_team_org_id = Set(team_org_id);
    am.updated_at = Set(chrono::Utc::now());
    am.update(db).await
}

pub async fn delete_by_id(db: &DatabaseConnection, id: i64) -> Result<(), DbErr> {
    Entity::delete_by_id(id).exec(db).await?;
    Ok(())
}
