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
pub mod m20260510_000001_create_runners;
pub mod m20260510_000002_alter_pipeline_jobs_add_runner_fields;
pub mod m20260510_000003_add_pipeline_jobs_updated_at;
pub mod m20260510_000004_create_artifacts;
pub mod m20260511_000001_add_pr_head_repo_id;
pub mod m20260511_000002_add_missing_indexes;
pub mod m20260511_000003_fix_fts5_triggers;
pub mod m20260512_000001_create_code_fts;
pub mod m20260607_000001_create_mirrors;
pub mod m20260607_000002_create_boards;
pub mod m20260607_000003_create_time_entries;
pub mod m20260607_000004_create_import_tasks;
pub mod m20260607_000005_create_package_registry;
pub mod m20260607_000006_alter_users_auth;
pub mod m20260607_000007_create_oauth_accounts;
pub mod m20260607_000008_create_mfa_backup_codes;
pub mod m20260607_000009_create_login_logs;
pub mod m20260607_000010_create_sso_providers;
pub mod m20260607_000011_create_audit_logs;
pub mod m20260608_000001_create_oci_tables;
pub mod m20260608_000002_oauth_accounts_unique;
pub mod m20260608_000003_add_job_tags;

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
            Box::new(m20260510_000001_create_runners::Migration),
            Box::new(m20260510_000002_alter_pipeline_jobs_add_runner_fields::Migration),
            Box::new(m20260510_000003_add_pipeline_jobs_updated_at::Migration),
            Box::new(m20260510_000004_create_artifacts::Migration),
            Box::new(m20260511_000001_add_pr_head_repo_id::Migration),
            Box::new(m20260511_000002_add_missing_indexes::Migration),
            Box::new(m20260511_000003_fix_fts5_triggers::Migration),
            Box::new(m20260512_000001_create_code_fts::Migration),
            Box::new(m20260607_000001_create_mirrors::Migration),
            Box::new(m20260607_000002_create_boards::Migration),
            Box::new(m20260607_000003_create_time_entries::Migration),
            Box::new(m20260607_000004_create_import_tasks::Migration),
            Box::new(m20260607_000005_create_package_registry::Migration),
            Box::new(m20260607_000006_alter_users_auth::Migration),
            Box::new(m20260607_000007_create_oauth_accounts::Migration),
            Box::new(m20260607_000008_create_mfa_backup_codes::Migration),
            Box::new(m20260607_000009_create_login_logs::Migration),
            Box::new(m20260607_000010_create_sso_providers::Migration),
            Box::new(m20260607_000011_create_audit_logs::Migration),
            Box::new(m20260608_000001_create_oci_tables::Migration),
            Box::new(m20260608_000002_oauth_accounts_unique::Migration),
            Box::new(m20260608_000003_add_job_tags::Migration),
        ]
    }
}
