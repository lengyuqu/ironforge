//! Skill bundles: the artifact format of the `skill` package type.
//!
//! A bundle is a gzip-compressed tar whose root holds exactly one `SKILL.md`
//! (YAML frontmatter + Markdown body) plus any supporting files. This module
//! owns three things and nothing else:
//!
//! * [`validate_files`] / [`parse_skill_md`] — what makes a set of files a legal
//!   skill (frontmatter rules, path safety, size limits);
//! * [`pack`] — a **deterministic** packer: the same files always produce the
//!   same tar byte for byte, so [`Packed::content_digest`] (the SHA-256 of the
//!   *uncompressed* tar) is reproducible by anyone, on any gzip implementation;
//! * [`read_bundle`] — the strict reader used to serve a stored bundle and to
//!   validate one.
//!
//! The frontmatter fields follow the open Agent Skills format
//! (`name`, `description`, optional `license`, `compatibility`, `metadata`,
//! `allowed-tools`). **The numeric limits below are IronForge's own, chosen to
//! bound work; the name rules should be re-checked against the published
//! specification before they are relied on by third parties.**
//!
//! Nothing here executes, interprets or grants anything for a skill:
//! `allowed-tools` is recorded and returned as text, never enforced.

use std::collections::{BTreeMap, HashSet};
use std::io::Read;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

/// The one required file, at the bundle root.
pub const SKILL_MD: &str = "SKILL.md";
/// Most files a bundle may hold.
pub const MAX_FILES: usize = 200;
/// Most bytes all files may add up to.
pub const MAX_UNPACKED_BYTES: u64 = 20 * 1024 * 1024;
/// Largest compressed bundle accepted or produced.
pub const MAX_ARCHIVE_BYTES: usize = 5 * 1024 * 1024;
/// Largest `SKILL.md`.
pub const MAX_SKILL_MD_BYTES: usize = 256 * 1024;
/// Longest bundle path, in bytes.
pub const MAX_PATH_BYTES: usize = 255;
/// Longest skill name.
pub const MAX_NAME_CHARS: usize = 64;
/// Longest `description`.
pub const MAX_DESCRIPTION_CHARS: usize = 1024;
/// Largest frontmatter block. It is stored whole with the version, in a column
/// that holds 64 KiB on MySQL, so it must stay far below that.
pub const MAX_FRONTMATTER_BYTES: usize = 16 * 1024;
/// Longest `compatibility`.
pub const MAX_COMPATIBILITY_CHARS: usize = 500;
/// Upper bound on the decompressed tar stream: payload plus generous header and
/// padding overhead, so a gzip bomb of zeros cannot exhaust memory.
const TAR_STREAM_LIMIT: u64 = MAX_UNPACKED_BYTES + (MAX_FILES as u64) * 2048 + 1024 * 1024;

/// A rejected bundle, with a stable machine-readable `code`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BundleError {
    pub code: &'static str,
    pub message: String,
}

impl BundleError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for BundleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for BundleError {}

type Result<T> = std::result::Result<T, BundleError>;

/// One file of a bundle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BundleFile {
    /// Relative, `/`-separated.
    pub path: String,
    pub executable: bool,
    pub data: Vec<u8>,
}

/// A file's entry in the stored manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileEntry {
    pub path: String,
    pub size: u64,
    /// Lowercase hex SHA-256 of the file content.
    pub sha256: String,
    pub executable: bool,
}

/// Parsed and validated `SKILL.md` frontmatter.
#[derive(Debug, Clone, PartialEq)]
pub struct SkillFrontmatter {
    pub name: String,
    pub description: String,
    pub license: Option<String>,
    pub compatibility: Option<String>,
    /// Space-separated, as the format defines it. Recorded, never enforced.
    pub allowed_tools: Option<String>,
    pub metadata: BTreeMap<String, String>,
    /// The whole parsed frontmatter, unknown keys included.
    pub raw: Value,
}

/// Output of [`pack`].
#[derive(Debug, Clone)]
pub struct Packed {
    /// The gzip-compressed tar to store and serve.
    pub archive: Vec<u8>,
    /// `sha256:<hex>` of the **uncompressed** tar. Independent of the gzip
    /// implementation; this is what version immutability is judged on.
    pub content_digest: String,
    /// `sha256:<hex>` of `archive`, the exact bytes a client downloads.
    pub archive_digest: String,
    pub files: Vec<FileEntry>,
    pub unpacked_size: u64,
    pub frontmatter: SkillFrontmatter,
}

