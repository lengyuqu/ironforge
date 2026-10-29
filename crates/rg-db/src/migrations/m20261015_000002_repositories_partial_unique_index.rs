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
//!   See [`rebuild_sqlite`] for why that rebuild must run on one pinned
//!   connection with foreign keys switched off *outside* any transaction:
//!   `PRAGMA foreign_keys` is a silent no-op inside a transaction, and with
//!   enforcement still on, `DROP TABLE repositories` performs an implicit
//!   `DELETE` that fires every `ON DELETE CASCADE` child (stars, labels,
//!   releases, secrets, ...) and wipes them.

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261015_000002_repositories_partial_unique_index"
    }
}

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
const LIVE_INDEX: &str = "uq_repositories_owner_name_live";
const UNIQUE_CLAUSE: &str = ", UNIQUE (\"owner_id\", \"name\")";
const FK_CLAUSE: &str = ", FOREIGN KEY (\"owner_id\")";

fn custom(context: &str, error: impl std::fmt::Display) -> DbErr {
    DbErr::Custom(format!("{context}: {error}"))
}

/// SQLite: rebuild the table so the inline `UNIQUE` is replaced by a partial
/// unique index that ignores soft-deleted rows.
async fn sqlite_up(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    if !manager.has_table("repositories").await? {
        return Ok(());
    }
    rebuild_sqlite(
        manager,
        |ddl| {
            if !ddl.contains(UNIQUE_CLAUSE) {
                // Already without the inline constraint (e.g. a partial run):
                // nothing to rebuild, only the index below is needed.
                return Ok(None);
            }
            Ok(Some(ddl.replacen(UNIQUE_CLAUSE, "", 1)))
        },
        &[format!(
            "CREATE UNIQUE INDEX IF NOT EXISTS {LIVE_INDEX} ON repositories (owner_id, name) WHERE deleted_at IS NULL"
        )],
        &[],
    )
    .await
}

/// SQLite rollback: restore the fully-unique constraint by rebuilding again.
async fn sqlite_down(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    if !manager.has_table("repositories").await? {
        return Ok(());
    }
    rebuild_sqlite(
        manager,
        |ddl| {
            if ddl.contains(UNIQUE_CLAUSE) {
                return Ok(None);
            }
            if !ddl.contains(FK_CLAUSE) {
                return Err(DbErr::Custom(
                    "cannot restore UNIQUE (owner_id, name): unexpected repositories DDL".into(),
                ));
            }
            Ok(Some(ddl.replacen(
                FK_CLAUSE,
                &format!("{UNIQUE_CLAUSE}{FK_CLAUSE}"),
                1,
            )))
        },
        &[],
        &[LIVE_INDEX],
    )
    .await
}

/// Rebuild `repositories` (SQLite's documented 12-step "ALTER TABLE" procedure).
///
/// * `transform` receives the live `CREATE TABLE` text and returns the new one
///   (`None` = no structural change needed). Deriving it from the live DDL —
///   rather than a hand-written column list — preserves every column type,
///   default and foreign key exactly.
/// * `after` statements run once the new table is in place (e.g. the partial
///   index).
/// * `skip_index_names` are indexes of the old table that must NOT be carried
///   over.
///
/// Correctness requirements, each of which was a real defect in the previous
/// version of this migration:
///
/// 1. **One pinned connection.** `PRAGMA foreign_keys` is per-connection; the
///    migrator hands us the pool, so a pooled `execute_unprepared` may land on a
///    different connection for every statement.
/// 2. **Foreign keys off outside a transaction.** Inside `BEGIN` the pragma is
///    ignored. With enforcement left on, `DROP TABLE` implicitly deletes every
///    row first and cascades into ~17 child tables.
/// 3. **Triggers and indexes are re-created.** They belong to the dropped table;
///    losing the `repos_fts_*` triggers silently stops the search index from
///    following updates and deletes.
/// 4. **Verify, then commit.** Row counts must match and `foreign_key_check`
///    must be clean, otherwise everything is rolled back.
/// 5. **Restore the pragma** before the connection goes back to the pool.
async fn rebuild_sqlite<F>(
    manager: &SchemaManager<'_>,
    transform: F,
    after: &[String],
    skip_index_names: &[&str],
) -> Result<(), DbErr>
where
    F: Fn(&str) -> Result<Option<String>, DbErr>,
{
    use sea_orm::sqlx::{query, query_scalar};

    let pool = match manager.get_connection() {
        SchemaManagerConnection::Connection(db) => db.get_sqlite_connection_pool(),
        SchemaManagerConnection::Transaction(_) => {
            return Err(DbErr::Custom(
                "repositories rebuild must not run inside a transaction \
                 (PRAGMA foreign_keys cannot be disabled there)"
                    .into(),
            ))
        }
    };
    let mut conn = pool
        .acquire()
        .await
        .map_err(|e| custom("acquire sqlite connection", e))?;

    let ddl: String = query_scalar(
        "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'repositories'",
    )
    .fetch_one(&mut *conn)
    .await
    .map_err(|e| custom("read repositories DDL", e))?;
    let new_ddl = transform(&ddl)?;

    // Objects that die with the old table and must be recreated.
    let attached: Vec<String> = query_scalar(
        "SELECT sql FROM sqlite_master \
         WHERE tbl_name = 'repositories' AND type IN ('index', 'trigger') AND sql IS NOT NULL \
         ORDER BY type DESC, name",
    )
    .fetch_all(&mut *conn)
    .await
    .map_err(|e| custom("read repositories indexes/triggers", e))?
    .into_iter()
    .filter(|sql: &String| !skip_index_names.iter().any(|name| sql.contains(name)))
    .collect();

    query("PRAGMA foreign_keys = OFF")
        .execute(&mut *conn)
        .await
        .map_err(|e| custom("disable foreign keys", e))?;
    let fk_state: i64 = query_scalar("PRAGMA foreign_keys")
        .fetch_one(&mut *conn)
        .await
        .map_err(|e| custom("read foreign_keys pragma", e))?;

    let outcome = if fk_state != 0 {
        Err(DbErr::Custom(
            "could not disable foreign keys; refusing to rebuild repositories \
             (it would cascade-delete child rows)"
                .into(),
        ))
    } else {
        run_rebuild(&mut conn, new_ddl.as_deref(), &attached, after).await
    };

    // Always restore enforcement before the connection returns to the pool.
    let restored = query("PRAGMA foreign_keys = ON").execute(&mut *conn).await;
    if restored.is_err() {
        // Never hand a connection with enforcement off back to the pool.
        conn.close_on_drop();
    }
    outcome?;
    restored.map_err(|e| custom("re-enable foreign keys", e))?;
    Ok(())
}

