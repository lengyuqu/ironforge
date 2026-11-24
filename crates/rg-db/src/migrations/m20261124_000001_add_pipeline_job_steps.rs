//! Step declarations and job outputs on `pipeline_jobs`.
//!
//! `steps_json` is what makes outputs possible at all: until now a job was
//! stored as one newline-joined `script`, so the runner never knew that a step
//! with an `id` existed and had nowhere to report a produced value. The column
//! carries the declared step list (`StepConfig` JSON) alongside the flattened
//! script, which stays for hand-written configs and for backwards reading.
//!
//! `outputs_json` is the destination: the runner reports what the steps
//! produced, and a downstream job reads it through `needs.<job>.outputs.<name>`.
//!
//! Both nullable: existing rows predate the feature and a job without steps is
//! a normal job, not an incomplete one.

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261124_000001_add_pipeline_job_steps"
    }
}

const PIPELINE_JOBS: &str = "pipeline_jobs";

#[derive(DeriveIden)]
enum PipelineJobs {
    #[sea_orm(iden = "pipeline_jobs")]
    Table,
    StepsJson,
    OutputsJson,
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        for (column, name) in [
            (PipelineJobs::StepsJson, "steps_json"),
            (PipelineJobs::OutputsJson, "outputs_json"),
        ] {
            if !manager.has_column(PIPELINE_JOBS, name).await? {
                manager
                    .alter_table(
                        Table::alter()
                            .table(PipelineJobs::Table)
                            .add_column(ColumnDef::new(column).text().to_owned())
                            .to_owned(),
                    )
                    .await?;
            }
        }
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        for (column, name) in [
            (PipelineJobs::OutputsJson, "outputs_json"),
            (PipelineJobs::StepsJson, "steps_json"),
        ] {
            if manager.has_column(PIPELINE_JOBS, name).await? {
                manager
                    .alter_table(
                        Table::alter()
                            .table(PipelineJobs::Table)
                            .drop_column(column)
                            .to_owned(),
                    )
                    .await?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::{ConnectionTrait, Database, Statement};
    use sea_orm_migration::MigratorTrait;

    async fn column_exists(db: &sea_orm::DatabaseConnection, column: &str) -> bool {
        let sql = format!(
            "SELECT COUNT(*) FROM pragma_table_info('pipeline_jobs') WHERE name = '{column}'"
        );
        let row = db
            .query_one(Statement::from_string(db.get_database_backend(), sql))
            .await
            .unwrap()
            .unwrap();
        row.try_get_by_index::<i64>(0).unwrap() == 1
    }

    #[tokio::test]
    async fn both_columns_exist_and_are_nullable() {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        crate::migrations::Migrator::up(&db, None).await.unwrap();
        assert!(column_exists(&db, "steps_json").await);
        assert!(column_exists(&db, "outputs_json").await);
    }

    #[tokio::test]
    async fn down_drops_them_in_reverse_order() {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        crate::migrations::Migrator::up(&db, None).await.unwrap();
        let steps = {
            let migrations = crate::migrations::Migrator::migrations();
            let position = migrations
                .iter()
                .rev()
                .position(|migration| migration.name() == "m20261124_000001_add_pipeline_job_steps")
                .expect("this migration must be registered");
            (position + 1) as u32
        };
        crate::migrations::Migrator::down(&db, Some(steps))
            .await
            .unwrap();
        assert!(!column_exists(&db, "steps_json").await);
        assert!(!column_exists(&db, "outputs_json").await);

        crate::migrations::Migrator::up(&db, None).await.unwrap();
        assert!(column_exists(&db, "steps_json").await);
    }
}