/// Output of [`read_bundle`].
#[derive(Debug, Clone)]
pub struct Bundle {
    pub files: Vec<BundleFile>,
    /// `sha256:<hex>` of the decompressed tar, comparable with
    /// [`Packed::content_digest`].
    pub content_digest: String,
    pub frontmatter: SkillFrontmatter,
}

fn sha256_hex(data: &[u8]) -> String {
    hex::encode(Sha256::digest(data))
}

// ── Frontmatter ──────────────────────────────────────────────────────────

/// Whether `name` is a legal skill name: 1–64 chars of `a-z0-9-`, not starting
/// or ending with `-`, no `--`.
pub fn is_valid_skill_name(name: &str) -> bool {
    !name.is_empty()
        && name.chars().count() <= MAX_NAME_CHARS
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && !name.starts_with('-')
        && !name.ends_with('-')
        && !name.contains("--")
}

/// Split `SKILL.md` into `(frontmatter yaml, body)`.
fn split_frontmatter(text: &str) -> Result<(&str, &str)> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut lines = text.split_inclusive('\n');
    let first = lines.next().unwrap_or("");
    if first.trim_end() != "---" {
        return Err(BundleError::new(
            "SKILL_INVALID_FRONTMATTER",
            "SKILL.md must start with a line containing only '---'",
        ));
    }
    let start = first.len();
    let mut offset = start;
    for line in lines {
        if line.trim_end() == "---" {
            return Ok((&text[start..offset], &text[offset + line.len()..]));
        }
        offset += line.len();
    }
    Err(BundleError::new(
        "SKILL_INVALID_FRONTMATTER",
        "SKILL.md frontmatter is not closed by a '---' line",
    ))
}

fn optional_string(map: &serde_json::Map<String, Value>, key: &str) -> Result<Option<String>> {
    match map.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.clone())),
        Some(_) => Err(BundleError::new(
            "SKILL_INVALID_FRONTMATTER",
            format!("frontmatter field '{key}' must be a string"),
        )),
    }
}

