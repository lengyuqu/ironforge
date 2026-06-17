//! Rename board/time tables from singular to plural to match entity declarations.
//!
//! Migration `m20260607_000002_create_boards` used `#[derive(Iden)] enum Board { Table }` which
//! generates the singular name "board". Entities declare `#[sea_orm(table_name = "boards")]`.
//! Same issue for board_column→board_columns, board_card→board_cards, time_entry→time_entries.

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str { "m20260617_000002_rename_board_time_tables_plural" }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        use sea_orm::ConnectionTrait;

        // board_cards must be renamed before board_columns (FK dependency)
        // board_columns before boards; wiki_revision → wiki_revisions
        for (old, new) in &[
            ("board_card",    "board_cards"),
            ("board_column",  "board_columns"),
            ("board",         "boards"),
            ("time_entry",    "time_entries"),
            ("wiki_revision", "wiki_revisions"),
        ] {
            // Skip if the old table doesn't exist (e.g. already renamed on a prior run)
            let check: Vec<_> = db
                .query_all(sea_orm::Statement::from_string(
                    sea_orm::DatabaseBackend::Sqlite,
                    format!("SELECT name FROM sqlite_master WHERE type='table' AND name='{}'", old),
                ))
                .await?;
            if check.is_empty() {
                continue; // already renamed (or never existed under this name)
            }
            db.execute(sea_orm::Statement::from_string(
                sea_orm::DatabaseBackend::Sqlite,
                format!("ALTER TABLE \"{}\" RENAME TO \"{}\"", old, new),
            )).await?;
        }
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        use sea_orm::ConnectionTrait;
        for (old, new) in &[
            ("board_cards",    "board_card"),
            ("board_columns",  "board_column"),
            ("boards",         "board"),
            ("time_entries",   "time_entry"),
            ("wiki_revisions", "wiki_revision"),
        ] {
            let check: Vec<_> = db
                .query_all(sea_orm::Statement::from_string(
                    sea_orm::DatabaseBackend::Sqlite,
                    format!("SELECT name FROM sqlite_master WHERE type='table' AND name='{}'", old),
                ))
                .await?;
            if check.is_empty() { continue; }
            db.execute(sea_orm::Statement::from_string(
                sea_orm::DatabaseBackend::Sqlite,
                format!("ALTER TABLE \"{}\" RENAME TO \"{}\"", old, new),
            )).await?;
        }
        Ok(())
    }
}
