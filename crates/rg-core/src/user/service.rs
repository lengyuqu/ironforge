//! User service — business logic for user registration, login, profile, admin management,
//! and password reset.

use chrono::{Duration, Utc};
use sea_orm::{ActiveValue::Set, DatabaseConnection};

use rg_db::{entities::user::ActiveModel as UserActiveModel, ops::user_ops};

use crate::auth::{jwt, password};
use crate::error::{CoreError, CoreResult};

/// Message carried by the [`CoreError::Forbidden`] that signals a failed
/// credential check. The HTTP login handler maps it (and every other
/// non-internal login failure) to the same generic 401 so the response never
/// reveals whether the account exists.
pub const INVALID_CREDENTIALS: &str = "invalid credentials";
/// Message carried by the [`CoreError::Forbidden`] that signals a locked account.
pub const ACCOUNT_LOCKED: &str = "account is temporarily locked";

/// Whether `error` (or anything in its cause chain) is a database
/// unique-constraint violation. Lets handlers turn the loser of a concurrent
/// insert race into a 409 instead of an opaque 500.
pub fn is_unique_violation(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        cause
            .downcast_ref::<sea_orm::DbErr>()
            .and_then(sea_orm::DbErr::sql_err)
            .is_some_and(|e| matches!(e, sea_orm::SqlErr::UniqueConstraintViolation(_)))
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoginMethod {
    Password,
    Ldap,
}

pub struct LoginOutcome {
    pub response: AuthResponse,
    pub method: LoginMethod,
}

/// A paginated list of users with total count.
pub struct PaginatedUsers {
    pub users: Vec<UserInfo>,
    pub total: i64,
}

/// Public user information (safe to return to clients).
#[derive(Debug, serde::Serialize)]
pub struct UserInfo {
    pub id: i64,
    pub username: String,
    pub email: String,
    pub display_name: Option<String>,
    pub avatar_url: Option<String>,
    pub bio: Option<String>,
    pub is_admin: bool,
    pub is_active: bool,
    pub auth_provider: String,
    pub last_login_at: Option<chrono::DateTime<Utc>>,
    pub login_attempts: i32,
    pub locked_until: Option<chrono::DateTime<Utc>>,
    pub created_at: chrono::DateTime<Utc>,
}

/// Response after a successful login or registration.
#[derive(Debug, serde::Serialize)]
pub struct AuthResponse {
    pub token: String,
    pub user_id: i64,
    pub username: String,
}

/// Validate a username according to IronForge rules.
///
/// Rules:
/// - Length: 3–30 characters
/// - Must start with an alphanumeric character
/// - May only contain alphanumeric characters, hyphens, and underscores
/// - Must not contain path traversal sequences (`..` or `/`)
///
/// Returns `Ok(())` if valid, `Err` with a descriptive message otherwise.
pub fn validate_username(username: &str) -> CoreResult<()> {
    if username.len() < 3 || username.len() > 30 {
        return Err(CoreError::invalid_input(
            "username must be between 3 and 30 characters",
        ));
    }

    let first_char = username.chars().next().unwrap(); // len >= 3, safe to unwrap
    if !first_char.is_ascii_alphanumeric() {
        return Err(CoreError::invalid_input(
            "username must start with an alphanumeric character",
        ));
    }

    if !username
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err(CoreError::invalid_input(
            "username must only contain alphanumeric characters, hyphens, and underscores",
        ));
    }

    // Path traversal prevention
    if username.contains("..") || username.contains('/') {
        return Err(CoreError::invalid_input(
            "username contains invalid characters",
        ));
    }

    Ok(())
}

