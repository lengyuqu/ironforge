//! Data migration import — GitHub / GitLab → IronForge.
//!
//! Supports importing repositories and their metadata (issues, PRs,
//! labels, milestones, releases, wiki) from external platforms.

pub mod github_client;
pub mod gitlab_client;
pub mod service;

use crate::error::CoreError;

/// Turn a non-success response from a source platform's API into a classified
/// error. Rejections caused by the request itself (bad token, unknown project,
/// rate limit) are the importing user's to fix; anything else is an upstream
/// outage and is kept out of client-visible text.
pub(crate) fn upstream_api_error(
    provider: &str,
    status: reqwest::StatusCode,
    body: &str,
) -> anyhow::Error {
    if status.is_client_error() {
        let detail: String = body.chars().take(200).collect();
        anyhow::Error::new(CoreError::invalid_input(format!(
            "{provider} API error ({status}): {detail}"
        )))
    } else {
        tracing::error!(provider, %status, "source platform API request failed");
        anyhow::Error::new(CoreError::internal(format!(
            "{provider} API error ({status})"
        )))
    }
}
