//! Database operations for users.

use anyhow::{Context, Result};
use sea_orm::*;

use crate::entities::user::{self, ActiveModel, Entity as UserEntity, Model as User};

/// Find a user by username (case-insensitive on SQLite).
pub async fn find_by_username(db: &DatabaseConnection, username: &str) -> Result<Option<User>> {
    UserEntity::find()
        .filter(user::Column::Username.eq(username))
        .one(db)
        .await
        .context("db: find user by username")
}

/// Find a user by email.
pub async fn find_by_email(db: &DatabaseConnection, email: &str) -> Result<Option<User>> {
    UserEntity::find()
        .filter(user::Column::Email.eq(email))
        .one(db)
        .await
        .context("db: find user by email")
}

/// Find a user by id.
pub async fn find_by_id(db: &DatabaseConnection, id: i64) -> Result<Option<User>> {
    UserEntity::find_by_id(id)
        .one(db)
        .await
        .context("db: find user by id")
}

/// Create a new user and return the persisted model.
pub async fn create(db: &DatabaseConnection, model: ActiveModel) -> Result<User> {
    model
        .insert(db)
        .await
        .context("db: create user")
}

/// Update a user.
pub async fn update(db: &DatabaseConnection, model: ActiveModel) -> Result<User> {
    model
        .update(db)
        .await
        .context("db: update user")
}
