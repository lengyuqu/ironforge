//! `remote_credentials`: the per-owner token dictionary that lets the server
//! authenticate against *other* Git hosts.
//!
//! Pulling from a private upstream (mirror, import) and pushing to one both
//! need a token, and every host mints them differently. Gitea-style platforms
//! ask for the token once, inside the migration dialog, and forget it — which
//! means the second mirror against the same host needs the same token typed in
//! again. Here the token is keyed by host instead, so one row covers every
//! repository on that platform for the owner that configured it.
//!
//! # Why `owner_kind` + `owner_id` and not two nullable columns
//!
//! A credential belongs to either a user or an organization. The obvious
//! encoding — `owner_user_id` / `owner_org_id`, one of them NULL — breaks
//! uniqueness on every backend: SQL treats NULLs as distinct, so
//! `UNIQUE(owner_user_id, host)` happily accepts two org rows with the same
//! host (and a partial unique index is not portable to MySQL). A discriminator
//! column keeps one plain three-column unique index that means the same thing
//! everywhere. Same shape as `usage_counters.subject_kind`.
//!
//! # Why no foreign key
//!
//! `owner_id` is polymorphic, so no single FK target exists. The credential is
//! also meant to outlive an account rename; only the owner row itself deleting
//! would orphan it, and that deletion path removes credentials explicitly
//! (`remote_credential_ops::delete_all_for_owner`).
//!
//! Only a new table; `up` is idempotent (table, columns and index are each
//! guarded) and `down` drops it.

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261123_000001_create_remote_credentials"
    }
}

const TABLE: &str = "remote_credentials";
const UNIQUE_INDEX: &str = "uq_remote_credentials_owner_host";

// Explicit table name, matching the entity's `table_name` (CLAUDE.md pitfall
// 13): a derived `Iden` would produce the singular `remote_credential`.
#[derive(DeriveIden)]
enum RemoteCredentials {
    #[sea_orm(iden = "remote_credentials")]
    Table,
    Id,
    OwnerKind,
    OwnerId,
    Host,
    Username,
    TokenEncrypted,
    AuthScheme,
    Label,
    LastUsedAt,
    CreatedAt,
    UpdatedAt,
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        if manager.has_table(TABLE).await? {
            // Repair a partially applied schema: every column is guarded, so a
            // re-run after a failed create (or a hand-edited database) converges.
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
                        .table(RemoteCredentials::Table)
                        .col(RemoteCredentials::OwnerKind)
                        .col(RemoteCredentials::OwnerId)
                        .col(RemoteCredentials::Host)
                        .unique()
                        .to_owned(),
                )
                .await?;
        }
        Ok(())
    }

    /// Drops the table. Tokens are user input, not derived data, but rolling
    /// back this migration means the feature is gone: nothing else in the
    /// schema references these rows, and re-applying starts from an empty
    /// dictionary that the UI can refill.
    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(
                Table::drop()
                    .table(RemoteCredentials::Table)
                    .if_exists()
                    .to_owned(),
            )
            .await
    }
}

