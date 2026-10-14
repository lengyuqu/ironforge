//! IronForge database layer — SeaORM + SQLite.
//!
//! # Usage
//!
//! ```rust,no_run
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     let db = rg_db::connect("sqlite:///tmp/ironforge/ironforge.db?mode=rwc").await?;
//!     rg_db::run_migrations(&db).await?;
//!     Ok(())
//! }
//! ```

pub mod entities;
pub mod migrations;
pub mod ops;

use std::str::FromStr;
use std::time::Duration;

use anyhow::{Context, Result};
use sea_orm::sqlx::sqlite::{
    SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous,
};
use sea_orm::SqlxSqliteConnector;
use sea_orm_migration::MigratorTrait;

pub use sea_orm;
pub use sea_orm::DatabaseConnection;

/// Which database backend a `database_url` selects, inferred from its scheme.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DbBackend {
    /// SQLite (embedded, zero-dependency, default).
    Sqlite,
    /// PostgreSQL.
    Postgres,
    /// MySQL / MariaDB.
    MySql,
}

/// Redact a password embedded in a database URL before logging or returning
/// the URL in an error. Usernames, hosts, ports, database names and query
/// options remain visible for diagnostics.
pub fn redact_database_url(db_url: &str) -> String {
    let Some(scheme_end) = db_url.find("://") else {
        return "<invalid database URL>".to_string();
    };
    let authority_start = scheme_end + 3;
    let authority_end = db_url[authority_start..]
        .find(['/', '?', '#'])
        .map(|offset| authority_start + offset)
        .unwrap_or(db_url.len());
    let authority = &db_url[authority_start..authority_end];
    let Some(at) = authority.rfind('@') else {
        return db_url.to_string();
    };
    let user_info = &authority[..at];
    let Some(password_separator) = user_info.find(':') else {
        return db_url.to_string();
    };

    format!(
        "{}{}:***@{}",
        &db_url[..authority_start],
        &user_info[..password_separator],
        &db_url[authority_start + at + 1..]
    )
}

/// Infer the backend from a `database_url` scheme.
///
/// Accepted schemes: `sqlite://` (`sqlite3://`), `postgres://` / `postgresql://`,
/// `mysql://`. Anything else fails loudly so misconfiguration is caught early.
pub fn detect_backend(db_url: &str) -> Result<DbBackend> {
    if db_url.starts_with("sqlite:") || db_url.starts_with("sqlite3:") {
        Ok(DbBackend::Sqlite)
    } else if db_url.starts_with("postgres:") || db_url.starts_with("postgresql:") {
        Ok(DbBackend::Postgres)
    } else if db_url.starts_with("mysql:") {
        Ok(DbBackend::MySql)
    } else {
        anyhow::bail!(
            "unsupported database_url scheme in '{}': expected sqlite://, postgres://, or mysql://",
            redact_database_url(db_url)
        )
    }
}

/// Convert portable `?` bind markers in raw SQL to the backend's syntax.
///
/// SeaORM does not rewrite placeholders in `Statement::from_sql_and_values`:
/// PostgreSQL requires `$1`, `$2`, ... while SQLite and MySQL use `?`. Keep
/// raw SQL readable and portable by passing it through this helper first.
/// Question marks inside quoted strings or identifiers are left untouched.
pub fn prepare_sql(backend: sea_orm::DatabaseBackend, sql: &str) -> String {
    if backend != sea_orm::DatabaseBackend::Postgres {
        return sql.to_string();
    }

    let mut result = String::with_capacity(sql.len());
    let mut chars = sql.chars().peekable();
    let mut quote = None;
    let mut parameter = 1;

    while let Some(ch) = chars.next() {
        if let Some(active_quote) = quote {
            result.push(ch);
            if ch == active_quote {
                if chars.peek() == Some(&active_quote) {
                    result.push(chars.next().unwrap());
                } else {
                    quote = None;
                }
            }
            continue;
        }

        match ch {
            '\'' | '"' | '`' => {
                quote = Some(ch);
                result.push(ch);
            }
            '?' => {
                result.push('$');
                result.push_str(&parameter.to_string());
                parameter += 1;
            }
            _ => result.push(ch),
        }
    }

    result
}

