pub mod m20260424_000001_create_users;
pub mod m20260424_000002_create_repositories;
pub mod m20260424_000003_create_keys_tokens;
pub mod m20260424_000004_create_issues;
pub mod m20260424_000005_create_pull_requests;

use sea_orm_migration::prelude::*;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20260424_000001_create_users::Migration),
            Box::new(m20260424_000002_create_repositories::Migration),
            Box::new(m20260424_000003_create_keys_tokens::Migration),
            Box::new(m20260424_000004_create_issues::Migration),
            Box::new(m20260424_000005_create_pull_requests::Migration),
        ]
    }
}
