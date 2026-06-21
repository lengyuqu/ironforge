//! Migration: create wiki_revisions table.

use sea_orm::Statement;
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260617_000001_create_wiki_revisions"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        if manager.has_table("wiki_revisions").await? {
            return Ok(());
        }
        let db = manager.get_connection();
        use sea_orm::ConnectionTrait;
        db.execute(Statement::from_string(
            sea_orm::DatabaseBackend::Sqlite,
            r#"CREATE TABLE IF NOT EXISTS "wiki_revisions" (
                "id"           INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
                "wiki_page_id" INTEGER NOT NULL,
                "content"      TEXT    NOT NULL,
                "message"      TEXT,
                "author_id"    INTEGER,
                "version"      INTEGER NOT NULL DEFAULT 1,
                "created_at"   TEXT    NOT NULL,
                FOREIGN KEY ("wiki_page_id") REFERENCES "wiki_pages"("id") ON DELETE CASCADE
            )"#
            .to_string(),
        ))
        .await?;

        db.execute(Statement::from_string(
            sea_orm::DatabaseBackend::Sqlite,
            r#"CREATE INDEX IF NOT EXISTS "idx_wiki_revisions_page"
               ON "wiki_revisions" ("wiki_page_id")"#
                .to_string(),
        ))
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        use sea_orm::ConnectionTrait;
        db.execute(Statement::from_string(
            sea_orm::DatabaseBackend::Sqlite,
            r#"DROP TABLE IF EXISTS "wiki_revisions""#.to_string(),
        ))
        .await?;
        Ok(())
    }
}
