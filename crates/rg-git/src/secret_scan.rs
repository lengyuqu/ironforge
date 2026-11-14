//! Push-time secret scanning (R7, issue #38).
//!
//! Only the blobs a push (or a REST commit) introduces are scanned — the
//! scanner never walks history, so the cost is proportional to the size of the
//! change, not to the size of the repository.
//!
//! A hit is graded by rule:
//!
//! - **block** — private-key headers, cloud access keys and platform tokens:
//!   the ref update is refused before any ref is written.
//! - **warn** — JWTs and generic high-entropy strings: an audit event is
//!   produced and the push goes through. A false positive must never stop a
//!   normal push.
//!
//! `rg-git` has no database, so the scanner only returns [`Report`]s and emits
//! [`AuditEvent`]s. Persisting them is the caller's job: rg-core writes audit
//! rows for the content API, and a push-side consumer can install a sink with
//! [`set_audit_sink`] (or drain the structured `tracing` records).
//!
//! Limits are deliberately fail-open: a file larger than [`MAX_FILE_BYTES`] is
//! not scanned, and a push touching more than [`MAX_FILES`] files skips the
//! scan as a whole. Both cases are reported as [`Skip`] and must produce a
//! `secret_scan_skipped` event — a skip is never silent.
//!
//! Nothing in this module ever puts the matched text into a report, a message
//! or a log: findings carry `path` + `line` + rule name only.

use std::collections::HashSet;
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};

use anyhow::Context;
use regex::Regex;

/// Per-file scan ceiling. Larger files are skipped, not rejected.
pub const MAX_FILE_BYTES: usize = 1024 * 1024;

/// Per-push (or per-batch) file ceiling. Above it the whole scan is skipped.
pub const MAX_FILES: usize = 500;

/// How many findings one file may contribute before the scan of that file stops.
pub const MAX_FINDINGS_PER_FILE: usize = 20;

/// Inline opt-out marker. When it appears on a line, that line is not reported.
pub const ALLOW_MARKER: &str = "ironforge:allow-secret";

/// Minimum token length for the generic entropy rule.
const ENTROPY_MIN_LEN: usize = 32;

/// Shannon entropy (bits per character) a token must reach to be reported.
const ENTROPY_THRESHOLD: f64 = 4.5;

/// Bytes inspected when deciding whether a blob is binary.
const BINARY_SNIFF_BYTES: usize = 8192;

/// Paths that are expected to contain secret-shaped strings (documentation,
/// fixtures, lockfiles) and are skipped unless the caller passes its own list.
pub const DEFAULT_ALLOW_GLOBS: &[&str] = &[
    "**/*.md",
    "**/*.mdx",
    "**/*.rst",
    "**/*.lock",
    "**/*.snap",
    "docs/**",
    "**/docs/**",
    "testdata/**",
    "**/testdata/**",
    "**/fixtures/**",
];

/// A detection rule. The first three block; the last two warn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Rule {
    /// `-----BEGIN … PRIVATE KEY-----`
    PrivateKeyHeader,
    /// `AKIA`/`ASIA` + 16 chars, `LTAI…`, `AKID…`
    CloudAccessKey,
    /// `ghp_`/`gho_`/`ghs_`, `glpat-`, `ifs_` prefixed tokens
    PlatformToken,
    /// Three base64url segments starting with `eyJ`
    Jwt,
    /// Length >= 32 and Shannon entropy >= 4.5
    HighEntropy,
}

impl Rule {
    /// Every rule, in report order.
    pub const ALL: [Rule; 5] = [
        Rule::PrivateKeyHeader,
        Rule::CloudAccessKey,
        Rule::PlatformToken,
        Rule::Jwt,
        Rule::HighEntropy,
    ];

    /// Stable rule name. This is what appears in audit events and in the
    /// rejection text the client sees.
    pub fn name(self) -> &'static str {
        match self {
            Rule::PrivateKeyHeader => "private_key",
            Rule::CloudAccessKey => "cloud_access_key",
            Rule::PlatformToken => "platform_token",
            Rule::Jwt => "jwt",
            Rule::HighEntropy => "high_entropy",
        }
    }

    /// Whether a hit refuses the write (`true`) or only produces an event.
    pub fn is_blocking(self) -> bool {
        matches!(
            self,
            Rule::PrivateKeyHeader | Rule::CloudAccessKey | Rule::PlatformToken
        )
    }
}

/// One detection. Deliberately excludes the matched text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub path: String,
    /// 1-based line number inside the blob.
    pub line: usize,
    pub rule: Rule,
}

/// Something the scanner could not look at, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Skip {
    /// The batch exceeded [`Limits::max_files`]; nothing was scanned.
    TooManyFiles { files: usize },
    /// One file exceeded [`Limits::max_file_bytes`]; it was not scanned.
    FileTooLarge { path: String, bytes: usize },
}

