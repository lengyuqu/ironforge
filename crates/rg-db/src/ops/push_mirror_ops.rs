//! Database operations for push mirrors.

use anyhow::{Context, Result};
use sea_orm::*;

use crate::entities::push_mirror::{self, ActiveModel, Entity as PushMirrorEntity, Model};

/// Create a push mirror record.
pub async fn create(db: &DatabaseConnection, model: ActiveModel) -> Result<Model> {
    model.insert(db).await.context("db: create push mirror")
}

/// Find a push mirror by ID.
pub async fn find_by_id(db: &DatabaseConnection, id: i64) -> Result<Option<Model>> {
    PushMirrorEntity::find_by_id(id)
        .one(db)
        .await
        .context("db: find push mirror by id")
}

/// Find the push mirror configured for a repository.
pub async fn find_by_repo_id(db: &DatabaseConnection, repo_id: i64) -> Result<Option<Model>> {
    PushMirrorEntity::find()
        .filter(push_mirror::Column::RepoId.eq(repo_id))
        .one(db)
        .await
        .context("db: find push mirror by repo_id")
}

/// Update a push mirror record.
pub async fn update(db: &DatabaseConnection, model: ActiveModel) -> Result<Model> {
    model.update(db).await.context("db: update push mirror")
}

/// Delete a push mirror by ID.
pub async fn delete_by_id(db: &DatabaseConnection, id: i64) -> Result<()> {
    PushMirrorEntity::delete_by_id(id)
        .exec(db)
        .await
        .context("db: delete push mirror")?;
    Ok(())
}

/// Every active push mirror, for the operator view.
pub async fn list_active(db: &DatabaseConnection) -> Result<Vec<Model>> {
    PushMirrorEntity::find()
        .filter(push_mirror::Column::Status.eq(push_mirror::STATUS_ACTIVE))
        .all(db)
        .await
        .context("db: list active push mirrors")
}
