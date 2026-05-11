use sea_orm_migration::prelude::*;
use sea_orm::Statement;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260510_000002_alter_pipeline_jobs_add_runner_fields"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        let backend = db.get_database_backend();

        // Columns to add: (name, definition)
        let cols = [
            ("runner_id", "bigint NULL"),
            ("started_at", "TIMESTAMP NULL"),
            ("finished_at", "TIMESTAMP NULL"),
        ];

        for (col_name, col_def) in cols.iter() {
            // Check existence via pragma_table_info
            let check_sql = format!(
                "SELECT 1 FROM pragma_table_info('pipeline_jobs') WHERE name='{}'",
                col_name
            );
            let exists = db
                .query_one(Statement::from_string(backend, check_sql))
                .await
                .ok()
                .flatten()
                .is_some();

            if exists {
                tracing::info!("Column '{}' already exists, skipping", col_name);
                continue;
            }

            let alter_sql = format!(
                "ALTER TABLE \"pipeline_jobs\" ADD COLUMN \"{}\" {}",
                col_name, col_def
            );
            db.execute(Statement::from_string(backend, alter_sql))
                .await?;
        }
        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Ok(())
    }
}
