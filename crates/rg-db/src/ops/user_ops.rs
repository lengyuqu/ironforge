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

/// List all users with optional pagination.
pub async fn list_users(
    db: &DatabaseConnection,
    page: u64,
    per_page: u64,
) -> Result<(Vec<User>, i64)> {
    let paginator = UserEntity::find()
        .order_by_desc(user::Column::CreatedAt)
        .paginate(db, per_page);

    let total = paginator.num_items().await.context("db: count users")?;
    let users = paginator.fetch_page(page).await.context("db: list users")?;

    Ok((users, total as i64))
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

///
/// CRITICAL: SeaORM single-row update (踩坑经验 #11)
///
/// To update a single row, you MUST first `find_by_id()` to get the model,
/// then convert it into an `ActiveModel`, modify fields, and call `update()`.
///
/// CORRECT pattern (used here):
///   let model = UserEntity::find_by_id(id)
///       .one(db).await?
///       .ok_or_else(|| anyhow::anyhow!("not found"))?;
///   let mut active: ActiveModel = model.into();
///   active.field = Set(value);
///   active.update(db).await
///
/// WRONG patterns:
///   ActiveModel { id: Set(id), ... }.update(db)  // MAY skip optimistic lock
///   Entity::update_many().col(...).filter(...).exec(db)  // batch only
pub async fn update_by_id(
    db: &DatabaseConnection,
    id: i64,
    display_name: Option<Option<String>>,
    bio: Option<Option<String>>,
    is_admin: Option<bool>,
    is_active: Option<bool>,
) -> Result<User> {
    let model = UserEntity::find_by_id(id)
        .one(db)
        .await
        .context("db: find user for update")?
        .ok_or_else(|| anyhow::anyhow!("user {} not found", id))?;

    let mut active: ActiveModel = model.into();

    if let Some(dn) = display_name {
        active.display_name = Set(dn);
    }
    if let Some(b) = bio {
        active.bio = Set(b);
    }
    if let Some(admin) = is_admin {
        active.is_admin = Set(admin);
    }
    if let Some(active_flag) = is_active {
        active.is_active = Set(active_flag);
    }

    active
        .update(db)
        .await
        .context("db: update user by admin")
}

/// Delete a user by ID.
pub async fn delete_by_id(db: &DatabaseConnection, id: i64) -> Result<()> {
    let model = UserEntity::find_by_id(id)
        .one(db)
        .await
        .context("db: find user for delete")?
        .ok_or_else(|| anyhow::anyhow!("user {} not found", id))?;

    model.delete(db).await.context("db: delete user")?;
    Ok(())
}