/// Default DB connect (acquire) timeout (seconds).
pub const DEFAULT_CONNECT_TIMEOUT_SECS: u64 = 10;
/// Default DB idle timeout (seconds).
pub const DEFAULT_IDLE_TIMEOUT_SECS: u64 = 600;
/// Default max pool connections.
///
/// In WAL mode SQLite allows many concurrent readers alongside a single
/// writer, so a small pool lets read-heavy API/UI traffic run in parallel
/// instead of serialising on one connection. Writers still serialise at the
/// SQLite level; `busy_timeout` (set per connection) absorbs brief overlaps.
/// For Postgres/MySQL the pool is shared by a single server, so a small pool
/// is still appropriate for an embedded-style deployment.
pub const DEFAULT_MAX_CONNECTIONS: u32 = 5;

/// Connect to the database selected by `db_url`'s scheme (SQLite / Postgres / MySQL).
/// URL example: `sqlite:///path/to/db?mode=rwc`, `postgres://user@localhost/ironforge`.
pub async fn connect(db_url: &str) -> Result<DatabaseConnection> {
    connect_with_timeouts(
        db_url,
        DEFAULT_CONNECT_TIMEOUT_SECS,
        DEFAULT_IDLE_TIMEOUT_SECS,
    )
    .await
}

/// Connect with configurable connect/idle timeouts and the default pool size.
pub async fn connect_with_timeouts(
    db_url: &str,
    connect_secs: u64,
    idle_secs: u64,
) -> Result<DatabaseConnection> {
    connect_with_pool(db_url, connect_secs, idle_secs, DEFAULT_MAX_CONNECTIONS).await
}

/// Connect to the database with full pool control, dispatching on the URL scheme.
///
/// SQLite applies per-connection PRAGMAs (see [`connect_sqlite`]). Postgres/MySQL
/// use their own connect options, gated behind the `db-postgres` / `db-mysql`
/// cargo features so the default binary stays SQLite-only.
pub async fn connect_with_pool(
    db_url: &str,
    connect_secs: u64,
    idle_secs: u64,
    max_connections: u32,
) -> Result<DatabaseConnection> {
    let max_connections = max_connections.max(1);
    let backend = detect_backend(db_url)?;
    tracing::info!(url = %redact_database_url(db_url), ?backend, connect_secs, idle_secs, max_connections, "Connecting to database");

    match backend {
        DbBackend::Sqlite => connect_sqlite(db_url, connect_secs, idle_secs, max_connections).await,
        DbBackend::Postgres => {
            connect_postgres(db_url, connect_secs, idle_secs, max_connections).await
        }
        DbBackend::MySql => connect_mysql(db_url, connect_secs, idle_secs, max_connections).await,
    }
}

/// Connect to SQLite with PRAGMA optimization.
///
/// PRAGMAs are attached to the sqlx [`SqliteConnectOptions`] so they are applied
/// to **every** physical connection the pool opens — including reconnections
/// after an idle timeout. (The previous approach ran the PRAGMAs once after
/// connect, which silently lost per-connection settings such as `foreign_keys`
/// and `busy_timeout` whenever the pool re-established a connection.)
async fn connect_sqlite(
    db_url: &str,
    connect_secs: u64,
    idle_secs: u64,
    max_connections: u32,
) -> Result<DatabaseConnection> {
    // Per-connection options — applied on every connect by sqlx.
    let conn_opts = SqliteConnectOptions::from_str(db_url)
        .with_context(|| format!("invalid sqlite url: {db_url}"))?
        .create_if_missing(true) // honour `?mode=rwc`
        .journal_mode(SqliteJournalMode::Wal) // WAL for reader/writer concurrency
        .synchronous(SqliteSynchronous::Normal) // good balance for WAL
        .busy_timeout(Duration::from_secs(5)) // wait instead of immediate SQLITE_BUSY
        .foreign_keys(true) // enforce FK constraints
        .pragma("cache_size", "-64000") // 64MB page cache
        .pragma("temp_store", "MEMORY") // temp tables in RAM
        .pragma("mmap_size", "268435456"); // 256MB memory-mapped I/O

    // WAL allows concurrent readers; writers serialise (busy_timeout absorbs
    // brief overlaps). Keep min_connections low to stay light on idle.
    let pool = SqlitePoolOptions::new()
        .max_connections(max_connections)
        .min_connections(1)
        .acquire_timeout(Duration::from_secs(connect_secs))
        .idle_timeout(Duration::from_secs(idle_secs))
        .connect_with(conn_opts)
        .await
        .with_context(|| format!("failed to connect to database: {db_url}"))?;

    tracing::info!("Applied SQLite PRAGMAs via per-connection options");
    Ok(SqlxSqliteConnector::from_sqlx_sqlite_pool(pool))
}