/// Parse and validate the frontmatter of a `SKILL.md`.
pub fn parse_skill_md(skill_md: &[u8]) -> Result<SkillFrontmatter> {
    if skill_md.len() > MAX_SKILL_MD_BYTES {
        return Err(BundleError::new(
            "SKILL_TOO_LARGE",
            format!("SKILL.md is larger than {MAX_SKILL_MD_BYTES} bytes"),
        ));
    }
    let text = std::str::from_utf8(skill_md)
        .map_err(|_| BundleError::new("SKILL_NOT_UTF8", "SKILL.md is not valid UTF-8"))?;
    let (yaml, _body) = split_frontmatter(text)?;
    if yaml.len() > MAX_FRONTMATTER_BYTES {
        return Err(BundleError::new(
            "SKILL_INVALID_FRONTMATTER",
            format!("the frontmatter is larger than {MAX_FRONTMATTER_BYTES} bytes"),
        ));
    }
    let raw: Value = crate::yaml::from_str(yaml).map_err(|error| {
        BundleError::new(
            "SKILL_INVALID_FRONTMATTER",
            format!("frontmatter is not valid YAML: {error}"),
        )
    })?;
    let Value::Object(map) = &raw else {
        return Err(BundleError::new(
            "SKILL_INVALID_FRONTMATTER",
            "frontmatter must be a YAML mapping",
        ));
    };

    let name = optional_string(map, "name")?.ok_or_else(|| {
        BundleError::new("SKILL_NAME_INVALID", "frontmatter field 'name' is required")
    })?;
    if !is_valid_skill_name(&name) {
        return Err(BundleError::new(
            "SKILL_NAME_INVALID",
            format!(
                "skill name '{name}' must be 1-{MAX_NAME_CHARS} characters of a-z, 0-9 and '-', \
                 not start or end with '-' and not contain '--'"
            ),
        ));
    }

    let description = optional_string(map, "description")?.unwrap_or_default();
    let description_chars = description.trim().chars().count();
    if description_chars == 0 || description.chars().count() > MAX_DESCRIPTION_CHARS {
        return Err(BundleError::new(
            "SKILL_DESCRIPTION_INVALID",
            format!("'description' is required and must be 1-{MAX_DESCRIPTION_CHARS} characters"),
        ));
    }

    let compatibility = optional_string(map, "compatibility")?;
    if compatibility
        .as_deref()
        .is_some_and(|c| c.chars().count() > MAX_COMPATIBILITY_CHARS)
    {
        return Err(BundleError::new(
            "SKILL_INVALID_FRONTMATTER",
            format!("'compatibility' is longer than {MAX_COMPATIBILITY_CHARS} characters"),
        ));
    }

    let allowed_tools = match map.get("allowed-tools") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => Some(s.clone()),
        // Some clients write a YAML list; normalise to the space-separated form.
        Some(Value::Array(items)) => {
            let mut parts = Vec::with_capacity(items.len());
            for item in items {
                match item {
                    Value::String(s) => parts.push(s.as_str()),
                    _ => {
                        return Err(BundleError::new(
                            "SKILL_INVALID_FRONTMATTER",
                            "'allowed-tools' list entries must be strings",
                        ))
                    }
                }
            }
            Some(parts.join(" "))
        }
        Some(_) => {
            return Err(BundleError::new(
                "SKILL_INVALID_FRONTMATTER",
                "'allowed-tools' must be a string or a list of strings",
            ))
        }
    };

    let mut metadata = BTreeMap::new();
    match map.get("metadata") {
        None | Some(Value::Null) => {}
        Some(Value::Object(entries)) => {
            for (key, value) in entries {
                match value {
                    Value::String(s) => {
                        metadata.insert(key.clone(), s.clone());
                    }
                    _ => {
                        return Err(BundleError::new(
                            "SKILL_INVALID_FRONTMATTER",
                            format!(
                                "metadata.{key} must be a string (quote numbers, e.g. \"1.0\")"
                            ),
                        ))
                    }
                }
            }
        }
        Some(_) => {
            return Err(BundleError::new(
                "SKILL_INVALID_FRONTMATTER",
                "'metadata' must be a mapping of strings to strings",
            ))
        }
    }

    Ok(SkillFrontmatter {
        name,
        description,
        license: optional_string(map, "license")?,
        compatibility,
        allowed_tools,
        metadata,
        raw,
    })
}

// ── Path and file-set validation ─────────────────────────────────────────

/// Reject any path that could escape, collide or surprise on extraction.
pub fn validate_entry_path(path: &str) -> Result<()> {
    let unsafe_path =
        |why: &str| BundleError::new("SKILL_UNSAFE_ENTRY", format!("unsafe path {path:?}: {why}"));
    if path.is_empty() {
        return Err(unsafe_path("empty"));
    }
    if path.len() > MAX_PATH_BYTES {
        return Err(unsafe_path("longer than the path limit"));
    }
    if path.starts_with('/') {
        return Err(unsafe_path("absolute"));
    }
    if path.contains('\\') {
        return Err(unsafe_path("contains a backslash"));
    }
    if path.chars().any(|c| c.is_control()) {
        return Err(unsafe_path("contains a control character"));
    }
    for segment in path.split('/') {
        match segment {
            "" => return Err(unsafe_path("empty segment")),
            "." | ".." => return Err(unsafe_path("'.' or '..' segment")),
            ".git" => return Err(unsafe_path("'.git' segment")),
            _ => {}
        }
    }
    Ok(())
}

