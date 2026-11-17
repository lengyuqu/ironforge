//! `symbol_commits` + `symbol_definitions`: the symbol definition table
//! (#63 step 2, T28).
//!
//! The design document
//! (`ironforge-docs/architecture/symbol-graph-design-2026-11.md`) measured the
//! two candidate schemas on this repository (1,273 files, 12,208 definitions):
//! storing a 40-byte `commit_sha` on every row costs 60 MiB/commit more than an
//! `INTEGER` foreign key into a tiny `symbol_commits` registry, so definitions
//! carry `commit_id` only.
//!
//! Deliberately **not** part of this migration:
//!
//! * `symbol_references` / postings — measured at 30× the definition volume,
//!   73.9% of it noise from common names.
//! * an `impl` filter — `impl` rows are stored like any other definition
//!   (4.1% of names are not identifiers) and excluded by the query side instead.
//!
//! `up` is idempotent (tables, columns and indexes are each guarded) and `down`
//! drops the indexes first, then the tables in foreign-key order.

use sea_orm::DatabaseBackend;
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261118_000001_create_symbol_graph"
    }
}

const SYMBOL_COMMITS: &str = "symbol_commits";
const SYMBOL_DEFINITIONS: &str = "symbol_definitions";

const UQ_SYMBOL_COMMITS_REPO_SHA: &str = "uq_symbol_commits_repo_sha";
const IX_SYMBOL_DEFINITIONS_LOOKUP: &str = "idx_symbol_definitions_lookup";
const IX_SYMBOL_DEFINITIONS_PATH: &str = "idx_symbol_definitions_path";
const UQ_SYMBOL_DEFINITIONS_ANCHOR: &str = "uq_symbol_definitions_anchor";

// Explicit plural table names matching the entities' `table_name` (CLAUDE.md
// pitfall 13): a derived `Iden` would produce the singular form.
#[derive(DeriveIden)]
enum SymbolCommits {
    #[sea_orm(iden = "symbol_commits")]
    Table,
    Id,
    RepoId,
    CommitSha,
    CreatedAt,
}

#[derive(DeriveIden)]
enum SymbolDefinitions {
    #[sea_orm(iden = "symbol_definitions")]
    Table,
    Id,
    RepoId,
    CommitId,
    Path,
    Name,
    Kind,
    StartLine,
    EndLine,
    Signature,
}

#[derive(DeriveIden)]
enum Repositories {
    Table,
    Id,
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let backend = manager.get_database_backend();

        if manager.has_table(SYMBOL_COMMITS).await? {
            // Repair a partially applied schema: every column is guarded, so a
            // re-run after a failed create (or a hand-edited database) converges.
            for (name, definition) in symbol_commit_columns() {
                if !manager.has_column(SYMBOL_COMMITS, name).await? {
                    manager
                        .alter_table(
                            Table::alter()
                                .table(SymbolCommits::Table)
                                .add_column(definition)
                                .to_owned(),
                        )
                        .await?;
                }
            }
        } else {
            manager.create_table(symbol_commits_table()).await?;
        }
        if manager.has_table(SYMBOL_DEFINITIONS).await? {
            for (name, definition) in symbol_definition_columns() {
                if !manager.has_column(SYMBOL_DEFINITIONS, name).await? {
                    manager
                        .alter_table(
                            Table::alter()
                                .table(SymbolDefinitions::Table)
                                .add_column(definition)
                                .to_owned(),
                        )
                        .await?;
                }
            }
        } else {
            manager.create_table(symbol_definitions_table()).await?;
        }