/// Connect to PostgreSQL. The sqlx Postgres driver is always linked (SeaORM's
/// default features include it), so no cargo feature gate is required.
async fn connect_postgres(
    db_url: &str,
    connect_secs: u64,
    idle_secs: u64,
    max_connections: u32,
) -> Result<DatabaseConnection> {
    use sea_orm::sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use sea_orm::SqlxPostgresConnector;

    let conn_opts = PgConnectOptions::from_str(db_url)
        .with_context(|| format!("invalid postgres url: {}", redact_database_url(db_url)))?;
    let pool = PgPoolOptions::new()
        .max_connections(max_connections)
        .min_connections(1)
        .acquire_timeout(Duration::from_secs(connect_secs))
        .idle_timeout(Duration::from_secs(idle_secs))
        .connect_with(conn_opts)
        .await
        .with_context(|| {
            format!(
                "failed to connect to postgres: {}",
                redact_database_url(db_url)
            )
        })?;
    tracing::info!("Connected to PostgreSQL");
    Ok(SqlxPostgresConnector::from_sqlx_postgres_pool(pool))
}

/// Connect to MySQL / MariaDB. The sqlx MySQL driver is always linked (SeaORM's
/// default features include it), so no cargo feature gate is required.
async fn connect_mysql(
    db_url: &str,
    connect_secs: u64,
    idle_secs: u64,
    max_connections: u32,
) -> Result<DatabaseConnection> {
    use sea_orm::sqlx::mysql::{MySqlConnectOptions, MySqlPoolOptions};
    use sea_orm::SqlxMySqlConnector;

    let conn_opts = MySqlConnectOptions::from_str(db_url)
        .with_context(|| format!("invalid mysql url: {}", redact_database_url(db_url)))?;
    let pool = MySqlPoolOptions::new()
        .max_connections(max_connections)
        .min_connections(1)
        .acquire_timeout(Duration::from_secs(connect_secs))
        .idle_timeout(Duration::from_secs(idle_secs))
        .connect_with(conn_opts)
        .await
        .with_context(|| {
            format!(
                "failed to connect to mysql: {}",
                redact_database_url(db_url)
            )
        })?;
    tracing::info!("Connected to MySQL");
    Ok(SqlxMySqlConnector::from_sqlx_mysql_pool(pool))
}

/// Run all pending migrations.
pub async fn run_migrations(db: &DatabaseConnection) -> Result<()> {
    tracing::info!("Running database migrations");
    migrations::Migrator::up(db, None)
        .await
        .context("migration failed")?;
    Ok(())
}

