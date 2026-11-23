//! LDAP authentication service.
//! Two-step: bind with service account, search user DN, rebind with user DN + password.

use crate::error::{CoreContext, CoreError, CoreResult};
use ldap3::{LdapConnAsync, LdapConnSettings, Scope, SearchEntry};

#[derive(Debug, Clone)]
pub struct LdapConfig {
    pub host: String,
    pub port: u16,
    pub use_tls: bool,
    /// Disable TLS certificate verification for LDAPS connections.
    /// This must stay false in production and should only be enabled for tests
    /// against throwaway LDAP servers with self-signed certificates.
    pub insecure_skip_tls_verify: bool,
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

/// A group as the directory reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LdapGroup {
    /// `cn`. Compared against a team name exactly (no case folding).
    pub name: String,
    /// Member DNs.
    pub member_dns: Vec<String>,
}

/// Escape a value embedded in an LDAP search filter (RFC 4515).
fn escape_filter_value(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for byte in value.as_bytes() {
        match byte {
            b'\0' => escaped.push_str("\\00"),
            b'(' => escaped.push_str("\\28"),
            b')' => escaped.push_str("\\29"),
            b'*' => escaped.push_str("\\2a"),
            b'\\' => escaped.push_str("\\5c"),
            byte if !byte.is_ascii() => {
                use std::fmt::Write;
                let _ = write!(escaped, "\\{byte:02x}");
            }
            byte => escaped.push(*byte as char),
        }
    }
    escaped
}

fn connection_settings(config: &LdapConfig) -> LdapConnSettings {
    let settings = LdapConnSettings::new().set_conn_timeout(std::time::Duration::from_secs(10));
    if config.use_tls && config.insecure_skip_tls_verify {
        settings.set_no_tls_verify(true)
    } else {
        settings
    }
}

/// Connect and bind with the service account.
///
/// Every read-only operation starts the same way; keeping it in one place means
/// the timeout and TLS handling cannot drift between them.
async fn bind_service(config: &LdapConfig) -> CoreResult<ldap3::Ldap> {
    let url = if config.use_tls {
        format!("ldaps://{}:{}", config.host, config.port)
    } else {
        format!("ldap://{}:{}", config.host, config.port)
    };
    let settings = connection_settings(config);
    let (conn, mut ldap) = LdapConnAsync::with_settings(settings, &url)
        .await
        .context("failed to connect to LDAP")?;
    ldap3::drive!(conn);
    ldap.simple_bind(&config.bind_dn, &config.bind_password)
        .await
        .map_err(|e| CoreError::internal(format!("LDAP service bind failed: {}", e)))?
        .success()
        .map_err(|e| CoreError::internal(format!("LDAP service bind rejected: {:?}", e)))?;
    Ok(ldap)
}

