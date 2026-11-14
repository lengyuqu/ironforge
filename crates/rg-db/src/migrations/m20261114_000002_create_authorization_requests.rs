//! `authorization_requests` + `authorization_decisions`: the generic approval
//! primitive (#19, step 1).
//!
//! A request is one ask to perform a privileged action, bound to
//! `(action, subject, head_sha)`. A decision is one person's vote on it; the
//! unique key `(request_id, decider_user_id)` is what makes a second vote by the
//! same person a no-op instead of a second ticket (same semantics as
//! `ci_environment_approvals`).
//!
//! Both tables are new; `up` is idempotent (table, columns and indexes are each
//! guarded) and `down` drops the decisions before their parent.
//!
//! `authorization_set_id` is a digest of the authorization rule in force when
//! the request was filed, not a foreign key: step 1 is limited to two tables, so
//! there is no `authorization_sets` row to point at. A change in the rule (who
//! may approve, how many votes) changes the digest, which is the "immutable
//! version" the request needs.

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261114_000002_create_authorization_requests"
    }
}

const REQUESTS: &str = "authorization_requests";
const DECISIONS: &str = "authorization_decisions";
const UQ_CLIENT_KEY: &str = "uq_authorization_requests_client_key";
const IX_SUBJECT: &str = "ix_authorization_requests_subject";
const IX_STATE: &str = "ix_authorization_requests_state";
const UQ_DECIDER: &str = "uq_authorization_decisions_request_decider";

// Explicit table names matching the entities' `table_name` (CLAUDE.md pitfall
// 13): a derived `Iden` would produce the singular `authorization_request`.
#[derive(DeriveIden)]
enum Requests {
    #[sea_orm(iden = "authorization_requests")]
    Table,
    Id,
    RepoId,
    Action,
    Subject,
    HeadSha,
    RequestedByKind,
    RequestedById,
    AuthorizationSetId,
    RequiredApprovals,
    State,
    StateReason,
    ExpiresAt,
    ConsumedAt,
    ConsumedBy,
    ConsumedEffectHash,
    ClientRequestId,
    PayloadHash,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum Decisions {
    #[sea_orm(iden = "authorization_decisions")]
    Table,
    Id,
    RequestId,
    DeciderUserId,
    Decision,
    PayloadHash,
    CreatedAt,
}

#[derive(DeriveIden)]
enum Repositories {
    Table,
    Id,
}

#[derive(DeriveIden)]
enum Users {
    Table,
    Id,
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        if manager.has_table(REQUESTS).await? {
            // Repair a partially applied schema: every column is guarded, so a
            // re-run after a failed create (or a hand-edited database) converges.
            add_missing_request_columns(manager).await?;
        } else {
            manager.create_table(request_table_create()).await?;
        }
        if !manager.has_index(REQUESTS, UQ_CLIENT_KEY).await? {
            manager.create_index(client_key_index()).await?;
        }
        if !manager.has_index(REQUESTS, IX_SUBJECT).await? {
            manager.create_index(subject_index()).await?;
        }
        if !manager.has_index(REQUESTS, IX_STATE).await? {
            manager.create_index(state_index()).await?;
        }

        if manager.has_table(DECISIONS).await? {
            add_missing_decision_columns(manager).await?;
        } else {
            manager.create_table(decision_table_create()).await?;
        }
        if !manager.has_index(DECISIONS, UQ_DECIDER).await? {
            manager.create_index(decider_index()).await?;
        }
        Ok(())
    }

    /// Drops the child table first so the parent's foreign key never blocks the
    /// drop on a backend that checks dependents.
    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Decisions::Table).if_exists().to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(Requests::Table).if_exists().to_owned())
            .await
    }
}

async fn add_missing_request_columns(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    for (name, definition) in request_column_defs() {
        if !manager.has_column(REQUESTS, name).await? {
            manager
                .alter_table(
                    Table::alter()
                        .table(Requests::Table)
                        .add_column(definition)
                        .to_owned(),
                )
                .await?;
        }
    }
    Ok(())
}

