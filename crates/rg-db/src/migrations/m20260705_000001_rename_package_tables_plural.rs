//! Corrective migration: m20260607_000005 used `#[derive(Iden)]` for package
//! tables, which creates singular table names for `Package`, `PackageVersion`
//! and `PackageFile`. The SeaORM entities use plural table names
//! (`packages`, `package_versions`, `package_files`), so package registry
//! queries fail at runtime on a fresh database.
//!
//! This mirrors the org/team corrective migration: rename the broken singular
//! table only when it exists and the plural table does not.

use sea_orm::Statement;
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260705_000001_rename_package_tables_plural"
    }
}

const RENAMES: &[(&str, &str)] = &[
    ("package", "packages"),
    ("package_version", "package_versions"),
    ("package_file", "package_files"),
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
