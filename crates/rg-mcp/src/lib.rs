//!
//! `rg-mcp` - IronForge MCP Server
//!
//! MCP (Model Context Protocol) server that exposes IronForge
//! repository data as Tools and Resources to AI agents.
//!
//! Supported transports:
//! - **stdio**: run as a subprocess of an MCP-capable agent (`ironforge-mcp`).
//! - **streamable HTTP**: `ironforge-mcp --http`, for agents that are not on the
//!   machine of the server (see [`remote`]). The legacy HTTP+SSE transport is
//!   not implemented.

pub mod client;
pub mod collab;
pub mod collab_contract;
pub mod error;
pub mod protocol;
pub mod remote;
pub mod resources;
pub mod server;
pub mod tools;

// Re-export for convenience
pub use error::{Error, Result};

/// IronForge API base URL + PAT cache.
///
/// Constructed once at startup from environment variables.
#[derive(Clone)]
pub struct AppState {
    pub api_base: String,
    pub pat: String,
    /// Collaboration session credentials held by this process (never shown to the model).
    pub collab: std::sync::Arc<collab::SessionStore>,
}

impl AppState {
    pub fn from_env() -> Result<Self> {
        use std::env;

        let api_base =
            env::var("IRONFORGE_URL").unwrap_or_else(|_| "http://localhost:8080".to_string());
        let pat = env::var("IRONFORGE_PAT").unwrap_or_default();

        if pat.is_empty() {
            tracing::warn!("IRONFORGE_PAT not set – API calls may fail");
        }

        Ok(Self {
            api_base,
            pat,
            collab: std::sync::Arc::new(collab::SessionStore::from_env()),
        })
    }

    /// Build a `reqwest::Client` with Bearer token header.
    pub fn http_client(&self) -> reqwest::Client {
        let mut headers = reqwest::header::HeaderMap::new();
        if !self.pat.is_empty() {
            let value = reqwest::header::HeaderValue::from_str(&format!("Bearer {}", self.pat))
                .unwrap_or(reqwest::header::HeaderValue::from_static(""));
            headers.insert(reqwest::header::AUTHORIZATION, value);
        }
        // unwrap is acceptable here — build() only fails if native TLS is
        // entirely unavailable, which means the system is fundamentally broken
        //
        // The longest legitimate call is a `wait_*` long poll of 55 s, so two
        // minutes only ends a server that has stopped answering: without it a
        // hung upstream would hold a (remote-transport) call slot forever.
        reqwest::Client::builder()
            .default_headers(headers)
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .expect("reqwest::Client::build() failed: no native TLS backend available")
    }
}