async fn add_missing_decision_columns(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    for (name, definition) in decision_column_defs() {
        if !manager.has_column(DECISIONS, name).await? {
            manager
                .alter_table(
                    Table::alter()
                        .table(Decisions::Table)
                        .add_column(definition)
                        .to_owned(),
                )
                .await?;
        }
    }
    Ok(())
}

/// `(column name, definition)`, in table order. The name is taken from the
/// `Iden` so the guard and the DDL cannot drift apart.
fn request_column_defs() -> Vec<(&'static str, ColumnDef)> {
    vec![
        (
            "id",
            ColumnDef::new(Requests::Id)
                .big_integer()
                .not_null()
                .auto_increment()
                .primary_key()
                .to_owned(),
        ),
        (
            "repo_id",
            ColumnDef::new(Requests::RepoId)
                .big_integer()
                .not_null()
                .to_owned(),
        ),
        // Platform enum (`rg_core::authorization::Action`), never a client string.
        (
            "action",
            ColumnDef::new(Requests::Action)
                .string_len(48)
                .not_null()
                .to_owned(),
        ),
        // The domain the decision covers: `pr_id` / `job_id` / `ref` / ...
        (
            "subject",
            ColumnDef::new(Requests::Subject)
                .string_len(255)
                .not_null()
                .to_owned(),
        ),
        // Binding point, normalised by `normalize_commit_sha`. Null for actions
        // that are not tied to a commit (a protected `ref`, a package version).
        (
            "head_sha",
            ColumnDef::new(Requests::HeadSha)
                .string_len(64)
                .null()
                .to_owned(),
        ),
        // `human` | `agent_session`.
        (
            "requested_by_kind",
            ColumnDef::new(Requests::RequestedByKind)
                .string_len(16)
                .not_null()
                .to_owned(),
        ),
        // `users.id` or `agent_sessions.id`, depending on the kind. Deliberately
        // no foreign key: the column carries two subject types.
        (
            "requested_by_id",
            ColumnDef::new(Requests::RequestedById)
                .big_integer()
                .not_null()
                .to_owned(),
        ),
        // Digest of the authorization rule in force; see the module comment.
        (
            "authorization_set_id",
            ColumnDef::new(Requests::AuthorizationSetId)
                .big_integer()
                .not_null()
                .to_owned(),
        ),
        // The rule's threshold, frozen with the request: `consumed` has to be
        // able to answer "were there enough votes" inside the caller's
        // transaction, where re-evaluating the rule would need the whole
        // permission stack. `authorization_set_id` freezes the rule; this is its
        // numeric half.
        (
            "required_approvals",
            ColumnDef::new(Requests::RequiredApprovals)
                .integer()
                .not_null()
                .default(1)
                .to_owned(),
        ),
        (
            "state",
            ColumnDef::new(Requests::State)
                .string_len(16)
                .not_null()
                .default("pending")
                .to_owned(),
        ),
        // Why the request left `pending`: `head_sha_changed`, `expired`,
        // `execution_failed_not_retryable`, ...
        (
            "state_reason",
            ColumnDef::new(Requests::StateReason)
                .string_len(48)
                .null()
                .to_owned(),
        ),
        (
            "expires_at",
            ColumnDef::new(Requests::ExpiresAt)
                .timestamp_with_time_zone()
                .not_null()
                .to_owned(),
        ),
        (
            "consumed_at",
            ColumnDef::new(Requests::ConsumedAt)
                .timestamp_with_time_zone()
                .null()
                .to_owned(),
        ),
        // Who executed the action. No foreign key: an audit row must outlive the
        // account it names (same reasoning as `audit_log.on_behalf_of_user_id`).
        (
            "consumed_by",
            ColumnDef::new(Requests::ConsumedBy)
                .big_integer()
                .null()
                .to_owned(),
        ),
        // Merge sha / release version id, for auditing the effect.
        (
            "consumed_effect_hash",
            ColumnDef::new(Requests::ConsumedEffectHash)
                .string_len(128)
                .null()
                .to_owned(),
        ),
        (
            "client_request_id",
            ColumnDef::new(Requests::ClientRequestId)
                .string_len(128)
                .not_null()
                .to_owned(),
        ),
        (
            "payload_hash",
            ColumnDef::new(Requests::PayloadHash)
                .string_len(64)
                .not_null()
                .to_owned(),
        ),
        (
            "created_at",
            ColumnDef::new(Requests::CreatedAt)
                .timestamp_with_time_zone()
                .not_null()
                .to_owned(),
        ),
        (
            "updated_at",
            ColumnDef::new(Requests::UpdatedAt)
                .timestamp_with_time_zone()
                .not_null()
                .to_owned(),
        ),
    ]
}

