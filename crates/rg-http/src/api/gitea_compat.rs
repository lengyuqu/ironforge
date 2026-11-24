//! Endpoints a Gitea client asks for before anything else.
//!
//! `tea`, the Gitea SDK and most third-party tooling start with
//! `GET /api/v1/version` to decide whether the server speaks their protocol,
//! then read `settings/api` for paging limits, then `GET /api/v1/user` to
//! identify the caller. Missing any of the three makes a client fail at
//! startup with a message that says nothing about the real problem — "not a
//! Gitea instance" — so they come before any richer endpoint.

use axum::{response::IntoResponse, Json};
use utoipa::ToSchema;

/// The Gitea version whose API this server implements.
///
/// This is a **compatibility claim**, not this project's own version: clients
/// branch on it to decide which fields to expect. It matches the baseline the
/// gap analysis is measured against.
const COMPATIBLE_GITEA_VERSION: &str = "1.26.4";

#[derive(serde::Serialize, ToSchema)]
pub struct VersionResponse {
    /// The Gitea version this API is compatible with.
    pub version: String,
}

/// GET /api/v1/version
#[utoipa::path(
    get,
    path = "/version",
    tag = "Miscellaneous",
    responses((status = 200, description = "Compatible Gitea version", body = VersionResponse)),
)]
pub async fn version() -> impl IntoResponse {
    Json(VersionResponse {
        version: COMPATIBLE_GITEA_VERSION.to_string(),
    })
}

#[derive(serde::Serialize, ToSchema)]
pub struct ApiSettingsResponse {
    /// Largest page a client may request.
    pub max_response_items: i64,
    /// Page size a client should use when it does not ask for one.
    pub default_paging_num: i64,
    /// Entries per page when listing a repository tree.
    pub default_git_trees_per_page: i64,
    /// Largest blob the API will return inline, in bytes.
    pub default_max_blob_size: i64,
}

/// GET /api/v1/settings/api
///
/// These are the values the server actually enforces, not Gitea's defaults
/// copied over: a client that trusts them would otherwise request pages this
/// server rejects.
#[utoipa::path(
    get,
    path = "/settings/api",
    tag = "Settings",
    responses((status = 200, description = "API limits", body = ApiSettingsResponse)),
)]
pub async fn api_settings() -> impl IntoResponse {
    Json(ApiSettingsResponse {
        max_response_items: crate::pagination::MAX_PER_PAGE as i64,
        default_paging_num: crate::pagination::default_per_page() as i64,
        default_git_trees_per_page: 1000,
        default_max_blob_size: 10 * 1024 * 1024,
    })
}

#[derive(serde::Serialize, ToSchema)]
pub struct UiSettingsResponse {
    /// Whether the instance allows creating repositories.
    pub allow_repository_creation: bool,
}

/// GET /api/v1/settings/ui
#[utoipa::path(
    get,
    path = "/settings/ui",
    tag = "Settings",
    responses((status = 200, description = "UI settings", body = UiSettingsResponse)),
)]
pub async fn ui_settings() -> impl IntoResponse {
    Json(UiSettingsResponse {
        allow_repository_creation: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use axum::response::IntoResponse;

    async fn json_body(response: axum::response::Response) -> serde_json::Value {
        let bytes = to_bytes(response.into_body(), 4096).await.unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    /// A client decides whether it can talk to this server by this value, so it
    /// has to be the Gitea version the API is measured against — not this
    /// project's own version.
    #[tokio::test]
    async fn version_reports_the_compatible_gitea_version() {
        let body = json_body(version().await.into_response()).await;
        assert_eq!(body["version"], super::COMPATIBLE_GITEA_VERSION);
        assert_eq!(body["version"], "1.26.4");
    }

    /// The limits must be the ones the server enforces. A client that trusts
    /// them and asks for a bigger page would otherwise be rejected for no
    /// reason it can see.
    #[tokio::test]
    async fn api_settings_report_the_enforced_page_limit() {
        let body = json_body(api_settings().await.into_response()).await;
        assert_eq!(
            body["max_response_items"].as_i64(),
            Some(crate::pagination::MAX_PER_PAGE as i64)
        );
        assert!(
            body["default_paging_num"].as_i64().unwrap() <= crate::pagination::MAX_PER_PAGE as i64,
            "the default must not exceed the maximum: {body}"
        );
    }
}
