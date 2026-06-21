//! User service — business logic for user registration, login, profile, admin management,
//! and password reset.

use anyhow::{bail, Context, Result};
use chrono::{Duration, Utc};
use sea_orm::{ActiveValue::Set, DatabaseConnection};

use rg_db::{entities::user::ActiveModel as UserActiveModel, ops::user_ops};

use crate::auth::{jwt, password};

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
    pub created_at: chrono::DateTime<Utc>,
}

/// Response after a successful login or registration.
#[derive(Debug, serde::Serialize)]
pub struct AuthResponse {
    pub token: String,
    pub user_id: i64,
    pub username: String,
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
) -> Result<AuthResponse> {
    // Validate inputs
    rg_db::ops::user_ops::find_by_username(db, username)
        .await?
        .map(|_| ())
        .map_or(Ok(()), |_| {
            bail!("username '{}' is already taken", username)
        })?;

    if user_ops::find_by_email(db, email).await?.is_some() {
        bail!("email '{}' is already registered", email);
    }

    // ── Username validation ──────────────────────────────────────
    if username.len() < 3 || username.len() > 30 {
        bail!("username must be between 3 and 30 characters");
    }

    let first_char = username.chars().next().unwrap(); // len >= 3, safe to unwrap
    if !first_char.is_ascii_alphanumeric() {
        bail!("username must start with an alphanumeric character");
    }

    if !username
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        bail!("username must only contain alphanumeric characters, hyphens, and underscores");
    }

    // Path traversal prevention
    if username.contains("..") || username.contains('/') {
        bail!("username contains invalid characters");
    }

    // ── Email validation ─────────────────────────────────────────
    match email.split_once('@') {
        Some((local, domain)) if !local.is_empty() && !domain.is_empty() => {}
        _ => bail!("email must contain '@' with a non-empty local and domain part"),
    }

    // ── Password validation ──────────────────────────────────────
    let password_validator = password::PasswordValidator::standard();
    password_validator
        .validate_with_username(plaintext_password, username)
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    let password_hash =
        password::hash_password(plaintext_password).context("failed to hash password")?;

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

    let user = user_ops::create(db, model).await?;
    let token = jwt::generate_token(user.id, &user.username, jwt_secret, 7)?;

    Ok(AuthResponse {
        token,
        user_id: user.id,
        username: user.username,
    })
}

/// Authenticate a user by username/password. Returns a JWT on success.
pub async fn login(
    db: &DatabaseConnection,
    username_or_email: &str,
    plaintext_password: &str,
    jwt_secret: &str,
) -> Result<AuthResponse> {
    // Try username first, then email
    let user = if username_or_email.contains('@') {
        user_ops::find_by_email(db, username_or_email).await?
    } else {
        user_ops::find_by_username(db, username_or_email).await?
    };

    let user = match user {
        Some(u) => u,
        None => bail!("invalid credentials"),
    };

    if !user.is_active {
        bail!("account is disabled");
    }

    if !password::verify_password(plaintext_password, &user.password_hash)? {
        bail!("invalid credentials");
    }

    let token = jwt::generate_token(user.id, &user.username, jwt_secret, 7)?;

    Ok(AuthResponse {
        token,
        user_id: user.id,
        username: user.username,
    })
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
            created_at: u.created_at,
        }
    }
}

/// List all users with pagination (admin only).
pub async fn list_users_admin(
    db: &DatabaseConnection,
    page: u64,
    per_page: u64,
) -> Result<PaginatedUsers> {
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
) -> Result<UserInfo> {
    let updated =
        user_ops::update_by_id(db, target_user_id, display_name, bio, is_admin, is_active).await?;
    Ok(updated.into())
}

/// Delete a user (admin only).
pub async fn delete_user(db: &DatabaseConnection, user_id: i64) -> Result<()> {
    user_ops::delete_by_id(db, user_id).await
}

/// Get a single user by ID (admin view).
pub async fn get_user_by_id(db: &DatabaseConnection, user_id: i64) -> Result<Option<UserInfo>> {
    let user = user_ops::find_by_id(db, user_id).await?;
    Ok(user.map(Into::into))
}

// ── Password reset ──────────────────────────────────────────────

/// Initiate a password reset. Generates a token and sends an email.
/// Silently succeeds even if the email is not found (to prevent user enumeration).
pub async fn forgot_password(
    db: &DatabaseConnection,
    email: &str,
    smtp_config: Option<&crate::email::SmtpConfig>,
    base_url: &str,
) -> Result<()> {
    let user = match user_ops::find_by_email(db, email).await? {
        Some(u) => u,
        None => {
            // Silently succeed to prevent email enumeration
            return Ok(());
        }
    };

    // Only local users can reset via email (LDAP/OAuth users use their provider)
    if user.auth_provider != "local" {
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
        let _ = crate::email::send_html_notification(
            smtp,
            &user.email,
            subject,
            &message,
            Some(&reset_url),
        )
        .await;
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
) -> Result<AuthResponse> {
    use sha2::Digest;
    let token_hash = hex::encode(sha2::Sha256::digest(raw_token.as_bytes()));

    let token_record = rg_db::ops::password_reset_token_ops::find_by_hash(db, &token_hash).await?;
    let token = match token_record {
        Some(t) if !t.used && t.expires_at > Utc::now() => t,
        _ => bail!("invalid or expired reset token"),
    };

    // Validate new password
    let user = user_ops::find_by_id(db, token.user_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("user not found"))?;

    let password_validator = password::PasswordValidator::standard();
    password_validator
        .validate_with_username(new_password, &user.username)
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    let new_hash = password::hash_password(new_password).context("failed to hash new password")?;

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

    // Generate new JWT
    let jwt_token = jwt::generate_token(user.id, &user.username, jwt_secret, 7)?;

    Ok(AuthResponse {
        token: jwt_token,
        user_id: user.id,
        username: user.username,
    })
}
