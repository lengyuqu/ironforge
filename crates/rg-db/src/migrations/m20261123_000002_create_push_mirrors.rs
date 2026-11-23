//! `push_mirrors`: forward a repository's refs to an external Git host.
//!
//! The counterpart of `mirrors` (pull). A pull mirror answers "keep a copy of
//! somewhere else here"; a push mirror answers "keep a copy of here somewhere
//! else", which is what a project needs when GitHub is the public face and this
//! instance is where the work happens.
//!
//! # Why a separate table
//!
//! `mirrors` is unique on `repo_id`, and both directions want to coexist on the
//! same repository. Adding a `direction` column would mean relaxing that unique
//! constraint to `(repo_id, direction)` plus a pile of conditions in every
//! existing query; a second table keeps each direction's fields honest. In
//! particular a push mirror has no `sync_interval_seconds` — it is driven by
//! the push that just happened, not by a clock — and it has no per-mirror
//! username/password: the credential comes from the host dictionary
//! (`remote_credentials`), which is the whole point of that feature.
//!
//! Only a new table; `up` is idempotent (table, columns and index are each
//! guarded) and `down` drops it.

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261123_000002_create_push_mirrors"
    }
}

const TABLE: &str = "push_mirrors";
const UNIQUE_INDEX: &str = "uq_push_mirrors_repo";

// Explicit table name, matching the entity's `table_name` (CLAUDE.md pitfall
// 13): a derived `Iden` would produce the singular `push_mirror`.
#[derive(DeriveIden)]
enum PushMirrors {
    #[sea_orm(iden = "push_mirrors")]
    Table,
    Id,
    RepoId,
    Url,
    PushTags,
    TriggerMode,
    Status,
    LastPushAt,
    LastPushError,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum Repositories {
    Table,
    Id,
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        if manager.has_table(TABLE).await? {
            self.add_missing_columns(manager).await?;
        } else {
            manager.create_table(table_create()).await?;
        }
        if !manager.has_index(TABLE, UNIQUE_INDEX).await? {
            manager
                .create_index(
                    Index::create()
                        .if_not_exists()
                        .name(UNIQUE_INDEX)
                        .table(PushMirrors::Table)
                        .col(PushMirrors::RepoId)
                        .unique()
                        .to_owned(),
                )
                .await?;
        }
        Ok(())
    }

    /// Drops the table. Nothing else references these rows and a re-apply
    /// starts from an empty configuration the UI can refill.
    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(
                Table::drop()
                    .table(PushMirrors::Table)
                    .if_exists()
                    .to_owned(),
            )
            .await
    }
}

impl Migration {
    async fn add_missing_columns(&self, manager: &SchemaManager<'_>) -> Result<(), DbErr> {
        let columns: [(PushMirrors, ColumnDef); 10] = [
            (
                PushMirrors::Id,
                ColumnDef::new(PushMirrors::Id)
                    .big_integer()
                    .not_null()
                    .auto_increment()
                    .primary_key()
                    .to_owned(),
            ),
            (
                PushMirrors::RepoId,
                ColumnDef::new(PushMirrors::RepoId)
                    .big_integer()
                    .not_null()
                    .to_owned(),
            ),
            (
                PushMirrors::Url,
                ColumnDef::new(PushMirrors::Url)
                    .text()
                    .not_null()
                    .to_owned(),
            ),
            (
                PushMirrors::PushTags,
                ColumnDef::new(PushMirrors::PushTags)
                    .boolean()
                    .not_null()
                    .default(true)
                    .to_owned(),
            ),
            (
                PushMirrors::TriggerMode,
                ColumnDef::new(PushMirrors::TriggerMode)
                    .string_len(16)
                    .not_null()
                    .default("on_push")
                    .to_owned(),
            ),
            (
                PushMirrors::Status,
                ColumnDef::new(PushMirrors::Status)
                    .string_len(16)
                    .not_null()
                    .default("active")
                    .to_owned(),
            ),
            (
                PushMirrors::LastPushAt,
                ColumnDef::new(PushMirrors::LastPushAt)
                    .timestamp_with_time_zone()
                    .to_owned(),
            ),
            (
                PushMirrors::LastPushError,
                ColumnDef::new(PushMirrors::LastPushError).text().to_owned(),
            ),
            (
                PushMirrors::CreatedAt,
                ColumnDef::new(PushMirrors::CreatedAt)
                    .timestamp_with_time_zone()
                    .not_null()
                    .to_owned(),
            ),
            (
                PushMirrors::UpdatedAt,
                ColumnDef::new(PushMirrors::UpdatedAt)
                    .timestamp_with_time_zone()
                    .not_null()
                    .to_owned(),
            ),
        ];
        for (column, definition) in columns {
            let name = column.to_string();
            if !manager.has_column(TABLE, name.as_str()).await? {
                manager
                    .alter_table(
                        Table::alter()
                            .table(PushMirrors::Table)
                            .add_column(definition)
                            .to_owned(),
                    )
                    .await?;
            }
        }
        Ok(())
    }
}