/// Register a new user.
///
/// Returns an `AuthResponse` with a JWT token.
pub async fn register(
    db: &DatabaseConnection,
    username: &str,
    email: &str,
    plaintext_password: &str,
    jwt_secret: &str,
) -> CoreResult<AuthResponse> {
    // Validate inputs
    if user_ops::find_by_username(db, username).await?.is_some() {
        return Err(CoreError::conflict(format!(
            "username '{}' is already taken",
            username
        )));
    }

    if user_ops::find_by_email(db, email).await?.is_some() {
        return Err(CoreError::conflict(format!(
            "email '{}' is already registered",
            email
        )));
    }

    // ── Username validation ──────────────────────────────────────
    validate_username(username)?;

    // ── Email validation ─────────────────────────────────────────
    match email.split_once('@') {
        Some((local, domain)) if !local.is_empty() && !domain.is_empty() => {}
        _ => {
            return Err(CoreError::invalid_input(
                "email must contain '@' with a non-empty local and domain part",
            ))
        }
    }

    // ── Password validation ──────────────────────────────────────
    let password_validator = password::PasswordValidator::standard();
    password_validator
        .validate_with_username(plaintext_password, username)
        .map_err(|e| CoreError::invalid_input(e.to_string()))?;

    let password_hash = password::hash_password(plaintext_password)?;

    let now = Utc::now();
    let model = UserActiveModel {
        username: Set(username.to_string()),
        email: Set(email.to_string()),
        password_hash: Set(password_hash),
        is_admin: Set(false),
        is_active: Set(true),
        created_at: Set(now),
        updated_at: Set(now),
        deleted_at: Set(None),
        ..Default::default()
    };

    // The pre-checks above are not atomic with this insert: the loser of a
    // concurrent registration trips the unique index and must see a conflict,
    // not a 500.
    let user = match user_ops::create(db, model).await {
        Ok(user) => user,
        Err(error) if is_unique_violation(&error) => {
            return Err(CoreError::conflict(
                "username or email is already registered",
            ))
        }
        Err(error) => return Err(error.into()),
    };
    let token = jwt::generate_token(user.id, &user.username, jwt_secret, 7, user.token_version)?;

    Ok(AuthResponse {
        token,
        user_id: user.id,
        username: user.username,
    })
}

/// Authenticate a user by username/password. Returns a JWT on success.
///
/// Credential failures are [`CoreError::Forbidden`] (see [`INVALID_CREDENTIALS`]);
/// only genuine infrastructure failures are [`CoreError::Internal`].
pub async fn login(
    db: &DatabaseConnection,
    username_or_email: &str,
    plaintext_password: &str,
    jwt_secret: &str,
) -> CoreResult<AuthResponse> {
    // Try username first, then email
    let user = if username_or_email.contains('@') {
        user_ops::find_by_email(db, username_or_email).await?
    } else {
        user_ops::find_by_username(db, username_or_email).await?
    };

    let user = match user {
        Some(u) => u,
        None => return Err(CoreError::forbidden(INVALID_CREDENTIALS)),
    };

    if !user.is_active {
        return Err(CoreError::forbidden("account is disabled"));
    }

    // A stored hash that cannot be parsed is a data problem, but surfacing it
    // as a different status than "wrong password" would reveal that the
    // account exists. Log it for operators and answer like any other failure.
    let verified = match password::verify_password(plaintext_password, &user.password_hash) {
        Ok(verified) => verified,
        Err(error) => {
            tracing::error!(user_id = user.id, %error, "stored password hash could not be verified");
            false
        }
    };
    if !verified {
        return Err(CoreError::forbidden(INVALID_CREDENTIALS));
    }

    let token = jwt::generate_token(user.id, &user.username, jwt_secret, 7, user.token_version)?;

    Ok(AuthResponse {
        token,
        user_id: user.id,
        username: user.username,
    })
}

/// Authenticate through the account's configured provider. Unknown users may
/// be provisioned only after a successful bind against an enabled LDAP source.
pub async fn login_with_configured_auth(
    db: &DatabaseConnection,
    username_or_email: &str,
    plaintext_password: &str,
    jwt_secret: &str,
) -> CoreResult<LoginOutcome> {
    let existing = find_login_user(db, username_or_email).await?;
    if existing.as_ref().is_some_and(|user| {
        user.locked_until
            .is_some_and(|locked_until| locked_until > Utc::now())
    }) {
        return Err(CoreError::forbidden(ACCOUNT_LOCKED));
    }
    match existing.as_ref().map(|user| user.auth_provider.as_str()) {
        Some("local") => Ok(LoginOutcome {
            response: login(db, username_or_email, plaintext_password, jwt_secret).await?,
            method: LoginMethod::Password,
        }),
        Some("ldap") | None => {
            login_via_ldap(
                db,
                existing,
                username_or_email,
                plaintext_password,
                jwt_secret,
            )
            .await
        }
        Some(_) => Err(CoreError::forbidden(INVALID_CREDENTIALS)),
    }
}

