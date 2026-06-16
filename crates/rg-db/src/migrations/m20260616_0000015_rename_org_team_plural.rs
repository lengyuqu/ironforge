//! Corrective migration: phase8 (m20260424_000009) created the org/team
//! tables via `#[derive(Iden)]`, which produced **singular** table names
//! (`organization`, `organization_member`, `team`, `team_member`). The
//! SeaORM entities, however, declare **plural** `table_name`s
//! (`organizations`, `organization_members`, `teams`, `team_members`), so
//! every org/team query failed at runtime with "no such table". The
//! notification table hit the same bug and was patched separately in
//! m20260511_000002; this migration fixes the remaining four tables.
//!
//! Strategy: rename the singular table to its plural name when (and only
//! when) the singular table exists and the plural one does not yet. This is
//! idempotent and safe on both fresh databases (phase8 just created the
//! singular tables) and any database already carrying the broken schema.
use sea_orm::Statement;
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260616_0000015_rename_org_team_plural"
    }
}

const RENAMES: &[(&str, &str)] = &[
    ("organization", "organizations"),
    ("organization_member", "organization_members"),
    ("team", "teams"),
    ("team_member", "team_members"),
];

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        let backend = db.get_database_backend();

        for (old, new) in RENAMES {
            if manager.has_table(*old).await? && !manager.has_table(*new).await? {
                db.execute(Statement::from_string(
                    backend,
                    format!("ALTER TABLE \"{old}\" RENAME TO \"{new}\";"),
                ))
                .await?;
            }
        }

        // Drop the dead singular `notification` table left over from phase8;
        // the live plural `notifications` table is created by m20260511_000002.
        if manager.has_table("notification").await? && manager.has_table("notifications").await? {
            db.execute(Statement::from_string(
                backend,
                "DROP TABLE \"notification\";".to_string(),
            ))
            .await?;
        }

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        let backend = db.get_database_backend();

        for (old, new) in RENAMES {
            if manager.has_table(*new).await? && !manager.has_table(*old).await? {
                db.execute(Statement::from_string(
                    backend,
                    format!("ALTER TABLE \"{new}\" RENAME TO \"{old}\";"),
                ))
                .await?;
            }
        }

        Ok(())
    }
}