fn decision_column_defs() -> Vec<(&'static str, ColumnDef)> {
    vec![
        (
            "id",
            ColumnDef::new(Decisions::Id)
                .big_integer()
                .not_null()
                .auto_increment()
                .primary_key()
                .to_owned(),
        ),
        (
            "request_id",
            ColumnDef::new(Decisions::RequestId)
                .big_integer()
                .not_null()
                .to_owned(),
        ),
        (
            "decider_user_id",
            ColumnDef::new(Decisions::DeciderUserId)
                .big_integer()
                .not_null()
                .to_owned(),
        ),
        // `approve` | `deny`.
        (
            "decision",
            ColumnDef::new(Decisions::Decision)
                .string_len(16)
                .not_null()
                .to_owned(),
        ),
        // Re-checked at decision time; a mismatch means the request moved under
        // the approver's feet (TOCTOU) and the vote is refused.
        (
            "payload_hash",
            ColumnDef::new(Decisions::PayloadHash)
                .string_len(64)
                .not_null()
                .to_owned(),
        ),
        (
            "created_at",
            ColumnDef::new(Decisions::CreatedAt)
                .timestamp_with_time_zone()
                .not_null()
                .to_owned(),
        ),
    ]
}

fn client_key_index() -> IndexCreateStatement {
    Index::create()
        .if_not_exists()
        .name(UQ_CLIENT_KEY)
        .table(Requests::Table)
        .col(Requests::RepoId)
        .col(Requests::RequestedByKind)
        .col(Requests::RequestedById)
        .col(Requests::ClientRequestId)
        .unique()
        .to_owned()
}

fn subject_index() -> IndexCreateStatement {
    Index::create()
        .if_not_exists()
        .name(IX_SUBJECT)
        .table(Requests::Table)
        .col(Requests::RepoId)
        .col(Requests::Action)
        .col(Requests::Subject)
        .to_owned()
}

fn state_index() -> IndexCreateStatement {
    Index::create()
        .if_not_exists()
        .name(IX_STATE)
        .table(Requests::Table)
        .col(Requests::RepoId)
        .col(Requests::State)
        .to_owned()
}

fn decider_index() -> IndexCreateStatement {
    Index::create()
        .if_not_exists()
        .name(UQ_DECIDER)
        .table(Decisions::Table)
        .col(Decisions::RequestId)
        .col(Decisions::DeciderUserId)
        .unique()
        .to_owned()
}

fn request_table_create() -> TableCreateStatement {
    let mut table = Table::create();
    table.table(Requests::Table).if_not_exists();
    for (_, definition) in request_column_defs() {
        table.col(definition);
    }
    table
        // Unique and plain indexes are created separately in `up`, each behind a
        // `has_index` guard: sea-query renders a non-unique `.index()` of a
        // `TableCreateStatement` as a bare `CONSTRAINT name (cols)`, which SQLite
        // rejects, and one guarded definition per index also covers the repair
        // path where the table already exists.
        .foreign_key(
            ForeignKey::create()
                .from(Requests::Table, Requests::RepoId)
                .to(Repositories::Table, Repositories::Id)
                .on_delete(ForeignKeyAction::Cascade),
        )
        .to_owned()
}

