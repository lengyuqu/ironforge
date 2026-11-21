//! `mcp-server` package adapter: a single `server.json` manifest.
//!
//! The format rules live in [`crate::package_registry::mcp_manifest`]. MCP
//! server manifests are published through the dedicated `…/mcp-servers` route;
//! the generic `…/packages/mcp-server/publish` route refuses the type.

use crate::package_registry::adapter::{ExtractedMetadata, PackageAdapter};
use crate::package_registry::mcp_manifest::analyze;

pub struct McpServerAdapter;

fn read(data: &[u8]) -> Result<crate::package_registry::mcp_manifest::Analysis, anyhow::Error> {
    let value: serde_json::Value = serde_json::from_slice(data)?;
    analyze(&value).map_err(|error| anyhow::anyhow!("{}: {}", error.code, error.message))
}

impl PackageAdapter for McpServerAdapter {
    fn package_type() -> &'static str {
        "mcp-server"
    }

    fn extract_metadata(
        &self,
        _filename: &str,
        data: &[u8],
    ) -> Result<ExtractedMetadata, anyhow::Error> {
        let analysis = read(data)?;
        Ok(ExtractedMetadata {
            name: analysis.name,
            semver: Some(analysis.version.clone()),
            version: analysis.version,
            description: Some(analysis.description),
            homepage: None,
            repository_url: None,
            keywords: None,
            license: None,
        })
    }

    fn validate(&self, data: &[u8]) -> Result<(), anyhow::Error> {
        read(data)?;
        Ok(())
    }

    fn content_type_for_file(&self, _filename: &str) -> String {
        "application/json".into()
    }

    fn default_content_type(&self) -> &'static str {
        "application/json"
    }

    fn supports_yank(&self) -> bool {
        true
    }
}