impl Skip {
    /// Human-readable reason for the `secret_scan_skipped` event.
    pub fn reason(&self) -> String {
        match self {
            Skip::TooManyFiles { files } => format!(
                "push touches {files} files, above the {} file scan limit",
                MAX_FILES
            ),
            Skip::FileTooLarge { path, bytes } => {
                format!("{path} is {bytes} bytes, above the 1 MiB file scan limit")
            }
        }
    }

    /// File count to report alongside the event, when known.
    pub fn files(&self) -> Option<usize> {
        match self {
            Skip::TooManyFiles { files } => Some(*files),
            Skip::FileTooLarge { .. } => None,
        }
    }

    /// Path to report alongside the event, when the skip is per-file.
    pub fn path(&self) -> Option<&str> {
        match self {
            Skip::TooManyFiles { .. } => None,
            Skip::FileTooLarge { path, .. } => Some(path),
        }
    }
}

/// Scan ceilings. [`Default`] is the production configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub max_files: usize,
    pub max_file_bytes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_files: MAX_FILES,
            max_file_bytes: MAX_FILE_BYTES,
        }
    }
}

impl Limits {
    /// Explicit ceilings, mostly for tests that must exercise the fail-open path.
    pub fn new(max_files: usize, max_file_bytes: usize) -> Self {
        Self {
            max_files,
            max_file_bytes,
        }
    }
}

/// Result of scanning one set of blobs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    /// Blobs whose contents were inspected.
    pub scanned_files: usize,
    pub findings: Vec<Finding>,
    pub skipped: Vec<Skip>,
}

impl Report {
    /// Findings that must refuse the write.
    pub fn blocking(&self) -> impl Iterator<Item = &Finding> {
        self.findings
            .iter()
            .filter(|finding| finding.rule.is_blocking())
    }

    /// Whether any blocking rule fired.
    pub fn has_blocking(&self) -> bool {
        self.blocking().next().is_some()
    }

    /// Report-status message for a blocked push, or `None` when nothing blocks.
    ///
    /// Names paths and rules only — never the matched text.
    pub fn rejection_message(&self) -> Option<String> {
        let blocking: Vec<&Finding> = self.blocking().collect();
        if blocking.is_empty() {
            return None;
        }
        let mut seen = HashSet::new();
        let mut paths: Vec<&str> = Vec::new();
        let mut rules: Vec<&'static str> = Vec::new();
        for finding in &blocking {
            if seen.insert(finding.path.as_str()) {
                paths.push(finding.path.as_str());
            }
            if !rules.contains(&finding.rule.name()) {
                rules.push(finding.rule.name());
            }
        }
        let first = paths.first().copied().unwrap_or("<unknown>");
        Some(if paths.len() == 1 {
            format!(
                "secret scanning rejected the push: {} in {first}",
                rules.join(", ")
            )
        } else {
            format!(
                "secret scanning rejected the push: {} in {} files (first: {first})",
                rules.join(", "),
                paths.len()
            )
        })
    }
}

/// One ref update to scan. `old_sha` is `None` for a newly created ref.
#[derive(Debug, Clone, Copy)]
pub struct RefScanRequest<'a> {
    pub refname: &'a str,
    pub old_sha: Option<&'a str>,
    pub new_sha: &'a str,
}

/// Scan result for a single ref update.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefScan {
    pub refname: String,
    pub report: Report,
}

/// Scan result for a whole push.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PushScan {
    /// Empty when [`PushScan::skipped`] is set: a skipped batch is not scanned.
    pub refs: Vec<RefScan>,
    /// Set when the push was too large to scan.
    pub skipped: Option<Skip>,
}

/// Path globs whose contents are not scanned. `*` stops at `/`, `**` does not,
/// and a leading `**/` also matches the repository root.
#[derive(Debug, Clone, Default)]
pub struct AllowList {
    globs: Vec<(String, Regex)>,
}

impl AllowList {
    /// Build a list from globs. A glob that cannot be compiled is dropped
    /// rather than making the whole list unusable.
    pub fn new<S: AsRef<str>>(globs: &[S]) -> Self {
        let globs = globs
            .iter()
            .filter_map(|glob| {
                let glob = glob.as_ref();
                Regex::new(&glob_to_regex(glob))
                    .ok()
                    .map(|regex| (glob.to_string(), regex))
            })
            .collect();
        Self { globs }
    }

    /// The built-in list: documentation, fixtures and lockfiles.
    pub fn default_rules() -> Self {
        Self::new(DEFAULT_ALLOW_GLOBS)
    }

    /// Nothing is allowed — used by tests that need the raw rule behaviour.
    pub fn empty() -> Self {
        Self { globs: Vec::new() }
    }

    /// Whether `path` (repository-relative) is exempt from scanning.
    pub fn is_allowed(&self, path: &str) -> bool {
        let path = path.trim_start_matches('/');
        self.globs.iter().any(|(_, regex)| regex.is_match(path))
    }