/// Rebuild full-text search indexes from the main tables.
///
/// * **SQLite** — the FTS5 tables are independent virtual tables, so we
///   `DELETE` + re-`INSERT` from the source rows (mirroring the original FTS5
///   `rebuild` command).
/// * **Postgres / MySQL** — the FTS columns/tables are maintained
///   automatically (generated `tsvector` column / `FULLTEXT` index), so a
///   manual rebuild is unnecessary; we just refresh statistics.
pub async fn rebuild_fts_indexes(db: &DatabaseConnection) -> Result<()> {
    use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};

    let backend = db.get_database_backend();
    tracing::info!(?backend, "Rebuilding FTS indexes...");

    match backend {
        DatabaseBackend::Sqlite => {
            tracing::info!("  Rebuilding repos_fts...");
            db.execute(Statement::from_sql_and_values(
                backend,
                "DELETE FROM repos_fts",
                [],
            ))
            .await?;
            db.execute(Statement::from_sql_and_values(
                backend,
                "INSERT INTO repos_fts(rowid, name, description) SELECT id, name, description FROM repositories WHERE deleted_at IS NULL",
                [],
            )).await?;

            tracing::info!("  Rebuilding issues_fts...");
            db.execute(Statement::from_sql_and_values(
                backend,
                "DELETE FROM issues_fts",
                [],
            ))
            .await?;
            db.execute(Statement::from_sql_and_values(
                backend,
                "INSERT INTO issues_fts(rowid, title, body) SELECT id, title, COALESCE(body, '') FROM issues",
                [],
            )).await?;

            tracing::info!("  Rebuilding wiki_pages_fts...");
            db.execute(Statement::from_sql_and_values(
                backend,
                "DELETE FROM wiki_pages_fts",
                [],
            ))
            .await?;
            db.execute(Statement::from_sql_and_values(
                backend,
                "INSERT INTO wiki_pages_fts(rowid, title, content) SELECT id, title, content FROM wiki_pages",
                [],
            )).await?;

            tracing::info!("FTS5 indexes rebuilt successfully");
        }
        DatabaseBackend::Postgres => {
            for t in ["repos_fts", "issues_fts", "wiki_pages_fts"] {
                db.execute(Statement::from_string(backend, format!("ANALYZE {t}")))
                    .await?;
            }
            tracing::info!(
                "Postgres FTS columns are generated and self-maintaining; statistics refreshed."
            );
        }
        DatabaseBackend::MySql => {
            db.execute(Statement::from_string(
                backend,
                "OPTIMIZE TABLE repos_fts, issues_fts, wiki_pages_fts",
            ))
            .await?;
            tracing::info!("MySQL FULLTEXT indexes optimized.");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};

    #[test]
    fn database_urls_are_redacted_before_diagnostics() {
        assert_eq!(
            redact_database_url(
                "postgres://ironforge:super-secret@db.internal:5432/ironforge?sslmode=require"
            ),
            "postgres://ironforge:***@db.internal:5432/ironforge?sslmode=require"
        );
        assert_eq!(
            redact_database_url("mysql://root:p%40ss%3Aword@127.0.0.1:3306/ironforge"),
            "mysql://root:***@127.0.0.1:3306/ironforge"
        );
        assert_eq!(
            redact_database_url("postgres://ironforge@db.internal/ironforge"),
            "postgres://ironforge@db.internal/ironforge"
        );
        assert_eq!(
            redact_database_url("sqlite:///tmp/ironforge.db?mode=rwc"),
            "sqlite:///tmp/ironforge.db?mode=rwc"
        );

        let error = detect_backend("custom://root:top-secret@db/ironforge")
            .expect_err("unsupported scheme")
            .to_string();
        assert!(!error.contains("top-secret"));
        assert!(error.contains("root:***@db"));
    }

    #[test]
    fn postgres_raw_sql_placeholders_are_numbered() {
        assert_eq!(
            prepare_sql(
                DatabaseBackend::Postgres,
                "SELECT '?' AS literal, id FROM t WHERE a = ? AND b = ?"
            ),
            "SELECT '?' AS literal, id FROM t WHERE a = $1 AND b = $2"
        );
        assert_eq!(
            prepare_sql(DatabaseBackend::MySql, "SELECT * FROM t WHERE id = ?"),
            "SELECT * FROM t WHERE id = ?"
        );
    }

    /// Regression guard: every connection handed out by the pool must have the
    /// per-connection PRAGMAs applied. A `connect()` that loses `foreign_keys`
    /// (e.g. by running PRAGMAs once instead of per-connection) silently
    /// disables FK enforcement — this asserts it stays on.
    #[tokio::test]
    async fn connect_applies_per_connection_pragmas() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!("ironforge_pragma_test_{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let url = format!("sqlite://{}?mode=rwc", path.display());

        let db = connect(&url).await.expect("connect");

        let row = db
            .query_one(Statement::from_string(
                DatabaseBackend::Sqlite,
                "PRAGMA foreign_keys".to_string(),
            ))
            .await
            .expect("query foreign_keys")
            .expect("one row");
        let fk: i32 = row.try_get_by_index(0).expect("fk value");
        assert_eq!(fk, 1, "foreign_keys must be ON for every connection");

        let row = db
            .query_one(Statement::from_string(
                DatabaseBackend::Sqlite,
                "PRAGMA journal_mode".to_string(),
            ))
            .await
            .expect("query journal_mode")
            .expect("one row");
        let mode: String = row.try_get_by_index(0).expect("journal mode value");
        assert_eq!(mode.to_lowercase(), "wal", "journal_mode must be WAL");

        let _ = std::fs::remove_file(&path);
    }

    /// Stands in for a load test: hammer the multi-connection pool with
    /// concurrent writers and readers and assert nothing errors and no write
    /// is lost. Proves WAL + per-connection busy_timeout safely absorb the
    /// write contention enabled by `max_connections > 1`.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_reads_and_writes_do_not_error() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!(
            "ironforge_concurrency_test_{}.db",
            std::process::id()
        ));
        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{}", path.display(), suffix));
        }
        let url = format!("sqlite://{}?mode=rwc", path.display());

        let db = connect(&url).await.expect("connect");
        db.execute(Statement::from_string(
            DatabaseBackend::Sqlite,
            "CREATE TABLE t (id INTEGER PRIMARY KEY, n INTEGER NOT NULL)".to_string(),
        ))
        .await
        .expect("create table");

        const TASKS: i64 = 32;
        let mut handles = Vec::new();
        for n in 0..TASKS {
            let db = db.clone();
            handles.push(tokio::spawn(async move {
                // Write
                db.execute(Statement::from_string(
                    DatabaseBackend::Sqlite,
                    format!("INSERT INTO t (n) VALUES ({n})"),
                ))
                .await
                .expect("concurrent insert should not error");
                // Read
                db.query_one(Statement::from_string(
                    DatabaseBackend::Sqlite,
                    "SELECT COUNT(*) FROM t".to_string(),
                ))
                .await
                .expect("concurrent read should not error");
            }));
        }
        for h in handles {
            h.await.expect("task panicked");
        }

        let row = db
            .query_one(Statement::from_string(
                DatabaseBackend::Sqlite,
                "SELECT COUNT(*) FROM t".to_string(),
            ))
            .await
            .expect("final count")
            .expect("one row");
        let count: i64 = row.try_get_by_index(0).expect("count value");
        assert_eq!(count, TASKS, "every concurrent write must be persisted");

        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{}", path.display(), suffix));
        }
    }
}

