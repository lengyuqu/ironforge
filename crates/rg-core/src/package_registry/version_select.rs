//! Choosing a version among the published versions of a registry entry.
//!
//! Shared by the entries that agents pin by constraint (skills, MCP server
//! manifests), so that `latest`, pre-releases and yanked versions mean the same
//! thing for both.

use rg_db::entities::package_version;

/// Version-list entries returned in an error's `available` list.
pub(super) const MAX_AVAILABLE_VERSIONS: usize = 100;

/// Versions whose string parses as semver, with the parsed value.
pub(super) fn parsed(
    versions: &[package_version::Model],
) -> Vec<(semver::Version, &package_version::Model)> {
    versions
        .iter()
        .filter_map(|v| semver::Version::parse(&v.version).ok().map(|p| (p, v)))
        .collect()
}

/// The version `latest` means: the highest non-yanked stable version, or, when
/// there is none, the highest non-yanked pre-release.
pub(super) fn pick_latest(versions: &[package_version::Model]) -> Option<&package_version::Model> {
    let candidates: Vec<_> = parsed(versions)
        .into_iter()
        .filter(|(_, v)| !v.is_yanked)
        .collect();
    candidates
        .iter()
        .filter(|(p, _)| p.pre.is_empty())
        .max_by(|a, b| a.0.cmp(&b.0))
        .or_else(|| candidates.iter().max_by(|a, b| a.0.cmp(&b.0)))
        .map(|(_, v)| *v)
}

/// A constraint as the text that was applied and its parsed requirement.
///
/// The syntax is Cargo's: `^1.2`, `~1.2.3`, `>=1, <2`, `=1.2.3` (a bare `1.2.3`
/// means `^1.2.3`). Empty or `latest` means any version. On failure the error is
/// the parser's message.
pub(super) fn parse_constraint(
    constraint: Option<&str>,
) -> Result<(String, semver::VersionReq), (String, String)> {
    let text = constraint
        .map(str::trim)
        .filter(|c| !c.is_empty())
        .unwrap_or("*");
    let text = if text == "latest" { "*" } else { text };
    match semver::VersionReq::parse(text) {
        Ok(requirement) => Ok((text.to_string(), requirement)),
        Err(error) => Err((text.to_string(), error.to_string())),
    }
}

/// The highest non-yanked version that satisfies `requirement`. Pre-releases
/// match only a requirement that names a pre-release itself.
pub(super) fn highest_matching<'a>(
    versions: &'a [package_version::Model],
    requirement: &semver::VersionReq,
) -> Option<&'a package_version::Model> {
    parsed(versions)
        .into_iter()
        .filter(|(p, v)| !v.is_yanked && requirement.matches(p))
        .max_by(|a, b| a.0.cmp(&b.0))
        .map(|(_, v)| v)
}

/// The non-yanked versions, for telling a caller what it could have asked for.
pub(super) fn available(versions: &[package_version::Model]) -> Vec<String> {
    let mut out: Vec<String> = parsed(versions)
        .into_iter()
        .filter(|(_, v)| !v.is_yanked)
        .map(|(_, v)| v.version.clone())
        .collect();
    out.truncate(MAX_AVAILABLE_VERSIONS);
    out
}
