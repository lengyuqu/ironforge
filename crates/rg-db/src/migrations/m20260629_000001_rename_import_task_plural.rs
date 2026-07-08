//! Corrective migration: rename `import_task` to `import_tasks`.
//!
//! The original import task migration used `#[derive(Iden)] enum ImportTask`
//! for the table identifier, which resolves to the singular table name
//! `import_task`. The entity maps to `import_tasks`, so fresh databases need
//! this compatibility rename when the singular table exists.

use sea_orm::Statement;
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260629_000001_rename_import_task_plural"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        let backend = db.get_database_backend();

        if manager.has_table("import_task").await? && !manager.has_table("import_tasks").await? {
            db.execute(Statement::from_string(
                backend,
                "ALTER TABLE \"import_task\" RENAME TO \"import_tasks\";".to_owned(),
            ))
            .await?;
        }

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        let backend = db.get_database_backend();

        if manager.has_table("import_tasks").await? && !manager.has_table("import_task").await? {
            db.execute(Statement::from_string(
                backend,
                "ALTER TABLE \"import_tasks\" RENAME TO \"import_task\";".to_owned(),
            ))
            .await?;
        }

        Ok(())
    }
}