    /// The globs this list was built from, in insertion order.
    pub fn globs(&self) -> impl Iterator<Item = &str> {
        self.globs.iter().map(|(glob, _)| glob.as_str())
    }
}

/// Scan already-materialised file contents (the content API path).
///
/// Uses the production [`Limits`]; see [`scan_files_with`] for a custom one.
pub fn scan_files(files: &[(&str, &[u8])], allowlist: &AllowList) -> Report {
    scan_files_with(files, allowlist, Limits::default())
}

/// [`scan_files`] with explicit ceilings.
///
/// Over [`Limits::max_files`] the batch is skipped as a whole: the report is
/// empty except for a [`Skip::TooManyFiles`] marker.
pub fn scan_files_with(files: &[(&str, &[u8])], allowlist: &AllowList, limits: Limits) -> Report {
    let mut report = Report::default();
    if files.len() > limits.max_files {
        report
            .skipped
            .push(Skip::TooManyFiles { files: files.len() });
        return report;
    }
    for (path, data) in files {
        scan_one(path, data, allowlist, limits, &mut report);
    }
    report
}

/// Scan the blobs introduced by one ref update (`old_sha..new_sha`).
pub fn scan_commit_range(
    repo_path: &Path,
    old_sha: Option<&str>,
    new_sha: &str,
) -> anyhow::Result<Report> {
    let request = RefScanRequest {
        refname: "",
        old_sha,
        new_sha,
    };
    let scan = scan_push(repo_path, &[request], &AllowList::default_rules())?;
    if let Some(skip) = scan.skipped {
        let mut report = Report::default();
        report.skipped.push(skip);
        return Ok(report);
    }
    Ok(scan
        .refs
        .into_iter()
        .next()
        .map(|scan| scan.report)
        .unwrap_or_default())
}