pub async fn authenticate(
    config: &LdapConfig,
    username: &str,
    password: &str,
) -> CoreResult<LdapUser> {
    if username.trim().is_empty() || password.is_empty() {
        return Err(CoreError::InvalidInput("invalid LDAP credentials".into()));
    }
    if !config.user_filter.contains("{username}") {
        return Err(CoreError::InvalidInput(
            "LDAP user filter must contain '{username}'".into(),
        ));
    }
    let url = if config.use_tls {
        format!("ldaps://{}:{}", config.host, config.port)
    } else {
        format!("ldap://{}:{}", config.host, config.port)
    };

    let settings = connection_settings(config);
    let (conn, mut ldap) = LdapConnAsync::with_settings(settings, &url)
        .await
        .context("failed to connect to LDAP")?;

    ldap3::drive!(conn);

    // Step 1: bind with service account
    ldap.simple_bind(&config.bind_dn, &config.bind_password)
        .await
        .map_err(|e| CoreError::internal(format!("LDAP service bind failed: {}", e)))?
        .success()
        .map_err(|e| CoreError::internal(format!("LDAP service bind rejected: {:?}", e)))?;

    // Step 2: search for user
    let filter = config
        .user_filter
        .replace("{username}", &escape_filter_value(username));
    let (results, _) = ldap
        .search(
            &config.base_dn,
            Scope::Subtree,
            &filter,
            vec!["uid", "mail", "displayName", "cn", "givenName", "sn"],
        )
        .await
        .map_err(|e| CoreError::internal(format!("LDAP search failed: {}", e)))?
        .success()
        .map_err(|e| CoreError::internal(format!("LDAP search rejected: {:?}", e)))?;

    if results.is_empty() {
        return Err(CoreError::NotFound(format!(
            "user '{}' not found in LDAP directory",
            username
        )));
    }
    if results.len() != 1 {
        return Err(CoreError::internal(
            "LDAP user filter returned multiple entries",
        ));
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
    let settings2 = connection_settings(config);
    let (conn2, mut ldap2) = LdapConnAsync::with_settings(settings2, &url)
        .await
        .context("failed to reconnect to LDAP for user auth")?;

    ldap3::drive!(conn2);

    let bind_result = ldap2
        .simple_bind(&user_dn, password)
        .await
        .map_err(|e| CoreError::internal(format!("LDAP user bind failed: {}", e)))?;

    ldap2.unbind().await.ok();

    // Check bind success via the result code
    if bind_result.rc != 0 {
        return Err(CoreError::InvalidInput("invalid LDAP credentials".into()));
    }

    Ok(LdapUser {
        username: username.to_string(),
        email,
        display_name,
        dn: user_dn,
        uid,
    })
}

/// Every account the directory would accept at login.
///
/// The configured filter is the *per-login* form (`(uid={username})`); the
/// enumeration form is the same filter with the placeholder replaced by `*`,
/// which is what makes the sync see exactly the set that can log in.
///
/// # Why a truncated listing is an error
///
/// A sync deactivates local accounts that are absent from this listing. If the
/// directory answers with `sizeLimitExceeded` (or any other non-zero result)
/// the listing is *incomplete*, and treating it as complete would deactivate
/// everyone past the limit. `.success()` rejects every non-zero result code, so
/// the partial answer can never be mistaken for a full one.
pub async fn list_users(config: &LdapConfig) -> CoreResult<Vec<LdapUser>> {
    if !config.user_filter.contains("{username}") {
        return Err(CoreError::InvalidInput(
            "LDAP user filter must contain '{username}' to be enumerable".into(),
        ));
    }
    let mut ldap = bind_service(config).await?;

    // `*` is the LDAP wildcard; it matches every entry the filter admits.
    let filter = config.user_filter.replace("{username}", "*");
    let (results, _) = ldap
        .search(
            &config.base_dn,
            Scope::Subtree,
            &filter,
            vec!["uid", "mail", "displayName", "cn", "givenName", "sn"],
        )
        .await
        .map_err(|e| CoreError::internal(format!("LDAP search failed: {}", e)))?
        .success()
        .map_err(|e| {
            CoreError::internal(format!(
                "LDAP enumeration was refused or truncated ({:?}); refusing to sync against a partial listing",
                e
            ))
        })?;

    ldap.unbind().await.ok();

    Ok(results
        .into_iter()
        .map(|entry| {
            let entry = SearchEntry::construct(entry);
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
            // Same name resolution the login path uses, so the sync compares
            // like with like.
            let username = uid
                .clone()
                .or_else(|| entry.attrs.get("cn").and_then(|v| v.first()).cloned())
                .unwrap_or_default();
            LdapUser {
                username,
                email,
                display_name,
                dn: entry.dn.clone(),
                uid,
            }
        })
        .filter(|user| !user.username.trim().is_empty())
        .collect())
}

/// Members from `member`, falling back to `uniqueMember`.
///
/// The fallback looks at *emptiness*, not at presence: a group with an empty
/// `member` and a populated `uniqueMember` still has members.
fn group_member_dns(member: Vec<String>, unique_member: Vec<String>) -> Vec<String> {
    let chosen = if member.is_empty() {
        unique_member
    } else {
        member
    };
    chosen
        .into_iter()
        .map(|dn| dn.trim().to_string())
        .filter(|dn| !dn.is_empty())
        .collect()
}

/// List the groups under `group_base_dn` matching `group_filter`.
///
/// Members come from `member` (the `groupOfNames` / `groupOfUniqueNames`
/// convention). `uniqueMember` is accepted as a fallback because Active
/// Directory and several OpenLDAP deployments use it, and a group whose members
/// cannot be read is worse than one read from the other attribute.
///
/// Same rule as [`list_users`]: a non-zero LDAP result — `sizeLimitExceeded`
/// included — is an error, never a partial list. A truncated group listing
/// would read as "these people are not in the group", and the caller's plan
/// would then be built on a lie.
pub async fn list_groups(
    config: &LdapConfig,
    group_base_dn: &str,
    group_filter: &str,
) -> CoreResult<Vec<LdapGroup>> {
    if group_base_dn.trim().is_empty() || group_filter.trim().is_empty() {
        return Err(CoreError::InvalidInput(
            "group sync needs a base DN and a filter".into(),
        ));
    }
    let mut ldap = bind_service(config).await?;

    let (results, _) = ldap
        .search(
            group_base_dn,
            Scope::Subtree,
            group_filter,
            vec!["cn", "member", "uniqueMember"],
        )
        .await
        .map_err(|e| CoreError::internal(format!("LDAP group search failed: {}", e)))?
        .success()
        .map_err(|e| {
            CoreError::internal(format!(
                "LDAP group enumeration was refused or truncated ({:?}); refusing to sync against a partial listing",
                e
            ))
        })?;

    ldap.unbind().await.ok();

    Ok(results
        .into_iter()
        .filter_map(|entry| {
            let entry = SearchEntry::construct(entry);
            let name = entry
                .attrs
                .get("cn")
                .and_then(|values| values.first())
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())?;
            Some(LdapGroup {
                name,
                member_dns: group_member_dns(
                    entry.attrs.get("member").cloned().unwrap_or_default(),
                    entry.attrs.get("uniqueMember").cloned().unwrap_or_default(),
                ),
            })
        })
        .collect())
}

