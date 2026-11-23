//! Database operations for remote credentials.

use anyhow::{Context, Result};
use chrono::Utc;
use sea_orm::*;

use crate::entities::remote_credential::{self, ActiveModel, Entity as CredentialEntity, Model};

/// Create a credential record.
pub async fn create(db: &DatabaseConnection, model: ActiveModel) -> Result<Model> {
    model
        .insert(db)
        .await
        .context("db: create remote credential")
}

/// Find a credential by its ID.
pub async fn find_by_id(db: &DatabaseConnection, id: i64) -> Result<Option<Model>> {
    CredentialEntity::find_by_id(id)
        .one(db)
        .await
        .context("db: find remote credential by id")
}

/// The lookup the whole feature exists for: the owner's token for one host.
///
/// `host` must already be normalised (`net_guard::url_host_key`); this function
/// does not trim, lowercase or otherwise guess.
pub async fn find_for_owner_host(
    db: &DatabaseConnection,
    owner_kind: &str,
    owner_id: i64,
    host: &str,
) -> Result<Option<Model>> {
    CredentialEntity::find()
        .filter(remote_credential::Column::OwnerKind.eq(owner_kind))
        .filter(remote_credential::Column::OwnerId.eq(owner_id))
        .filter(remote_credential::Column::Host.eq(host))
        .one(db)
        .await
        .context("db: find remote credential by owner and host")
}

/// Every credential an owner configured, newest first.
pub async fn list_for_owner(
    db: &DatabaseConnection,
    owner_kind: &str,
    owner_id: i64,
) -> Result<Vec<Model>> {
    CredentialEntity::find()
        .filter(remote_credential::Column::OwnerKind.eq(owner_kind))
        .filter(remote_credential::Column::OwnerId.eq(owner_id))
        .order_by_asc(remote_credential::Column::Host)
        .all(db)
        .await
        .context("db: list remote credentials for owner")
}

/// Update a credential record.
pub async fn update(db: &DatabaseConnection, model: ActiveModel) -> Result<Model> {
    model
        .update(db)
        .await
        .context("db: update remote credential")
}

/// Delete a credential by ID.
pub async fn delete_by_id(db: &DatabaseConnection, id: i64) -> Result<()> {
    CredentialEntity::delete_by_id(id)
        .exec(db)
        .await
        .context("db: delete remote credential")?;
    Ok(())
}

/// Delete every credential an owner configured.
///
/// Called when the owning user or organization is deleted. The table has no
/// foreign key on purpose (see the migration), so this is the only thing that
/// keeps the dictionary from accumulating tokens for accounts that no longer
/// exist.
pub async fn delete_all_for_owner(
    db: &DatabaseConnection,
    owner_kind: &str,
    owner_id: i64,
) -> Result<u64> {
    let result = CredentialEntity::delete_many()
        .filter(remote_credential::Column::OwnerKind.eq(owner_kind))
        .filter(remote_credential::Column::OwnerId.eq(owner_id))
        .exec(db)
        .await
        .context("db: delete remote credentials for owner")?;
    Ok(result.rows_affected)
}

/// Record that a git call used this token.
///
/// Best-effort by design: it is called on the sync path, and a failed
/// bookkeeping write must never turn a successful fetch into an error. Callers
/// log the failure instead of propagating it.
pub async fn touch_last_used(
    db: &DatabaseConnection,
    id: i64,
    at: chrono::DateTime<Utc>,
) -> Result<()> {
    CredentialEntity::update_many()
        .col_expr(
            remote_credential::Column::LastUsedAt,
            sea_orm::sea_query::Expr::value(at),
        )
        .filter(remote_credential::Column::Id.eq(id))
        .exec(db)
        .await
        .context("db: touch remote credential last_used_at")?;
    Ok(())
}