/// Scan every ref update of a push, using the production [`Limits`].
pub fn scan_push(
    repo_path: &Path,
    requests: &[RefScanRequest<'_>],
    allowlist: &AllowList,
) -> anyhow::Result<PushScan> {
    scan_push_with(repo_path, requests, allowlist, Limits::default())
}

/// [`scan_push`] with explicit ceilings.
///
/// The file budget is shared by all refs of the push: exceeding it skips the
/// scan as a whole instead of scanning an arbitrary subset.
pub fn scan_push_with(
    repo_path: &Path,
    requests: &[RefScanRequest<'_>],
    allowlist: &AllowList,
    limits: Limits,
) -> anyhow::Result<PushScan> {
    if requests.is_empty() {
        return Ok(PushScan::default());
    }
    let repo = gix::open(repo_path)
        .with_context(|| format!("failed to open repository {}", repo_path.display()))?;

    let mut per_ref: Vec<(String, Vec<(String, gix::hash::ObjectId)>)> = Vec::new();
    let mut total_files = 0usize;
    for request in requests {
        let new_id = gix::hash::ObjectId::from_hex(request.new_sha.as_bytes())
            .with_context(|| format!("invalid new sha '{}'", request.new_sha))?;
        let old_id = request
            .old_sha
            .and_then(|sha| gix::hash::ObjectId::from_hex(sha.as_bytes()).ok())
            .filter(|id| !id.is_null());
        let new_tree = repo
            .find_object(new_id)
            .with_context(|| format!("failed to read {}", request.new_sha))?
            .peel_to_tree()
            .with_context(|| format!("failed to peel {} to a tree", request.new_sha))?;
        let old_tree = match old_id {
            Some(id) => Some(
                repo.find_object(id)
                    .with_context(|| format!("failed to read old sha for {}", request.refname))?
                    .peel_to_tree()
                    .with_context(|| {
                        format!("failed to peel old sha for {} to a tree", request.refname)
                    })?,
            ),
            None => None,
        };
        let (blobs, count) = changed_blobs(&repo, old_tree.as_ref(), &new_tree, limits)?;
        total_files += count;
        per_ref.push((request.refname.to_string(), blobs));
    }

    if total_files > limits.max_files {
        // Fail open, never silently: the caller turns this into an event.
        return Ok(PushScan {
            refs: Vec::new(),
            skipped: Some(Skip::TooManyFiles { files: total_files }),
        });
    }

    let mut refs = Vec::with_capacity(per_ref.len());
    for (refname, blobs) in per_ref {
        let mut report = Report::default();
        for (path, id) in blobs {
            let header = repo
                .find_header(id)
                .with_context(|| format!("failed to read blob header for {path}"))?;
            if header.kind() != gix::object::Kind::Blob {
                continue;
            }
            let size = usize::try_from(header.size()).unwrap_or(usize::MAX);
            if size > limits.max_file_bytes {
                report
                    .skipped
                    .push(Skip::FileTooLarge { path, bytes: size });
                continue;
            }
            let object = repo
                .find_object(id)
                .with_context(|| format!("failed to read blob for {path}"))?;
            scan_one(&path, &object.data, allowlist, limits, &mut report);
        }
        refs.push(RefScan { refname, report });
    }

    Ok(PushScan {
        refs,
        skipped: None,
    })
}

/// Changed non-tree entries between two trees, capped at `limits.max_files`
/// collected paths. The returned count is the true number of changed files,
/// so an over-limit push is reported with its real size.
fn changed_blobs(
    repo: &gix::Repository,
    old: Option<&gix::Tree<'_>>,
    new: &gix::Tree<'_>,
    limits: Limits,
) -> anyhow::Result<(Vec<(String, gix::hash::ObjectId)>, usize)> {
    let empty = repo.empty_tree();
    let old = old.unwrap_or(&empty);
    let mut platform = old
        .changes()
        .context("failed to prepare a tree diff for the pushed objects")?;
    platform.options(|options| {
        options.track_path();
        // Rename tracking only affects how a rename is spelled; a rename is a
        // deletion plus an addition, and neither needs blob content.
        options.track_rewrites(None);
    });
    let mut blobs = Vec::new();
    let mut count = 0usize;
    platform
        .for_each_to_obtain_tree(new, |change| {
            if let Some((path, id)) = added_or_changed_blob(&change) {
                count += 1;
                if blobs.len() < limits.max_files {
                    blobs.push((path, id));
                }
            }
            Ok(std::ops::ControlFlow::Continue(()))
        })
        .map_err(|error| anyhow::anyhow!("tree diff failed: {error}"))?;
    Ok((blobs, count))
}

/// The `(path, blob id)` of an added or modified file entry, if this change is
/// one. Trees (directories) and deletions carry nothing to scan.
fn added_or_changed_blob(
    change: &gix::object::tree::diff::Change<'_, '_, '_>,
) -> Option<(String, gix::hash::ObjectId)> {
    match change {
        gix::object::tree::diff::Change::Addition {
            location,
            entry_mode,
            id,
            ..
        }
        | gix::object::tree::diff::Change::Modification {
            location,
            entry_mode,
            id,
            ..
        }
        | gix::object::tree::diff::Change::Rewrite {
            location,
            entry_mode,
            id,
            ..
        } => entry_mode
            .is_blob()
            .then(|| (location.to_string(), id.detach())),
        gix::object::tree::diff::Change::Deletion { .. } => None,
    }
}

fn scan_one(path: &str, data: &[u8], allowlist: &AllowList, limits: Limits, report: &mut Report) {
    if allowlist.is_allowed(path) {
        return;
    }
    if data.len() > limits.max_file_bytes {
        report.skipped.push(Skip::FileTooLarge {
            path: path.to_string(),
            bytes: data.len(),
        });
        return;
    }
    if looks_binary(data) {
        return;
    }
    report.scanned_files += 1;
    let text = String::from_utf8_lossy(data);
    let mut found_here = 0usize;
    for (index, line) in text.lines().enumerate() {
        if found_here >= MAX_FINDINGS_PER_FILE {
            break;
        }
        if line.contains(ALLOW_MARKER) {
            continue;
        }
        for rule in rules_in(line) {
            report.findings.push(Finding {
                path: path.to_string(),
                line: index + 1,
                rule,
            });
            found_here += 1;
        }
    }
}

/// Rules matching one line. A blocking hit suppresses the generic entropy rule
/// on the same line: the token is already reported by its specific rule.
fn rules_in(line: &str) -> Vec<Rule> {
    let patterns = patterns();
    let mut rules = Vec::new();
    if patterns.private_key.is_match(line) {
        rules.push(Rule::PrivateKeyHeader);
    }
    if patterns.cloud_access_key.is_match(line) {
        rules.push(Rule::CloudAccessKey);
    }
    if patterns.platform_token.is_match(line) {
        rules.push(Rule::PlatformToken);
    }
    if patterns.jwt.is_match(line) {
        rules.push(Rule::Jwt);
    }
    if rules.is_empty() && has_high_entropy_token(line) {
        rules.push(Rule::HighEntropy);
    }
    rules
}

struct Patterns {
    private_key: Regex,
    cloud_access_key: Regex,
    platform_token: Regex,
    jwt: Regex,
}

fn patterns() -> &'static Patterns {
    static PATTERNS: OnceLock<Patterns> = OnceLock::new();
    PATTERNS.get_or_init(|| Patterns {
        // PKCS#8, PKCS#1, SEC1, OpenSSH and PGP armour headers.
        private_key: Regex::new(r"-----BEGIN (?:[A-Z0-9]+ )?PRIVATE KEY(?: BLOCK)?-----")
            .expect("private key pattern is valid"),
        // AWS (`AKIA`/`ASIA` + 16), Alibaba Cloud (`LTAI`), Tencent Cloud (`AKID`).
        cloud_access_key: Regex::new(
            r"\b(?:(?:AKIA|ASIA)[A-Z0-9]{16}|LTAI[A-Za-z0-9]{12,24}|AKID[A-Za-z0-9]{13,40})\b",
        )
        .expect("cloud access key pattern is valid"),
        // GitHub, GitLab and Gitee personal access tokens.
        platform_token: Regex::new(
            r"\b(?:gh[pousr]_[A-Za-z0-9]{20,}|glpat-[A-Za-z0-9_\-]{20,}|ifs_[A-Za-z0-9]{20,})\b",
        )
        .expect("platform token pattern is valid"),
        jwt: Regex::new(r"\beyJ[A-Za-z0-9_\-]{8,}\.[A-Za-z0-9_\-]{8,}\.[A-Za-z0-9_\-]{8,}")
            .expect("jwt pattern is valid"),
    })
}

fn looks_binary(data: &[u8]) -> bool {
    data.iter().take(BINARY_SNIFF_BYTES).any(|byte| *byte == 0)
}

fn is_token_char(character: char) -> bool {
    character.is_ascii_alphanumeric() || matches!(character, '+' | '/' | '=' | '_' | '-')
}

/// Whether any base64url-ish run on the line crosses the entropy threshold.
fn has_high_entropy_token(line: &str) -> bool {
    let mut current = String::new();
    for character in line.chars().chain(std::iter::once(' ')) {
        if is_token_char(character) {
            current.push(character);
            continue;
        }
        if current.len() >= ENTROPY_MIN_LEN
            && shannon_entropy(current.as_bytes()) >= ENTROPY_THRESHOLD
        {
            return true;
        }
        current.clear();
    }
    false
}

/// Shannon entropy in bits per byte.
fn shannon_entropy(data: &[u8]) -> f64 {
    if data.is_empty() {
        return 0.0;
    }
    let mut counts = [0usize; 256];
    for byte in data {
        counts[*byte as usize] += 1;
    }
    let len = data.len() as f64;
    -counts
        .iter()
        .filter(|count| **count > 0)
        .map(|count| {
            let probability = *count as f64 / len;
            probability * probability.log2()
        })
        .sum::<f64>()
}

/// Translate a path glob into an anchored regular expression.
fn glob_to_regex(glob: &str) -> String {
    let mut regex = String::from("^");
    let mut characters = glob.chars().peekable();
    while let Some(character) = characters.next() {
        match character {
            '*' if characters.peek() == Some(&'*') => {
                characters.next();
                if characters.peek() == Some(&'/') {
                    characters.next();
                    // `**/x` also matches `x` at the repository root.
                    regex.push_str("(?:.*/)?");
                } else {
                    regex.push_str(".*");
                }
            }
            '*' => regex.push_str("[^/]*"),
            '?' => regex.push_str("[^/]"),
            other => regex.push_str(&regex::escape(&other.to_string())),
        }
    }
    regex.push('$');
    regex
}

/// What an audit event reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuditKind {
    /// A blocking rule refused a ref update.
    Rejected,
    /// A warning-level rule matched; the write went through.
    Warned,
    /// Something was not scanned (too many files, file too large, scan error).
    Skipped,
}

