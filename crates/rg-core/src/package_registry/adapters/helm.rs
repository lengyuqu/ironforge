//! Helm (Kubernetes) chart adapter.
//!
//! Handles `.tgz` / `.tar.gz` archives containing:
//! - `Chart.yaml` — chart metadata (required)
//! - `values.yaml` — default values
//! - `templates/` — Kubernetes resource templates
//!
//! ## Helm Repository API
//!
//! Helm clients expect:
//! - `GET /index.yaml` — repository index listing all charts
//! - Chart files served at paths relative to the index
//!
//! IronForge serves these at:
//! - Index: `GET /api/v1/repos/{owner}/{repo}/packages/helm/index.yaml`
//! - Download: standard package download endpoint

use flate2::read::GzDecoder;
use std::io::Read;

use crate::package_registry::adapter::{ExtractedMetadata, PackageAdapter};

pub struct HelmAdapter;

impl PackageAdapter for HelmAdapter {
    fn package_type() -> &'static str {
        "helm"
    }

    fn extract_metadata(
        &self,
        _filename: &str,
        data: &[u8],
    ) -> Result<ExtractedMetadata, anyhow::Error> {
        extract_from_chart(data)
    }

    fn validate(&self, data: &[u8]) -> Result<(), anyhow::Error> {
        if data.len() < 2 || data[0] != 0x1f || data[1] != 0x8b {
            anyhow::bail!("invalid Helm chart: not a gzip file");
        }

        // Check for Chart.yaml inside
        let mut decoder = GzDecoder::new(data);
        let mut tar_data = Vec::new();
        decoder
            .read_to_end(&mut tar_data)
            .map_err(|e| anyhow::anyhow!("invalid Helm chart (gzip decode failed): {e}"))?;

        let mut archive = tar::Archive::new(&tar_data[..]);
        let mut found = false;
        for entry in archive.entries()? {
            let entry = entry?;
            let path = entry.path()?;
            if path.file_name().map(|n| n == "Chart.yaml").unwrap_or(false) {
                found = true;
                break;
            }
        }

        if !found {
            anyhow::bail!("invalid Helm chart: no Chart.yaml found");
        }
        Ok(())
    }

    fn content_type_for_file(&self, filename: &str) -> String {
        if filename.ends_with(".tgz") || filename.ends_with(".tar.gz") {
            "application/gzip".into()
        } else {
            self.default_content_type().into()
        }
    }

    fn default_content_type(&self) -> &'static str {
        "application/gzip"
    }

    fn has_protocol_endpoint(&self) -> bool {
        true
    }
}

/// Extract metadata from a Helm chart (.tgz containing Chart.yaml).
fn extract_from_chart(data: &[u8]) -> Result<ExtractedMetadata, anyhow::Error> {
    let decoder = GzDecoder::new(data);
    let mut archive = tar::Archive::new(decoder);

    let mut chart_yaml = None;

    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?.to_path_buf();

        // Chart.yaml is in the chart root directory: {chartname}/Chart.yaml
        if path.file_name().map(|n| n == "Chart.yaml").unwrap_or(false) {
            let mut content = String::new();
            entry.read_to_string(&mut content)?;
            chart_yaml = Some(content);
            break;
        }
    }

    let yaml_str =
        chart_yaml.ok_or_else(|| anyhow::anyhow!("invalid Helm chart: no Chart.yaml found"))?;

    parse_chart_yaml(&yaml_str)
}

/// Parse Chart.yaml content.
fn parse_chart_yaml(yaml: &str) -> Result<ExtractedMetadata, anyhow::Error> {
    let doc: crate::yaml::Value =
        crate::yaml::from_str(yaml).map_err(|e| anyhow::anyhow!("invalid Chart.yaml: {e}"))?;

    let name = doc
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Chart.yaml missing 'name'"))?
        .to_string();

    let version = doc
        .get("version")
        .and_then(|v| {
            // version can be string or number
            v.as_str()
                .map(str::to_string)
                .or_else(|| v.as_f64().map(|n| n.to_string()))
        })
        .ok_or_else(|| anyhow::anyhow!("Chart.yaml missing 'version'"))?;

    let description = doc
        .get("description")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(String::from);

    let homepage = doc
        .get("home")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(String::from);

    let keywords = doc
        .get("keywords")
        .and_then(|v| {
            v.as_array().map(|seq| {
                seq.iter()
                    .filter_map(|k| k.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            })
        })
        .filter(|s| !s.is_empty());

    // Helm charts can have a "sources" list for repository URLs
    let repository_url = doc
        .get("sources")
        .and_then(|v| v.as_array())
        .and_then(|seq| seq.first())
        .and_then(|s| s.as_str())
        .map(String::from);

    Ok(ExtractedMetadata {
        name,
        version: version.clone(),
        description,
        homepage,
        repository_url,
        keywords,
        license: None, // Helm Chart.yaml doesn't standardize license
        semver: Some(version),
    })
}

// ── Helm repository index helpers ─────────────────────────

/// Entry for one chart version in index.yaml.
pub struct HelmIndexEntry {
    pub name: String,
    pub version: String,
    pub app_version: Option<String>,
    pub description: Option<String>,
    pub api_version: Option<String>,
    pub home: Option<String>,
    pub sources: Vec<String>,
    pub keywords: Vec<String>,
    pub created: String,
    pub digest: Option<String>,
    pub urls: Vec<String>,
}

/// One version of a chart in `index.yaml`. Field order is the order Helm's own
/// tooling writes, and unset optional fields are left out.
#[derive(serde::Serialize)]
struct IndexVersion<'a> {
    name: &'a str,
    version: &'a str,
    #[serde(rename = "appVersion", skip_serializing_if = "Option::is_none")]
    app_version: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<&'a str>,
    #[serde(rename = "apiVersion", skip_serializing_if = "Option::is_none")]
    api_version: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    home: Option<&'a str>,
    #[serde(skip_serializing_if = "slice_is_empty")]
    sources: &'a [String],
    #[serde(skip_serializing_if = "slice_is_empty")]
    keywords: &'a [String],
    created: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    digest: Option<&'a str>,
    urls: &'a [String],
}

