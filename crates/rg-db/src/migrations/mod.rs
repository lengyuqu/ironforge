pub mod m20260424_000001_create_users;
pub mod m20260424_000002_create_repositories;
pub mod m20260424_000003_create_keys_tokens;
pub mod m20260424_000004_create_issues;
pub mod m20260424_000005_create_pull_requests;
pub mod m20260424_000006_create_wiki_lfs_webhooks;
pub mod m20260424_000007_create_pipelines;
pub mod m20260424_000008_create_phase6;
pub mod m20260424_000009_create_phase8;
pub mod m20260427_000001_add_lfs_compression;
pub mod m20260508_000001_create_repo_stars_watches;
pub mod m20260508_000006_add_repo_soft_delete;
pub mod m20260508_000002_create_releases;
pub mod m20260508_000003_create_labels;
pub mod m20260508_000004_create_commit_statuses;
pub mod m20260508_000005_create_fts5_indexes;

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
            Box::new(m20260424_000006_create_wiki_lfs_webhooks::Migration),
            Box::new(m20260424_000007_create_pipelines::Migration),
            Box::new(m20260424_000008_create_phase6::Migration),
            Box::new(m20260424_000009_create_phase8::Migration),
            Box::new(m20260427_000001_add_lfs_compression::Migration),
            Box::new(m20260508_000001_create_repo_stars_watches::Migration),
            Box::new(m20260508_000006_add_repo_soft_delete::Migration),
            Box::new(m20260508_000002_create_releases::Migration),
            Box::new(m20260508_000003_create_labels::Migration),
            Box::new(m20260508_000004_create_commit_statuses::Migration),
            Box::new(m20260508_000005_create_fts5_indexes::Migration),
        ]
    }
}