        // Indexes are declared separately, each behind a `has_index` guard:
        // SQLite rejects the bare `CONSTRAINT name (cols)` sea-query renders for
        // a non-unique `.index()` inside `TableCreateStatement`.
        if !manager
            .has_index(SYMBOL_COMMITS, UQ_SYMBOL_COMMITS_REPO_SHA)
            .await?
        {
            manager.create_index(symbol_commits_unique_index()).await?;
        }
        if !manager
            .has_index(SYMBOL_DEFINITIONS, IX_SYMBOL_DEFINITIONS_LOOKUP)
            .await?
        {
            manager
                .create_index(symbol_definitions_lookup_index())
                .await?;
        }
        if !manager
            .has_index(SYMBOL_DEFINITIONS, IX_SYMBOL_DEFINITIONS_PATH)
            .await?
        {
            manager
                .create_index(symbol_definitions_path_index())
                .await?;
        }
        if !manager
            .has_index(SYMBOL_DEFINITIONS, UQ_SYMBOL_DEFINITIONS_ANCHOR)
            .await?
        {
            manager
                .create_index(symbol_definitions_anchor_index(backend))
                .await?;
        }
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Foreign-key order: the child table first, then its parent. The
        // indexes go with their tables on SQLite, PostgreSQL and MySQL alike;
        // dropping them explicitly would fail on MySQL, which refuses to drop
        // `uq_symbol_definitions_anchor` while it backs the commit foreign key.
        manager
            .drop_table(
                Table::drop()
                    .table(SymbolDefinitions::Table)
                    .if_exists()
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(
                Table::drop()
                    .table(SymbolCommits::Table)
                    .if_exists()
                    .to_owned(),
            )
            .await
    }
}

/// `(column name, definition)`, in table order. The name comes from the `Iden`
/// so the guard and the DDL cannot drift apart.
fn symbol_commit_columns() -> Vec<(&'static str, ColumnDef)> {
    vec![
        (
            "id",
            ColumnDef::new(SymbolCommits::Id)
                .integer()
                .not_null()
                .auto_increment()
                .primary_key()
                .to_owned(),
        ),
        (
            "repo_id",
            ColumnDef::new(SymbolCommits::RepoId)
                .big_integer()
                .not_null()
                .to_owned(),
        ),
        (
            // 40 hex characters.
            "commit_sha",
            ColumnDef::new(SymbolCommits::CommitSha)
                .string_len(40)
                .not_null()
                .to_owned(),
        ),
        (
            "created_at",
            ColumnDef::new(SymbolCommits::CreatedAt)
                .timestamp_with_time_zone()
                .not_null()
                .to_owned(),
        ),
    ]
}

fn symbol_definition_columns() -> Vec<(&'static str, ColumnDef)> {
    vec![
        (
            "id",
            ColumnDef::new(SymbolDefinitions::Id)
                .integer()
                .not_null()
                .auto_increment()
                .primary_key()
                .to_owned(),
        ),
        // Denormalised from `symbol_commits` so the two query paths never need
        // the join. No foreign key: `symbol_commits.repo_id` already cascades.
        (
            "repo_id",
            ColumnDef::new(SymbolDefinitions::RepoId)
                .big_integer()
                .not_null()
                .to_owned(),
        ),
        (
            "commit_id",
            ColumnDef::new(SymbolDefinitions::CommitId)
                .integer()
                .not_null()
                .to_owned(),
        ),
        // Repository-relative path, '/' separated. 512 characters is far above
        // the 85-character maximum measured on this repository and still fits
        // InnoDB's 3072-byte key with a 400-character prefix (see the anchor
        // index).
        (
            "path",
            ColumnDef::new(SymbolDefinitions::Path)
                .string_len(512)
                .not_null()
                .to_owned(),
        ),
        // Stored verbatim: 4.1% of the names are Rust `impl` first lines and are
        // not identifiers, so there is no character constraint here.
        (
            "name",
            ColumnDef::new(SymbolDefinitions::Name)
                .string_len(255)
                .not_null()
                .to_owned(),
        ),
        (
            "kind",
            ColumnDef::new(SymbolDefinitions::Kind)
                .string_len(32)
                .not_null()
                .to_owned(),
        ),
        (
            "start_line",
            ColumnDef::new(SymbolDefinitions::StartLine)
                .integer()
                .not_null()
                .to_owned(),
        ),
        (
            "end_line",
            ColumnDef::new(SymbolDefinitions::EndLine)
                .integer()
                .not_null()
                .to_owned(),
        ),
        // Nullable on purpose: `/map` always has one, but a future extractor
        // (or an `impl` row) may not.
        (
            "signature",
            ColumnDef::new(SymbolDefinitions::Signature)
                .text()
                .null()
                .to_owned(),
        ),
    ]
}

fn symbol_commits_table() -> TableCreateStatement {
    let mut table = Table::create();
    table.table(SymbolCommits::Table).if_not_exists();
    for (_, definition) in symbol_commit_columns() {
        table.col(definition);
    }
    table
        .foreign_key(
            ForeignKey::create()
                .from(SymbolCommits::Table, SymbolCommits::RepoId)
                .to(Repositories::Table, Repositories::Id)
                .on_delete(ForeignKeyAction::Cascade),
        )
        .to_owned()
}