fn slice_is_empty(items: &&[String]) -> bool {
    items.is_empty()
}

/// Chart name to its versions, written as a mapping in the order the charts
/// first appear (a typed map would sort them).
struct ChartGroups<'a>(Vec<(&'a str, Vec<IndexVersion<'a>>)>);

impl serde::Serialize for ChartGroups<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_map(self.0.iter().map(|(name, versions)| (name, versions)))
    }
}

#[derive(serde::Serialize)]
struct HelmIndex<'a> {
    #[serde(rename = "apiVersion")]
    api_version: &'static str,
    generated: String,
    entries: ChartGroups<'a>,
}

/// Build a Helm repository index.yaml.
///
/// Format: https://helm.sh/docs/topics/chart_repository/#the-chart-repository-structure
pub fn build_helm_index(entries: &[HelmIndexEntry]) -> String {
    let mut groups: Vec<(&str, Vec<IndexVersion>)> = Vec::new();

    // Group entries by chart name, keeping first-seen order.
    for entry in entries {
        let version = IndexVersion {
            name: &entry.name,
            version: &entry.version,
            app_version: entry.app_version.as_deref(),
            description: entry.description.as_deref(),
            api_version: entry.api_version.as_deref(),
            home: entry.home.as_deref(),
            sources: &entry.sources,
            keywords: &entry.keywords,
            created: &entry.created,
            digest: entry.digest.as_deref(),
            urls: &entry.urls,
        };
        match groups.iter_mut().find(|(name, _)| *name == entry.name) {
            Some((_, versions)) => versions.push(version),
            None => groups.push((&entry.name, vec![version])),
        }
    }

    let index = HelmIndex {
        api_version: "v1",
        generated: chrono::Utc::now().to_rfc3339(),
        entries: ChartGroups(groups),
    };
    crate::yaml::to_string(&index).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::write::GzEncoder;
    use flate2::Compression;
    use std::io::Write;

    /// Create a minimal .tgz Helm chart for testing.
    fn make_chart(chart_yaml: &str) -> Vec<u8> {
        // Create a tar archive with Chart.yaml
        let mut tar_buf = Vec::new();
        {
            let mut tar = tar::Builder::new(&mut tar_buf);
            let mut header = tar::Header::new_gnu();
            header.set_path("mychart/Chart.yaml").unwrap();
            header.set_size(chart_yaml.len() as u64);
            header.set_mode(0o644);
            header.set_entry_type(tar::EntryType::Regular);
            header.set_cksum();
            tar.append(&header, chart_yaml.as_bytes()).unwrap();
            tar.finish().unwrap();
        }

        // Gzip the tar
        let mut gz_buf = Vec::new();
        {
            let mut encoder = GzEncoder::new(&mut gz_buf, Compression::default());
            encoder.write_all(&tar_buf).unwrap();
            encoder.finish().unwrap();
        }

        gz_buf
    }

    #[test]
    fn test_extract_chart_basic() {
        let yaml = r#"apiVersion: v2
name: nginx
version: 1.2.3
description: A Helm chart for nginx
home: https://github.com/kubernetes/ingress-nginx
keywords:
  - nginx
  - ingress
  - web
sources:
  - https://github.com/kubernetes/ingress-nginx
"#;

        let data = make_chart(yaml);
        let adapter = HelmAdapter;

        let meta = adapter.extract_metadata("nginx-1.2.3.tgz", &data).unwrap();
        assert_eq!(meta.name, "nginx");
        assert_eq!(meta.version, "1.2.3");
        assert_eq!(meta.description.unwrap(), "A Helm chart for nginx");
        assert_eq!(
            meta.homepage.unwrap(),
            "https://github.com/kubernetes/ingress-nginx"
        );
        assert_eq!(meta.keywords.unwrap(), "nginx, ingress, web");
        assert!(meta.repository_url.is_some());
    }

    #[test]
    fn test_validate_valid_chart() {
        let yaml = "name: test\nversion: 1.0.0\n";
        let data = make_chart(yaml);
        let adapter = HelmAdapter;
        assert!(adapter.validate(&data).is_ok());
    }

    #[test]
    fn test_validate_rejects_non_gzip() {
        let adapter = HelmAdapter;
        let err = adapter.validate(b"not a gzip file").unwrap_err();
        assert!(err.to_string().contains("not a gzip"));
    }

    #[test]
    fn test_validate_rejects_no_chart_yaml() {
        // Create a .tgz without Chart.yaml
        let mut tar_buf = Vec::new();
        {
            let mut tar = tar::Builder::new(&mut tar_buf);
            let mut header = tar::Header::new_gnu();
            header.set_path("values.yaml").unwrap();
            header.set_size(3);
            header.set_entry_type(tar::EntryType::Regular);
            header.set_cksum();
            tar.append(&header, &b"{}"[..]).unwrap();
            tar.finish().unwrap();
        }
        let mut gz_buf = Vec::new();
        {
            let mut encoder = GzEncoder::new(&mut gz_buf, Compression::default());
            encoder.write_all(&tar_buf).unwrap();
            encoder.finish().unwrap();
        }

        let adapter = HelmAdapter;
        let err = adapter.validate(&gz_buf).unwrap_err();
        assert!(err.to_string().contains("no Chart.yaml"));
    }

    #[test]
    fn test_extract_chart_version_number() {
        // version as a number — YAML parses 1.0 as float, Display strips trailing zero
        let yaml = "name: test\nversion: 1.0\n";
        let data = make_chart(yaml);
        let adapter = HelmAdapter;
        let meta = adapter.extract_metadata("test-1.0.tgz", &data).unwrap();
        assert_eq!(meta.version, "1");
    }

    #[test]
    fn test_build_helm_index() {
        let entries = vec![HelmIndexEntry {
            name: "nginx".into(),
            version: "1.2.3".into(),
            app_version: Some("1.19.0".into()),
            description: Some("A Helm chart".into()),
            api_version: Some("v2".into()),
            home: Some("https://example.com".into()),
            sources: vec!["https://github.com/x/y".into()],
            keywords: vec!["web".into(), "proxy".into()],
            created: "2024-01-01T00:00:00Z".into(),
            digest: Some("sha256:abc123".into()),
            urls: vec!["https://example.com/charts/nginx-1.2.3.tgz".into()],
        }];

        let yaml = build_helm_index(&entries);
        assert!(yaml.contains("apiVersion: v1"));
        assert!(yaml.contains("nginx"));
        assert!(yaml.contains("1.2.3"));
        assert!(yaml.contains("sha256:abc123"));
        assert!(yaml.contains("generated"));
    }

    #[test]
    fn helm_index_keeps_helm_key_order_and_reads_back() {
        let entry = |name: &str, version: &str| HelmIndexEntry {
            name: name.into(),
            version: version.into(),
            app_version: Some("1.19.0".into()),
            description: Some("A chart: with \"quotes\" and # hash".into()),
            api_version: Some("v2".into()),
            home: None,
            sources: vec![],
            keywords: vec!["web".into()],
            created: "2024-01-01T00:00:00Z".into(),
            digest: Some("sha256:abc123".into()),
            urls: vec!["https://example.com/charts/x.tgz".into()],
        };
        // Two versions of one chart and a second chart: grouped by name, in the
        // order the charts first appear.
        let entries = vec![entry("nginx", "1.0.0"), entry("redis", "7.0.0"), entry("nginx", "1.1.0")];
        let yaml = build_helm_index(&entries);

        // Top-level keys and each entry's keys come out in a fixed, familiar order.
        let at = |needle: &str| yaml.find(needle).unwrap_or_else(|| panic!("{needle} missing in:\n{yaml}"));
        assert!(at("apiVersion: v1") < at("generated:") && at("generated:") < at("entries:"));
        assert!(at("nginx:") < at("redis:"));
        // Values may or may not be quoted; only the key order is checked here, and
        // the values themselves below, after reading the document back.
        assert!(at("name: nginx") < at("  version:"));
        assert!(at("  version:") < at("appVersion:"));
        assert!(at("appVersion:") < at("description:"));
        assert!(at("description:") < at("apiVersion: v2"));
        assert!(at("created:") < at("digest:") && at("digest:") < at("urls:"));
        // Optional fields that are unset are left out entirely.
        assert!(!yaml.contains("home:") && !yaml.contains("sources:"));
        assert!(!yaml.starts_with('%'), "directive in index.yaml: {yaml}");

        // And the whole document reads back with the values intact.
        let doc: crate::yaml::Value = crate::yaml::from_str(&yaml).unwrap();
        assert_eq!(doc["apiVersion"], "v1");
        let nginx = doc["entries"]["nginx"].as_array().unwrap();
        assert_eq!(nginx.len(), 2);
        assert_eq!(nginx[0]["version"], "1.0.0");
        assert_eq!(nginx[1]["version"], "1.1.0");
        assert_eq!(nginx[0]["description"], "A chart: with \"quotes\" and # hash");
        assert_eq!(nginx[0]["keywords"][0], "web");
        assert_eq!(doc["entries"]["redis"][0]["version"], "7.0.0");
    }
}
