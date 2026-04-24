//! Database operations for repositories.

use anyhow::{Context, Result};
use sea_orm::*;

use crate::entities::repository::{self, ActiveModel, Entity as RepoEntity, Model as Repo};

/// Find a repo by (owner_id, name).
pub async fn find_by_owner_and_name(
    db: &DatabaseConnection,
    owner_id: i64,
    name: &str,
) -> Result<Option<Repo>> {
    RepoEntity::find()
        .filter(repository::Column::OwnerId.eq(owner_id))
        .filter(repository::Column::Name.eq(name))
        .one(db)
        .await
        .context("db: find repo by owner and name")
}

/// List all repos owned by a user.
pub async fn list_by_owner(db: &DatabaseConnection, owner_id: i64) -> Result<Vec<Repo>> {
    RepoEntity::find()
        .filter(repository::Column::OwnerId.eq(owner_id))
        .order_by_asc(repository::Column::Name)
        .all(db)
        .await
        .context("db: list repos by owner")
}

/// Find a repo by (org_id, name).
pub async fn find_by_org_and_name(
    db: &DatabaseConnection,
    org_id: i64,
    name: &str,
) -> Result<Option<Repo>> {
    RepoEntity::find()
        .filter(repository::Column::OrgId.eq(org_id))
        .filter(repository::Column::Name.eq(name))
        .one(db)
        .await
        .context("db: find repo by org and name")
}

/// List all repos belonging to an organization.
pub async fn list_by_org(db: &DatabaseConnection, org_id: i64) -> Result<Vec<Repo>> {
    RepoEntity::find()
        .filter(repository::Column::OrgId.eq(org_id))
        .order_by_asc(repository::Column::Name)
        .all(db)
        .await
        .context("db: list repos by org")
}

/// Create a new repo.
pub async fn create(db: &DatabaseConnection, model: ActiveModel) -> Result<Repo> {
    model.insert(db).await.context("db: create repo")
}

/// Delete a repo by id.
pub async fn delete_by_id(db: &DatabaseConnection, id: i64) -> Result<()> {
    RepoEntity::delete_by_id(id)
        .exec(db)
        .await
        .context("db: delete repo")?;
    Ok(())
}