async fn find_login_user(
    db: &DatabaseConnection,
    username_or_email: &str,
) -> CoreResult<Option<rg_db::entities::user::Model>> {
    let user = if username_or_email.contains('@') {
        user_ops::find_by_email(db, username_or_email).await?
    } else {
        user_ops::find_by_username(db, username_or_email).await?
    };
    Ok(user)
}

async fn login_via_ldap(
    db: &DatabaseConnection,
    existing: Option<rg_db::entities::user::Model>,
    username_or_email: &str,
    plaintext_password: &str,
    jwt_secret: &str,
) -> CoreResult<LoginOutcome> {
    if plaintext_password.is_empty() {
        return Err(CoreError::forbidden(INVALID_CREDENTIALS));
    }
    if existing.as_ref().is_some_and(|user| !user.is_active) {
        return Err(CoreError::forbidden("account is disabled"));
    }

    let lookup = existing
        .as_ref()
        .and_then(|user| user.ldap_uid.as_deref())
        .unwrap_or(username_or_email);
    let mut providers: Vec<_> = rg_db::ops::sso_provider_ops::list_enabled(db)
        .await?
        .into_iter()
        .filter(|provider| provider.provider_type == "ldap")
        .collect();
    if let Some(provider_id) = existing.as_ref().and_then(|user| user.ldap_provider_id) {
        providers.retain(|provider| provider.id == provider_id);
    } else if existing.is_some() && providers.len() != 1 {
        return Err(CoreError::forbidden(INVALID_CREDENTIALS));
    }
    for provider in providers {
        let config = match ldap_config_from_provider(&provider, jwt_secret) {
            Ok(config) => config,
            Err(error) => {
                tracing::warn!(provider_id = provider.id, %error, "ignoring invalid LDAP provider configuration");
                continue;
            }
        };
        let ldap_user = match tokio::time::timeout(
            std::time::Duration::from_secs(10),
            crate::auth::ldap::authenticate(&config, lookup, plaintext_password),
        )
        .await
        {
            Ok(Ok(user)) => user,
            Ok(Err(error)) => {
                tracing::warn!(provider_id = provider.id, %error, "LDAP authentication attempt failed");
                continue;
            }
            Err(_) => {
                tracing::warn!(
                    provider_id = provider.id,
                    "LDAP authentication attempt timed out"
                );
                continue;
            }
        };

        let user = match resolve_ldap_identity(db, existing.as_ref(), provider.id, ldap_user).await
        {
            Ok(user) => user,
            // A conflicting or incomplete directory entry is a credential-style
            // failure (never reveal why); a storage failure is a real 500.
            Err(error @ CoreError::Internal(_)) => return Err(error),
            Err(error) => {
                tracing::warn!(provider_id = provider.id, %error, "LDAP identity could not be linked");
                return Err(CoreError::forbidden(INVALID_CREDENTIALS));
            }
        };
        let token = jwt::generate_token(user.id, &user.username, jwt_secret, 7, user.token_version)?;
        return Ok(LoginOutcome {
            response: AuthResponse {
                token,
                user_id: user.id,
                username: user.username,
            },
            method: LoginMethod::Ldap,
        });
    }
    Err(CoreError::forbidden(INVALID_CREDENTIALS))
}

