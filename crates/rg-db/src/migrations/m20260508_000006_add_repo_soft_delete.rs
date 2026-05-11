use sea_orm_migration::prelude::*;
use sea_orm::Statement;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260508_000006_add_repo_soft_delete"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // SQLite does not support multiple ADD COLUMN in a single ALTER TABLE.
        // Use raw SQL to add columns one by one.
        let db = manager.get_connection();
        let backend = db.get_database_backend();

        db.execute(Statement::from_string(
            backend,
            "ALTER TABLE repositories ADD COLUMN deleted_at TIMESTAMP NULL;",
        ))
        .await?;

        db.execute(Statement::from_string(
            backend,
            "ALTER TABLE repositories ADD COLUMN origin_repo_id BIGINT NULL;",
        ))
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // SQLite 3.35+ supports DROP COLUMN.
        // Drop origin_repo_id first (reverse order of ADD).
        let db = manager.get_connection();
        let backend = db.get_database_backend();

        db.execute(Statement::from_string(
            backend,
            "ALTER TABLE repositories DROP COLUMN origin_repo_id;",
        ))
        .await?;

        db.execute(Statement::from_string(
            backend,
            "ALTER TABLE repositories DROP COLUMN deleted_at;",
        ))
        .await?;

        Ok(())
    }
}
