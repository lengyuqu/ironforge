//! LDAP group → team sync configuration on `sso_providers`.
//!
//! Three nullable columns, all-or-nothing. They deliberately do not extend
//! `sso_provider_ops::upsert`, which already takes seventeen arguments: group
//! sync is a second, optional configuration of a provider, written by its own
//! endpoint, and keeping it out of the create path means an existing provider
//! edit cannot accidentally clear it.
//!
//! * `ldap_group_base_dn` — where to search for groups
//! * `ldap_group_filter` — which entries are groups, e.g. `(objectClass=groupOfNames)`
//! * `ldap_group_team_org_id` — the org whose teams the group names refer to
//!
//! Group sync is enabled only when all three are present. Half a configuration
//! would silently sync nothing, which is the failure mode that looks like
//! "the feature is broken" three weeks later.
//!
//! `up` is idempotent (each column guarded by `has_column`) and `down` drops
//! the three columns in reverse order, so it runs on SQLite, PostgreSQL and
//! MySQL alike.

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261123_000003_add_ldap_group_sync"
    }
}

const SSO_PROVIDERS: &str = "sso_providers";

#[derive(DeriveIden)]
enum SsoProviders {
    #[sea_orm(iden = "sso_providers")]
    Table,
    LdapGroupBaseDn,
    LdapGroupFilter,
    LdapGroupTeamOrgId,
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let columns: [(SsoProviders, ColumnDef); 3] = [
            (
                SsoProviders::LdapGroupBaseDn,
                ColumnDef::new(SsoProviders::LdapGroupBaseDn)
                    .text()
                    .to_owned(),
            ),
            (
                SsoProviders::LdapGroupFilter,
                ColumnDef::new(SsoProviders::LdapGroupFilter)
                    .text()
                    .to_owned(),
            ),
            (
                SsoProviders::LdapGroupTeamOrgId,
                ColumnDef::new(SsoProviders::LdapGroupTeamOrgId)
                    .big_integer()
                    .to_owned(),
            ),
        ];
        for (column, definition) in columns {
            let name = column.to_string();
            if !manager.has_column(SSO_PROVIDERS, name.as_str()).await? {
                manager
                    .alter_table(
                        Table::alter()
                            .table(SsoProviders::Table)
                            .add_column(definition)
                            .to_owned(),
                    )
                    .await?;
            }
        }
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        for column in [
            SsoProviders::LdapGroupTeamOrgId,
            SsoProviders::LdapGroupFilter,
            SsoProviders::LdapGroupBaseDn,
        ] {
            let name = column.to_string();
            if manager.has_column(SSO_PROVIDERS, name.as_str()).await? {
                manager
                    .alter_table(
                        Table::alter()
                            .table(SsoProviders::Table)
                            .drop_column(column)
                            .to_owned(),
                    )
                    .await?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::{ConnectionTrait, Database, Statement};
    use sea_orm_migration::MigratorTrait;

    async fn has_column(db: &sea_orm::DatabaseConnection, column: &str) -> bool {
        let sql = format!(
            "SELECT COUNT(*) FROM pragma_table_info('sso_providers') WHERE name = '{column}'"
        );
        let row = db
            .query_one(Statement::from_string(db.get_database_backend(), sql))
            .await
            .unwrap()
            .unwrap();
        row.try_get_by_index::<i64>(0).unwrap() == 1
    }

    fn rollback_steps() -> u32 {
        let migrations = crate::migrations::Migrator::migrations();
        let position = migrations
            .iter()
            .rev()
            .position(|migration| migration.name() == "m20261123_000003_add_ldap_group_sync")
            .expect("this migration must be registered");
        (position + 1) as u32
    }

    #[tokio::test]
    async fn the_three_columns_exist_and_are_all_optional() {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        crate::migrations::Migrator::up(&db, None).await.unwrap();
        for column in [
            "ldap_group_base_dn",
            "ldap_group_filter",
            "ldap_group_team_org_id",
        ] {
            assert!(has_column(&db, column).await, "{column} is missing");
        }

        // An existing provider row must survive the migration untouched: a
        // nullable column addition must not require a backfill.
        db.execute(Statement::from_string(
            db.get_database_backend(),
            "INSERT INTO sso_providers (name, slug, provider_type, enabled, created_at, updated_at) \
             VALUES ('corp', 'corp', 'ldap', 1, '2026-01-01 00:00:00+00:00', '2026-01-01 00:00:00+00:00')",
        ))
        .await
        .unwrap();
        let row = db
            .query_one(Statement::from_string(
                db.get_database_backend(),
                "SELECT ldap_group_base_dn, ldap_group_filter, ldap_group_team_org_id FROM sso_providers",
            ))
            .await
            .unwrap()
            .unwrap();
        assert!(row.try_get_by_index::<Option<String>>(0).unwrap().is_none());
        assert!(row.try_get_by_index::<Option<String>>(1).unwrap().is_none());
        assert!(row.try_get_by_index::<Option<i64>>(2).unwrap().is_none());
    }

    #[tokio::test]
    async fn down_drops_the_columns_and_up_puts_them_back() {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        crate::migrations::Migrator::up(&db, None).await.unwrap();
        assert!(has_column(&db, "ldap_group_team_org_id").await);

        crate::migrations::Migrator::down(&db, Some(rollback_steps()))
            .await
            .unwrap();
        assert!(
            !has_column(&db, "ldap_group_team_org_id").await,
            "down must drop the column"
        );

        crate::migrations::Migrator::up(&db, None).await.unwrap();
        assert!(
            has_column(&db, "ldap_group_team_org_id").await,
            "up must add it back"
        );
    }
}