async fn run_rebuild(
    conn: &mut sea_orm::sqlx::pool::PoolConnection<sea_orm::sqlx::Sqlite>,
    new_ddl: Option<&str>,
    attached: &[String],
    after: &[String],
) -> Result<(), DbErr> {
    use sea_orm::sqlx::{query, query_scalar};

    query("BEGIN IMMEDIATE")
        .execute(&mut **conn)
        .await
        .map_err(|e| custom("begin", e))?;

    let result: Result<(), DbErr> = async {
        if let Some(ddl) = new_ddl {
            let before: i64 = query_scalar("SELECT COUNT(*) FROM repositories")
                .fetch_one(&mut **conn)
                .await
                .map_err(|e| custom("count rows", e))?;

            let create = ddl.replacen(
                "CREATE TABLE \"repositories\"",
                "CREATE TABLE \"repositories_new\"",
                1,
            );
            if create == ddl {
                return Err(DbErr::Custom(
                    "unexpected repositories DDL: cannot derive the replacement table".into(),
                ));
            }
            query(&create)
                .execute(&mut **conn)
                .await
                .map_err(|e| custom("create repositories_new", e))?;
            query("INSERT INTO repositories_new SELECT * FROM repositories")
                .execute(&mut **conn)
                .await
                .map_err(|e| custom("copy rows", e))?;

            let copied: i64 = query_scalar("SELECT COUNT(*) FROM repositories_new")
                .fetch_one(&mut **conn)
                .await
                .map_err(|e| custom("count copied rows", e))?;
            if copied != before {
                return Err(DbErr::Custom(format!(
                    "repositories copy lost rows ({copied} of {before})"
                )));
            }

            query("DROP TABLE repositories")
                .execute(&mut **conn)
                .await
                .map_err(|e| custom("drop repositories", e))?;
            query("ALTER TABLE repositories_new RENAME TO repositories")
                .execute(&mut **conn)
                .await
                .map_err(|e| custom("rename repositories_new", e))?;
            for sql in attached {
                query(sql)
                    .execute(&mut **conn)
                    .await
                    .map_err(|e| custom("recreate index/trigger", e))?;
            }
        }
        for sql in after {
            query(sql)
                .execute(&mut **conn)
                .await
                .map_err(|e| custom("post-rebuild statement", e))?;
        }

        let violations = query("PRAGMA foreign_key_check")
            .fetch_all(&mut **conn)
            .await
            .map_err(|e| custom("foreign_key_check", e))?;
        if !violations.is_empty() {
            return Err(DbErr::Custom(format!(
                "repositories rebuild left {} foreign-key violation(s)",
                violations.len()
            )));
        }
        Ok(())
    }
    .await;

    match result {
        Ok(()) => {
            query("COMMIT")
                .execute(&mut **conn)
                .await
                .map_err(|e| custom("commit", e))?;
            Ok(())
        }
        Err(error) => {
            let _ = query("ROLLBACK").execute(&mut **conn).await;
            Err(error)
        }
    }
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
///
/// The original `UNIQUE (owner_id, name)` is declared without a name, and MySQL
/// names such an index after its first column (`owner_id`), not
/// `repositories_owner_id_name_key` like PostgreSQL. So the index is found by
/// its columns. The `owner_id` foreign key is using it, so a plain index takes
/// over that job first; MySQL refuses to drop an index a foreign key needs.
async fn mysql_up(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    if !manager.has_table("repositories").await? {
        return Ok(());
    }
    let connection = manager.get_connection();
    let rows = connection
        .query_all(Statement::from_string(
            DatabaseBackend::MySql,
            "SELECT INDEX_NAME FROM information_schema.STATISTICS \
             WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = 'repositories' \
               AND NON_UNIQUE = 0 \
             GROUP BY INDEX_NAME \
             HAVING GROUP_CONCAT(COLUMN_NAME ORDER BY SEQ_IN_INDEX) = 'owner_id,name'",
        ))
        .await?;
    // Already dropped (or never created): nothing to do, so a re-run is safe.
    let Some(row) = rows.first() else {
        return Ok(());
    };
    let index_name: String = row.try_get_by_index(0)?;

    if !manager
        .has_index("repositories", "idx_repositories_owner_id")
        .await?
    {
        connection
            .execute_unprepared(
                "CREATE INDEX idx_repositories_owner_id ON repositories (owner_id)",
            )
            .await?;
    }
    connection
        .execute_unprepared(&format!(
            "ALTER TABLE repositories DROP INDEX `{}`",
            index_name.replace('`', "``")
        ))
        .await?;
    Ok(())
}
