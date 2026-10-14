//! Make the `repositories` name uniqueness ignore soft-deleted rows.
//!
//! `repositories` has `UNIQUE (owner_id, name)`, but rows are only soft-deleted
//! (`deleted_at` is set). A deleted repository therefore keeps its name reserved
//! forever: `repo_ops::find_by_owner_and_name` filters `deleted_at IS NULL` and
//! reports the name as free, while the insert then fails on the constraint.
//!
//! The fix is a partial index that only covers live rows:
//!
//! ```sql
//! CREATE UNIQUE INDEX uq_repositories_owner_name_live
//!     ON repositories (owner_id, name) WHERE deleted_at IS NULL
//! ```
//!
//! Backend handling differs because the original constraint is declared
//! differently on each:
//!
//! - **PostgreSQL / MySQL** support dropping a named constraint. PostgreSQL
//!   cannot add an index with a `WHERE` predicate through SeaORM's portable
//!   builder, so the SQL is issued directly; MySQL has no partial indexes at
//!   all and falls back to a plain unique index.
//! - **SQLite** cannot drop an inline `UNIQUE`; the table has to be rebuilt.
//!   SQLite also ignores `ON DELETE` clauses on the copy, so foreign keys are
//!   disabled for the rebuild and re-checked afterwards.

use sea_orm::{DatabaseBackend, Statement, TransactionTrait};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261015_000002_repositories_partial_unique_index"
    }
}

