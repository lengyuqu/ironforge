//! Label service — business logic for label CRUD.

use crate::error::{CoreError, CoreResult};
use chrono::Utc;
use sea_orm::{ActiveValue::Set, DatabaseConnection, SqlErr};

use rg_db::entities::label::{ActiveModel as LabelActiveModel, Model as Label};
use rg_db::entities::repository;
use rg_db::ops::{issue_label_ops, label_ops, repo_ops, user_ops};

/// List all labels for a repository.
pub async fn list_labels(
    db: &DatabaseConnection,
    owner: &str,
    repo_name: &str,
) -> CoreResult<Vec<Label>> {
    let repo = resolve_repo(db, owner, repo_name).await?;
    Ok(label_ops::list_by_repo(db, repo.id).await?)
}

/// Get a single label by ID.
pub async fn get_label(
    db: &DatabaseConnection,
    owner: &str,
    repo_name: &str,
    label_id: i64,
) -> CoreResult<Label> {
    let repo = resolve_repo(db, owner, repo_name).await?;
    find_label_in_repo(db, repo.id, label_id).await
}

/// Create a new label.
pub async fn create_label(
    db: &DatabaseConnection,
    owner: &str,
    repo_name: &str,
    name: String,
    color: String,
    description: Option<String>,
) -> CoreResult<Label> {
    let repo = resolve_repo(db, owner, repo_name).await?;
    let name_for_error = name.clone();

    if name.trim().is_empty() {
        return Err(CoreError::invalid_input("label name cannot be empty"));
    }
    if !color.starts_with('#') || color.len() != 7 {
        return Err(CoreError::invalid_input(
            "color must be a hex string like #ff0000",
        ));
    }
    ensure_name_available(db, repo.id, &name, None).await?;

    let now = Utc::now();
    let model = LabelActiveModel {
        repo_id: Set(repo.id),
        name: Set(name),
        color: Set(color),
        description: Set(description),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    };

    label_ops::create(db, model)
        .await
        .map_err(|e| unique_or_internal(e, &name_for_error))
}

/// Update an existing label of the given repository.
///
/// A label id belonging to another repository is reported as not found.
pub async fn update_label(
    db: &DatabaseConnection,
    repo_id: i64,
    label_id: i64,
    name: Option<String>,
    color: Option<String>,
    description: Option<Option<String>>,
) -> CoreResult<Label> {
    let mut label = find_label_in_repo(db, repo_id, label_id).await?;

    if let Some(n) = name {
        if n.trim().is_empty() {
            return Err(CoreError::invalid_input("label name cannot be empty"));
        }
        if n != label.name {
            ensure_name_available(db, repo_id, &n, Some(label_id)).await?;
        }
        label.name = n;
    }
    if let Some(c) = color {
        if !c.starts_with('#') || c.len() != 7 {
            return Err(CoreError::invalid_input(
                "invalid color: must be a hex string like #ff0000",
            ));
        }
        label.color = c;
    }
    if let Some(d) = description {
        label.description = d;
    }

    label.updated_at = Utc::now();

    let name_for_error = label.name.clone();
    let active: LabelActiveModel = label.into();
    label_ops::update(db, active)
        .await
        .map_err(|e| unique_or_internal(e, &name_for_error))
}

/// Delete a label of the given repository.
pub async fn delete_label(db: &DatabaseConnection, repo_id: i64, label_id: i64) -> CoreResult<()> {
    find_label_in_repo(db, repo_id, label_id).await?;
    // Delete all issue_labels referencing this label first
    issue_label_ops::delete_by_label_id(db, label_id).await?;
    Ok(label_ops::delete_by_id(db, label_id).await?)
}

/// Get labels for an issue (batch query — avoids N+1).
pub async fn get_issue_labels(db: &DatabaseConnection, issue_id: i64) -> CoreResult<Vec<Label>> {
    let label_ids = issue_label_ops::get_label_ids(db, issue_id).await?;
    Ok(label_ops::find_by_ids(db, &label_ids).await?)
}

/// Set labels for an issue.
pub async fn set_issue_labels(
    db: &DatabaseConnection,
    issue_id: i64,
    label_ids: Vec<i64>,
) -> CoreResult<()> {
    Ok(issue_label_ops::set_labels(db, issue_id, label_ids).await?)
}

/// Resolve owner/repo_name to a repository model.
async fn resolve_repo(
    db: &DatabaseConnection,
    owner: &str,
    repo_name: &str,
) -> CoreResult<repository::Model> {
    let user = user_ops::find_by_username(db, owner)
        .await?
        .ok_or_else(|| CoreError::not_found("owner not found"))?;
    repo_ops::find_by_owner_and_name(db, user.id, repo_name)
        .await?
        .ok_or_else(|| CoreError::not_found("repository not found"))
}