fn ldap_config_from_provider(
    provider: &rg_db::entities::sso_provider::Model,
    jwt_secret: &str,
) -> CoreResult<crate::auth::ldap::LdapConfig> {
    let raw_host = provider
        .ldap_host
        .as_deref()
        .map(str::trim)
        .filter(|host| !host.is_empty())
        .ok_or_else(|| CoreError::invalid_input("LDAP host is missing"))?;
    let (host, explicit_tls) = raw_host
        .strip_prefix("ldaps://")
        .map(|host| (host, Some(true)))
        .or_else(|| {
            raw_host
                .strip_prefix("ldap://")
                .map(|host| (host, Some(false)))
        })
        .unwrap_or((raw_host, None));
    if host.is_empty() || host.contains('/') {
        return Err(CoreError::invalid_input("LDAP host is invalid"));
    }
    let use_tls = explicit_tls.unwrap_or(match provider.ldap_port {
        Some(port) => port == 636,
        None => true,
    });
    let port = provider
        .ldap_port
        .unwrap_or(if use_tls { 636 } else { 389 });
    let port = u16::try_from(port)
        .ok()
        .filter(|port| *port != 0)
        .ok_or_else(|| CoreError::invalid_input("LDAP port is invalid"))?;
    let bind_password_enc = provider
        .ldap_bind_password_enc
        .as_deref()
        .ok_or_else(|| CoreError::invalid_input("LDAP bind password is missing"))?;
    let key = crate::auth::encryption::derive_key(jwt_secret);
    // Failing to decrypt a stored secret is a server-side problem (rotated or
    // corrupted key), not something the caller can fix by changing input.
    let bind_password = crate::auth::encryption::decrypt(bind_password_enc, &key)
        .map_err(|e| CoreError::internal(format!("LDAP bind password could not be decrypted: {e}")))?;
    let required = |value: Option<&str>, name: &str| -> CoreResult<String> {
        value
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .ok_or_else(|| CoreError::invalid_input(format!("LDAP {name} is missing")))
    };
    let user_filter = provider
        .ldap_user_filter
        .as_deref()
        .unwrap_or("(uid={username})")
        .trim()
        .to_string();
    if !user_filter.contains("{username}") {
        return Err(CoreError::invalid_input(
            "LDAP user filter must contain '{username}'",
        ));
    }
    Ok(crate::auth::ldap::LdapConfig {
        host: host.to_string(),
        port,
        use_tls,
        insecure_skip_tls_verify: false,
        bind_dn: required(provider.ldap_bind_dn.as_deref(), "bind DN")?,
        bind_password,
        base_dn: required(provider.ldap_base_dn.as_deref(), "base DN")?,
        user_filter,
    })
}

pub async fn test_ldap_provider_connection(
    provider: &rg_db::entities::sso_provider::Model,
    jwt_secret: &str,
) -> CoreResult<()> {
    if provider.provider_type != "ldap" {
        return Err(CoreError::invalid_input("provider is not LDAP"));
    }
    let config = ldap_config_from_provider(provider, jwt_secret)?;
    tokio::time::timeout(
        std::time::Duration::from_secs(10),
        crate::auth::ldap::test_connection(&config),
    )
    .await
    .map_err(|_| CoreError::invalid_input("LDAP connection test timed out"))??;
    Ok(())
}

async fn resolve_ldap_identity(
    db: &DatabaseConnection,
    existing: Option<&rg_db::entities::user::Model>,
    ldap_provider_id: i64,
    ldap_user: crate::auth::ldap::LdapUser,
) -> CoreResult<rg_db::entities::user::Model> {
    let username = ldap_user
        .uid
        .as_deref()
        .unwrap_or(&ldap_user.username)
        .trim();
    validate_username(username)?;

    if let Some(user) = existing {
        if user.auth_provider != "ldap"
            || user.username != username
            || user
                .ldap_provider_id
                .is_some_and(|provider_id| provider_id != ldap_provider_id)
        {
            return Err(CoreError::conflict(
                "LDAP identity conflicts with an existing account",
            ));
        }
        let synced = user_ops::sync_ldap_identity(
            db,
            user.id,
            ldap_provider_id,
            ldap_user.display_name.as_deref(),
            &ldap_user.dn,
            ldap_user.uid.as_deref(),
        )
        .await?;
        return Ok(synced);
    }

    if user_ops::find_by_username(db, username).await?.is_some() {
        return Err(CoreError::conflict(
            "LDAP identity conflicts with an existing account",
        ));
    }
    let email = ldap_user
        .email
        .as_deref()
        .map(str::trim)
        .filter(|email| valid_email(email))
        .ok_or_else(|| {
            CoreError::invalid_input("LDAP account does not have a valid email address")
        })?;
    if user_ops::find_by_email(db, email).await?.is_some() {
        return Err(CoreError::conflict(
            "LDAP identity conflicts with an existing account",
        ));
    }
    let created = user_ops::create_ldap_user(
        db,
        ldap_provider_id,
        username,
        email,
        ldap_user.display_name.as_deref(),
        &ldap_user.dn,
        ldap_user.uid.as_deref(),
    )
    .await?;
    Ok(created)
}