/// Validate a whole file set as a skill and return its parsed frontmatter.
pub fn validate_files(files: &[BundleFile]) -> Result<SkillFrontmatter> {
    if files.len() > MAX_FILES {
        return Err(BundleError::new(
            "SKILL_TOO_MANY_FILES",
            format!(
                "a skill may hold at most {MAX_FILES} files, found {}",
                files.len()
            ),
        ));
    }
    let mut total: u64 = 0;
    let mut seen: HashSet<&str> = HashSet::new();
    let mut seen_folded: HashSet<String> = HashSet::new();
    for file in files {
        validate_entry_path(&file.path)?;
        total = total.saturating_add(file.data.len() as u64);
        if total > MAX_UNPACKED_BYTES {
            return Err(BundleError::new(
                "SKILL_TOO_LARGE",
                format!("a skill may hold at most {MAX_UNPACKED_BYTES} bytes of files"),
            ));
        }
        if !seen.insert(file.path.as_str()) {
            return Err(BundleError::new(
                "SKILL_UNSAFE_ENTRY",
                format!("duplicate path {:?}", file.path),
            ));
        }
        // Two paths that differ only by case collide on case-insensitive
        // filesystems (macOS, Windows), where extraction would overwrite one.
        if !seen_folded.insert(file.path.to_lowercase()) {
            return Err(BundleError::new(
                "SKILL_UNSAFE_ENTRY",
                format!("path {:?} differs from another only by case", file.path),
            ));
        }
    }
    // A path may not be both a file and a directory.
    for file in files {
        let mut end = 0;
        while let Some(i) = file.path[end..].find('/') {
            end += i;
            if seen.contains(&file.path[..end]) {
                return Err(BundleError::new(
                    "SKILL_UNSAFE_ENTRY",
                    format!(
                        "{:?} is used both as a file and as a directory",
                        &file.path[..end]
                    ),
                ));
            }
            end += 1;
        }
    }
    let skill_md = files.iter().find(|f| f.path == SKILL_MD).ok_or_else(|| {
        BundleError::new(
            "SKILL_MISSING_SKILL_MD",
            "the bundle root must contain SKILL.md",
        )
    })?;
    parse_skill_md(&skill_md.data)
}

// ── Packing ──────────────────────────────────────────────────────────────

fn build_tar(files: &[BundleFile]) -> Result<Vec<u8>> {
    let failed = |error: std::io::Error| {
        BundleError::new("SKILL_PACK_FAILED", format!("failed to write tar: {error}"))
    };
    let mut builder = tar::Builder::new(Vec::new());
    for file in files {
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Regular);
        header.set_size(file.data.len() as u64);
        header.set_mode(if file.executable { 0o755 } else { 0o644 });
        header.set_mtime(0);
        header.set_uid(0);
        header.set_gid(0);
        builder
            .append_data(&mut header, &file.path, file.data.as_slice())
            .map_err(failed)?;
    }
    builder.into_inner().map_err(failed)
}

fn gzip(tar: &[u8]) -> Result<Vec<u8>> {
    use std::io::Write;
    let failed = |error: std::io::Error| {
        BundleError::new("SKILL_PACK_FAILED", format!("failed to compress: {error}"))
    };
    let mut encoder = flate2::GzBuilder::new()
        .mtime(0)
        .operating_system(255)
        .write(Vec::new(), flate2::Compression::new(6));
    encoder.write_all(tar).map_err(failed)?;
    encoder.finish().map_err(failed)
}

/// The manifest entries for `files`, in the order given.
pub fn file_entries(files: &[BundleFile]) -> Vec<FileEntry> {
    files
        .iter()
        .map(|f| FileEntry {
            path: f.path.clone(),
            size: f.data.len() as u64,
            sha256: sha256_hex(&f.data),
            executable: f.executable,
        })
        .collect()
}

/// Pack `files` into a deterministic bundle after validating them.
///
/// Entries are sorted by path; every header has mtime 0, uid/gid 0, no owner
/// names and mode 0644 or 0755; only regular files are written.
pub fn pack(mut files: Vec<BundleFile>) -> Result<Packed> {
    let frontmatter = validate_files(&files)?;
    files.sort_by(|a, b| a.path.as_bytes().cmp(b.path.as_bytes()));

    let tar = build_tar(&files)?;
    let archive = gzip(&tar)?;
    if archive.len() > MAX_ARCHIVE_BYTES {
        return Err(BundleError::new(
            "SKILL_TOO_LARGE",
            format!("the compressed bundle is larger than {MAX_ARCHIVE_BYTES} bytes"),
        ));
    }
    let unpacked_size = files.iter().map(|f| f.data.len() as u64).sum();
    let entries = file_entries(&files);
    Ok(Packed {
        content_digest: format!("sha256:{}", sha256_hex(&tar)),
        archive_digest: format!("sha256:{}", sha256_hex(&archive)),
        archive,
        files: entries,
        unpacked_size,
        frontmatter,
    })
}

// ── Reading ──────────────────────────────────────────────────────────────