impl AuditKind {
    /// Stable event name for audit rows and log fields.
    pub fn name(self) -> &'static str {
        match self {
            AuditKind::Rejected => "secret_scan_rejected",
            AuditKind::Warned => "secret_scan_findings",
            AuditKind::Skipped => "secret_scan_skipped",
        }
    }
}

/// One audit event. Carries no matched text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditEvent {
    pub kind: AuditKind,
    pub refname: Option<String>,
    pub path: Option<String>,
    pub rule: Option<Rule>,
    /// File count, for a batch-level [`AuditKind::Skipped`].
    pub files: Option<usize>,
    pub reason: Option<String>,
}

/// Where audit events go besides `tracing`.
pub type AuditSink = dyn Fn(&AuditEvent) + Send + Sync;

static AUDIT_SINK: OnceLock<Mutex<Option<Arc<AuditSink>>>> = OnceLock::new();

fn sink_slot() -> &'static Mutex<Option<Arc<AuditSink>>> {
    AUDIT_SINK.get_or_init(|| Mutex::new(None))
}

/// Route future audit events to `sink` in addition to `tracing`.
///
/// `rg-git` cannot reach a database, so a caller that can (rg-core, or the
/// push hook in rg-http once it owns one) installs the persistence here.
pub fn set_audit_sink(sink: Arc<AuditSink>) {
    *sink_slot()
        .lock()
        .unwrap_or_else(|error| error.into_inner()) = Some(sink);
}

/// Remove the installed sink, if any.
pub fn clear_audit_sink() {
    *sink_slot()
        .lock()
        .unwrap_or_else(|error| error.into_inner()) = None;
}