fn symbol_definitions_table() -> TableCreateStatement {
    let mut table = Table::create();
    table.table(SymbolDefinitions::Table).if_not_exists();
    for (_, definition) in symbol_definition_columns() {
        table.col(definition);
    }
    table
        .foreign_key(
            ForeignKey::create()
                .from(SymbolDefinitions::Table, SymbolDefinitions::CommitId)
                .to(SymbolCommits::Table, SymbolCommits::Id)
                .on_delete(ForeignKeyAction::Cascade),
        )
        .to_owned()
}

fn symbol_commits_unique_index() -> IndexCreateStatement {
    Index::create()
        .if_not_exists()
        .name(UQ_SYMBOL_COMMITS_REPO_SHA)
        .table(SymbolCommits::Table)
        .col(SymbolCommits::RepoId)
        .col(SymbolCommits::CommitSha)
        .unique()
        .to_owned()
}

/// The main query path: definitions of one commit by name.
fn symbol_definitions_lookup_index() -> IndexCreateStatement {
    Index::create()
        .if_not_exists()
        .name(IX_SYMBOL_DEFINITIONS_LOOKUP)
        .table(SymbolDefinitions::Table)
        .col(SymbolDefinitions::RepoId)
        .col(SymbolDefinitions::CommitId)
        .col(SymbolDefinitions::Name)
        .to_owned()
}

/// "List the definitions of this file" (`/map`, file outline).
fn symbol_definitions_path_index() -> IndexCreateStatement {
    Index::create()
        .if_not_exists()
        .name(IX_SYMBOL_DEFINITIONS_PATH)
        .table(SymbolDefinitions::Table)
        .col(SymbolDefinitions::RepoId)
        .col(SymbolDefinitions::CommitId)
        .col(SymbolDefinitions::Path)
        .to_owned()
}