/// Regression guard for the singular/plural table-name class of bug.
///
/// Migrations that declare `#[derive(Iden)] enum Thing { Table }` create a
/// **singular** table name (`thing`), while the SeaORM entity maps to
/// `#[sea_orm(table_name = "things")]`. When the two drift, every query for
/// that entity fails at runtime with `no such table`, and the failure is
/// invisible until the feature is exercised — `mirrors` shipped broken this
/// way and the periodic mirror task logged `no such table: mirrors` on every
/// tick.
///
/// This asserts that a freshly migrated database contains a table for every
/// declared entity, so any new mismatch fails here instead of in production.
#[cfg(test)]
mod table_name_tests {
    use super::*;
    use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};

    /// Collect every `#[sea_orm(table_name = "...")]` declared in the entity
    /// layer by scanning the source files.
    ///
    /// Scanning the directory (rather than keeping a hand-maintained list)
    /// means a newly added entity is covered automatically — the earlier
    /// constant silently missed new entities, which is exactly how the
    /// `mirrors` mismatch shipped.
    fn declared_entity_tables() -> Vec<String> {
        let entities_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/entities");
        let mut tables = Vec::new();
        for entry in std::fs::read_dir(&entities_dir).expect("read entities dir") {
            let entry = entry.expect("entity dir entry");
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
                continue;
            }
            let source = std::fs::read_to_string(&path).expect("read entity source");
            for line in source.lines() {
                let Some(rest) = line.split("table_name = \"").nth(1) else {
                    continue;
                };
                let Some(name) = rest.split('\"').next() else {
                    continue;
                };
                if !name.is_empty() {
                    tables.push(name.to_string());
                }
            }
        }
        tables.sort();
        tables.dedup();
        tables
    }

    #[tokio::test]
    async fn every_entity_table_exists_after_fresh_migration() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!(
            "ironforge_table_names_test_{}.db",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let url = format!("sqlite://{}?mode=rwc", path.display());

        let db = connect(&url).await.expect("connect");
        run_migrations(&db).await.expect("run migrations");

        let rows = db
            .query_all(Statement::from_string(
                DatabaseBackend::Sqlite,
                "SELECT name FROM sqlite_master WHERE type = 'table'".to_string(),
            ))
            .await
            .expect("list tables");

        let existing: std::collections::HashSet<String> = rows
            .iter()
            .filter_map(|row| row.try_get_by_index::<String>(0).ok())
            .collect();

        let declared = declared_entity_tables();
        let missing: Vec<&String> = declared
            .iter()
            .filter(|table| !existing.contains(*table))
            .collect();

        // The scan must actually find entities; otherwise it would pass
        // vacuously if the directory layout ever changes.
        assert!(
            declared.len() > 50,
            "entity scan found only {} table(s); check the entities directory path",
            declared.len()
        );

        assert!(
            missing.is_empty(),
            "entities declare tables that the migrations never created \
             (queries would fail with `no such table`): {missing:?}"
        );

        let _ = std::fs::remove_file(&path);
    }
}