/// Emit one audit event: a structured log record, plus the installed sink.
pub fn record(event: AuditEvent) {
    tracing::warn!(
        event = event.kind.name(),
        refname = ?event.refname,
        path = ?event.path,
        rule = ?event.rule.map(Rule::name),
        files = ?event.files,
        reason = ?event.reason,
        "secret scan audit event"
    );
    let sink = sink_slot()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .clone();
    if let Some(sink) = sink {
        sink(&event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PRIVATE_KEY: &str = "-----BEGIN RSA PRIVATE KEY-----";
    const AWS_KEY: &str = "AKIAIOSFODNN7EXAMPLE";
    const GITHUB_TOKEN: &str = "ghp_0123456789abcdefghijklmnopqrstuvwxyzAB";
    const JWT: &str = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.dozjgNryP4J3jVmNHl0w5N_XgL0n3I9PlFUP0THsR8U";

    fn scan(path: &str, content: &str) -> Report {
        scan_files(&[(path, content.as_bytes())], &AllowList::default_rules())
    }

    #[test]
    fn private_key_header_blocks() {
        let report = scan("deploy/id_rsa", PRIVATE_KEY);
        assert!(report.has_blocking(), "{report:?}");
        assert_eq!(report.findings[0].rule, Rule::PrivateKeyHeader);
        assert_eq!(report.findings[0].line, 1);
    }

    #[test]
    fn every_private_key_flavour_blocks() {
        for header in [
            "-----BEGIN PRIVATE KEY-----",
            "-----BEGIN EC PRIVATE KEY-----",
            "-----BEGIN OPENSSH PRIVATE KEY-----",
            "-----BEGIN PGP PRIVATE KEY BLOCK-----",
        ] {
            assert!(
                scan("key.txt", header).has_blocking(),
                "expected {header} to block"
            );
        }
    }

    #[test]
    fn cloud_access_keys_block() {
        for key in [
            AWS_KEY,
            "ASIAIOSFODNN7EXAMPLE",
            "LTAI5tDk3yExample9",
            "AKIDz8krbsJ5yKBZQpn74WFkmLPx3gnPhESA",
        ] {
            let report = scan("config.yaml", &format!("access_key: {key}\n"));
            assert!(report.has_blocking(), "expected {key} to block");
            assert_eq!(report.findings[0].rule, Rule::CloudAccessKey);
        }
    }

    #[test]
    fn platform_tokens_block() {
        for token in [
            GITHUB_TOKEN,
            "gho_0123456789abcdefghijklmnopqrstuvwxyzAB",
            "ghs_0123456789abcdefghijklmnopqrstuvwxyzAB",
            "glpat-0123456789abcdefghij",
            "ifs_0123456789abcdefghijklmn",
        ] {
            let report = scan(".env", &format!("TOKEN={token}\n"));
            assert!(report.has_blocking(), "expected {token} to block");
            assert_eq!(report.findings[0].rule, Rule::PlatformToken);
        }
    }

    #[test]
    fn jwt_warns_without_blocking() {
        let report = scan("src/auth.rs", JWT);
        assert!(!report.has_blocking(), "{report:?}");
        assert_eq!(report.findings[0].rule, Rule::Jwt);
    }

    #[test]
    fn high_entropy_string_warns_without_blocking() {
        let token = "Zx9Qm2Lp7Rt4Vb8Nc1Hf6Jk3Wd5Ys0Ga";
        let report = scan(
            "src/config.rs",
            &format!("const KEY: &str = \"{token}\";\n"),
        );
        assert!(!report.has_blocking(), "{report:?}");
        assert_eq!(report.findings[0].rule, Rule::HighEntropy);
    }

    #[test]
    fn git_shas_are_not_high_entropy() {
        // Hex tops out at 4 bits per character, below the 4.5 threshold.
        let sha = "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08";
        assert!(scan("Cargo.lock", sha).findings.is_empty());
    }

    #[test]
    fn inline_marker_suppresses_only_its_own_line() {
        let content = format!("key = {AWS_KEY} # {ALLOW_MARKER}\nother = {AWS_KEY}\n");
        let report = scan("app.py", &content);
        assert_eq!(report.findings.len(), 1, "{report:?}");
        assert_eq!(report.findings[0].line, 2);
    }

    #[test]
    fn allowlisted_paths_are_skipped() {
        for path in [
            "README.md",
            "docs/security.md",
            "crates/rg-git/testdata/sample.pem",
            "tests/fixtures/key.json",
            "Cargo.lock",
        ] {
            assert!(
                scan(path, PRIVATE_KEY).findings.is_empty(),
                "expected {path} to be allow-listed"
            );
        }
    }

    #[test]
    fn allowlist_globs_do_not_match_unrelated_paths() {
        let allowlist = AllowList::default_rules();
        assert!(!allowlist.is_allowed("src/main.rs"));
        assert!(!allowlist.is_allowed("deploy/secret.md.bak"));
        assert!(allowlist.is_allowed("/docs/security.md"));
    }

    #[test]
    fn rejection_message_never_contains_the_secret() {
        let report = scan("deploy/id_rsa", PRIVATE_KEY);
        let message = report.rejection_message().expect("blocking");
        assert!(message.contains("deploy/id_rsa"), "{message}");
        assert!(message.contains("private_key"), "{message}");
        assert!(!message.contains("PRIVATE KEY"), "{message}");
    }

    #[test]
    fn rejection_message_names_every_file_at_most_once() {
        let content = format!("{PRIVATE_KEY}\n{AWS_KEY}\n");
        let report = scan("one.pem", &content);
        let message = report.rejection_message().unwrap();
        assert!(message.contains("private_key"), "{message}");
        assert!(message.contains("cloud_access_key"), "{message}");
        assert_eq!(message.matches("one.pem").count(), 1, "{message}");
    }

    #[test]
    fn a_file_over_the_size_limit_is_skipped() {
        let limits = Limits::new(MAX_FILES, 8);
        let report = scan_files_with(&[("big.txt", b"0123456789")], &AllowList::empty(), limits);
        assert_eq!(report.scanned_files, 0);
        assert_eq!(
            report.skipped,
            vec![Skip::FileTooLarge {
                path: "big.txt".to_string(),
                bytes: 10
            }]
        );
    }

    #[test]
    fn a_batch_over_the_file_limit_skips_everything() {
        let limits = Limits::new(1, MAX_FILE_BYTES);
        let files: [(&str, &[u8]); 2] = [("a.rs", b"safe"), ("b.rs", PRIVATE_KEY.as_bytes())];
        let report = scan_files_with(&files, &AllowList::empty(), limits);
        assert_eq!(report.scanned_files, 0);
        assert!(
            report.findings.is_empty(),
            "a skipped batch is not scanned: {report:?}"
        );
        assert_eq!(report.skipped, vec![Skip::TooManyFiles { files: 2 }]);
    }

    #[test]
    fn binary_blobs_are_not_scanned() {
        let mut data = PRIVATE_KEY.as_bytes().to_vec();
        data.push(0);
        let report = scan_files(&[("blob.bin", &data)], &AllowList::empty());
        assert_eq!(report.scanned_files, 0);
        assert!(report.findings.is_empty());
    }

    #[test]
    fn glob_translation_handles_root_and_nested_matches() {
        assert!(Regex::new(&glob_to_regex("**/*.md"))
            .unwrap()
            .is_match("a/b/c.md"));
        assert!(Regex::new(&glob_to_regex("**/*.md"))
            .unwrap()
            .is_match("README.md"));
        assert!(!Regex::new(&glob_to_regex("**/*.md"))
            .unwrap()
            .is_match("a/b/c.rs"));
        assert!(!Regex::new(&glob_to_regex("docs/**"))
            .unwrap()
            .is_match("src/docs/x"));
        assert!(Regex::new(&glob_to_regex("docs/**"))
            .unwrap()
            .is_match("docs/x/y"));
    }

    #[test]
    fn entropy_threshold_is_four_point_five() {
        assert!(shannon_entropy(b"abababab") < 1.1);
        assert!(shannon_entropy(b"0123456789abcdef") > 3.9);
    }

    fn commit(repo: &Path, branch: &str, parent: Option<&str>, files: &[(&str, &str)]) -> String {
        let edits: Vec<crate::ops::TreeEdit> = files
            .iter()
            .map(|(path, content)| crate::ops::TreeEdit::SetFile {
                path: (*path).to_string(),
                content: content.as_bytes().to_vec(),
                expected_blob_sha: None,
            })
            .collect();
        match crate::ops::commit_tree_edits(
            repo,
            branch,
            parent,
            &edits,
            "test commit",
            "Test",
            "test@example.invalid",
            parent.is_none(),
        )
        .expect("commit")
        {
            crate::ops::CommitTreeOutcome::Committed(sha) => sha,
            crate::ops::CommitTreeOutcome::TipAdvanced { actual } => {
                panic!("tip advanced to {actual:?}")
            }
        }
    }

    fn init_bare() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        gix::init_bare(dir.path()).expect("init bare repository");
        dir
    }

    #[test]
    fn push_scan_reports_the_file_and_rule_of_a_new_blob() {
        let dir = init_bare();
        let first = commit(dir.path(), "main", None, &[("app.rs", "fn main() {}\n")]);
        let second = commit(
            dir.path(),
            "main",
            Some(&first),
            &[
                ("app.rs", "fn main() {\n    println!(\"hi\");\n}\n"),
                ("deploy/id_rsa", PRIVATE_KEY),
            ],
        );
        let report = scan_commit_range(dir.path(), Some(&first), &second).unwrap();
        assert_eq!(report.scanned_files, 2, "{report:?}");
        let blocking: Vec<&Finding> = report.blocking().collect();
        assert_eq!(blocking.len(), 1, "{report:?}");
        assert_eq!(blocking[0].path, "deploy/id_rsa");
        assert_eq!(blocking[0].rule, Rule::PrivateKeyHeader);
    }

    #[test]
    fn push_scan_only_looks_at_blobs_the_push_introduces() {
        let dir = init_bare();
        let first = commit(dir.path(), "main", None, &[("deploy/id_rsa", PRIVATE_KEY)]);
        let second = commit(
            dir.path(),
            "main",
            Some(&first),
            &[("deploy/id_rsa", PRIVATE_KEY), ("app.rs", "fn main() {}\n")],
        );
        // The secret blob is unchanged by the second commit, so it is not read.
        let report = scan_commit_range(dir.path(), Some(&first), &second).unwrap();
        assert!(report.findings.is_empty(), "{report:?}");
        assert_eq!(report.scanned_files, 1, "{report:?}");
    }

    #[test]
    fn a_new_ref_is_scanned_in_full() {
        let dir = init_bare();
        let tip = commit(dir.path(), "main", None, &[("deploy/id_rsa", PRIVATE_KEY)]);
        let report = scan_commit_range(dir.path(), None, &tip).unwrap();
        assert_eq!(report.scanned_files, 1, "{report:?}");
        assert!(report.has_blocking(), "{report:?}");
    }

    #[test]
    fn a_push_over_the_file_budget_is_skipped_as_a_whole() {
        let dir = init_bare();
        let tip = commit(
            dir.path(),
            "main",
            None,
            &[("a.rs", "fn a() {}\n"), ("deploy/id_rsa", PRIVATE_KEY)],
        );
        let request = RefScanRequest {
            refname: "refs/heads/main",
            old_sha: None,
            new_sha: &tip,
        };
        let scan = scan_push_with(
            dir.path(),
            &[request],
            &AllowList::empty(),
            Limits::new(1, MAX_FILE_BYTES),
        )
        .unwrap();
        assert!(scan.refs.is_empty(), "{scan:?}");
        assert_eq!(scan.skipped, Some(Skip::TooManyFiles { files: 2 }));
        assert!(scan.skipped.unwrap().reason().contains('2'));
    }

    #[test]
    fn audit_events_reach_the_installed_sink() {
        let captured: Arc<Mutex<Vec<AuditEvent>>> = Arc::new(Mutex::new(Vec::new()));
        let sink = captured.clone();
        set_audit_sink(Arc::new(move |event: &AuditEvent| {
            sink.lock().unwrap().push(event.clone());
        }));
        record(AuditEvent {
            kind: AuditKind::Skipped,
            refname: Some("refs/heads/main".to_string()),
            path: None,
            rule: None,
            files: Some(600),
            reason: Some("too many files".to_string()),
        });
        clear_audit_sink();

        let events = captured.lock().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind.name(), "secret_scan_skipped");
        assert_eq!(events[0].files, Some(600));
    }

    /// Manual measurement of the push-path cost. Not part of the normal test
    /// run: `cargo test -p rg-git --release perf_ -- --ignored --nocapture`.
    #[test]
    #[ignore = "manual performance measurement; run with --release --ignored"]
    fn perf_push_scan_is_proportional_to_the_change() {
        let dir = init_bare();
        let base: Vec<(String, String)> = (0..2000)
            .map(|index| {
                (
                    format!("src/module_{index:04}.rs"),
                    format!("pub fn value_{index}() -> u32 {{ {index} }}\n"),
                )
            })
            .collect();
        let base_refs: Vec<(&str, &str)> = base
            .iter()
            .map(|(path, content)| (path.as_str(), content.as_str()))
            .collect();
        let first = commit(dir.path(), "main", None, &base_refs);

        let mut changed = base.clone();
        for (index, file) in changed.iter_mut().enumerate().take(50) {
            file.1 = format!("pub fn changed_{index}() -> u32 {{ {index} }}\n");
        }
        let changed_refs: Vec<(&str, &str)> = changed
            .iter()
            .map(|(path, content)| (path.as_str(), content.as_str()))
            .collect();
        let second = commit(dir.path(), "main", Some(&first), &changed_refs);

        let request = RefScanRequest {
            refname: "refs/heads/main",
            old_sha: Some(&first),
            new_sha: &second,
        };
        let allowlist = AllowList::default_rules();
        let mut best = u128::MAX;
        for _ in 0..5 {
            let started = std::time::Instant::now();
            let scan =
                scan_push_with(dir.path(), &[request], &allowlist, Limits::default()).unwrap();
            best = best.min(started.elapsed().as_micros());
            assert_eq!(scan.refs[0].report.scanned_files, 50, "{scan:?}");
        }
        println!("50-file push scan: {best} us (repository holds 2000 files)");

        let small = commit(
            dir.path(),
            "main",
            Some(&second),
            &[("src/main.rs", "fn main() {}\n")],
        );
        let request = RefScanRequest {
            refname: "refs/heads/main",
            old_sha: Some(&second),
            new_sha: &small,
        };
        let started = std::time::Instant::now();
        let scan = scan_push_with(dir.path(), &[request], &allowlist, Limits::default()).unwrap();
        let elapsed = started.elapsed().as_micros();
        assert_eq!(scan.refs[0].report.scanned_files, 1);
        println!("1-file push scan: {elapsed} us (repository holds 2001 files)");
    }
}
