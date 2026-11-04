//! Repository collaboration rules: where an agent finds `AGENTS.md`-style
//! guidance for a path, and at which commit.
//!
//! The platform only *locates* files. It never merges, rewrites or ranks their
//! text, never follows symlinks, and marks everything it returns
//! `repository_content` — a maintainer-written, possibly hostile document, not
//! a system instruction. How rules nest and how they rank against the user's
//! own instructions is for the calling client to decide.
//!
//! Discovery (version 1):
//! * `.ironforge/agent-context.toml` at the resolved commit, when present and
//!   valid, lists the entries and the paths they apply to, in order;
//! * otherwise `AGENTS.md` at the root and in every directory on the way down to
//!   the target path, shallowest first (the deepest is the most specific).

use std::path::Path;

use serde::{Deserialize, Serialize};

use super::error::{CollabError, CollabResult};

pub const DISCOVERY_VERSION: u32 = 1;
pub const MANIFEST_PATH: &str = ".ironforge/agent-context.toml";
pub const DEFAULT_RULE_FILE: &str = "AGENTS.md";
pub const ORIGIN_REPOSITORY_CONTENT: &str = "repository_content";

/// Most entries one response lists; more is reported through `truncated`.
pub const MAX_ENTRIES: usize = 50;
/// Deepest directory nesting accepted for `path` and manifest paths.
pub const MAX_DEPTH: usize = 32;
const MAX_PATH_BYTES: usize = 1024;
const MAX_MANIFEST_BYTES: u64 = 64 * 1024;
/// Entries above this size are listed with `oversize: true`; the client decides
/// whether to read them.
pub const MAX_RULE_FILE_BYTES: u64 = 256 * 1024;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RuleEntry {
    /// Repository-relative path of the rule file.
    pub path: String,
    pub blob_sha: String,
    pub size: u64,
    pub oversize: bool,
    /// Directory the rules govern (`""` is the whole repository).
    pub applies_to: String,
    /// `manifest` or `default_discovery`.
    pub source: &'static str,
    pub origin: &'static str,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SkippedRule {
    pub path: String,
    /// `symlink`, `submodule` or `not_a_file`.
    pub reason: &'static str,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ManifestStatus {
    /// `absent`, `valid` or `invalid`.
    pub status: &'static str,
    /// Why the manifest was rejected (discovery then fell back to the default).
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RulesDiscovery {
    pub requested_ref: String,
    /// `None` only for a repository without commits.
    pub resolved_commit: Option<String>,
    pub path: String,
    pub discovery_version: u32,
    pub manifest: ManifestStatus,
    pub entries: Vec<RuleEntry>,
    pub skipped: Vec<SkippedRule>,
    pub truncated: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    #[serde(default)]
    version: Option<u32>,
    #[serde(default)]
    rule: Vec<ManifestRule>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestRule {
    path: String,
    #[serde(default)]
    applies_to: String,
}

/// Normalize a repository-relative path: no leading slash, no `.`/`..`/empty
/// segments, no backslashes or control characters, bounded depth and length.
/// `""` names the repository root. A trailing slash is ignored.
pub fn normalize_path(input: &str) -> CollabResult<String> {
    let trimmed = input.strip_suffix('/').unwrap_or(input);
    if trimmed.is_empty() {
        return Ok(String::new());
    }
    if trimmed.len() > MAX_PATH_BYTES {
        return Err(CollabError::bad_request("path is too long"));
    }
    if trimmed.starts_with('/') || trimmed.contains('\\') || trimmed.chars().any(char::is_control) {
        return Err(CollabError::bad_request(
            "path must be repository-relative with '/' separators and no control characters",
        ));
    }
    let segments: Vec<&str> = trimmed.split('/').collect();
    if segments.iter().any(|s| s.is_empty() || *s == "." || *s == "..") {
        return Err(CollabError::bad_request(
            "path must not contain empty, '.' or '..' segments",
        ));
    }
    if segments.len() > MAX_DEPTH {
        return Err(CollabError::bad_request("path is nested too deeply"));
    }
    Ok(segments.join("/"))
}

/// Directories from the root down to `path` itself: `a/b` → `["", "a", "a/b"]`.
pub fn ancestor_dirs(path: &str) -> Vec<String> {
    let mut dirs = vec![String::new()];
    let mut current = String::new();
    for segment in path.split('/').filter(|s| !s.is_empty()) {
        if !current.is_empty() {
            current.push('/');
        }
        current.push_str(segment);
        dirs.push(current.clone());
    }
    dirs
}

/// Whether rules for directory `applies_to` govern `target`.
pub fn applies(applies_to: &str, target: &str) -> bool {
    applies_to.is_empty()
        || target == applies_to
        || target
            .strip_prefix(applies_to)
            .is_some_and(|rest| rest.starts_with('/'))
}

fn join(dir: &str, file: &str) -> String {
    if dir.is_empty() {
        file.to_string()
    } else {
        format!("{dir}/{file}")
    }
}

/// Parse and validate a manifest into `(rule file path, applies_to)` pairs.
fn parse_manifest(text: &str) -> Result<Vec<(String, String)>, String> {
    let manifest: Manifest = toml::from_str(text).map_err(|e| e.message().to_string())?;
    if manifest.version.is_some_and(|v| v != DISCOVERY_VERSION) {
        return Err(format!("unsupported manifest version (expected {DISCOVERY_VERSION})"));
    }
    if manifest.rule.len() > MAX_ENTRIES {
        return Err(format!("at most {MAX_ENTRIES} rules are allowed"));
    }
    let mut rules = Vec::with_capacity(manifest.rule.len());
    for rule in manifest.rule {
        let path = normalize_path(&rule.path).map_err(|e| e.to_string())?;
        if path.is_empty() {
            return Err("rule path must name a file".to_string());
        }
        let applies_to = normalize_path(&rule.applies_to).map_err(|e| e.to_string())?;
        rules.push((path, applies_to));
    }
    Ok(rules)
}

enum Lookup {
    Missing,
    File { blob_sha: String, size: u64 },
    Skipped(&'static str),
}

fn lookup(
    repo: &gix::Repository,
    tree: &gix::Tree<'_>,
    path: &str,
) -> CollabResult<Lookup> {
    let entry = tree
        .lookup_entry_by_path(Path::new(path))
        .map_err(|e| CollabError::Internal(anyhow::anyhow!("tree lookup failed: {e}")))?;
    let Some(entry) = entry else {
        return Ok(Lookup::Missing);
    };
    let mode = entry.mode();
    if mode.is_link() {
        return Ok(Lookup::Skipped("symlink"));
    }
    if mode.is_commit() {
        return Ok(Lookup::Skipped("submodule"));
    }
    if !mode.is_blob() {
        return Ok(Lookup::Skipped("not_a_file"));
    }
    let id = entry.object_id();
    let size = repo
        .find_header(id)
        .map_err(|e| CollabError::Internal(anyhow::anyhow!("object header lookup failed: {e}")))?
        .size();
    Ok(Lookup::File {
        blob_sha: id.to_string(),
        size,
    })
}

/// Locate the rule files governing `path` at `git_ref` (default `HEAD`).
///
/// Blocking: call from `spawn_blocking`. An explicit `git_ref` that does not
/// resolve is `NotFound`; an unborn default `HEAD` yields an empty result.
pub fn discover(
    repo_path: &Path,
    git_ref: Option<&str>,
    path: &str,
) -> CollabResult<RulesDiscovery> {
    let path = normalize_path(path)?;
    let requested = git_ref.unwrap_or("HEAD");
    let repo = gix::open(repo_path)
        .map_err(|e| CollabError::Internal(anyhow::anyhow!("open repository: {e}")))?;

    let mut out = RulesDiscovery {
        requested_ref: requested.to_string(),
        resolved_commit: None,
        path: path.clone(),
        discovery_version: DISCOVERY_VERSION,
        manifest: ManifestStatus { status: "absent", error: None },
        entries: Vec::new(),
        skipped: Vec::new(),
        truncated: false,
    };

    let id = match repo.rev_parse_single(requested) {
        Ok(id) => id,
        Err(_) if git_ref.is_none() => return Ok(out),
        Err(_) => return Err(CollabError::not_found(format!("ref '{requested}' not found"))),
    };
    let commit = repo
        .find_object(id)
        .map_err(|e| CollabError::Internal(anyhow::anyhow!("find object: {e}")))?
        .peel_to_commit()
        .map_err(|_| CollabError::not_found(format!("ref '{requested}' is not a commit")))?;
    out.resolved_commit = Some(commit.id.to_string());
    let tree = commit
        .tree()
        .map_err(|e| CollabError::Internal(anyhow::anyhow!("read tree: {e}")))?;

    // Manifest first: when it is usable it replaces the default discovery.
    let mut manifest_rules: Option<Vec<(String, String)>> = None;
    match lookup(&repo, &tree, MANIFEST_PATH)? {
        Lookup::Missing => {}
        Lookup::Skipped(reason) => {
            out.manifest = ManifestStatus {
                status: "invalid",
                error: Some(format!("{MANIFEST_PATH} is a {reason}, not a regular file")),
            };
        }
        Lookup::File { blob_sha, size } => {
            let verdict = if size > MAX_MANIFEST_BYTES {
                Err(format!("manifest exceeds {MAX_MANIFEST_BYTES} bytes"))
            } else {
                let oid = gix::ObjectId::from_hex(blob_sha.as_bytes())
                    .map_err(|e| CollabError::Internal(anyhow::anyhow!("blob id: {e}")))?;
                let blob = repo
                    .find_object(oid)
                    .map_err(|e| CollabError::Internal(anyhow::anyhow!("read manifest: {e}")))?;
                match std::str::from_utf8(&blob.data) {
                    Ok(text) => parse_manifest(text),
                    Err(_) => Err("manifest is not valid UTF-8".to_string()),
                }
            };
            match verdict {
                Ok(rules) => {
                    out.manifest = ManifestStatus { status: "valid", error: None };
                    manifest_rules = Some(rules);
                }
                Err(error) => {
                    out.manifest = ManifestStatus {
                        status: "invalid",
                        error: Some(error.chars().take(200).collect()),
                    };
                }
            }
        }
    }

    let (candidates, source): (Vec<(String, String)>, &'static str) = match manifest_rules {
        Some(rules) => (
            rules.into_iter().filter(|(_, to)| applies(to, &path)).collect(),
            "manifest",
        ),
        None => (
            ancestor_dirs(&path)
                .into_iter()
                .map(|dir| (join(&dir, DEFAULT_RULE_FILE), dir))
                .collect(),
            "default_discovery",
        ),
    };

    let mut seen = std::collections::HashSet::new();
    for (file, applies_to) in candidates {
        if !seen.insert(file.clone()) {
            continue;
        }
        match lookup(&repo, &tree, &file)? {
            // A rule file that is not there is simply not a rule.
            Lookup::Missing => {}
            Lookup::Skipped(reason) => out.skipped.push(SkippedRule { path: file, reason }),
            Lookup::File { blob_sha, size } => {
                if out.entries.len() == MAX_ENTRIES {
                    out.truncated = true;
                    break;
                }
                out.entries.push(RuleEntry {
                    path: file,
                    blob_sha,
                    size,
                    oversize: size > MAX_RULE_FILE_BYTES,
                    applies_to,
                    source,
                    origin: ORIGIN_REPOSITORY_CONTENT,
                });
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_are_normalized_or_rejected() {
        assert_eq!(normalize_path("").unwrap(), "");
        assert_eq!(normalize_path("a/b/").unwrap(), "a/b");
        for bad in ["/abs", "a//b", "a/./b", "a/../b", "a\\b", "a/\0", "a/\n"] {
            assert!(normalize_path(bad).is_err(), "{bad:?} must be rejected");
        }
        assert!(normalize_path(&"d/".repeat(MAX_DEPTH + 1)).is_err());
        assert!(normalize_path(&vec!["d"; MAX_DEPTH].join("/")).is_ok());
        assert!(normalize_path(&"x".repeat(MAX_PATH_BYTES + 1)).is_err());
    }

    #[test]
    fn ancestors_run_from_the_root_down() {
        assert_eq!(ancestor_dirs(""), vec![""]);
        assert_eq!(ancestor_dirs("a/b"), vec!["", "a", "a/b"]);
    }

    #[test]
    fn scope_matches_on_directory_boundaries() {
        assert!(applies("", "anything/at/all"));
        assert!(applies("src", "src"));
        assert!(applies("src", "src/lib/x.rs"));
        assert!(!applies("src", "srcs/x.rs"), "'srcs' is not under 'src'");
        assert!(!applies("src/lib", "src"));
    }

    #[test]
    fn manifest_is_strict() {
        let ok = parse_manifest(
            "version = 1\n[[rule]]\npath = \"AGENTS.md\"\n[[rule]]\npath = \"web/RULES.md\"\napplies_to = \"web\"\n",
        )
        .unwrap();
        assert_eq!(
            ok,
            vec![
                ("AGENTS.md".to_string(), String::new()),
                ("web/RULES.md".to_string(), "web".to_string())
            ]
        );
        assert!(parse_manifest("version = 2").is_err());
        assert!(parse_manifest("[[rule]]\npath = \"../x\"").is_err());
        assert!(parse_manifest("[[rule]]\npath = \"\"").is_err());
        assert!(parse_manifest("[[rule]]\npath = \"a\"\nbogus = 1").is_err());
        assert!(parse_manifest("not toml {{").is_err());
    }
}
