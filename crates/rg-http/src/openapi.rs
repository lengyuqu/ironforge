//! OpenAPI (Swagger) documentation for IronForge REST API.
//!
//! Provides auto-generated OpenAPI 3.0 spec via utoipa.
//! Access at:
//!   - OpenAPI JSON: GET /api-docs/openapi.json
//!   - Swagger UI:    GET /api-docs/

use std::sync::Arc;

use utoipa::OpenApi;

/// Paginated response wrapper for repository listing.
#[derive(utoipa::ToSchema)]
pub struct PaginatedRepoResponse {
    pub data: Vec<crate::api::repos::RepoResponse>,
    pub pagination: crate::pagination::PaginationMeta,
}

/// IronForge API — OpenAPI specification.
#[derive(OpenApi)]
#[openapi(
    info(
        title = "IronForge API",
        version = "0.1.0",
        description = "IronForge is a self-hosted Git platform written in Rust. \
            This API provides repository management, issue tracking, pull requests, \
            CI/CD pipelines, wiki, LFS, webhooks, and more.",
    ),
    paths(
        // Users
        crate::api::users::register,
        crate::api::users::login,
        crate::api::users::me,
        crate::api::users::list_tokens,
        crate::api::users::create_token,
        crate::api::users::delete_token,
        // Repositories
        crate::api::repos::create_repo,
        crate::api::repos::list_repos,
        crate::api::repos::get_repo,
        crate::api::repos::delete_repo_handler,
        crate::api::repos::star_repo,
        crate::api::repos::get_stargazers,
        crate::api::repos::watch_repo,
        crate::api::repos::unwatch_repo,
        crate::api::repos::fork_repo_handler,
        crate::api::repos::list_forks_handler,
        crate::api::repos::transfer_repo_handler,
        crate::api::repos::create_commit_status,
        crate::api::repos::list_commit_statuses,
        crate::api::repos::get_combined_status,
        // Issues
        crate::api::issues::list_issues,
        crate::api::issues::get_issue,
        crate::api::issues::create_issue,
        crate::api::issues::update_issue,
        crate::api::issues::list_comments,
        crate::api::issues::add_comment,
        crate::api::issues::list_milestones,
        crate::api::issues::create_milestone,
        crate::api::issues::get_milestone,
        crate::api::issues::update_milestone,
        crate::api::issues::delete_milestone,
        crate::api::issues::get_issue_labels,
        // Labels
        crate::api::labels::list_labels,
        crate::api::labels::get_label,
        crate::api::labels::create_label,
        crate::api::labels::update_label,
        crate::api::labels::delete_label,
        // Pull Requests
        crate::api::pulls::list_prs,
        crate::api::pulls::get_pr,
        crate::api::pulls::create_pr,
        crate::api::pulls::update_pr,
        crate::api::pulls::get_diff,
        crate::api::pulls::merge_pr,
        // Reviews
        crate::api::reviews::list_reviews,
        crate::api::reviews::submit_review,
        crate::api::reviews::get_review,
        crate::api::reviews::dismiss_review,
        crate::api::reviews::list_review_comments,
        crate::api::reviews::create_review_comment,
        // Wiki
        crate::api::wiki::list_pages,
        crate::api::wiki::get_page,
        crate::api::wiki::create_page,
        crate::api::wiki::update_page,
        crate::api::wiki::delete_page,
        // LFS
        crate::api::lfs::batch,
        crate::api::lfs::upload_object,
        crate::api::lfs::download_object,
        // Webhooks
        crate::api::webhooks::list_webhooks,
        crate::api::webhooks::create_webhook,
        crate::api::webhooks::get_webhook,
        crate::api::webhooks::update_webhook,
        crate::api::webhooks::delete_webhook,
        crate::api::webhooks::list_deliveries,
        crate::api::webhooks::redeliver,
        // CI/CD
        crate::api::ci::list_pipelines,
        crate::api::ci::get_pipeline,
        crate::api::ci::get_job,
        crate::api::ci::trigger_pipeline,
        crate::api::ci::retry_pipeline,
        crate::api::ci::cancel_pipeline,
        // Releases
        crate::api::releases::list_releases,
        crate::api::releases::create_release,
        crate::api::releases::get_release,
        crate::api::releases::update_release,
        crate::api::releases::delete_release,
        crate::api::releases::list_assets,
        crate::api::releases::upload_asset,
        crate::api::releases::get_asset,
        crate::api::releases::download_asset,
        crate::api::releases::delete_asset,
        // Organizations
        crate::api::orgs::create_org,
        crate::api::orgs::get_org,
        crate::api::orgs::list_orgs,
        crate::api::orgs::update_org,
        crate::api::orgs::delete_org,
        crate::api::orgs::list_org_members,
        crate::api::orgs::add_org_member,
        crate::api::orgs::remove_org_member,
        crate::api::orgs::create_team,
        crate::api::orgs::list_org_teams,
        crate::api::orgs::get_team,
        crate::api::orgs::delete_team,
        crate::api::orgs::list_team_members,
        crate::api::orgs::add_team_member,
        crate::api::orgs::remove_team_member,
        // Notifications
        crate::api::notifications::list_notifications,
        crate::api::notifications::unread_count,
        crate::api::notifications::mark_read,
        crate::api::notifications::mark_all_read,
        crate::api::notifications::delete_notification,
        // Search
        crate::api::search::search,
        // Branch Protection
        crate::api::branch_protection::list_protections,
        crate::api::branch_protection::create_protection,
        crate::api::branch_protection::get_protection,
        crate::api::branch_protection::update_protection,
        crate::api::branch_protection::delete_protection,
        // Collaborators
        crate::api::collaborators::list_collaborators,
        crate::api::collaborators::add_collaborator,
        crate::api::collaborators::update_permission,
        crate::api::collaborators::remove_collaborator,
        // Repository Content
        crate::api::repo_content::list_tree,
        crate::api::repo_content::get_blob,
        crate::api::repo_content::get_log,
        crate::api::repo_content::list_branches,
        crate::api::repo_content::list_tags,
        crate::api::repo_content::get_commit_signature,
        // Runners
        crate::api::runners::register,
        crate::api::runners::heartbeat,
        crate::api::runners::poll_job,
        crate::api::runners::start_job,
        crate::api::runners::upload_log,
        crate::api::runners::finish_job,
        crate::api::runners::list_runners_admin,
        crate::api::runners::delete_runner_admin,
        // Artifacts
        crate::api::artifacts::upload_artifact,
        crate::api::artifacts::list_pipeline_artifacts,
        crate::api::artifacts::get_artifact,
        crate::api::artifacts::delete_artifact,
        // Admin
        crate::api::admin::list_users,
        crate::api::admin::get_user,
        crate::api::admin::update_user,
        crate::api::admin::delete_user,
        crate::api::admin::list_orgs,
        crate::api::admin::get_org,
        crate::api::admin::delete_org,
        // AI Agent endpoints
        crate::api::ai::ai_repo_summary,
        crate::api::ai::ai_list_issues,
        crate::api::ai::ai_list_prs,
        crate::api::ai::ai_repo_tree,
        crate::api::ai::ai_search_code,
        // Mirrors
        crate::api::mirrors::create_mirror,
        crate::api::mirrors::get_mirror,
        crate::api::mirrors::update_mirror,
        crate::api::mirrors::delete_mirror,
        crate::api::mirrors::trigger_mirror_sync,
        // Boards
        crate::api::boards::create_board,
        crate::api::boards::list_boards,
        crate::api::boards::get_board,
        crate::api::boards::update_board,
        crate::api::boards::delete_board,
        crate::api::boards::create_column,
        crate::api::boards::update_column,
        crate::api::boards::delete_column,
        crate::api::boards::create_card,
        crate::api::boards::update_card,
        crate::api::boards::move_card,
        crate::api::boards::reorder_cards,
        crate::api::boards::delete_card,
        // Time Tracking
        crate::api::time_tracking::add_time,
        crate::api::time_tracking::list_time_entries,
        crate::api::time_tracking::total_time,
        crate::api::time_tracking::delete_time_entry,
    ),
    components(
        schemas(
            crate::api::users::RegisterRequest,
            crate::api::users::LoginRequest,
            crate::api::users::AuthResponse,
            crate::api::users::UserProfile,
            crate::api::users::CreateTokenRequest,
            crate::api::repos::CreateRepoRequest,
            crate::api::repos::RepoResponse,
            crate::api::repos::WatchRequest,
            crate::api::repos::ForkRequest,
            crate::api::repos::TransferRequest,
            crate::api::repos::CreateCommitStatusRequest,
            crate::pagination::PaginationParams,
            crate::pagination::PaginationMeta,
            PaginatedRepoResponse,
            crate::api::runners::RegisterRunnerRequest,
            crate::api::runners::RegisterRunnerResponse,
            crate::api::runners::HeartbeatResponse,
            crate::api::runners::PollJobResponse,
            crate::api::runners::RunnerInfoResponse,
            crate::api::runners::FinishJobRequest,
            crate::api::artifacts::ArtifactResponse,
            crate::api::artifacts::UploadArtifactResponse,
            crate::api::artifacts::UploadArtifactRequest,
            crate::api::ai::RepoSummary,
            crate::api::ai::IssueSummary,
            crate::api::ai::PrSummary,
            crate::api::mirrors::CreateMirrorRequest,
            crate::api::mirrors::UpdateMirrorRequest,
            crate::api::boards::CreateBoardRequest,
            crate::api::boards::UpdateBoardRequest,
            crate::api::boards::CreateColumnRequest,
            crate::api::boards::UpdateColumnRequest,
            crate::api::boards::CreateCardRequest,
            crate::api::boards::UpdateCardRequest,
            crate::api::boards::MoveCardRequest,
            crate::api::boards::ReorderCardsRequest,
            crate::api::time_tracking::AddTimeRequest,
        )
    ),
    tags(
        (name = "Users", description = "User registration, authentication, and profile"),
        (name = "Repositories", description = "Repository CRUD and management"),
        (name = "Issues", description = "Issue tracking"),
        (name = "Labels", description = "Label management"),
        (name = "Pull Requests", description = "Pull request workflow"),
        (name = "Reviews", description = "Code review"),
        (name = "Wiki", description = "Wiki pages"),
        (name = "LFS", description = "Git Large File Storage"),
        (name = "Webhooks", description = "Webhook management"),
        (name = "CI/CD", description = "Continuous Integration and Delivery"),
        (name = "Releases", description = "Release management"),
        (name = "Organizations", description = "Organization management"),
        (name = "Notifications", description = "User notifications"),
        (name = "Search", description = "Full-text search"),
        (name = "Branch Protection", description = "Branch protection rules"),
        (name = "Collaborators", description = "Repository collaborators"),
        (name = "Repository Content", description = "Browse repository files"),
        (name = "Runners", description = "CI/CD runner management"),
        (name = "Artifacts", description = "CI/CD artifacts"),
        (name = "Admin", description = "Administration"),
        (name = "AI", description = "AI Agent专用端点，提供AI友好的仓库/Issue/PR数据"),
        (name = "Mirrors", description = "Repository mirroring"),
        (name = "Boards", description = "Project boards (Kanban)"),
        (name = "Time Tracking", description = "Issue time tracking"),
    )
)]
pub struct ApiDoc;

/// Return the OpenAPI spec as a JSON string.
pub fn openapi_spec() -> String {
    ApiDoc::openapi().to_pretty_json().unwrap_or_default()
}

/// Lazy-initialized Swagger UI config (avoids re-computing on every request).
pub fn swagger_config() -> Arc<utoipa_swagger_ui::Config<'static>> {
    Arc::new(utoipa_swagger_ui::Config::from(
        "/api-docs/openapi.json",
    ))
}
