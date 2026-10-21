//! Scope repository-name uniqueness by namespace.
//!
//! `m20261015_000002` left one partial unique index,
//! `UNIQUE (owner_id, name) WHERE deleted_at IS NULL`. That key is wrong for
//! organization repositories, whose `owner_id` is only the member who created
//! them:
//!
//! * two members of one org could each create the same org repo name
//!   (different `owner_id`, so no conflict), and
//! * a user's personal repo `alice/x` and org repo `acme/x` (both
//!   `owner_id = alice`) were rejected as duplicates although they live in
//!   different namespaces and on different disk paths.
//!
//! A repository's namespace is its organization when it has one, otherwise its
//! owning user, so uniqueness is split accordingly:
//!
//! ```sql
//! CREATE UNIQUE INDEX uq_repositories_personal_name_live
//!     ON repositories (owner_id, name) WHERE deleted_at IS NULL AND org_id IS NULL;
//! CREATE UNIQUE INDEX uq_repositories_org_name_live
//!     ON repositories (org_id, name)   WHERE deleted_at IS NULL AND org_id IS NOT NULL;
//! ```
//!
//! Existing rows that already violate the new keys (possible only if two
//! members created the same org repo name) are listed and the migration stops;
//! it never picks a repository to drop.
//!
//! MySQL has no partial indexes, so there the application-level check
//! (`repo::service::repo_name_taken`) is the only enforcement, exactly as it
//! was after `m20261015_000002`.

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261022_000001_scope_repo_names_by_namespace"
    }
}

const OLD_INDEX: &str = "uq_repositories_owner_name_live";
const PERSONAL_INDEX: &str = "uq_repositories_personal_name_live";
const ORG_INDEX: &str = "uq_repositories_org_name_live";

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let backend = manager.get_database_backend();
        if backend == DatabaseBackend::MySql || !manager.has_table("repositories").await? {
            return Ok(());
        }
        let connection = manager.get_connection();

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
                .query_all(Statement::from_string(backend, sql))
                .await?;
            for row in rows {
                let ns: i64 = row.try_get_by_index(0)?;
                let name: String = row.try_get_by_index(1)?;
                let n: i64 = row.try_get_by_index(2)?;
                problems.push(format!("{label} namespace {ns}: '{name}' x{n}"));
            }
        }
        if !problems.is_empty() {
            return Err(DbErr::Custom(format!(
                "cannot enforce per-namespace repository name uniqueness: duplicate live \
                 repositories exist ({}). Soft-delete or rename the extras, then re-run the \
                 migration.",
                problems.join("; ")
            )));
        }

        // New keys first, old key second: there is never a window without a
        // uniqueness guarantee.
        connection
            .execute_unprepared(&format!(
                "CREATE UNIQUE INDEX IF NOT EXISTS {PERSONAL_INDEX} \
                 ON repositories (owner_id, name) WHERE deleted_at IS NULL AND org_id IS NULL"
            ))
            .await?;
        connection
            .execute_unprepared(&format!(
                "CREATE UNIQUE INDEX IF NOT EXISTS {ORG_INDEX} \
                 ON repositories (org_id, name) WHERE deleted_at IS NULL AND org_id IS NOT NULL"
            ))
            .await?;
        connection
            .execute_unprepared(&format!("DROP INDEX IF EXISTS {OLD_INDEX}"))
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let backend = manager.get_database_backend();
        if backend == DatabaseBackend::MySql || !manager.has_table("repositories").await? {
            return Ok(());
        }
        let connection = manager.get_connection();
        // Fails (leaving everything as it was) if a personal and an org repo
        // now share a name for the same user — that is what the old key forbade.
        connection
            .execute_unprepared(&format!(
                "CREATE UNIQUE INDEX IF NOT EXISTS {OLD_INDEX} \
                 ON repositories (owner_id, name) WHERE deleted_at IS NULL"
            ))
            .await?;
        connection
            .execute_unprepared(&format!("DROP INDEX IF EXISTS {PERSONAL_INDEX}"))
            .await?;
        connection
            .execute_unprepared(&format!("DROP INDEX IF EXISTS {ORG_INDEX}"))
            .await?;
        Ok(())
    }
}
