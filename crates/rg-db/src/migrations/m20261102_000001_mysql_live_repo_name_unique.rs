//! MySQL: let the database enforce live repository-name uniqueness per namespace.
//!
//! SQLite and PostgreSQL got this from `m20261022_000001` as two *partial*
//! unique indexes (`WHERE deleted_at IS NULL ...`). MySQL has no partial
//! indexes, so until now only the application check (`repo_name_taken`) stood
//! between two concurrent creators and two live repositories with one name.
//!
//! MySQL does allow what partial indexes are normally used for: a unique index
//! never conflicts on rows where any indexed column is `NULL`. Two virtual
//! generated columns are therefore `NULL` exactly when a row does not take part
//! in a namespace's uniqueness, and the unique indexes include them:
//!
//! ```sql
//! live_personal_owner_id = IF(deleted_at IS NULL AND org_id IS NULL, owner_id, NULL)
//! live_org_id            = IF(deleted_at IS NULL, org_id, NULL)
//!
//! UNIQUE (live_personal_owner_id, name)   -- personal namespace
//! UNIQUE (live_org_id, name)              -- organization namespace
//! ```
//!
//! This mirrors the two PostgreSQL/SQLite keys, so a soft-deleted repository
//! frees its name and an organization's repositories are unique per
//! organization rather than per creator. Name comparison uses the column's own
//! collation, the same one the application's `WHERE name = ?` lookup uses, so
//! the database and the check agree on what "the same name" means.
//!
//! If live duplicates already exist the migration stops and lists them. It never
//! picks a repository to drop or rename.
//!
//! Every step is guarded, so a run that stopped half way (MySQL DDL is not
//! transactional) or a rollback-then-reapply cycle can simply be run again.
//! SQLite and PostgreSQL are untouched.

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261102_000001_mysql_live_repo_name_unique"
    }
}

const TABLE: &str = "repositories";
const PERSONAL_COLUMN: &str = "live_personal_owner_id";
const ORG_COLUMN: &str = "live_org_id";
const PERSONAL_INDEX: &str = "uq_repositories_personal_name_live";
const ORG_INDEX: &str = "uq_repositories_org_name_live";

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        if manager.get_database_backend() != DatabaseBackend::MySql
            || !manager.has_table(TABLE).await?
        {
            return Ok(());
        }
        let connection = manager.get_connection();

        reject_existing_duplicates(connection).await?;

        // Columns first, then the indexes over them. Each step is skipped when
        // already done, so the whole `up` can be repeated.
        if !manager.has_column(TABLE, PERSONAL_COLUMN).await? {
            connection
                .execute_unprepared(&format!(
                    "ALTER TABLE {TABLE} ADD COLUMN {PERSONAL_COLUMN} BIGINT \
                     GENERATED ALWAYS AS (IF(deleted_at IS NULL AND org_id IS NULL, owner_id, NULL)) VIRTUAL"
                ))
                .await?;
        }
        if !manager.has_column(TABLE, ORG_COLUMN).await? {
            connection
                .execute_unprepared(&format!(
                    "ALTER TABLE {TABLE} ADD COLUMN {ORG_COLUMN} BIGINT \
                     GENERATED ALWAYS AS (IF(deleted_at IS NULL, org_id, NULL)) VIRTUAL"
                ))
                .await?;
        }
        if !manager.has_index(TABLE, PERSONAL_INDEX).await? {
            connection
                .execute_unprepared(&format!(
                    "CREATE UNIQUE INDEX {PERSONAL_INDEX} ON {TABLE} ({PERSONAL_COLUMN}, name)"
                ))
                .await?;
        }
        if !manager.has_index(TABLE, ORG_INDEX).await? {
            connection
                .execute_unprepared(&format!(
                    "CREATE UNIQUE INDEX {ORG_INDEX} ON {TABLE} ({ORG_COLUMN}, name)"
                ))
                .await?;
        }
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        if manager.get_database_backend() != DatabaseBackend::MySql
            || !manager.has_table(TABLE).await?
        {
            return Ok(());
        }
        let connection = manager.get_connection();
        // Indexes before the columns they cover.
        for index in [PERSONAL_INDEX, ORG_INDEX] {
            if manager.has_index(TABLE, index).await? {
                connection
                    .execute_unprepared(&format!("DROP INDEX {index} ON {TABLE}"))
                    .await?;
            }
        }
        for column in [PERSONAL_COLUMN, ORG_COLUMN] {
            if manager.has_column(TABLE, column).await? {
                connection
                    .execute_unprepared(&format!("ALTER TABLE {TABLE} DROP COLUMN {column}"))
                    .await?;
            }
        }
        Ok(())
    }
}

/// Stop, listing them, if two live repositories already share a name in one
/// namespace; creating the indexes would fail on them anyway, and a readable
/// list is what the operator needs to resolve it.
async fn reject_existing_duplicates(connection: &impl ConnectionTrait) -> Result<(), DbErr> {
    let mut problems = Vec::new();
    for (label, sql) in [
        (
            "personal",
            "SELECT owner_id AS ns, name, COUNT(*) AS n FROM repositories \
             WHERE deleted_at IS NULL AND org_id IS NULL \
             GROUP BY owner_id, name HAVING COUNT(*) > 1",
        ),
        (
            "organization",
            "SELECT org_id AS ns, name, COUNT(*) AS n FROM repositories \
             WHERE deleted_at IS NULL AND org_id IS NOT NULL \
             GROUP BY org_id, name HAVING COUNT(*) > 1",
        ),
    ] {
        let rows = connection
            .query_all(Statement::from_string(DatabaseBackend::MySql, sql))
            .await?;
        for row in rows {
            let ns: i64 = row.try_get_by_index(0)?;
            let name: String = row.try_get_by_index(1)?;
            let n: i64 = row.try_get_by_index(2)?;
            problems.push(format!("{label} namespace {ns}: '{name}' x{n}"));
        }
    }
    if problems.is_empty() {
        return Ok(());
    }
    Err(DbErr::Custom(format!(
        "cannot enforce per-namespace repository name uniqueness on MySQL: duplicate live \
         repositories exist ({}). Soft-delete or rename the extras, then start the server \
         again.",
        problems.join("; ")
    )))
}