fn table_create() -> TableCreateStatement {
    Table::create()
        .table(PushMirrors::Table)
        .if_not_exists()
        .col(
            ColumnDef::new(PushMirrors::Id)
                .big_integer()
                .not_null()
                .auto_increment()
                .primary_key(),
        )
        .col(ColumnDef::new(PushMirrors::RepoId).big_integer().not_null())
        // Where the refs are sent. Normalised/validated by
        // `net_guard::validate_mirror_url` before it is stored, and re-validated
        // on every push (rows can predate the guard or be written by hand).
        .col(ColumnDef::new(PushMirrors::Url).text().not_null())
        // Tags are pushed alongside branches unless the owner turns them off —
        // a release that exists here but not there is the usual surprise.
        .col(
            ColumnDef::new(PushMirrors::PushTags)
                .boolean()
                .not_null()
                .default(true),
        )
        // `on_push` (after every push to this repository) or `manual` (only via
        // the sync endpoint). No interval mode: a push mirror that fires on a
        // timer duplicates work and can publish a half-finished series of
        // commits.
        //
        // Named `trigger_mode`, not `trigger`: the latter is a reserved word on
        // all three backends and would need quoting in every hand-written query.
        .col(
            ColumnDef::new(PushMirrors::TriggerMode)
                .string_len(16)
                .not_null()
                .default("on_push"),
        )
        // `active` | `paused` | `error`.
        .col(
            ColumnDef::new(PushMirrors::Status)
                .string_len(16)
                .not_null()
                .default("active"),
        )
        .col(ColumnDef::new(PushMirrors::LastPushAt).timestamp_with_time_zone())
        // Operator-facing failure text. Never contains the remote URL with
        // credentials embedded, because the URL never carries any.
        .col(ColumnDef::new(PushMirrors::LastPushError).text())
        .col(
            ColumnDef::new(PushMirrors::CreatedAt)
                .timestamp_with_time_zone()
                .not_null(),
        )
        .col(
            ColumnDef::new(PushMirrors::UpdatedAt)
                .timestamp_with_time_zone()
                .not_null(),
        )
        // One push target per repository: the arbiter of concurrent creates.
        .index(
            Index::create()
                .if_not_exists()
                .name(UNIQUE_INDEX)
                .col(PushMirrors::RepoId)
                .unique(),
        )
        .foreign_key(
            ForeignKey::create()
                .from(PushMirrors::Table, PushMirrors::RepoId)
                .to(Repositories::Table, Repositories::Id)
                .on_delete(ForeignKeyAction::Cascade),
        )
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

    fn rollback_steps() -> u32 {
        let migrations = crate::migrations::Migrator::migrations();
        let position = migrations
            .iter()
            .rev()
            .position(|migration| migration.name() == "m20261123_000002_create_push_mirrors")
            .expect("this migration must be registered");
        (position + 1) as u32
    }

    #[test]
    fn ddl_uses_the_plural_table_name_and_unique_index_on_all_backends() {
        for sql in [
            table_create().to_string(SqliteQueryBuilder),
            table_create().to_string(PostgresQueryBuilder),
            table_create().to_string(MysqlQueryBuilder),
        ] {
            assert!(sql.contains(TABLE), "{TABLE} missing in: {sql}");
            assert!(!sql.contains("push_mirror "), "singular table in: {sql}");
            assert!(
                sql.contains(UNIQUE_INDEX),
                "{UNIQUE_INDEX} missing in: {sql}"
            );
        }
    }

    #[tokio::test]
    async fn down_drops_the_table_and_up_recreates_it() {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        crate::migrations::Migrator::up(&db, None).await.unwrap();
        assert_eq!(
            count(
                &db,
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'push_mirrors'"
            )
            .await,
            1
        );

        let steps = rollback_steps();
        crate::migrations::Migrator::down(&db, Some(steps))
            .await
            .unwrap();
        assert_eq!(
            count(
                &db,
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'push_mirrors'"
            )
            .await,
            0,
            "down must drop the table"
        );

        crate::migrations::Migrator::up(&db, None).await.unwrap();
        assert_eq!(
            count(
                &db,
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name = 'uq_push_mirrors_repo'"
            )
            .await,
            1,
            "the unique index must be back after re-apply"
        );
    }

    /// One push target per repository — the constraint the whole design leans on.
    #[tokio::test]
    async fn a_repository_may_have_only_one_push_target() {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        crate::migrations::Migrator::up(&db, None).await.unwrap();
        db.execute(Statement::from_string(
            db.get_database_backend(),
            "INSERT INTO users(id, username, email, password_hash, is_admin, is_active, created_at, updated_at) \
             VALUES(1, 'u', 'u@x.io', 'x', 0, 1, '2024-01-01 00:00:00', '2024-01-01 00:00:00')",
        ))
        .await
        .unwrap();
        db.execute(Statement::from_string(
            db.get_database_backend(),
            "INSERT INTO repositories(id, owner_id, name, is_private, default_branch, stars_count, forks_count, created_at, updated_at) \
             VALUES(1, 1, 'r', 0, 'main', 0, 0, '2024-01-01 00:00:00', '2024-01-01 00:00:00')",
        ))
        .await
        .unwrap();

        let insert = |url: &str| {
            format!(
                "INSERT INTO push_mirrors (repo_id, url, push_tags, trigger_mode, status, created_at, updated_at) \
                 VALUES (1, '{url}', 1, 'on_push', 'active', \
                 '2026-11-23 00:00:00+00:00', '2026-11-23 00:00:00+00:00')"
            )
        };
        db.execute(Statement::from_string(
            db.get_database_backend(),
            insert("https://a.example/x.git"),
        ))
        .await
        .unwrap();
        assert!(
            db.execute(Statement::from_string(
                db.get_database_backend(),
                insert("https://b.example/x.git")
            ))
            .await
            .is_err(),
            "a second push target for the same repository must be rejected"
        );
    }
}