/// Decompress and strictly validate a stored or uploaded bundle.
pub fn read_bundle(archive: &[u8]) -> Result<Bundle> {
    if archive.len() > MAX_ARCHIVE_BYTES {
        return Err(BundleError::new(
            "SKILL_TOO_LARGE",
            format!("the compressed bundle is larger than {MAX_ARCHIVE_BYTES} bytes"),
        ));
    }
    let mut tar_bytes = Vec::new();
    flate2::read::GzDecoder::new(archive)
        .take(TAR_STREAM_LIMIT + 1)
        .read_to_end(&mut tar_bytes)
        .map_err(|error| {
            BundleError::new("SKILL_NOT_GZIP_TAR", format!("not a gzip stream: {error}"))
        })?;
    if tar_bytes.len() as u64 > TAR_STREAM_LIMIT {
        return Err(BundleError::new(
            "SKILL_TOO_LARGE",
            "the decompressed bundle exceeds the size limit",
        ));
    }
    let content_digest = format!("sha256:{}", sha256_hex(&tar_bytes));

    let bad_tar = |error: std::io::Error| {
        BundleError::new("SKILL_NOT_GZIP_TAR", format!("not a valid tar: {error}"))
    };
    let mut files: Vec<BundleFile> = Vec::new();
    let mut total: u64 = 0;
    let mut tar = tar::Archive::new(tar_bytes.as_slice());
    for entry in tar.entries().map_err(bad_tar)? {
        let mut entry = entry.map_err(bad_tar)?;
        let kind = entry.header().entry_type();
        let path = entry
            .path()
            .map_err(bad_tar)?
            .to_str()
            .map(str::to_owned)
            .ok_or_else(|| BundleError::new("SKILL_UNSAFE_ENTRY", "an entry path is not UTF-8"))?;
        if kind.is_dir() {
            validate_entry_path(path.trim_end_matches('/'))?;
            continue;
        }
        if !kind.is_file() {
            return Err(BundleError::new(
                "SKILL_UNSAFE_ENTRY",
                format!("entry {path:?} is not a regular file (links and devices are refused)"),
            ));
        }
        validate_entry_path(&path)?;
        if files.len() >= MAX_FILES {
            return Err(BundleError::new(
                "SKILL_TOO_MANY_FILES",
                format!("a skill may hold at most {MAX_FILES} files"),
            ));
        }
        let size = entry.size();
        total = total.saturating_add(size);
        if total > MAX_UNPACKED_BYTES {
            return Err(BundleError::new(
                "SKILL_TOO_LARGE",
                format!("a skill may hold at most {MAX_UNPACKED_BYTES} bytes of files"),
            ));
        }
        let mode = entry.header().mode().unwrap_or(0o644);
        let mut data = Vec::with_capacity(size as usize);
        entry.read_to_end(&mut data).map_err(bad_tar)?;
        files.push(BundleFile {
            path,
            executable: mode & 0o111 != 0,
            data,
        });
    }
    let frontmatter = validate_files(&files)?;
    Ok(Bundle {
        files,
        content_digest,
        frontmatter,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(path: &str, data: &str) -> BundleFile {
        BundleFile {
            path: path.into(),
            executable: false,
            data: data.as_bytes().to_vec(),
        }
    }

    fn skill_md(name: &str) -> String {
        format!("---\nname: {name}\ndescription: Does a thing.\n---\n# Body\n")
    }

    fn bundle(name: &str) -> Vec<BundleFile> {
        vec![
            file("SKILL.md", &skill_md(name)),
            file("scripts/run.sh", "#!/bin/sh\necho hi\n"),
            file("reference.md", "more"),
        ]
    }

    fn err_code(result: Result<impl std::fmt::Debug>) -> &'static str {
        result.unwrap_err().code
    }

    #[test]
    fn packing_is_deterministic_and_order_independent() {
        let a = pack(bundle("demo")).unwrap();
        let mut reversed = bundle("demo");
        reversed.reverse();
        let b = pack(reversed).unwrap();
        assert_eq!(a.archive, b.archive);
        assert_eq!(a.content_digest, b.content_digest);
        assert_eq!(a.archive_digest, b.archive_digest);
        let paths: Vec<_> = a.files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, ["SKILL.md", "reference.md", "scripts/run.sh"]);
    }

    #[test]
    fn one_changed_byte_changes_the_digest() {
        let a = pack(bundle("demo")).unwrap();
        let mut changed = bundle("demo");
        changed[2].data = b"more!".to_vec();
        let b = pack(changed).unwrap();
        assert_ne!(a.content_digest, b.content_digest);
    }

    #[test]
    fn executable_bit_is_part_of_the_content() {
        let a = pack(bundle("demo")).unwrap();
        let mut changed = bundle("demo");
        changed[1].executable = true;
        let b = pack(changed).unwrap();
        assert_ne!(a.content_digest, b.content_digest);
    }

    #[test]
    fn a_packed_bundle_reads_back_with_the_same_digest() {
        let packed = pack(bundle("demo")).unwrap();
        let read = read_bundle(&packed.archive).unwrap();
        assert_eq!(read.content_digest, packed.content_digest);
        assert_eq!(read.frontmatter.name, "demo");
        assert_eq!(read.files.len(), 3);
        assert!(read.files.iter().all(|f| !f.executable));
    }

    #[test]
    fn long_paths_survive_a_round_trip() {
        let long = format!("{}/{}.md", "d".repeat(120), "f".repeat(100));
        assert!(long.len() > 100 && long.len() <= MAX_PATH_BYTES);
        let mut files = bundle("demo");
        files.push(file(&long, "x"));
        let packed = pack(files).unwrap();
        let read = read_bundle(&packed.archive).unwrap();
        assert!(read.files.iter().any(|f| f.path == long));
    }

    #[test]
    fn name_rules() {
        for ok in ["a", "pdf-tools", "a1-b2", &"a".repeat(64)] {
            assert!(is_valid_skill_name(ok), "{ok}");
        }
        for bad in ["", "-a", "a-", "a--b", "PDF", "a_b", "a b", &"a".repeat(65)] {
            assert!(!is_valid_skill_name(bad), "{bad}");
        }
        let md = skill_md("Bad_Name");
        assert_eq!(
            err_code(parse_skill_md(md.as_bytes())),
            "SKILL_NAME_INVALID"
        );
    }

    #[test]
    fn frontmatter_requires_name_and_description() {
        assert_eq!(
            err_code(parse_skill_md(b"---\ndescription: x\n---\n")),
            "SKILL_NAME_INVALID"
        );
        assert_eq!(
            err_code(parse_skill_md(b"---\nname: a\n---\n")),
            "SKILL_DESCRIPTION_INVALID"
        );
        assert_eq!(
            err_code(parse_skill_md(b"---\nname: a\ndescription: '   '\n---\n")),
            "SKILL_DESCRIPTION_INVALID"
        );
        let long = format!("---\nname: a\ndescription: {}\n---\n", "x".repeat(1025));
        assert_eq!(
            err_code(parse_skill_md(long.as_bytes())),
            "SKILL_DESCRIPTION_INVALID"
        );
    }

    #[test]
    fn an_oversized_frontmatter_is_refused() {
        let padding: String = (0..400)
            .map(|i| format!("k{i}: {}\n", "v".repeat(60)))
            .collect();
        let md = format!("---\nname: a\ndescription: b\n{padding}---\n");
        assert!(
            md.len() < MAX_SKILL_MD_BYTES,
            "only the frontmatter rule can refuse this"
        );
        assert_eq!(
            err_code(parse_skill_md(md.as_bytes())),
            "SKILL_INVALID_FRONTMATTER"
        );
    }

    #[test]
    fn frontmatter_shape_errors() {
        assert_eq!(
            err_code(parse_skill_md(b"no frontmatter")),
            "SKILL_INVALID_FRONTMATTER"
        );
        assert_eq!(
            err_code(parse_skill_md(b"---\nname: a\ndescription: b\n")),
            "SKILL_INVALID_FRONTMATTER"
        );
        assert_eq!(
            err_code(parse_skill_md(b"---\n- a\n- b\n---\n")),
            "SKILL_INVALID_FRONTMATTER"
        );
        assert_eq!(
            err_code(parse_skill_md(
                b"---\nname: a\ndescription: b\nmetadata:\n  version: 1.0\n---\n"
            )),
            "SKILL_INVALID_FRONTMATTER"
        );
        assert_eq!(err_code(parse_skill_md(&[0xff, 0xfe])), "SKILL_NOT_UTF8");
    }

    #[test]
    fn optional_fields_are_recorded_and_allowed_tools_normalised() {
        let md = "---\nname: a\ndescription: b\nlicense: MIT\ncompatibility: any\n\
                  allowed-tools:\n  - Read\n  - Bash(git:*)\nmetadata:\n  version: \"1.2.3\"\nextra: kept\n---\nbody";
        let fm = parse_skill_md(md.as_bytes()).unwrap();
        assert_eq!(fm.license.as_deref(), Some("MIT"));
        assert_eq!(fm.allowed_tools.as_deref(), Some("Read Bash(git:*)"));
        assert_eq!(fm.metadata["version"], "1.2.3");
        assert_eq!(fm.raw["extra"], "kept");
    }

    #[test]
    fn crlf_and_bom_frontmatter_parse() {
        let md = "\u{feff}---\r\nname: a\r\ndescription: b\r\n---\r\nbody";
        assert_eq!(parse_skill_md(md.as_bytes()).unwrap().name, "a");
    }

    #[test]
    fn unsafe_paths_are_refused() {
        for bad in [
            "../x",
            "a/../b",
            "/abs",
            "a//b",
            "./a",
            "a/.",
            "a\\b",
            ".git/config",
            "a/.git/x",
            "ctl\u{7}x",
            "",
        ] {
            assert_eq!(
                validate_entry_path(bad).unwrap_err().code,
                "SKILL_UNSAFE_ENTRY",
                "{bad:?}"
            );
        }
        let long = "a".repeat(MAX_PATH_BYTES + 1);
        assert!(validate_entry_path(&long).is_err());
        assert!(validate_entry_path("a/b-c_d.e/f").is_ok());
    }

    #[test]
    fn file_set_rules() {
        let mut files = bundle("demo");
        files.push(file("reference.md", "dup"));
        assert_eq!(err_code(pack(files)), "SKILL_UNSAFE_ENTRY");

        let mut files = bundle("demo");
        files.push(file("Reference.md", "case"));
        assert_eq!(err_code(pack(files)), "SKILL_UNSAFE_ENTRY");

        let mut files = bundle("demo");
        files.push(file("scripts", "now a file and a directory"));
        assert_eq!(err_code(pack(files)), "SKILL_UNSAFE_ENTRY");

        let files = vec![file("nested/SKILL.md", &skill_md("demo"))];
        assert_eq!(err_code(pack(files)), "SKILL_MISSING_SKILL_MD");
    }

    #[test]
    fn size_limits_are_errors_not_truncation() {
        let mut many = bundle("demo");
        for i in 0..MAX_FILES {
            many.push(file(&format!("f{i}.txt"), "x"));
        }
        assert_eq!(err_code(pack(many)), "SKILL_TOO_MANY_FILES");

        let mut big = bundle("demo");
        big.push(BundleFile {
            path: "big.bin".into(),
            executable: false,
            data: vec![0u8; MAX_UNPACKED_BYTES as usize],
        });
        assert_eq!(err_code(pack(big)), "SKILL_TOO_LARGE");

        let huge_md = format!(
            "---\nname: a\ndescription: b\n---\n{}",
            "x".repeat(MAX_SKILL_MD_BYTES)
        );
        assert_eq!(
            err_code(parse_skill_md(huge_md.as_bytes())),
            "SKILL_TOO_LARGE"
        );
    }

    #[test]
    fn the_reader_refuses_links_and_garbage() {
        let mut builder = tar::Builder::new(Vec::new());
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Symlink);
        header.set_size(0);
        builder
            .append_link(&mut header, "SKILL.md", "/etc/passwd")
            .unwrap();
        let tar = builder.into_inner().unwrap();
        let archive = gzip(&tar).unwrap();
        assert_eq!(err_code(read_bundle(&archive)), "SKILL_UNSAFE_ENTRY");

        assert_eq!(err_code(read_bundle(b"not gzip")), "SKILL_NOT_GZIP_TAR");
    }

    #[test]
    fn a_gzip_bomb_is_stopped() {
        let zeros = vec![0u8; (TAR_STREAM_LIMIT + 4096) as usize];
        let archive = gzip(&zeros).unwrap();
        assert!(
            archive.len() < MAX_ARCHIVE_BYTES,
            "the bomb must be small to be a bomb"
        );
        assert_eq!(err_code(read_bundle(&archive)), "SKILL_TOO_LARGE");
    }
}