impl Migration {
    async fn add_missing_columns(&self, manager: &SchemaManager<'_>) -> Result<(), DbErr> {
        let columns: [(RemoteCredentials, ColumnDef); 11] = [
            (
                RemoteCredentials::Id,
                ColumnDef::new(RemoteCredentials::Id)
                    .big_integer()
                    .not_null()
                    .auto_increment()
                    .primary_key()
                    .to_owned(),
            ),
            (
                RemoteCredentials::OwnerKind,
                ColumnDef::new(RemoteCredentials::OwnerKind)
                    .string_len(16)
                    .not_null()
                    .to_owned(),
            ),
            (
                RemoteCredentials::OwnerId,
                ColumnDef::new(RemoteCredentials::OwnerId)
                    .big_integer()
                    .not_null()
                    .to_owned(),
            ),
            (
                RemoteCredentials::Host,
                ColumnDef::new(RemoteCredentials::Host)
                    .string_len(255)
                    .not_null()
                    .to_owned(),
            ),
            (
                RemoteCredentials::Username,
                ColumnDef::new(RemoteCredentials::Username)
                    .string_len(255)
                    .to_owned(),
            ),
            (
                RemoteCredentials::TokenEncrypted,
                ColumnDef::new(RemoteCredentials::TokenEncrypted)
                    .text()
                    .not_null()
                    .to_owned(),
            ),
            (
                RemoteCredentials::AuthScheme,
                ColumnDef::new(RemoteCredentials::AuthScheme)
                    .string_len(16)
                    .not_null()
                    .default("basic")
                    .to_owned(),
            ),
            (
                RemoteCredentials::Label,
                ColumnDef::new(RemoteCredentials::Label)
                    .string_len(128)
                    .to_owned(),
            ),
            (
                RemoteCredentials::LastUsedAt,
                ColumnDef::new(RemoteCredentials::LastUsedAt)
                    .timestamp_with_time_zone()
                    .to_owned(),
            ),
            (
                RemoteCredentials::CreatedAt,
                ColumnDef::new(RemoteCredentials::CreatedAt)
                    .timestamp_with_time_zone()
                    .not_null()
                    .to_owned(),
            ),
            (
                RemoteCredentials::UpdatedAt,
                ColumnDef::new(RemoteCredentials::UpdatedAt)
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
                            .table(RemoteCredentials::Table)
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
        .table(RemoteCredentials::Table)
        .if_not_exists()
        .col(
            ColumnDef::new(RemoteCredentials::Id)
                .big_integer()
                .not_null()
                .auto_increment()
                .primary_key(),
        )
        // `user` | `org`.
        .col(
            ColumnDef::new(RemoteCredentials::OwnerKind)
                .string_len(16)
                .not_null(),
        )
        // `users.id` or `organizations.id`, depending on `owner_kind`.
        .col(
            ColumnDef::new(RemoteCredentials::OwnerId)
                .big_integer()
                .not_null(),
        )
        // Normalised key from `net_guard::url_host_key`: lowercase, default
        // ports (80/443/9418) dropped, non-default ones kept as `host:port`.
        // Nothing else is stored about the remote — the host *is* the lookup
        // key, so `https://github.com` and `git://github.com` share one token.
        .col(
            ColumnDef::new(RemoteCredentials::Host)
                .string_len(255)
                .not_null(),
        )
        // Basic auth user. NULL means "use the scheme default"
        // (`x-access-token` for basic, ignored for bearer).
        .col(ColumnDef::new(RemoteCredentials::Username).string_len(255))
        // AES-256-GCM ciphertext, key derived from the instance `jwt_secret`
        // (the same envelope as `ci_secrets` / `sso_providers`). Never plain.
        .col(
            ColumnDef::new(RemoteCredentials::TokenEncrypted)
                .text()
                .not_null(),
        )
        // `basic` (Authorization: Basic base64(user:token)) or
        // `bearer` (Authorization: Bearer <token>).
        .col(
            ColumnDef::new(RemoteCredentials::AuthScheme)
                .string_len(16)
                .not_null()
                .default("basic"),
        )
        // Display name; the host stays the lookup key.
        .col(ColumnDef::new(RemoteCredentials::Label).string_len(128))
        // Last time a git call actually used this token. Presence of a value is
        // how the UI distinguishes "configured" from "configured and proven".
        .col(ColumnDef::new(RemoteCredentials::LastUsedAt).timestamp_with_time_zone())
        .col(
            ColumnDef::new(RemoteCredentials::CreatedAt)
                .timestamp_with_time_zone()
                .not_null(),
        )
        .col(
            ColumnDef::new(RemoteCredentials::UpdatedAt)
                .timestamp_with_time_zone()
                .not_null(),
        )
        // One token per host per owner. The arbiter of concurrent creates:
        // the loser of a race hits this index instead of writing a second row.
        .index(
            Index::create()
                .if_not_exists()
                .name(UNIQUE_INDEX)
                .col(RemoteCredentials::OwnerKind)
                .col(RemoteCredentials::OwnerId)
                .col(RemoteCredentials::Host)
                .unique(),
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

    /// How many migrations to roll back so this one (and everything after it)
    /// is undone.
    fn rollback_steps() -> u32 {
        let migrations = crate::migrations::Migrator::migrations();
        let position = migrations
            .iter()
            .rev()
            .position(|migration| migration.name() == "m20261123_000001_create_remote_credentials")
            .expect("this migration must be registered");
        (position + 1) as u32
    }

    /// CLAUDE.md pitfall #13: the explicit table name must match the entity, on
    /// every backend, and the unique index must survive each dialect.
    #[test]
    fn ddl_uses_the_plural_table_name_and_unique_index_on_all_backends() {
        for sql in [
            table_create().to_string(SqliteQueryBuilder),
            table_create().to_string(PostgresQueryBuilder),
            table_create().to_string(MysqlQueryBuilder),
        ] {
            assert!(sql.contains(TABLE), "{TABLE} missing in: {sql}");
            assert!(!sql.contains("remote_credential "), "singular table in: {sql}");
            assert!(
                sql.contains(UNIQUE_INDEX),
                "{UNIQUE_INDEX} missing in: {sql}"
            );
        }
    }

    /// `down` really drops the table, and `up` recreates it: rollback followed
    /// by re-apply must not leave a half-applied schema behind.
    #[tokio::test]
    async fn down_drops_the_table_and_up_recreates_it() {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        crate::migrations::Migrator::up(&db, None).await.unwrap();
        assert_eq!(
            count(
                &db,
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'remote_credentials'"
            )
            .await,
            1
        );

        // Roll back to just before this migration, not "one step": another
        // migration is allowed to be appended after it, and a hard-coded 1 then
        // drops the wrong table. Counting from the end keeps this test true
        // forever.
        let steps = rollback_steps();
        crate::migrations::Migrator::down(&db, Some(steps))
            .await
            .unwrap();
        assert_eq!(
            count(
                &db,
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'remote_credentials'"
            )
            .await,
            0,
            "down must drop the table"
        );

        crate::migrations::Migrator::up(&db, None).await.unwrap();
        assert_eq!(
            count(
                &db,
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name = 'uq_remote_credentials_owner_host'"
            )
            .await,
            1,
            "the unique index must be back after re-apply"
        );
    }

    /// The whole point of `owner_kind`: a user row and an org row may share a
    /// host, but the same owner may not configure the same host twice.
    #[tokio::test]
    async fn uniqueness_is_per_owner_and_host() {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        crate::migrations::Migrator::up(&db, None).await.unwrap();

        let insert = |kind: &str, owner: i64, host: &str| {
            format!(
                "INSERT INTO remote_credentials \
                 (owner_kind, owner_id, host, token_encrypted, auth_scheme, created_at, updated_at) \
                 VALUES ('{kind}', {owner}, '{host}', 'cipher', 'basic', \
                 '2026-11-23 00:00:00+00:00', '2026-11-23 00:00:00+00:00')"
            )
        };
        let exec = |sql: String| {
            let db = db.clone();
            async move { db.execute(Statement::from_string(db.get_database_backend(), sql)).await }
        };

        exec(insert("user", 1, "github.com")).await.unwrap();
        exec(insert("org", 7, "github.com")).await.unwrap();
        assert!(
            exec(insert("user", 1, "github.com")).await.is_err(),
            "a second row for the same (owner, host) must be rejected"
        );
        assert!(
            exec(insert("user", 1, "gitlab.com")).await.is_ok(),
            "a different host is a different key"
        );
    }
}
