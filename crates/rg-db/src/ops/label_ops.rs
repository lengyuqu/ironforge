//! Database operations for labels.

use anyhow::{Context, Result};
use sea_orm::*;

use crate::entities::label::{self, ActiveModel, Entity as LabelEntity, Model as Label};

/// Find a label by ID.
pub async fn find_by_id(db: &DatabaseConnection, id: i64) -> Result<Option<Label>> {
    LabelEntity::find_by_id(id)
        .one(db)
        .await
        .context("db: find label by id")
}

/// List all labels for a repo.
pub async fn list_by_repo(db: &DatabaseConnection, repo_id: i64) -> Result<Vec<Label>> {
    LabelEntity::find()
        .filter(label::Column::RepoId.eq(repo_id))
        .order_by_asc(label::Column::Name)
        .all(db)
        .await
        .context("db: list labels by repo")
}

/// Create a new label.
pub async fn create(db: &DatabaseConnection, model: ActiveModel) -> Result<Label> {
    model.insert(db).await.context("db: create label")
}

/// Update a label.
pub async fn update(db: &DatabaseConnection, model: ActiveModel) -> Result<Label> {
    model.update(db).await.context("db: update label")
}

/// Delete a label by ID.
pub async fn delete_by_id(db: &DatabaseConnection, id: i64) -> Result<()> {
    LabelEntity::delete_by_id(id)
        .exec(db)
        .await
        .context("db: delete label")?;
    Ok(())
}
