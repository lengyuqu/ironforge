use crate::entities::{ci_environment, ci_environment_approval, pipeline_job};
use anyhow::{Context, Result};
use sea_orm::sea_query::Expr;
use sea_orm::*;

fn is_duplicate_approval(error: &DbErr) -> bool {
    matches!(error.sql_err(), Some(SqlErr::UniqueConstraintViolation(_)))
        || error.to_string().to_ascii_lowercase().contains("unique")
}

pub async fn list(db: &DatabaseConnection, repo_id: i64) -> Result<Vec<ci_environment::Model>> {
    ci_environment::Entity::find()
        .filter(ci_environment::Column::RepoId.eq(repo_id))
        .order_by_asc(ci_environment::Column::Name)
        .all(db)
        .await
        .context("db: list CI environments")
}
pub async fn find_by_id(db: &DatabaseConnection, id: i64) -> Result<Option<ci_environment::Model>> {
    ci_environment::Entity::find_by_id(id)
        .one(db)
        .await
        .context("db: find CI environment")
}
pub async fn find_by_name(
    db: &DatabaseConnection,
    repo_id: i64,
    name: &str,
) -> Result<Option<ci_environment::Model>> {
    ci_environment::Entity::find()
        .filter(ci_environment::Column::RepoId.eq(repo_id))
        .filter(ci_environment::Column::Name.eq(name))
        .one(db)
        .await
        .context("db: find CI environment by name")
}
pub async fn create(
    db: &DatabaseConnection,
    model: ci_environment::ActiveModel,
) -> Result<ci_environment::Model> {
    model.insert(db).await.context("db: create CI environment")
}
pub async fn update(
    db: &DatabaseConnection,
    model: ci_environment::ActiveModel,
) -> Result<ci_environment::Model> {
    model.update(db).await.context("db: update CI environment")
}
pub async fn delete(db: &DatabaseConnection, id: i64) -> Result<()> {
    ci_environment::Entity::delete_by_id(id)
        .exec(db)
        .await
        .context("db: delete CI environment")?;
    Ok(())
}

pub async fn attach_job(
    db: &DatabaseConnection,
    job_id: i64,
    environment: Option<&ci_environment::Model>,
    environment_name: &str,
) -> Result<()> {
    let mut update = pipeline_job::Entity::update_many()
        .filter(pipeline_job::Column::Id.eq(job_id))
        .col_expr(
            pipeline_job::Column::EnvironmentName,
            Expr::value(environment_name),
        );
    if let Some(environment) = environment {
        update = update.col_expr(
            pipeline_job::Column::EnvironmentId,
            Expr::value(environment.id),
        );
        if environment.protected {
            update = update.col_expr(
                pipeline_job::Column::Status,
                Expr::value("waiting_approval"),
            );
        }
    }
    update
        .exec(db)
        .await
        .context("db: attach job environment")?;
    Ok(())
}

pub async fn add_approval(
    db: &DatabaseConnection,
    job_id: i64,
    environment_id: i64,
    approved_by: i64,
) -> Result<bool> {
    if db.get_database_backend() == DatabaseBackend::Postgres {
        // Do not decode a RETURNING row after the insert has committed: older
        // schemas store job_id as INTEGER while the model exposes i64.
        let result = db.execute(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "INSERT INTO ci_environment_approvals (job_id, environment_id, approved_by, created_at) VALUES ($1::bigint, $2, $3, $4)",
            [job_id.into(), environment_id.into(), approved_by.into(), chrono::Utc::now().into()],
        )).await;
        return match result {
            Ok(_) => Ok(true),
            Err(error) if is_duplicate_approval(&error) => Ok(false),
            Err(error) => Err(error).context("db: add environment approval"),
        };
    }
    let model = ci_environment_approval::ActiveModel {
        job_id: Set(job_id),
        environment_id: Set(environment_id),
        approved_by: Set(approved_by),
        created_at: Set(chrono::Utc::now()),
        ..Default::default()
    };
    match model.insert(db).await {
        Ok(_) => Ok(true),
        Err(error) if is_duplicate_approval(&error) => Ok(false),
        Err(error) => Err(error).context("db: add environment approval"),
    }
}
pub async fn count_approvals(db: &DatabaseConnection, job_id: i64) -> Result<u64> {
    ci_environment_approval::Entity::find()
        .filter(ci_environment_approval::Column::JobId.eq(job_id))
        .count(db)
        .await
        .context("db: count environment approvals")
}
pub async fn list_approvals(
    db: &DatabaseConnection,
    job_id: i64,
) -> Result<Vec<ci_environment_approval::Model>> {
    if db.get_database_backend() == DatabaseBackend::Postgres {
        return ci_environment_approval::Model::find_by_statement(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT id, job_id::bigint AS job_id, environment_id, approved_by, created_at FROM ci_environment_approvals WHERE job_id = $1::bigint ORDER BY created_at ASC",
            [job_id.into()],
        ))
        .all(db)
        .await
        .context("db: list environment approvals");
    }
    ci_environment_approval::Entity::find()
        .filter(ci_environment_approval::Column::JobId.eq(job_id))
        .order_by_asc(ci_environment_approval::Column::CreatedAt)
        .all(db)
        .await
        .context("db: list environment approvals")
}
pub async fn release_approved_job(db: &DatabaseConnection, job_id: i64) -> Result<bool> {
    let result = pipeline_job::Entity::update_many()
        .filter(pipeline_job::Column::Id.eq(job_id))
        .filter(pipeline_job::Column::Status.eq("waiting_approval"))
        .col_expr(pipeline_job::Column::Status, Expr::value("pending"))
        .col_expr(
            pipeline_job::Column::UpdatedAt,
            Expr::value(chrono::Utc::now().naive_utc()),
        )
        .exec(db)
        .await
        .context("db: release approved environment job")?;
    Ok(result.rows_affected == 1)
}

pub async fn has_jobs(db: &DatabaseConnection, environment_id: i64) -> Result<bool> {
    Ok(pipeline_job::Entity::find()
        .filter(pipeline_job::Column::EnvironmentId.eq(environment_id))
        .count(db)
        .await
        .context("db: count environment jobs")?
        > 0)
}
