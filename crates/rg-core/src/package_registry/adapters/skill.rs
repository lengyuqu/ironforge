//! `skill` package adapter: a gzip tar bundle with a root `SKILL.md`.
//!
//! The format rules live in [`crate::package_registry::skill_bundle`]; this
//! adapter only exposes them through the registry's [`PackageAdapter`] trait.
//! Skills are published through the dedicated `…/skills/publish` route (which
//! builds the bundle from a Git ref and records its source); the generic
//! `…/packages/skill/publish` route refuses the type.

use crate::package_registry::adapter::{ExtractedMetadata, PackageAdapter};
use crate::package_registry::skill_bundle::read_bundle;

pub struct SkillAdapter;

impl PackageAdapter for SkillAdapter {
    fn package_type() -> &'static str {
        "skill"
    }

    fn extract_metadata(
        &self,
        _filename: &str,
        data: &[u8],
    ) -> Result<ExtractedMetadata, anyhow::Error> {
        let bundle = read_bundle(data)?;
        let fm = bundle.frontmatter;
        let version = fm.metadata.get("version").cloned().unwrap_or_default();
        Ok(ExtractedMetadata {
            name: fm.name,
            semver: (!version.is_empty()).then(|| version.clone()),
            version,
            description: Some(fm.description),
            homepage: None,
            repository_url: None,
            keywords: None,
            license: fm.license,
        })
    }

    fn validate(&self, data: &[u8]) -> Result<(), anyhow::Error> {
        read_bundle(data)?;
        Ok(())
    }

    fn content_type_for_file(&self, _filename: &str) -> String {
        "application/gzip".into()
    }

    fn default_content_type(&self) -> &'static str {
        "application/gzip"
    }

    fn supports_yank(&self) -> bool {
        true
    }
}