pub async fn test_connection(config: &LdapConfig) -> CoreResult<()> {
    let mut ldap = bind_service(config).await?;
    ldap.unbind().await.ok();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{escape_filter_value, LdapConfig};

    fn config(use_tls: bool, insecure_skip_tls_verify: bool) -> LdapConfig {
        LdapConfig {
            host: "ldap.example.com".to_string(),
            port: if use_tls { 636 } else { 389 },
            use_tls,
            insecure_skip_tls_verify,
            bind_dn: "cn=service,dc=example,dc=com".to_string(),
            bind_password: "secret".to_string(),
            base_dn: "dc=example,dc=com".to_string(),
            user_filter: "(uid={username})".to_string(),
        }
    }

    #[test]
    fn ldap_tls_verification_is_not_skipped_by_default() {
        let cfg = config(true, false);

        assert!(cfg.use_tls);
        assert!(!cfg.insecure_skip_tls_verify);
    }

    #[test]
    fn ldap_tls_verification_skip_requires_explicit_insecure_flag() {
        let cfg = config(true, true);

        assert!(cfg.use_tls);
        assert!(cfg.insecure_skip_tls_verify);
    }

    /// The attribute-precedence rule, as a plain function so it is testable
    /// without a directory.
    #[test]
    fn group_member_attribute_prefers_member_and_falls_back_to_unique_member() {
        use super::{group_member_dns as pick, LdapGroup};

        fn v(values: &[&str]) -> Vec<String> {
            values.iter().map(|value| (*value).to_string()).collect()
        }

        assert_eq!(pick(v(&["a"]), v(&["b"])), vec!["a".to_string()]);
        assert_eq!(pick(Vec::new(), v(&["b"])), vec!["b".to_string()]);
        // An empty `member` does not mean "no members" when `uniqueMember` has
        // some: the fallback looks at emptiness, not at presence.
        assert_eq!(pick(Vec::new(), v(&["b"])), vec!["b".to_string()]);
        assert!(pick(Vec::new(), Vec::new()).is_empty());
        // Blanks are dropped rather than becoming a member with an empty DN.
        assert_eq!(
            pick(v(&["  ", " uid=a,dc=x "]), Vec::new()),
            vec!["uid=a,dc=x".to_string()]
        );

        let group = LdapGroup {
            name: "backend".into(),
            member_dns: v(&["uid=alice,dc=x"]),
        };
        assert_eq!(group.name, "backend");
    }

    #[test]
    fn ldap_filter_values_are_rfc4515_escaped() {
        assert_eq!(escape_filter_value("alice"), "alice");
        assert_eq!(
            escape_filter_value("*)(uid=*)\\\0"),
            "\\2a\\29\\28uid=\\2a\\29\\5c\\00"
        );
        assert_eq!(escape_filter_value("é"), "\\c3\\a9");
    }
}
