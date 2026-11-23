//! `repositories.is_template` — marks a repository as a source for new ones.
//!
//! A plain boolean rather than a separate table: a template is a repository
//! with one extra bit of meaning, and keeping it in place means the existing
//! visibility, permission and deletion paths apply to it unchanged.
//!
//! Existing rows default to `false`, so nothing becomes a template by accident.

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261123_000004_add_repo_template_flag"
    }
}

const REPOSITORIES: &str = "repositories";

#[derive(DeriveIden)]
enum Repositories {
    #[sea_orm(iden = "repositories")]
    Table,
    IsTemplate,
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        if !manager.has_column(REPOSITORIES, "is_template").await? {
            manager
                .alter_table(
                    Table::alter()
                        .table(Repositories::Table)
                        .add_column(
                            ColumnDef::new(Repositories::IsTemplate)
                                .boolean()
                                .not_null()
                                .default(false),
                        )
                        .to_owned(),
                )
                .await?;
        }
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        if manager.has_column(REPOSITORIES, "is_template").await? {
            manager
                .alter_table(
                    Table::alter()
                        .table(Repositories::Table)
                        .drop_column(Repositories::IsTemplate)
                        .to_owned(),
                )
                .await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::{ConnectionTrait, Database, Statement};
    use sea_orm_migration::MigratorTrait;

    async fn column_exists(db: &sea_orm::DatabaseConnection, table: &str, column: &str) -> bool {
        let sql =
            format!("SELECT COUNT(*) FROM pragma_table_info('{table}') WHERE name = '{column}'");
        let row = db
            .query_one(Statement::from_string(db.get_database_backend(), sql))
            .await
            .unwrap()
            .unwrap();
        row.try_get_by_index::<i64>(0).unwrap() == 1
    }

    /// `pragma_table_info.dflt_value` is NULL when the column has no default.
    async fn column_has_default(
        db: &sea_orm::DatabaseConnection,
        table: &str,
        column: &str,
    ) -> bool {
        let sql =
            format!("SELECT dflt_value FROM pragma_table_info('{table}') WHERE name = '{column}'");
        let row = db
            .query_one(Statement::from_string(db.get_database_backend(), sql))
            .await
            .unwrap()
            .unwrap();
        row.try_get_by_index::<Option<String>>(0).unwrap().is_some()
    }

    #[tokio::test]
    async fn the_column_is_added_with_a_false_default() {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        crate::migrations::Migrator::up(&db, None).await.unwrap();

        assert!(column_exists(&db, REPOSITORIES, "is_template").await);
        // NOT NULL with a default is what makes existing rows read as `false`
        // without a backfill.
        assert!(
            column_has_default(&db, REPOSITORIES, "is_template").await,
            "a NOT NULL column added to a populated table needs a default"
        );

        // Adding again must be a no-op (the guard, not the column, is what the
        // idempotency test is about).
        crate::migrations::Migrator::up(&db, None).await.unwrap();
        assert!(column_exists(&db, REPOSITORIES, "is_template").await);
    }

    #[tokio::test]
    async fn down_drops_the_column_and_up_puts_it_back() {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        crate::migrations::Migrator::up(&db, None).await.unwrap();
        let steps = {
            let migrations = crate::migrations::Migrator::migrations();
            let position = migrations
                .iter()
                .rev()
                .position(|migration| migration.name() == "m20261123_000004_add_repo_template_flag")
                .expect("this migration must be registered");
            (position + 1) as u32
        };

        crate::migrations::Migrator::down(&db, Some(steps))
            .await
            .unwrap();
        assert!(!column_exists(&db, REPOSITORIES, "is_template").await);

        crate::migrations::Migrator::up(&db, None).await.unwrap();
        assert!(column_exists(&db, REPOSITORIES, "is_template").await);
    }
}
