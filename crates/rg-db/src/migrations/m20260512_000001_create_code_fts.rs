use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260512_000001_create_code_fts"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let sql = r#"
            -- Clean up any previous partial state
            DROP TABLE IF EXISTS code_fts;

            -- Create FTS5 virtual table for code search
            -- Columns: repo_id, file_path, file_name, content, language
            CREATE VIRTUAL TABLE code_fts USING fts5(
                repo_id,
                file_path,
                file_name,
                content,
                language
            );

            -- Note: No triggers needed because:
            -- 1. Git blob content is not stored in database tables
            -- 2. Indexing is done by a separate service (CodeIndexer)
            -- 3. The service scans Git objects and updates FTS5 directly
        "#;

        manager
            .get_connection()
            .execute_unprepared(sql)
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let sql = r#"
            DROP TABLE IF EXISTS code_fts;
        "#;

        manager
            .get_connection()
            .execute_unprepared(sql)
            .await?;

        Ok(())
    }
}
