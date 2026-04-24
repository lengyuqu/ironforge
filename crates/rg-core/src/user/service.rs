//! User service — business logic for user registration, login, and profile management.

use anyhow::{bail, Context, Result};
use chrono::Utc;
use sea_orm::{ActiveValue::Set, DatabaseConnection};

use rg_db::{
    entities::user::ActiveModel as UserActiveModel,
    ops::user_ops,
};

use crate::auth::{jwt, password};

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
