use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260508_000005_create_fts5_indexes"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Create FTS5 virtual tables with content= sync mode
        let sql = r#"
            CREATE VIRTUAL TABLE IF NOT EXISTS repos_fts USING fts5(name, description, owner, content=repositories, content_rowid=id);

            CREATE VIRTUAL TABLE IF NOT EXISTS issues_fts USING fts5(title, body, content=issues, content_rowid=id);

            CREATE VIRTUAL TABLE IF NOT EXISTS wiki_pages_fts USING fts5(title, content, content=wiki_pages, content_rowid=id);

            -- Triggers for repos_fts
            CREATE TRIGGER IF NOT EXISTS repos_fts_insert AFTER INSERT ON repositories BEGIN
                INSERT INTO repos_fts(rowid, name, description, owner) VALUES (new.id, new.name, COALESCE(new.description, ''), '');
            END;

            CREATE TRIGGER IF NOT EXISTS repos_fts_delete AFTER DELETE ON repositories BEGIN
                INSERT INTO repos_fts(repos_fts, rowid, name, description, owner) VALUES('delete', old.id, old.name, COALESCE(old.description, ''), '');
            END;

            CREATE TRIGGER IF NOT EXISTS repos_fts_update AFTER UPDATE ON repositories BEGIN
                INSERT INTO repos_fts(repos_fts, rowid, name, description, owner) VALUES('delete', old.id, old.name, COALESCE(old.description, ''), '');
                INSERT INTO repos_fts(rowid, name, description, owner) VALUES (new.id, new.name, COALESCE(new.description, ''), '');
            END;

            -- Triggers for issues_fts
            CREATE TRIGGER IF NOT EXISTS issues_fts_insert AFTER INSERT ON issues BEGIN
                INSERT INTO issues_fts(rowid, title, body) VALUES (new.id, new.title, COALESCE(new.body, ''));
            END;

            CREATE TRIGGER IF NOT EXISTS issues_fts_delete AFTER DELETE ON issues BEGIN
                INSERT INTO issues_fts(issues_fts, rowid, title, body) VALUES('delete', old.id, old.title, COALESCE(old.body, ''));
            END;

            CREATE TRIGGER IF NOT EXISTS issues_fts_update AFTER UPDATE ON issues BEGIN
                INSERT INTO issues_fts(issues_fts, rowid, title, body) VALUES('delete', old.id, old.title, COALESCE(old.body, ''));
                INSERT INTO issues_fts(rowid, title, body) VALUES (new.id, new.title, COALESCE(new.body, ''));
            END;

            -- Triggers for wiki_pages_fts
            CREATE TRIGGER IF NOT EXISTS wiki_pages_fts_insert AFTER INSERT ON wiki_pages BEGIN
                INSERT INTO wiki_pages_fts(rowid, title, content) VALUES (new.id, new.title, COALESCE(new.content, ''));
            END;

            CREATE TRIGGER IF NOT EXISTS wiki_pages_fts_delete AFTER DELETE ON wiki_pages BEGIN
                INSERT INTO wiki_pages_fts(wiki_pages_fts, rowid, title, content) VALUES('delete', old.id, old.title, COALESCE(old.content, ''));
            END;

            CREATE TRIGGER IF NOT EXISTS wiki_pages_fts_update AFTER UPDATE ON wiki_pages BEGIN
                INSERT INTO wiki_pages_fts(wiki_pages_fts, rowid, title, content) VALUES('delete', old.id, old.title, COALESCE(old.content, ''));
                INSERT INTO wiki_pages_fts(rowid, title, content) VALUES (new.id, new.title, COALESCE(new.content, ''));
            END;

            -- Rebuild FTS indexes from existing data
            INSERT INTO repos_fts(repos_fts) VALUES('rebuild');
            INSERT INTO issues_fts(issues_fts) VALUES('rebuild');
            INSERT INTO wiki_pages_fts(wiki_pages_fts) VALUES('rebuild');
        "#;

        // Execute the SQL in one statement
        manager
            .get_connection()
            .execute_unprepared(sql)
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let sql = r#"
            DROP TRIGGER IF EXISTS repos_fts_update;
            DROP TRIGGER IF EXISTS repos_fts_delete;
            DROP TRIGGER IF EXISTS repos_fts_insert;
            DROP TRIGGER IF EXISTS issues_fts_update;
            DROP TRIGGER IF EXISTS issues_fts_delete;
            DROP TRIGGER IF EXISTS issues_fts_insert;
            DROP TRIGGER IF EXISTS wiki_pages_fts_update;
            DROP TRIGGER IF EXISTS wiki_pages_fts_delete;
            DROP TRIGGER IF EXISTS wiki_pages_fts_insert;
            DROP TABLE IF EXISTS repos_fts;
            DROP TABLE IF EXISTS issues_fts;
            DROP TABLE IF EXISTS wiki_pages_fts;
        "#;

        manager
            .get_connection()
            .execute_unprepared(sql)
            .await?;

        Ok(())
    }
}
