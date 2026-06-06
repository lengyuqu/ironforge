//! User service — business logic for user registration, login, profile, and admin management.

use anyhow::{bail, Context, Result};
use chrono::Utc;
use sea_orm::{ActiveValue::Set, DatabaseConnection};

use rg_db::{
    entities::user::ActiveModel as UserActiveModel,
    ops::user_ops,
};

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
        .map_or(Ok(()), |_| bail!("username '{}' is already taken", username))?;

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

    if !username.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
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
    if plaintext_password.len() < 8 {
        bail!("password must be at least 8 characters");
    }

    let password_hash = password::hash_password(plaintext_password)
        .context("failed to hash password")?;

    let now = Utc::now();
    let model = UserActiveModel {
        username: Set(username.to_string()),
        email: Set(email.to_string()),
        password_hash: Set(password_hash),
        is_admin: Set(false),
        is_active: Set(true),
        created_at: Set(now),
        updated_at: Set(now),
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
    let updated = user_ops::update_by_id(
        db,
        target_user_id,
        display_name,
        bio,
        is_admin,
        is_active,
    )
    .await?;
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
