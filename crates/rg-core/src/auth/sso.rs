//! OAuth2 SSO service. Supports GitHub, GitLab, and custom OAuth2 providers.
//! Uses reqwest directly instead of the oauth2 crate to avoid type-state complexity.

use anyhow::{Context, Result};
use rand::Rng;
use serde::Deserialize;

#[derive(Debug, Clone)]
pub struct SsoProviderConfig {
    pub slug: String,
    pub provider_type: String,
    pub client_id: String,
    pub client_secret: String,
    pub redirect_url: String,
    pub scopes: Vec<String>,
    pub discovery_url: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SsoUserInfo {
    pub provider_user_id: String,
    pub provider_username: String,
    pub email: String,
    pub display_name: Option<String>,
    pub avatar_url: Option<String>,
}

/// Generate OAuth2 authorization URL. Returns (auth_url, csrf_token).
pub fn oauth2_authorize_url(config: &SsoProviderConfig) -> Result<(String, String)> {
    let base_url = config
        .default_oauth2_auth_url()
        .ok_or_else(|| anyhow::anyhow!("no auth URL for provider: {}", config.slug))?;

    let csrf_token: String = rand::thread_rng()
        .sample_iter(&rand::distributions::Alphanumeric)
        .take(32)
        .map(char::from)
        .collect();

    let scopes = config.scopes.join(" ");

    // Manual URL construction with minimal encoding for known-safe characters.
    let url = format!(
        "{}?client_id={}&redirect_uri={}&scope={}&state={}&response_type=code",
        base_url,
        url_encode(&config.client_id),
        url_encode(&config.redirect_url),
        url_encode(&scopes),
        url_encode(&csrf_token),
    );

    Ok((url, csrf_token))
}

/// Exchange authorization code for an access token.
pub async fn oauth2_exchange_code(
    config: &SsoProviderConfig,
    code: &str,
) -> Result<String> {
    let token_url = config
        .default_oauth2_token_url()
        .ok_or_else(|| anyhow::anyhow!("no token URL for provider: {}", config.slug))?;

    let client = reqwest::Client::new();

    #[derive(Deserialize)]
    struct TokenResponse {
        access_token: String,
    }

    // GitHub accepts JSON; GitLab also accepts JSON.
    let resp = client
        .post(&token_url)
        .form(&[
            ("client_id", config.client_id.as_str()),
            ("client_secret", config.client_secret.as_str()),
            ("code", code),
            ("redirect_uri", config.redirect_url.as_str()),
            ("grant_type", "authorization_code"),
        ])
        .header("Accept", "application/json")
        .send()
        .await
        .context("failed to exchange OAuth2 code")?;

    let token: TokenResponse = resp
        .json()
        .await
        .context("failed to parse token response")?;

    Ok(token.access_token)
}

/// Fetch user info from an OAuth2 provider.
pub async fn oauth2_fetch_user_info(
    config: &SsoProviderConfig,
    access_token: &str,
) -> Result<SsoUserInfo> {
    match config.slug.as_str() {
        "github" => fetch_github_user(access_token).await,
        "gitlab" => fetch_gitlab_user(access_token).await,
        _ => Err(anyhow::anyhow!(
            "unsupported SSO provider: {}",
            config.slug
        )),
    }
}

// ── GitHub user info ────────────────────────────────────────────────

async fn fetch_github_user(access_token: &str) -> Result<SsoUserInfo> {
    let client = reqwest::Client::new();

    let user_resp = client
        .get("https://api.github.com/user")
        .header("Authorization", format!("Bearer {}", access_token))
        .header("User-Agent", "IronForge/0.1")
        .header("Accept", "application/vnd.github.v3+json")
        .send()
        .await
        .context("GitHub user API request failed")?;

    let user: serde_json::Value = user_resp
        .json()
        .await
        .context("failed to parse GitHub user response")?;

    let provider_user_id = user["id"]
        .as_i64()
        .map(|id| id.to_string())
        .or_else(|| user["node_id"].as_str().map(|s| s.to_string()))
        .unwrap_or_default();

    let provider_username = user["login"].as_str().unwrap_or("").to_string();
    let display_name = user["name"].as_str().map(str::to_string);
    let avatar_url = user["avatar_url"].as_str().map(str::to_string);
    let email = fetch_github_email(&client, access_token)
        .await
        .unwrap_or_else(|| user["email"].as_str().unwrap_or("").to_string());

    Ok(SsoUserInfo {
        provider_user_id,
        provider_username,
        email,
        display_name,
        avatar_url,
    })
}

async fn fetch_github_email(client: &reqwest::Client, access_token: &str) -> Option<String> {
    let resp = client
        .get("https://api.github.com/user/emails")
        .header("Authorization", format!("Bearer {}", access_token))
        .header("User-Agent", "IronForge/0.1")
        .header("Accept", "application/vnd.github.v3+json")
        .send()
        .await
        .ok()?;

    let emails: Vec<serde_json::Value> = resp.json().await.ok()?;

    // Prefer primary + verified email.
    for email in &emails {
        let primary = email["primary"].as_bool().unwrap_or(false);
        let verified = email["verified"].as_bool().unwrap_or(false);
        if primary && verified {
            if let Some(e) = email["email"].as_str() {
                return Some(e.to_string());
            }
        }
    }

    // Fallback: any verified email.
    emails
        .iter()
        .find(|e| e["verified"].as_bool().unwrap_or(false))
        .and_then(|e| e["email"].as_str())
        .map(str::to_string)
}

// ── GitLab user info ────────────────────────────────────────────────

async fn fetch_gitlab_user(access_token: &str) -> Result<SsoUserInfo> {
    let client = reqwest::Client::new();
    let resp = client
        .get("https://gitlab.com/api/v4/user")
        .header("Authorization", format!("Bearer {}", access_token))
        .send()
        .await
        .context("GitLab user API request failed")?;

    let user: serde_json::Value = resp
        .json()
        .await
        .context("failed to parse GitLab user response")?;

    Ok(SsoUserInfo {
        provider_user_id: user["id"]
            .as_i64()
            .map(|id| id.to_string())
            .unwrap_or_default(),
        provider_username: user["username"].as_str().unwrap_or("").to_string(),
        email: user["email"].as_str().unwrap_or("").to_string(),
        display_name: user["name"].as_str().map(str::to_string),
        avatar_url: user["avatar_url"].as_str().map(str::to_string),
    })
}

// ── URL helpers ────────────────────────────────────────────────────

/// Minimal percent-encoding for OAuth2 query parameters.
fn url_encode(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            // unreserved characters per RFC 3986
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '.' | '_' | '~' => c.to_string(),
            ' ' => "%20".to_string(),
            // encode everything else
            other => {
                let bytes = other.to_string().into_bytes();
                bytes
                    .iter()
                    .map(|b| format!("%{:02X}", b))
                    .collect::<Vec<_>>()
                    .join("")
            }
        })
        .collect()
}

// ── Default endpoints ─────────────────────────────────────────────

impl SsoProviderConfig {
    fn default_oauth2_auth_url(&self) -> Option<String> {
        match self.slug.as_str() {
            "github" => Some("https://github.com/login/oauth/authorize".into()),
            "gitlab" => Some("https://gitlab.com/oauth/authorize".into()),
            _ => None,
        }
    }

    fn default_oauth2_token_url(&self) -> Option<String> {
        match self.slug.as_str() {
            "github" => Some("https://github.com/login/oauth/access_token".into()),
            "gitlab" => Some("https://gitlab.com/oauth/token".into()),
            _ => None,
        }
    }
}