/// Load a label and make sure it belongs to `repo_id`; a label of another
/// repository is indistinguishable from a missing one.
async fn find_label_in_repo(
    db: &DatabaseConnection,
    repo_id: i64,
    label_id: i64,
) -> CoreResult<Label> {
    match label_ops::find_by_id(db, label_id).await? {
        Some(label) if label.repo_id == repo_id => Ok(label),
        _ => Err(CoreError::not_found("label not found")),
    }
}

/// Reject a label name that another label of the repository already uses.
async fn ensure_name_available(
    db: &DatabaseConnection,
    repo_id: i64,
    name: &str,
    except_label_id: Option<i64>,
) -> CoreResult<()> {
    let taken = label_ops::list_by_repo(db, repo_id)
        .await?
        .iter()
        .any(|l| l.name == name && Some(l.id) != except_label_id);
    if taken {
        return Err(duplicate_label(name));
    }
    Ok(())
}

fn duplicate_label(name: &str) -> CoreError {
    CoreError::conflict(format!("label '{name}' already exists"))
}

/// Map a unique-index violation (lost race against another writer) to a
/// conflict; every other database failure stays internal.
fn unique_or_internal(error: anyhow::Error, name: &str) -> CoreError {
    let is_unique = error
        .downcast_ref::<sea_orm::DbErr>()
        .and_then(|e| e.sql_err())
        .is_some_and(|e| matches!(e, SqlErr::UniqueConstraintViolation(_)));
    if is_unique {
        duplicate_label(name)
    } else {
        CoreError::Internal(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::{ConnectionTrait, Statement};

    async fn db_with_repo() -> (DatabaseConnection, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let url = format!("sqlite://{}?mode=rwc", dir.path().join("labels.db").display());
        let db = sea_orm::Database::connect(sea_orm::ConnectOptions::new(url))
            .await
            .unwrap();
        rg_db::run_migrations(&db).await.unwrap();
        for sql in [
            "INSERT INTO users(id, username, email, password_hash, is_admin, is_active, created_at, updated_at) VALUES(1, 'u', 'u@x.io', 'x', 0, 1, '2024-01-01 00:00:00', '2024-01-01 00:00:00')",
            "INSERT INTO repositories(id, owner_id, name, is_private, default_branch, stars_count, forks_count, created_at, updated_at) VALUES(1, 1, 'r', 0, 'main', 0, 0, '2024-01-01 00:00:00', '2024-01-01 00:00:00')",
        ] {
            db.execute(Statement::from_string(sea_orm::DatabaseBackend::Sqlite, sql))
                .await
                .unwrap();
        }
        (db, dir)
    }

    fn model(name: &str) -> LabelActiveModel {
        let now = Utc::now();
        LabelActiveModel {
            repo_id: Set(1),
            name: Set(name.to_string()),
            color: Set("#ffffff".into()),
            description: Set(None),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn lost_race_on_unique_name_index_is_a_conflict_other_failures_are_internal() {
        let (db, _dir) = db_with_repo().await;
        label_ops::create(&db, model("bug")).await.unwrap();

        // Bypass the pre-check, as a concurrent writer would.
        let err = label_ops::create(&db, model("bug")).await.unwrap_err();
        assert!(matches!(
            unique_or_internal(err, "bug"),
            CoreError::Conflict(_)
        ));

        let other = anyhow::anyhow!("disk exploded");
        assert!(matches!(
            unique_or_internal(other, "bug"),
            CoreError::Internal(_)
        ));
    }

    #[tokio::test]
    async fn service_reports_duplicates_and_foreign_labels_with_the_right_variant() {
        let (db, _dir) = db_with_repo().await;
        create_label(&db, "u", "r", "bug".into(), "#ff0000".into(), None)
            .await
            .unwrap();
        assert!(matches!(
            create_label(&db, "u", "r", "bug".into(), "#ff0000".into(), None).await,
            Err(CoreError::Conflict(_))
        ));
        assert!(matches!(
            create_label(&db, "u", "r", " ".into(), "#ff0000".into(), None).await,
            Err(CoreError::InvalidInput(_))
        ));
        assert!(matches!(
            create_label(&db, "nobody", "r", "x".into(), "#ff0000".into(), None).await,
            Err(CoreError::NotFound(_))
        ));
        // A label id that exists, addressed through another repository id.
        let id = list_labels(&db, "u", "r").await.unwrap()[0].id;
        assert!(matches!(
            delete_label(&db, 999, id).await,
            Err(CoreError::NotFound(_))
        ));
        assert!(matches!(
            update_label(&db, 999, id, Some("z".into()), None, None).await,
            Err(CoreError::NotFound(_))
        ));
    }
}