fn decision_table_create() -> TableCreateStatement {
    let mut table = Table::create();
    table.table(Decisions::Table).if_not_exists();
    for (_, definition) in decision_column_defs() {
        table.col(definition);
    }
    table
        .foreign_key(
            ForeignKey::create()
                .from(Decisions::Table, Decisions::RequestId)
                .to(Requests::Table, Requests::Id)
                .on_delete(ForeignKeyAction::Cascade),
        )
        .foreign_key(
            ForeignKey::create()
                .from(Decisions::Table, Decisions::DeciderUserId)
                .to(Users::Table, Users::Id)
                .on_delete(ForeignKeyAction::Cascade),
        )
        // One person, one vote; a repeat is `Ok(false)`, not a second ticket.
        // The unique index itself is `decider_index()`, created in `up` behind a
        // `has_index` guard.
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

    /// CLAUDE.md pitfall #13: explicit plural table names on every backend, and
    /// the constraints survive each dialect.
    #[test]
    fn ddl_uses_plural_table_names_and_constraints_on_all_backends() {
        let requests = request_table_create();
        let decisions = decision_table_create();
        for sql in [
            requests.to_string(SqliteQueryBuilder),
            requests.to_string(PostgresQueryBuilder),
            requests.to_string(MysqlQueryBuilder),
        ] {
            assert!(sql.contains(REQUESTS), "{REQUESTS} missing in: {sql}");
            assert!(
                !sql.contains("authorization_request "),
                "singular table in: {sql}"
            );
            assert!(
                sql.contains("FOREIGN KEY (\"repo_id\")")
                    || sql.contains("FOREIGN KEY (`repo_id`)"),
                "repo foreign key missing: {sql}"
            );
        }
        for sql in [
            decisions.to_string(SqliteQueryBuilder),
            decisions.to_string(PostgresQueryBuilder),
            decisions.to_string(MysqlQueryBuilder),
        ] {
            assert!(sql.contains(DECISIONS), "{DECISIONS} missing in: {sql}");
            assert!(
                !sql.contains("authorization_decision "),
                "singular table in: {sql}"
            );
            assert!(
                sql.contains("FOREIGN KEY (\"request_id\")")
                    || sql.contains("FOREIGN KEY (`request_id`)"),
                "request foreign key missing: {sql}"
            );
        }
        for (name, index) in [
            (UQ_CLIENT_KEY, client_key_index()),
            (IX_SUBJECT, subject_index()),
            (IX_STATE, state_index()),
            (UQ_DECIDER, decider_index()),
        ] {
            for sql in [
                index.to_string(SqliteQueryBuilder),
                index.to_string(PostgresQueryBuilder),
                index.to_string(MysqlQueryBuilder),
            ] {
                assert!(sql.contains(name), "{name} missing in: {sql}");
            }
        }
    }

    /// How many migrations to roll back so this one is undone. Counting from the
    /// end keeps the test correct when a later migration is appended.
    fn rollback_steps() -> u32 {
        let migrations = crate::migrations::Migrator::migrations();
        let position = migrations
            .iter()
            .rev()
            .position(|migration| {
                migration.name() == "m20261114_000002_create_authorization_requests"
            })
            .expect("this migration must be registered");
        (position + 1) as u32
    }

    /// `down` really drops both tables (child first) and `up` recreates them; the
    /// rollback must not leave a half-applied schema behind.
    #[tokio::test]
    async fn down_drops_both_tables_and_up_recreates_them() {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        crate::migrations::Migrator::up(&db, None).await.unwrap();
        assert_eq!(table_exists(&db, REQUESTS).await, 1);
        assert_eq!(table_exists(&db, DECISIONS).await, 1);

        crate::migrations::Migrator::down(&db, Some(rollback_steps()))
            .await
            .unwrap();
        assert_eq!(
            table_exists(&db, REQUESTS).await,
            0,
            "down must drop requests"
        );
        assert_eq!(
            table_exists(&db, DECISIONS).await,
            0,
            "down must drop decisions"
        );

        crate::migrations::Migrator::up(&db, None).await.unwrap();
        assert_eq!(table_exists(&db, REQUESTS).await, 1);
        assert_eq!(table_exists(&db, DECISIONS).await, 1);
        assert_eq!(
            count(
                &db,
                &format!(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name = '{UQ_DECIDER}'"
                )
            )
            .await,
            1,
            "the decision unique key must be back after re-apply"
        );
    }
}
