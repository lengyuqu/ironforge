//! IronForge core business logic.
//!
//! Handles users, repositories, issues, pull requests, wikis, and webhooks.

use anyhow::Result;

/// Check if a username is valid.
pub fn validate_username(username: &str) -> Result<()> {
    if username.is_empty() {
        anyhow::bail!("username cannot be empty");
    }
    if username.len() > 39 {
        anyhow::bail!("username too long (max 39 characters)");
    }
    for c in username.chars() {
        if !c.is_alphanumeric() && c != '-' && c != '_' {
            anyhow::bail!("username contains invalid character: {}", c);
        }
    }
    Ok(())
}

/// Check if a repository name is valid.
pub fn validate_repo_name(name: &str) -> Result<()> {
    if name.is_empty() {
        anyhow::bail!("repository name cannot be empty");
    }
    if name.len() > 100 {
        anyhow::bail!("repository name too long (max 100 characters)");
    }
    for c in name.chars() {
        if !c.is_alphanumeric() && c != '-' && c != '_' && c != '.' {
            anyhow::bail!("repository name contains invalid character: {}", c);
        }
    }
    Ok(())
}
