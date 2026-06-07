//! LDAP authentication service.
//! Two-step: bind with service account, search user DN, rebind with user DN + password.

use anyhow::{Context, Result};
use ldap3::{LdapConnAsync, LdapConnSettings, Scope, SearchEntry};

#[derive(Debug, Clone)]
pub struct LdapConfig {
    pub host: String,
    pub port: u16,
    pub use_tls: bool,
    pub bind_dn: String,
    pub bind_password: String,
    pub base_dn: String,
    pub user_filter: String,
}

#[derive(Debug, Clone)]
pub struct LdapUser {
    pub username: String,
    pub email: Option<String>,
    pub display_name: Option<String>,
    pub dn: String,
    pub uid: Option<String>,
}

pub async fn authenticate(
    config: &LdapConfig,
    username: &str,
    password: &str,
) -> Result<LdapUser> {
    let url = if config.use_tls {
        format!("ldaps://{}:{}", config.host, config.port)
    } else {
        format!("ldap://{}:{}", config.host, config.port)
    };

    let settings = LdapConnSettings::new().set_no_tls_verify(true);
    let (conn, mut ldap) = LdapConnAsync::with_settings(settings, &url)
        .await
        .context("failed to connect to LDAP")?;

    ldap3::drive!(conn);

    // Step 1: bind with service account
    ldap.simple_bind(&config.bind_dn, &config.bind_password)
        .await
        .map_err(|e| anyhow::anyhow!("LDAP service bind failed: {}", e))?
        .success()
        .map_err(|e| anyhow::anyhow!("LDAP service bind rejected: {:?}", e))?;

    // Step 2: search for user
    let filter = config.user_filter.replace("{username}", username);
    let (results, _) = ldap
        .search(
            &config.base_dn,
            Scope::Subtree,
            &filter,
            vec!["uid", "mail", "displayName", "cn", "givenName", "sn"],
        )
        .await
        .map_err(|e| anyhow::anyhow!("LDAP search failed: {}", e))?
        .success()
        .map_err(|e| anyhow::anyhow!("LDAP search rejected: {:?}", e))?;

    if results.is_empty() {
        anyhow::bail!("user '{}' not found in LDAP directory", username);
    }

    // ldap3 v0.11: SearchResultEntry has (dn, attrs) pattern
    let entry = SearchEntry::construct(results[0].clone());
    let user_dn = entry.dn.clone();

    let email = entry.attrs.get("mail").and_then(|v| v.first()).cloned();
    let uid = entry.attrs.get("uid").and_then(|v| v.first()).cloned();
    let display_name = entry
        .attrs
        .get("displayName")
        .and_then(|v| v.first())
        .cloned()
        .or_else(|| {
            let first = entry.attrs.get("givenName").and_then(|v| v.first());
            let last = entry.attrs.get("sn").and_then(|v| v.first());
            match (first, last) {
                (Some(f), Some(l)) => Some(format!("{} {}", f, l)),
                (Some(n), None) | (None, Some(n)) => Some(n.clone()),
                _ => None,
            }
        });

    // Step 3: unbind service
    ldap.unbind().await.ok();

    // Step 4: rebind with user DN + password to verify
    let settings2 = LdapConnSettings::new().set_no_tls_verify(true);
    let (conn2, mut ldap2) = LdapConnAsync::with_settings(settings2, &url)
        .await
        .context("failed to reconnect to LDAP for user auth")?;

    ldap3::drive!(conn2);

    let bind_result = ldap2
        .simple_bind(&user_dn, password)
        .await
        .map_err(|e| anyhow::anyhow!("LDAP user bind failed: {}", e))?;

    ldap2.unbind().await.ok();

    // Check bind success via the result code
    if bind_result.rc != 0 {
        anyhow::bail!("invalid LDAP credentials");
    }

    Ok(LdapUser {
        username: username.to_string(),
        email,
        display_name,
        dn: user_dn,
        uid,
    })
}

pub async fn test_connection(config: &LdapConfig) -> Result<()> {
    let url = if config.use_tls {
        format!("ldaps://{}:{}", config.host, config.port)
    } else {
        format!("ldap://{}:{}", config.host, config.port)
    };

    let settings = LdapConnSettings::new().set_no_tls_verify(true);
    let (conn, mut ldap) = LdapConnAsync::with_settings(settings, &url)
        .await
        .context("failed to connect to LDAP")?;

    ldap3::drive!(conn);

    ldap.simple_bind(&config.bind_dn, &config.bind_password)
        .await
        .map_err(|e| anyhow::anyhow!("LDAP bind failed: {}", e))?
        .success()
        .map_err(|e| anyhow::anyhow!("LDAP bind rejected: {:?}", e))?;

    ldap.unbind().await.ok();
    Ok(())
}
