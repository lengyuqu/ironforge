//! Corrective migration: m20260607_000004_create_import_tasks originally used
//! `#[derive(Iden)] enum ImportTask { Table, ... }`, which created the singular
//! table `import_task` while the SeaORM entity maps to `import_tasks`.

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
        if manager.has_table("import_task").await? && !manager.has_table("import_tasks").await? {
            let db = manager.get_connection();
            db.execute(Statement::from_string(
                db.get_database_backend(),
                "ALTER TABLE \"import_task\" RENAME TO \"import_tasks\";".to_string(),
            ))
            .await?;
        }

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        if manager.has_table("import_tasks").await? && !manager.has_table("import_task").await? {
            let db = manager.get_connection();
            db.execute(Statement::from_string(
                db.get_database_backend(),
                "ALTER TABLE \"import_tasks\" RENAME TO \"import_task\";".to_string(),
            ))
            .await?;
        }

        Ok(())
    }
}