fn valid_email(email: &str) -> bool {
    matches!(email.split_once('@'), Some((local, domain)) if !local.is_empty() && !domain.is_empty())
}

// ── Admin user management ───────────────────────────────────────

impl From<rg_db::entities::user::Model> for UserInfo {
    fn from(u: rg_db::entities::user::Model) -> Self {
        Self {
            id: u.id,
            username: u.username,
            email: u.email,
            display_name: u.display_name,
            avatar_url: u.avatar_url,
            bio: u.bio,
            is_admin: u.is_admin,
            is_active: u.is_active,
            auth_provider: u.auth_provider,
            last_login_at: u.last_login_at,
            login_attempts: u.login_attempts,
            locked_until: u.locked_until,
            created_at: u.created_at,
        }
    }
}

/// List all users with pagination (admin only).
pub async fn list_users_admin(
    db: &DatabaseConnection,
    page: u64,
    per_page: u64,
) -> CoreResult<PaginatedUsers> {
    let (users, total) = user_ops::list_users(db, page, per_page).await?;
    Ok(PaginatedUsers {
        users: users.into_iter().map(Into::into).collect(),
        total,
    })
}

/// Update any user's profile fields (admin only).
pub async fn update_user_admin(
    db: &DatabaseConnection,
    target_user_id: i64,
    display_name: Option<Option<String>>,
    bio: Option<Option<String>>,
    is_admin: Option<bool>,
    is_active: Option<bool>,
) -> CoreResult<UserInfo> {
    // `user_ops::update_by_id` reports a missing row as a plain error, which is
    // indistinguishable from a storage failure; look the user up first so a
    // missing target is a 404.
    if user_ops::find_by_id(db, target_user_id).await?.is_none() {
        return Err(CoreError::not_found("user not found"));
    }
    let updated =
        user_ops::update_by_id(db, target_user_id, display_name, bio, is_admin, is_active).await?;
    Ok(updated.into())
}

/// Delete a user (admin only).
pub async fn delete_user(db: &DatabaseConnection, user_id: i64) -> CoreResult<()> {
    if user_ops::find_by_id(db, user_id).await?.is_none() {
        return Err(CoreError::not_found("user not found"));
    }
    user_ops::delete_by_id(db, user_id).await?;
    Ok(())
}

/// Get a single user by ID (admin view).
pub async fn get_user_by_id(db: &DatabaseConnection, user_id: i64) -> CoreResult<Option<UserInfo>> {
    let user = user_ops::find_by_id(db, user_id).await?;
    Ok(user.map(Into::into))
}

// ── Password reset ──────────────────────────────────────────────