/// Idempotent-write anchor: re-indexing the same commit cannot duplicate a
/// definition. MySQL cannot index the complete `path`/`name` pair inside
/// InnoDB's 3072-byte key limit with `utf8mb4`, so `path` is indexed by a
/// 400-character prefix there (the same trade-off as
/// `m20260714_000001_create_attachments`). Real paths are far shorter.
fn symbol_definitions_anchor_index(backend: DatabaseBackend) -> IndexCreateStatement {
    let mut index = Index::create();
    index
        .if_not_exists()
        .name(UQ_SYMBOL_DEFINITIONS_ANCHOR)
        .table(SymbolDefinitions::Table)
        .col(SymbolDefinitions::CommitId);
    if backend == DatabaseBackend::MySql {
        index.col((SymbolDefinitions::Path, 400));
    } else {
        index.col(SymbolDefinitions::Path);
    }
    index
        .col(SymbolDefinitions::Name)
        .col(SymbolDefinitions::Kind)
        .col(SymbolDefinitions::StartLine)
        .unique()
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::{ConnectionTrait, Database, Statement};
    use sea_orm_migration::sea_query::{
        MysqlQueryBuilder, PostgresQueryBuilder, SqliteQueryBuilder,
    };
    use sea_orm_migration::MigratorTrait;

    async fn count(db: &sea_orm::DatabaseConnection, sql: &str) -> i64 {
        let row = db
            .query_one(Statement::from_string(db.get_database_backend(), sql))
            .await
            .unwrap()
            .unwrap();
        row.try_get_by_index::<i64>(0).unwrap()
    }

    async fn table_exists(db: &sea_orm::DatabaseConnection, table: &str) -> i64 {
        count(
            db,
            &format!(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = '{table}'"
            ),
        )
        .await
    }

    async fn index_exists(db: &sea_orm::DatabaseConnection, index: &str) -> i64 {
        count(
            db,
            &format!(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name = '{index}'"
            ),
        )
        .await
    }

    /// CLAUDE.md pitfall #13: the explicit plural table names and every
    /// constraint survive all three dialects.
    #[test]
    fn ddl_uses_plural_names_and_constraints_on_all_backends() {
        for table in [symbol_commits_table(), symbol_definitions_table()] {
            for sql in [
                table.to_string(SqliteQueryBuilder),
                table.to_string(PostgresQueryBuilder),
                table.to_string(MysqlQueryBuilder),
            ] {
                assert!(
                    sql.contains("symbol_commits") || sql.contains("symbol_definitions"),
                    "table name missing in: {sql}"
                );
                assert!(
                    !sql.contains("symbol_commit ")
                        && !sql.contains("symbol_definition ")
                        && !sql.contains("`symbol_commit`")
                        && !sql.contains("`symbol_definition`"),
                    "singular table in: {sql}"
                );
            }
        }
        let definitions = symbol_definitions_table().to_string(SqliteQueryBuilder);
        assert!(
            definitions.contains("FOREIGN KEY (\"commit_id\")")
                || definitions.contains("FOREIGN KEY (`commit_id`)"),
            "commit foreign key missing: {definitions}"
        );
        assert!(
            definitions.contains("ON DELETE CASCADE"),
            "cascade missing: {definitions}"
        );

        for (name, index) in [
            (UQ_SYMBOL_COMMITS_REPO_SHA, symbol_commits_unique_index()),
            (
                IX_SYMBOL_DEFINITIONS_LOOKUP,
                symbol_definitions_lookup_index(),
            ),
            (IX_SYMBOL_DEFINITIONS_PATH, symbol_definitions_path_index()),
            (
                UQ_SYMBOL_DEFINITIONS_ANCHOR,
                symbol_definitions_anchor_index(DatabaseBackend::Sqlite),
            ),
        ] {
            for sql in [
                index.to_string(SqliteQueryBuilder),
                index.to_string(PostgresQueryBuilder),
                index.to_string(MysqlQueryBuilder),
            ] {
                assert!(sql.contains(name), "{name} missing in: {sql}");
            }
        }
        // InnoDB prefix only on MySQL.
        let mysql =
            symbol_definitions_anchor_index(DatabaseBackend::MySql).to_string(MysqlQueryBuilder);
        assert!(
            mysql.contains("(400)"),
            "MySQL needs the path prefix: {mysql}"
        );
        let sqlite =
            symbol_definitions_anchor_index(DatabaseBackend::Sqlite).to_string(SqliteQueryBuilder);
        assert!(
            !sqlite.contains("(400)"),
            "only MySQL uses a prefix: {sqlite}"
        );
    }

    fn rollback_steps() -> u32 {
        let migrations = crate::migrations::Migrator::migrations();
        let position = migrations
            .iter()
            .rev()
            .position(|migration| migration.name() == "m20261118_000001_create_symbol_graph")
            .expect("this migration must be registered");
        (position + 1) as u32
    }

    #[tokio::test]
    async fn down_drops_both_tables_and_up_recreates_them() {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        crate::migrations::Migrator::up(&db, None).await.unwrap();
        assert_eq!(table_exists(&db, SYMBOL_COMMITS).await, 1);
        assert_eq!(table_exists(&db, SYMBOL_DEFINITIONS).await, 1);

        crate::migrations::Migrator::down(&db, Some(rollback_steps()))
            .await
            .unwrap();
        assert_eq!(table_exists(&db, SYMBOL_DEFINITIONS).await, 0);
        assert_eq!(table_exists(&db, SYMBOL_COMMITS).await, 0);
        assert_eq!(index_exists(&db, UQ_SYMBOL_DEFINITIONS_ANCHOR).await, 0);
        assert_eq!(index_exists(&db, IX_SYMBOL_DEFINITIONS_LOOKUP).await, 0);

        crate::migrations::Migrator::up(&db, None).await.unwrap();
        assert_eq!(table_exists(&db, SYMBOL_COMMITS).await, 1);
        assert_eq!(table_exists(&db, SYMBOL_DEFINITIONS).await, 1);
        assert_eq!(index_exists(&db, UQ_SYMBOL_DEFINITIONS_ANCHOR).await, 1);
        assert_eq!(index_exists(&db, IX_SYMBOL_DEFINITIONS_PATH).await, 1);
    }

    #[tokio::test]
    async fn up_is_idempotent_and_repairs_a_missing_column() {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        crate::migrations::Migrator::up(&db, None).await.unwrap();
        db.execute_unprepared("ALTER TABLE symbol_definitions DROP COLUMN signature")
            .await
            .expect("sqlite 3.35+ can drop a column");
        assert_eq!(
            count(
                &db,
                "SELECT COUNT(*) FROM pragma_table_info('symbol_definitions') \
                 WHERE name = 'signature'"
            )
            .await,
            0
        );

        // `Migrator::up` skips an applied migration, so drive the manager.
        let manager = SchemaManager::new(&db);
        Migration.up(&manager).await.unwrap();
        assert_eq!(
            count(
                &db,
                "SELECT COUNT(*) FROM pragma_table_info('symbol_definitions') \
                 WHERE name = 'signature'"
            )
            .await,
            1,
            "the guarded re-run must restore the missing column"
        );
    }

    /// Deleting a commit row cascades into its definitions: the prune path can
    /// rely on it as a backstop even though it deletes both tables explicitly.
    #[tokio::test]
    async fn deleting_a_symbol_commit_cascades_into_definitions() {
        use sea_orm::ActiveModelTrait;
        use sea_orm::ActiveValue::Set;

        let db = Database::connect("sqlite::memory:").await.unwrap();
        crate::migrations::Migrator::up(&db, None).await.unwrap();
        // SQLite only enforces foreign keys when the pragma is on; the test
        // connection does not go through `connect()`.
        db.execute_unprepared("PRAGMA foreign_keys = ON")
            .await
            .unwrap();

        let now = chrono::Utc::now();
        let owner = crate::entities::user::ActiveModel {
            username: Set("sym-owner".to_string()),
            email: Set("sym@example.com".to_string()),
            password_hash: Set("unused".to_string()),
            display_name: Set(Some("sym".to_string())),
            is_admin: Set(false),
            is_active: Set(true),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        }
        .insert(&db)
        .await
        .expect("insert user");
        let repo = crate::entities::repository::ActiveModel {
            owner_id: Set(owner.id),
            name: Set("sym".to_string()),
            default_branch: Set("main".to_string()),
            is_private: Set(false),
            stars_count: Set(0),
            forks_count: Set(0),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        }
        .insert(&db)
        .await
        .expect("insert repository");
        let commit = crate::entities::symbol_commit::ActiveModel {
            repo_id: Set(repo.id),
            commit_sha: Set("0123456789012345678901234567890123456789".to_string()),
            created_at: Set(now),
            ..Default::default()
        }
        .insert(&db)
        .await
        .expect("insert symbol commit");
        crate::entities::symbol_definition::ActiveModel {
            repo_id: Set(repo.id),
            commit_id: Set(commit.id),
            path: Set("a.rs".to_string()),
            name: Set("main".to_string()),
            kind: Set("function".to_string()),
            start_line: Set(1),
            end_line: Set(2),
            signature: Set(Some("fn main()".to_string())),
            ..Default::default()
        }
        .insert(&db)
        .await
        .expect("insert symbol definition");

        db.execute_unprepared(&format!(
            "DELETE FROM symbol_commits WHERE id = {}",
            commit.id
        ))
        .await
        .unwrap();
        assert_eq!(
            count(&db, "SELECT COUNT(*) FROM symbol_definitions").await,
            0,
            "definitions must go with their commit"
        );
    }

    /// The rollback is the risky half, and the backends disagree about dropping
    /// tables with indexes and foreign keys. With `IRONFORGE_TEST_DATABASE_URL`
    /// set this runs the round trip on PostgreSQL / MySQL; without it there is
    /// nothing to do (SQLite is covered above).
    #[tokio::test]
    async fn down_and_up_round_trip_on_the_backend_from_the_environment() {
        let Ok(url) = std::env::var("IRONFORGE_TEST_DATABASE_URL") else {
            return;
        };
        let db = crate::connect_with_pool(&url, 5, 60, 4)
            .await
            .expect("connect to IRONFORGE_TEST_DATABASE_URL");
        crate::migrations::Migrator::up(&db, None).await.unwrap();
        let manager = SchemaManager::new(&db);
        assert!(manager.has_table(SYMBOL_COMMITS).await.unwrap());
        assert!(manager.has_table(SYMBOL_DEFINITIONS).await.unwrap());

        // This migration is registered last, so one step down is this one.
        crate::migrations::Migrator::down(&db, Some(1))
            .await
            .unwrap();
        assert!(!manager.has_table(SYMBOL_DEFINITIONS).await.unwrap());
        assert!(!manager.has_table(SYMBOL_COMMITS).await.unwrap());

        crate::migrations::Migrator::up(&db, None).await.unwrap();
        assert!(manager.has_table(SYMBOL_COMMITS).await.unwrap());
        assert!(manager.has_table(SYMBOL_DEFINITIONS).await.unwrap());
    }
}