/// Regression: a soft-deleted repository must not keep its name reserved.
///
/// `repo_ops::find_by_owner_and_name` filters `deleted_at IS NULL`, so it
/// reports the name as free — but the original `UNIQUE (owner_id, name)` still
/// rejected the insert. The partial index only covers live rows, so recreating
/// works while duplicate live names stay rejected.
#[cfg(test)]
mod soft_delete_tests {
    use super::*;
    use sea_orm::{ActiveModelTrait, ConnectionTrait, DatabaseBackend, Statement};

    async fn owner(db: &DatabaseConnection, name: &str) -> i64 {
        crate::entities::user::ActiveModel {
            username: sea_orm::Set(name.to_string()),
            email: sea_orm::Set(format!("{name}@example.com")),
            password_hash: sea_orm::Set("unused".to_string()),
            display_name: sea_orm::Set(Some(name.to_string())),
            is_admin: sea_orm::Set(false),
            is_active: sea_orm::Set(true),
            created_at: sea_orm::Set(chrono::Utc::now()),
            updated_at: sea_orm::Set(chrono::Utc::now()),
            ..Default::default()
        }
        .insert(db)
        .await
        .expect("insert user")
        .id
    }

    async fn repository(db: &DatabaseConnection, owner_id: i64, name: &str) -> i64 {
        crate::entities::repository::ActiveModel {
            owner_id: sea_orm::Set(owner_id),
            name: sea_orm::Set(name.to_string()),
            default_branch: sea_orm::Set("main".to_string()),
            is_private: sea_orm::Set(false),
            stars_count: sea_orm::Set(0),
            forks_count: sea_orm::Set(0),
            created_at: sea_orm::Set(chrono::Utc::now()),
            updated_at: sea_orm::Set(chrono::Utc::now()),
            ..Default::default()
        }
        .insert(db)
        .await
        .expect("insert repository")
        .id
    }

    #[tokio::test]
    async fn soft_deleted_repository_name_can_be_reused() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!("ironforge_soft_delete_test_{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let url = format!("sqlite://{}?mode=rwc", path.display());

        let db = connect(&url).await.expect("connect");
        run_migrations(&db).await.expect("run migrations");

        // The partial index must exist after migration.
        let index = db
            .query_one(Statement::from_string(
                DatabaseBackend::Sqlite,
                "SELECT name FROM sqlite_master WHERE type = 'index' AND name = 'uq_repositories_owner_name_live'",
            ))
            .await
            .expect("query index")
            .and_then(|row| row.try_get_by_index::<String>(0).ok());
        assert_eq!(index.as_deref(), Some("uq_repositories_owner_name_live"));

        // The rebuild must preserve every column the entity declares, and the
        // `ON DELETE CASCADE` clause that the copy would otherwise drop.
        let ddl = db
            .query_one(Statement::from_string(
                DatabaseBackend::Sqlite,
                "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'repositories'",
            ))
            .await
            .expect("query table")
            .and_then(|row| row.try_get_by_index::<String>(0).ok())
            .expect("repositories DDL");
        for column in [
            "id",
            "owner_id",
            "name",
            "description",
            "is_private",
            "default_branch",
            "fork_id",
            "stars_count",
            "forks_count",
            "created_at",
            "updated_at",
            "org_id",
            "deleted_at",
            "origin_repo_id",
        ] {
            assert!(ddl.contains(column), "rebuild dropped column {column}: {ddl}");
        }
        assert!(
            ddl.to_uppercase().contains("ON DELETE CASCADE"),
            "rebuild must preserve the owner foreign-key cascade: {ddl}"
        );

        let owner_id = owner(&db, "softdelete-owner").await;

        // Create, soft-delete, then recreate under the same name.
        let first = repository(&db, owner_id, "reusable").await;
        crate::ops::repo_ops::soft_delete(&db, first)
            .await
            .expect("soft delete");
        let second = repository(&db, owner_id, "reusable").await;
        assert_ne!(first, second, "recreating must produce a new row");

        // Two live rows with the same name must still be rejected.
        let duplicate = crate::entities::repository::ActiveModel {
            owner_id: sea_orm::Set(owner_id),
            name: sea_orm::Set("reusable".to_string()),
            default_branch: sea_orm::Set("main".to_string()),
            is_private: sea_orm::Set(false),
            stars_count: sea_orm::Set(0),
            forks_count: sea_orm::Set(0),
            created_at: sea_orm::Set(chrono::Utc::now()),
            updated_at: sea_orm::Set(chrono::Utc::now()),
            ..Default::default()
        };
        assert!(
            duplicate.insert(&db).await.is_err(),
            "two live repositories with the same owner+name must be rejected"
        );

        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{}", path.display(), suffix));
        }
    }
}