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
        crate::api::users::register,
        crate::api::users::login,
        crate::api::users::me,
        crate::api::repos::create_repo,
        crate::api::repos::list_repos,
        crate::api::repos::get_repo,
    ),
    components(
        schemas(
            crate::api::users::RegisterRequest,
            crate::api::users::LoginRequest,
            crate::api::users::AuthResponse,
            crate::api::users::UserProfile,
            crate::api::repos::CreateRepoRequest,
            crate::api::repos::RepoResponse,
            crate::pagination::PaginationParams,
            crate::pagination::PaginationMeta,
            PaginatedRepoResponse,
        )
    ),
    tags(
        (name = "Users", description = "User registration, authentication, and profile"),
        (name = "Repositories", description = "Repository CRUD and management"),
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