/// Columns of `repositories`, in the order the entity declares them.
const COLUMNS: &[(&str, &str)] = &[
    ("id", "integer NOT NULL PRIMARY KEY AUTOINCREMENT"),
    ("owner_id", "bigint NOT NULL"),
    ("name", "varchar NOT NULL"),
    ("description", "text NULL"),
    ("is_private", "boolean NOT NULL DEFAULT FALSE"),
    ("default_branch", "varchar NOT NULL DEFAULT 'main'"),
    ("fork_id", "bigint NULL"),
    ("stars_count", "bigint NOT NULL DEFAULT 0"),
    ("forks_count", "bigint NOT NULL DEFAULT 0"),
    ("created_at", "timestamp_with_timezone_text NOT NULL"),
    ("updated_at", "timestamp_with_timezone_text NOT NULL"),
    ("org_id", "bigint NULL"),
    ("deleted_at", "TIMESTAMP NULL"),
    ("origin_repo_id", "BIGINT NULL"),
];

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        match manager.get_database_backend() {
            DatabaseBackend::Sqlite => sqlite_up(manager).await,
            DatabaseBackend::Postgres => postgres_up(manager).await,
            DatabaseBackend::MySql => mysql_up(manager).await,
        }
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        match manager.get_database_backend() {
            DatabaseBackend::Sqlite => sqlite_down(manager).await,
            DatabaseBackend::Postgres => {
                manager
                    .get_connection()
                    .execute_unprepared(
                        "DROP INDEX IF EXISTS uq_repositories_owner_name_live",
                    )
                    .await?;
                Ok(())
            }
            // MySQL dropped its original index; the fully-unique constraint is
            // not restored because that would require another table rebuild.
            DatabaseBackend::MySql => Ok(()),
        }
    }
}
/// SQLite: rebuild the table so the inline `UNIQUE` is replaced by a partial
/// unique index that ignores soft-deleted rows.
async fn sqlite_up(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    if !manager.has_table("repositories").await? {
        return Ok(());
    }
    let connection = manager.get_connection();

    // Defensive idempotence: skip if the new index already exists.
    let has_new_index = connection
        .query_one(Statement::from_string(
            DatabaseBackend::Sqlite,
            "SELECT 1 FROM sqlite_master WHERE type = 'index' AND name = 'uq_repositories_owner_name_live'",
        ))
        .await?
        .is_some();
    if has_new_index {
        return Ok(());
    }

    let column_defs = COLUMNS
        .iter()
        .map(|(name, definition)| format!("\"{name}\" {definition}"))
        .collect::<Vec<_>>()
        .join(", ");
    let column_names = COLUMNS
        .iter()
        .map(|(name, _)| format!("\"{name}\"")
        )
        .collect::<Vec<_>>()
        .join(", ");

    // Foreign keys are disabled around the rebuild: the copied table would
    // otherwise drop `ON DELETE CASCADE` (SQLite cannot express it in
    // `CREATE TABLE ... AS`), and dependent tables reference `repositories`.
    // The whole rebuild must run on ONE connection: `PRAGMA foreign_keys` is
    // per-connection, and a pooled `execute_unprepared` may pick a different
    // connection for every statement (which also makes the schema change
    // invisible to a connection holding a stale read snapshot). A SeaORM
    // transaction pins a single connection for the whole sequence.
    let transaction = connection.begin().await?;
    transaction
        .execute_unprepared("PRAGMA foreign_keys = OFF")
        .await?;
    transaction
        .execute_unprepared(&format!(
            "CREATE TABLE repositories_new ({column_defs}, FOREIGN KEY (\"owner_id\") REFERENCES \"users\" (\"id\") ON DELETE CASCADE)"
        ))
        .await?;
    transaction
        .execute_unprepared(&format!(
            "INSERT INTO repositories_new ({column_names}) SELECT {column_names} FROM repositories"
        ))
        .await?;
    transaction
        .execute_unprepared("DROP TABLE repositories")
        .await?;
    transaction
        .execute_unprepared("ALTER TABLE repositories_new RENAME TO repositories")
        .await?;
    transaction
        .execute_unprepared(
            "CREATE UNIQUE INDEX uq_repositories_owner_name_live ON repositories (owner_id, name) WHERE deleted_at IS NULL",
        )
        .await?;
    transaction
        .execute_unprepared("CREATE INDEX idx_repositories_org_id ON repositories (org_id)")
        .await?;
    transaction
        .execute_unprepared(
            "CREATE INDEX idx_repositories_origin_repo_id ON repositories (origin_repo_id)",
        )
        .await?;
    transaction.commit().await?;

    // Fail loudly if the rebuild silently broke referential integrity.
    let violations = connection
        .query_all(Statement::from_string(
            DatabaseBackend::Sqlite,
            "PRAGMA foreign_key_check",
        ))
        .await?;
    if !violations.is_empty() {
        return Err(DbErr::Custom(format!(
            "repositories rebuild left {} foreign-key violation(s)",
            violations.len()
        )));
    }

    Ok(())
}
/// SQLite rollback: restore the fully-unique constraint by rebuilding again.
async fn sqlite_down(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    if !manager.has_table("repositories").await? {
        return Ok(());
    }
    let connection = manager.get_connection();
    let has_new_index = connection
        .query_one(Statement::from_string(
            DatabaseBackend::Sqlite,
            "SELECT 1 FROM sqlite_master WHERE type = 'index' AND name = 'uq_repositories_owner_name_live'",
        ))
        .await?
        .is_some();
    if !has_new_index {
        return Ok(());
    }

    let column_defs = COLUMNS
        .iter()
        .map(|(name, definition)| format!("\"{name}\" {definition}"))
        .collect::<Vec<_>>()
        .join(", ");
    let column_names = COLUMNS
        .iter()
        .map(|(name, _)| format!("\"{name}\"")
        )
        .collect::<Vec<_>>()
        .join(", ");

    // Same single-connection requirement as `sqlite_up`.
    let transaction = connection.begin().await?;
    transaction
        .execute_unprepared("PRAGMA foreign_keys = OFF")
        .await?;
    transaction
        .execute_unprepared(&format!(
            "CREATE TABLE repositories_old ({column_defs}, UNIQUE (\"owner_id\", \"name\"), FOREIGN KEY (\"owner_id\") REFERENCES \"users\" (\"id\") ON DELETE CASCADE)"
        ))
        .await?;
    transaction
        .execute_unprepared(&format!(
            "INSERT INTO repositories_old ({column_names}) SELECT {column_names} FROM repositories"
        ))
        .await?;
    transaction
        .execute_unprepared("DROP TABLE repositories")
        .await?;
    transaction
        .execute_unprepared("ALTER TABLE repositories_old RENAME TO repositories")
        .await?;
    transaction
        .execute_unprepared("CREATE INDEX idx_repositories_org_id ON repositories (org_id)")
        .await?;
    transaction
        .execute_unprepared(
            "CREATE INDEX idx_repositories_origin_repo_id ON repositories (origin_repo_id)",
        )
        .await?;
    transaction.commit().await?;
    Ok(())
}

/// PostgreSQL: drop the named constraint and add a partial unique index.
async fn postgres_up(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    if !manager.has_table("repositories").await? {
        return Ok(());
    }
    let connection = manager.get_connection();
    // `ALTER TABLE ... DROP CONSTRAINT IF EXISTS` makes the step idempotent
    // regardless of the constraint name SeaORM generated.
    connection
        .execute_unprepared(
            "ALTER TABLE repositories DROP CONSTRAINT IF EXISTS repositories_owner_id_name_key",
        )
        .await?;
    connection
        .execute_unprepared(
            "CREATE UNIQUE INDEX IF NOT EXISTS uq_repositories_owner_name_live \
             ON repositories (owner_id, name) WHERE deleted_at IS NULL",
        )
        .await?;
    Ok(())
}

/// MySQL: no partial indexes. Drop the constraint so deleted names can be
/// reused, accepting that live names are no longer database-enforced.
async fn mysql_up(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    if !manager.has_table("repositories").await? {
        return Ok(());
    }
    manager
        .get_connection()
        .execute_unprepared("ALTER TABLE repositories DROP INDEX repositories_owner_id_name_key")
        .await?;
    Ok(())
}