/// Initiate a password reset. Generates a token and sends an email.
/// Silently succeeds even if the email is not found (to prevent user enumeration).
/// H-5: All code paths perform similar work to prevent timing-based email enumeration.
pub async fn forgot_password(
    db: &DatabaseConnection,
    email: &str,
    smtp_config: Option<&crate::email::SmtpConfig>,
    base_url: &str,
) -> CoreResult<()> {
    let user = match user_ops::find_by_email(db, email).await? {
        Some(u) => u,
        None => {
            // H-5: Perform dummy token generation + delay to normalize timing
            let _ = uuid::Uuid::new_v4();
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            return Ok(());
        }
    };

    // Only local users can reset via email (LDAP/OAuth users use their provider)
    if user.auth_provider != "local" {
        // H-5: Same delay as the "not found" path to prevent timing-based enumeration
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        return Ok(());
    }

    // Invalidate old unused tokens
    rg_db::ops::password_reset_token_ops::invalidate_user_tokens(db, user.id).await?;

    // Generate a random token
    let raw_token = uuid::Uuid::new_v4().to_string();
    use sha2::Digest;
    let token_hash = hex::encode(sha2::Sha256::digest(raw_token.as_bytes()));

    // Token valid for 15 minutes
    let expires_at = Utc::now() + Duration::minutes(15);

    rg_db::ops::password_reset_token_ops::create(db, user.id, &token_hash, expires_at).await?;

    // Build reset link
    let reset_url = format!(
        "{}/reset-password?token={}",
        base_url.trim_end_matches('/'),
        raw_token
    );

    // Send email
    if let Some(smtp) = smtp_config {
        let subject = "Reset your IronForge password";
        let message = format!(
            "We received a request to reset the password for your IronForge account ({}). \
             Click the button below to set a new password. This link expires in 15 minutes.",
            user.username
        );
        if let Err(e) = crate::email::send_html_notification(
            smtp,
            &user.email,
            subject,
            &message,
            Some(&reset_url),
        )
        .await
        {
            // QUEUE-001: durable retry instead of dropping the email.
            let queue = crate::background_jobs::BackgroundJobQueue::new(db.clone());
            let payload = serde_json::json!({
                "to": user.email,
                "subject": subject,
                "message": message,
                "action_url": reset_url,
            });
            if let Err(retry_err) = queue
                .enqueue(crate::background_jobs::TASK_EMAIL_SEND, &payload)
                .await
            {
                tracing::error!(
                    error = %retry_err,
                    "failed to enqueue password-reset email retry"
                );
            }
            tracing::warn!(error = %e, "password reset email send failed; retry scheduled");
        }
    }

    tracing::info!(user_id = user.id, "password reset requested");

    Ok(())
}

/// Reset a password using a valid reset token.
pub async fn reset_password(
    db: &DatabaseConnection,
    raw_token: &str,
    new_password: &str,
    jwt_secret: &str,
) -> CoreResult<AuthResponse> {
    use sha2::Digest;
    let token_hash = hex::encode(sha2::Sha256::digest(raw_token.as_bytes()));

    let token_record = rg_db::ops::password_reset_token_ops::find_by_hash(db, &token_hash).await?;
    let token = match token_record {
        Some(t) if !t.used && t.expires_at > Utc::now() => t,
        _ => {
            return Err(CoreError::invalid_input(
                "invalid or expired reset token",
            ))
        }
    };

    // Validate new password. A token whose user has since been deleted is just
    // as unusable as an expired one; do not reveal the difference.
    let user = user_ops::find_by_id(db, token.user_id)
        .await?
        .ok_or_else(|| CoreError::invalid_input("invalid or expired reset token"))?;

    let password_validator = password::PasswordValidator::standard();
    password_validator
        .validate_with_username(new_password, &user.username)
        .map_err(|e| CoreError::invalid_input(e.to_string()))?;

    let new_hash = password::hash_password(new_password)?;

    // Update password
    use sea_orm::ActiveModelTrait;
    let mut active: UserActiveModel = user.clone().into();
    active.password_hash = Set(new_hash);
    active.updated_at = Set(Utc::now());
    active.update(db).await?;

    // Mark token as used
    rg_db::ops::password_reset_token_ops::mark_used(db, token.id).await?;

    // Invalidate any other unused tokens for this user
    rg_db::ops::password_reset_token_ops::invalidate_user_tokens(db, user.id).await?;

    // Revoke all previously issued session tokens (#5): the password just
    // changed, so any JWT minted under the old credential must stop working.
    user_ops::bump_token_version(db, user.id).await?;
    let user = user_ops::find_by_id(db, user.id)
        .await?
        .ok_or_else(|| CoreError::internal("user not found after token version bump"))?;

    // Generate new JWT carrying the fresh token_version
    let jwt_token = jwt::generate_token(user.id, &user.username, jwt_secret, 7, user.token_version)?;

    Ok(AuthResponse {
        token: jwt_token,
        user_id: user.id,
        username: user.username,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ldap_provider(secret: &str) -> rg_db::entities::sso_provider::Model {
        let key = crate::auth::encryption::derive_key(secret);
        let now = chrono::Utc::now();
        rg_db::entities::sso_provider::Model {
            id: 1,
            name: "Directory".into(),
            slug: "directory".into(),
            provider_type: "ldap".into(),
            client_id: None,
            client_secret_enc: None,
            discovery_url: None,
            scopes: None,
            ldap_host: Some("ldaps://ldap.example.com".into()),
            ldap_port: None,
            ldap_bind_dn: Some("cn=service,dc=example,dc=com".into()),
            ldap_bind_password_enc: Some(
                crate::auth::encryption::encrypt("bind-secret", &key).unwrap(),
            ),
            ldap_base_dn: Some("dc=example,dc=com".into()),
            ldap_user_filter: Some("(uid={username})".into()),
            enabled: true,
            icon_url: None,
            created_at: now,
            updated_at: now,
        }
    }

    #[test]
    fn builds_fail_closed_tls_ldap_config_from_encrypted_provider() {
        let config = ldap_config_from_provider(&ldap_provider("jwt-secret"), "jwt-secret").unwrap();
        assert_eq!(config.host, "ldap.example.com");
        assert_eq!(config.port, 636);
        assert!(config.use_tls);
        assert!(!config.insecure_skip_tls_verify);
        assert_eq!(config.bind_password, "bind-secret");

        let mut implicit_tls = ldap_provider("jwt-secret");
        implicit_tls.ldap_host = Some("ldap.example.com".into());
        let config = ldap_config_from_provider(&implicit_tls, "jwt-secret").unwrap();
        assert!(config.use_tls);
        assert_eq!(config.port, 636);

        let mut explicit_plaintext = ldap_provider("jwt-secret");
        explicit_plaintext.ldap_host = Some("ldap://ldap.example.com".into());
        let config = ldap_config_from_provider(&explicit_plaintext, "jwt-secret").unwrap();
        assert!(!config.use_tls);
        assert_eq!(config.port, 389);

        let mut invalid = ldap_provider("jwt-secret");
        invalid.ldap_user_filter = Some("(objectClass=person)".into());
        assert!(ldap_config_from_provider(&invalid, "jwt-secret").is_err());
        assert!(ldap_config_from_provider(&ldap_provider("other-secret"), "jwt-secret").is_err());
    }

    async fn memory_db() -> DatabaseConnection {
        let db = rg_db::connect("sqlite::memory:").await.unwrap();
        rg_db::run_migrations(&db).await.unwrap();
        db
    }

    #[tokio::test]
    async fn service_errors_are_classified_not_flattened() {
        let db = memory_db().await;
        assert!(matches!(
            validate_username("a"),
            Err(CoreError::InvalidInput(_))
        ));
        register(&db, "classify", "classify@example.com", "Qz7$wRtm", "secret")
            .await
            .unwrap();

        // duplicates are conflicts, malformed input is invalid input
        assert!(matches!(
            register(&db, "classify", "x@example.com", "Qz7$wRtm", "secret").await,
            Err(CoreError::Conflict(_))
        ));
        assert!(matches!(
            register(&db, "classify2", "classify@example.com", "Qz7$wRtm", "secret").await,
            Err(CoreError::Conflict(_))
        ));
        assert!(matches!(
            register(&db, "classify3", "no-at", "Qz7$wRtm", "secret").await,
            Err(CoreError::InvalidInput(_))
        ));
        assert!(matches!(
            register(&db, "classify3", "c3@example.com", "abc", "secret").await,
            Err(CoreError::InvalidInput(_))
        ));

        // credential failures carry the generic marker, never an internal error
        for (login_name, pw) in [("classify", "Wrong#pass1"), ("ghost", "Qz7$wRtm")] {
            match login(&db, login_name, pw, "secret").await {
                Err(CoreError::Forbidden(m)) => assert_eq!(m, INVALID_CREDENTIALS),
                other => panic!("expected generic credential failure, got {other:?}"),
            }
        }

        // missing targets of admin operations are 404s, not generic failures
        assert!(matches!(
            update_user_admin(&db, 9999, None, None, None, None).await,
            Err(CoreError::NotFound(_))
        ));
        assert!(matches!(
            delete_user(&db, 9999).await,
            Err(CoreError::NotFound(_))
        ));
        assert!(matches!(
            reset_password(&db, "no-such-token", "Zx9#plmQ", "secret").await,
            Err(CoreError::InvalidInput(_))
        ));
    }

    #[tokio::test]
    async fn storage_failures_stay_internal() {
        use sea_orm::ConnectionTrait;
        let db = memory_db().await;
        db.execute_unprepared("ALTER TABLE users RENAME TO users_gone")
            .await
            .unwrap();
        assert!(matches!(
            register(&db, "someone", "someone@example.com", "Qz7$wRtm", "secret").await,
            Err(CoreError::Internal(_))
        ));
        assert!(matches!(
            login(&db, "someone", "Qz7$wRtm", "secret").await,
            Err(CoreError::Internal(_))
        ));
    }

    #[tokio::test]
    async fn real_unique_violation_is_recognised_through_context_layers() {
        let db = memory_db().await;
        let make = || UserActiveModel {
            username: Set("dup".into()),
            email: Set("dup@example.com".into()),
            password_hash: Set("x".into()),
            is_admin: Set(false),
            is_active: Set(true),
            created_at: Set(Utc::now()),
            updated_at: Set(Utc::now()),
            deleted_at: Set(None),
            ..Default::default()
        };
        user_ops::create(&db, make()).await.unwrap();
        // `user_ops::create` wraps the driver error in an anyhow context.
        let error = user_ops::create(&db, make()).await.unwrap_err();
        assert!(is_unique_violation(&error), "{error:#}");
    }

    #[test]
    fn unique_violations_are_recognised_through_context_layers() {
        use sea_orm::{DbErr, RuntimeErr};
        // A non-unique driver error is not a conflict.
        let other = anyhow::Error::new(DbErr::Conn(RuntimeErr::Internal("boom".into())))
            .context("db: create thing");
        assert!(!is_unique_violation(&other));
        assert!(!is_unique_violation(&anyhow::anyhow!("unique")));
    }

    #[tokio::test]
    async fn provisions_and_syncs_ldap_identity_without_a_local_password() {
        let db = rg_db::connect("sqlite::memory:").await.unwrap();
        rg_db::run_migrations(&db).await.unwrap();
        let created = resolve_ldap_identity(
            &db,
            None,
            1,
            crate::auth::ldap::LdapUser {
                username: "alice".into(),
                email: Some("alice@example.com".into()),
                display_name: Some("Alice".into()),
                dn: "uid=alice,dc=example,dc=com".into(),
                uid: Some("alice".into()),
            },
        )
        .await
        .unwrap();
        assert_eq!(created.auth_provider, "ldap");
        assert_eq!(created.ldap_provider_id, Some(1));
        assert!(created.password_hash.is_empty());
        assert!(!created.is_admin);

        let synced = resolve_ldap_identity(
            &db,
            Some(&created),
            1,
            crate::auth::ldap::LdapUser {
                username: "alice".into(),
                email: Some("changed@example.com".into()),
                display_name: Some("Alice Updated".into()),
                dn: "uid=alice,ou=people,dc=example,dc=com".into(),
                uid: Some("alice".into()),
            },
        )
        .await
        .unwrap();
        assert_eq!(synced.id, created.id);
        assert_eq!(synced.email, "alice@example.com");
        assert_eq!(synced.display_name.as_deref(), Some("Alice Updated"));
        assert_eq!(
            synced.ldap_dn.as_deref(),
            Some("uid=alice,ou=people,dc=example,dc=com")
        );
    }
}
