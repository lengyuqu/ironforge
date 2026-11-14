//! Repository map: a token-budgeted structural view of a repository.
//!
//! [`build`] takes the outlines extracted by [`crate::search::outline`] and
//! turns them into the smallest payload that still answers "what is in this
//! repository": a directory roll-up, then whole-file outlines ranked by how
//! much structure they carry.
//!
//! # Trimming order
//!
//! The budget is spent top-down, never by cutting a payload at a byte offset:
//!
//! 1. Non-source paths are rejected before parsing (`unsupported language` in
//!    `skipped`) and cost no budget at all.
//! 2. Directories are aggregated first — an entry is a path, a count and the
//!    language set, so the whole directory layer is cheap.
//! 3. Files are ranked by `Σ(kind weight × export bonus)`, descending: a file
//!    full of exported structs is worth more than one full of private helpers.
//!    The ranking decides whose items survive.
//! 4. While the estimate exceeds the budget: the lowest-ranked file that still
//!    has items loses its least important root item first (source order is kept
//!    for the rest); the file stays in `files` and `items_omitted` is set. Only
//!    when a file has lost every item does it move to `skipped` with reason
//!    `budget`.
//! 5. If even the `skipped` list (or, at absurdly small budgets, the directory
//!    layer) does not fit, entries are dropped from the tail. That is the last
//!    resort and still reports `truncated: true`.
//!
//! A signature is never shortened: it is either present in full or its item is
//! dropped whole.
//!
//! # Token estimate
//!
//! `estimated_tokens = ceil(json_chars / 4)` over the **entire** response body,
//! including `ref`/`commit_sha`/`budget`. For ASCII source code that is within
//! ~15% of a BPE tokenizer; CJK text tokenises at ~1.5–2 chars per token, so a
//! repository of Chinese comments can cost up to ~2.5× the estimate. The
//! estimate is a soft ceiling by design, but it is self-checked: [`build`]
//! keeps trimming until the *measured* body fits the budget, so callers can
//! rely on `estimated_tokens <= budget`.

use serde::Serialize;

use crate::search::outline::{ItemKind, Language, Outline, OutlineItem};

/// Default budget when the request does not name one.
pub const DEFAULT_BUDGET: usize = 4_000;

/// Smallest accepted budget. The response envelope alone costs ~40 tokens with
/// an empty body, so a smaller budget could not be satisfied honestly.
pub const MIN_BUDGET: usize = 64;

/// Largest accepted budget.
pub const MAX_BUDGET: usize = 32_000;

/// Files parsed per request. Paths past this limit are reported as
/// `too many files` without being read.
pub const MAX_FILES: usize = 2_000;

/// Characters per token used by [`estimate_tokens`].
pub const CHARS_PER_TOKEN: usize = 4;

/// Why a path is not in the map's `files` array.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipReason {
    /// Extension is not one of the outlined languages.
    UnsupportedLanguage,
    /// Past [`MAX_FILES`], so it was never read.
    TooManyFiles,
    /// Larger than [`crate::search::outline::MAX_FILE_BYTES`].
    TooLarge,
    /// Contains NUL bytes.
    Binary,
    /// The file could not be read or decoded as UTF-8.
    Unreadable,
    /// Tree-sitter reported syntax errors and nothing parsed.
    ParseError,
    /// Dropped to stay within the token budget.
    Budget,
}

impl SkipReason {
    /// Stable string used in the JSON body.
    pub fn as_str(self) -> &'static str {
        match self {
            SkipReason::UnsupportedLanguage => "unsupported language",
            SkipReason::TooManyFiles => "too many files",
            SkipReason::TooLarge => "file too large",
            SkipReason::Binary => "binary content",
            SkipReason::Unreadable => "unreadable",
            SkipReason::ParseError => "parse error",
            SkipReason::Budget => "budget",
        }
    }

    fn from_outline_error(error: &str) -> Self {
        match error {
            "unsupported language" => SkipReason::UnsupportedLanguage,
            "file too large" => SkipReason::TooLarge,
            "binary content" => SkipReason::Binary,
            _ => SkipReason::ParseError,
        }
    }
}

/// A path that was considered but is not mapped.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct MapSkipped {
    pub path: String,
    pub reason: &'static str,
}

impl MapSkipped {
    pub fn new(path: impl Into<String>, reason: SkipReason) -> Self {
        Self {
            path: path.into(),
            reason: reason.as_str(),
        }
    }
}

/// One declaration in a mapped file. Children keep the nesting that
/// [`OutlineItem`] produced (`impl` → methods, `class` → methods).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct MapItem {
    pub name: String,
    pub kind: &'static str,
    pub start_line: u32,
    pub end_line: u32,
    /// First source line, whitespace-collapsed. Never shortened by budget
    /// trimming: items are dropped whole instead.
    pub signature: String,
    pub exported: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<MapItem>,
}

impl MapItem {
    fn from_outline(item: &OutlineItem) -> Self {
        Self {
            name: item.name.clone(),
            kind: item.kind.as_str(),
            start_line: item.start_line,
            end_line: item.end_line,
            signature: item.signature.clone(),
            exported: item.exported,
            children: item.children.iter().map(MapItem::from_outline).collect(),
        }
    }
}

/// A mapped source file.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct MapFile {
    pub path: String,
    pub language: &'static str,
    pub items: Vec<MapItem>,
    /// Some items were dropped by the budget; `items` is what survived.
    #[serde(skip_serializing_if = "is_false")]
    pub items_omitted: bool,
    /// Honest caveat about the extraction (Svelte degradation, partial parse,
    /// per-file item cap). Absent when the outline is complete.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notice: Option<String>,
}

fn is_false(value: &bool) -> bool {
    !*value
}

/// Roll-up of one directory.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct MapDirectory {
    /// Repository-relative directory path; `""` is the repository root.
    pub path: String,
    /// Source files found directly in this directory (before budget trimming).
    pub files: usize,
    /// Languages present, sorted.
    pub languages: Vec<&'static str>,
}

/// A file together with the outline extracted from it.
#[derive(Debug, Clone)]
pub struct MappedFile {
    pub path: String,
    pub outline: Outline,
}

/// The `/ai/repos/{o}/{r}/map` body.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RepoMap {
    #[serde(rename = "ref")]
    pub git_ref: String,
    pub commit_sha: Option<String>,
    pub budget: usize,
    pub estimated_tokens: usize,
    /// Content was dropped because of the budget.
    pub truncated: bool,
    pub directories: Vec<MapDirectory>,
    pub files: Vec<MapFile>,
    pub skipped: Vec<MapSkipped>,
}

impl RepoMap {
    /// Measured cost of this body in estimated tokens.
    pub fn measure(&self) -> usize {
        let mut probe = self.clone();
        probe.estimated_tokens = 0;
        let base = serialized_len(&probe);
        let mut estimate = estimate_tokens(base);
        // The only feedback loop is the digit count of `estimated_tokens`.
        for _ in 0..4 {
            let next = estimate_tokens(base - 1 + estimate.to_string().len());
            if next == estimate {
                break;
            }
            estimate = next;
        }
        estimate
    }
}

/// Estimated tokens for a JSON body of `chars` bytes.
pub fn estimate_tokens(chars: usize) -> usize {
    chars.div_ceil(CHARS_PER_TOKEN)
}

/// Build the map for `files` within `budget` tokens.
///
/// `extra_skipped` carries the paths the caller already rejected (unsupported
/// extension, past [`MAX_FILES`], oversized). The returned map always satisfies
/// `estimated_tokens <= budget`; `budget` is clamped into
/// [`MIN_BUDGET`]..=[`MAX_BUDGET`].
pub fn build(
    git_ref: impl Into<String>,
    commit_sha: Option<String>,
    mut files: Vec<MappedFile>,
    extra_skipped: Vec<MapSkipped>,
    budget: usize,
) -> RepoMap {
    let budget = budget.clamp(MIN_BUDGET, MAX_BUDGET);
    let git_ref = git_ref.into();
    let directories = aggregate_directories(&files);

    let mut skipped: Vec<MapSkipped> = extra_skipped;
    let mut states: Vec<FileState> = Vec::with_capacity(files.len());
    for file in std::mem::take(&mut files) {
        match FileState::classify(file) {
            Ok(state) => states.push(state),
            Err(skip) => skipped.push(skip),
        }
    }
    // Most informative first; ties broken by path so output is stable.
    states.sort_by(|a, b| b.weight.cmp(&a.weight).then_with(|| a.path.cmp(&b.path)));

    let mut cost = BodyCost::new(&RepoMap {
        git_ref: git_ref.clone(),
        commit_sha: commit_sha.clone(),
        budget,
        estimated_tokens: 0,
        truncated: false,
        directories: directories.clone(),
        files: Vec::new(),
        skipped: Vec::new(),
    });
    for state in &states {
        cost.add_file(state.len());
    }

    // The budget is in tokens; the running accounting is in characters.
    let budget_chars = budget * CHARS_PER_TOKEN;

    // Phases 1+2 — spend the budget on map *content* first, walking the ranking
    // from the least informative file upwards. Each file loses its least
    // important root item first; once it has none left, the file itself goes.
    // `skipped` deliberately does not compete for these characters: on a
    // 2 000-file repository a small budget must buy a few complete outlines,
    // not a long list of files that did not fit.
    let mut dropped_files = vec![false; states.len()];
    let mut lowest = states.len();
    let mut total = cost.total();
    while total > budget_chars && lowest > 0 {
        let index = lowest - 1;
        if !states[index].items.is_empty() {
            cost.trim_item(states[index].drop_least_important_item());
            total = cost.total();
            continue;
        }
        cost.remove_file(states[index].len());
        dropped_files[index] = true;
        lowest -= 1;
        total = cost.total();
    }

    // Phase 4 — at absurdly small budgets, retire directories from the tail.
    let mut directories = directories;
    while total > budget_chars && !directories.is_empty() {
        directories.pop();
        total = cost.recompute_dirs(&directories);
    }

    // Phase 3 — whatever budget is left is spent on `skipped`, cheapest
    // information first: everything the caller rejected, then the files this
    // budget could not hold. Entries that do not fit are simply absent;
    // `truncated` already says the map is not the whole repository.
    let budget_skipped = states
        .iter()
        .zip(dropped_files.iter())
        .filter(|(_, dropped)| **dropped)
        .map(|(state, _)| MapSkipped::new(state.path.clone(), SkipReason::Budget));

    let mut slack = budget_chars.saturating_sub(total);
    let mut kept_skipped: Vec<MapSkipped> = Vec::new();
    for entry in skipped.drain(..).chain(budget_skipped) {
        let need = serialized_len(&entry) + usize::from(!kept_skipped.is_empty());
        if need > slack {
            break;
        }
        slack -= need;
        kept_skipped.push(entry);
    }

    let mut map = RepoMap {
        git_ref,
        commit_sha,
        budget,
        estimated_tokens: 0,
        truncated: false,
        directories,
        files: states
            .iter()
            .zip(dropped_files.iter())
            .filter(|(_, dropped)| !**dropped)
            .map(|(state, _)| state.to_map_file())
            .collect(),
        skipped: kept_skipped,
    };

    map.truncated = states.iter().any(|state| state.dropped_items)
        || dropped_files.iter().any(|dropped| *dropped)
        || map.skipped.len() < skipped.len();

    map.estimated_tokens = map.measure();
    // Deterministic safety net: the accounting above mirrors the serialised
    // length, but a final measurement guards the advertised invariant against
    // any drift.
    while map.estimated_tokens > budget {
        let trimmed = if let Some(file) = map.files.iter_mut().rev().find(|f| !f.items.is_empty()) {
            file.items.pop();
            file.items_omitted = true;
            true
        } else if !map.files.is_empty() {
            map.files.pop();
            true
        } else if !map.skipped.is_empty() {
            map.skipped.pop();
            true
        } else if !map.directories.is_empty() {
            map.directories.pop();
            true
        } else {
            false
        };
        if !trimmed {
            break;
        }
        map.truncated = true;
        map.estimated_tokens = map.measure();
    }

    map
}

fn serialized_len<T: Serialize>(value: &T) -> usize {
    serde_json::to_string(value).map(|s| s.len()).unwrap_or(0)
}

/// Serialised length of the body and of the two content arrays.
///
/// `skipped` is intentionally absent: it is filled from whatever budget is left
/// *after* the content fits (see [`build`]), so it never drives the trimming
/// loop.
struct BodyCost {
    /// Body length with all three arrays empty.
    body_empty: usize,
    dirs_len: usize,
    dirs_count: usize,
    files_len: usize,
    files_count: usize,
}

impl BodyCost {
    fn new(map: &RepoMap) -> Self {
        let empty = RepoMap {
            git_ref: map.git_ref.clone(),
            commit_sha: map.commit_sha.clone(),
            budget: map.budget,
            estimated_tokens: 0,
            truncated: false,
            directories: Vec::new(),
            files: Vec::new(),
            skipped: Vec::new(),
        };
        Self {
            body_empty: serialized_len(&empty),
            dirs_len: map.directories.iter().map(serialized_len).sum(),
            dirs_count: map.directories.len(),
            files_len: 0,
            files_count: 0,
        }
    }

    fn add_file(&mut self, len: usize) {
        self.files_len += len;
        self.files_count += 1;
    }

    fn trim_item(&mut self, freed: usize) {
        self.files_len = self.files_len.saturating_sub(freed);
    }

    fn remove_file(&mut self, len: usize) {
        self.files_len = self.files_len.saturating_sub(len);
        self.files_count = self.files_count.saturating_sub(1);
    }

    fn recompute_dirs(&mut self, dirs: &[MapDirectory]) -> usize {
        self.dirs_len = dirs.iter().map(serialized_len).sum();
        self.dirs_count = dirs.len();
        self.total()
    }

    /// Body characters with the content arrays only (empty `skipped`).
    fn total(&self) -> usize {
        self.body_empty
            + array_len(self.dirs_len, self.dirs_count)
            + array_len(self.files_len, self.files_count)
    }
}

/// Content length of a JSON array holding `count` entries of `len` chars each.
///
/// `[]` has zero content characters, which is why this returns `0` for an empty
/// array: entries carry their own trailing comma in [`BodyCost`].
fn array_len(len: usize, count: usize) -> usize {
    if count == 0 {
        0
    } else {
        len + count - 1
    }
}

/// One file's outline plus the bookkeeping needed to trim it.
struct FileState {
    path: String,
    language: &'static str,
    notice: Option<String>,
    /// Surviving root items, source order.
    items: Vec<MapItem>,
    /// Serialised length of every surviving root item, comma included.
    item_lens: Vec<usize>,
    /// Weight of every surviving root item; the lowest goes first.
    item_weights: Vec<u64>,
    /// Characters of the remaining items (sum of `item_lens`).
    items_len: usize,
    /// Length of the file object with `items: []`, flag either way.
    base_complete: usize,
    base_omitted: usize,
    weight: u64,
    dropped_items: bool,
}

impl FileState {
    fn classify(file: MappedFile) -> Result<Self, MapSkipped> {
        let MappedFile { path, outline } = file;
        if outline.items.is_empty() {
            if let Some(error) = outline.error.as_deref() {
                return Err(MapSkipped::new(path, SkipReason::from_outline_error(error)));
            }
        }

        let items: Vec<MapItem> = outline.items.iter().map(MapItem::from_outline).collect();
        let item_lens: Vec<usize> = items.iter().map(serialized_len).collect();
        let item_weights: Vec<u64> = outline.items.iter().map(node_weight).collect();
        let weight = item_weights.iter().sum();

        let base = MapFile {
            path: path.clone(),
            language: outline.language.as_str(),
            items: Vec::new(),
            items_omitted: false,
            notice: None,
        };
        let base_complete = serialized_len(&base);
        let base_omitted = serialized_len(&MapFile {
            items_omitted: true,
            ..base
        });

        Ok(Self {
            path,
            language: outline.language.as_str(),
            notice: notice_for(&outline),
            items_len: item_lens.iter().sum(),
            items,
            item_lens,
            item_weights,
            base_omitted,
            base_complete,
            weight,
            dropped_items: false,
        })
    }

    fn len(&self) -> usize {
        let base = if self.dropped_items {
            self.base_omitted
        } else {
            self.base_complete
        };
        base + array_len(self.items_len, self.items.len())
    }

    fn to_map_file(&self) -> MapFile {
        MapFile {
            path: self.path.clone(),
            language: self.language,
            items: self.items.clone(),
            items_omitted: self.dropped_items,
            notice: self.notice.clone(),
        }
    }

    /// Drop the lowest-weight root item still present and return the characters
    /// that frees (the item, its separator, and the `items_omitted` flag on the
    /// first drop).
    fn drop_least_important_item(&mut self) -> usize {
        let Some(index) = self
            .item_weights
            .iter()
            .enumerate()
            .min_by_key(|(index, weight)| (**weight, std::cmp::Reverse(*index)))
            .map(|(index, _)| index)
        else {
            return 0;
        };
        let item_len = self.item_lens.remove(index);
        self.item_weights.remove(index);
        self.items.remove(index);
        self.items_len -= item_len;

        let mut freed = item_len;
        if !self.items.is_empty() {
            freed += 1; // one separator fewer
        }
        if !self.dropped_items {
            self.dropped_items = true;
            freed += self.base_omitted - self.base_complete;
        }
        freed
    }
}

/// Honest caveats about an extraction, or `None` when it is complete.
fn notice_for(outline: &Outline) -> Option<String> {
    let mut parts = Vec::new();
    if outline.language == Language::Svelte {
        parts.push(
            "svelte: degraded outline, only <script> declarations are parsed (markup is not)"
                .to_string(),
        );
    }
    if outline.truncated {
        parts.push(format!(
            "outline stopped at the per-file item cap ({})",
            crate::search::outline::MAX_ITEMS
        ));
    }
    if let Some(error) = outline.error.as_deref() {
        parts.push(format!("partial outline: {error}"));
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("; "))
    }
}

/// Importance of an item kind. Types outrank functions, which outrank
/// bookkeeping; methods and fields ride on their parent's weight.
fn kind_weight(kind: ItemKind) -> u64 {
    match kind {
        ItemKind::Struct
        | ItemKind::Class
        | ItemKind::Trait
        | ItemKind::Interface
        | ItemKind::Enum
        | ItemKind::Type => 3,
        ItemKind::Function | ItemKind::Impl | ItemKind::Module => 2,
        ItemKind::Method | ItemKind::Const | ItemKind::Macro | ItemKind::Other => 1,
    }
}

/// Weight of an item subtree; exported declarations count double.
fn node_weight(item: &OutlineItem) -> u64 {
    let base = kind_weight(item.kind) * if item.exported { 2 } else { 1 };
    base + item.children.iter().map(node_weight).sum::<u64>()
}

fn aggregate_directories(files: &[MappedFile]) -> Vec<MapDirectory> {
    use std::collections::BTreeMap;

    let mut by_dir: BTreeMap<String, (usize, Vec<&'static str>)> = BTreeMap::new();
    for file in files {
        let dir = match file.path.rfind('/') {
            Some(index) => &file.path[..index],
            None => "",
        };
        let entry = by_dir.entry(dir.to_string()).or_insert((0, Vec::new()));
        entry.0 += 1;
        let language = file.outline.language.as_str();
        if !entry.1.contains(&language) {
            entry.1.push(language);
        }
    }
    by_dir
        .into_iter()
        .map(|(path, (files, mut languages))| {
            languages.sort_unstable();
            MapDirectory {
                path,
                files,
                languages,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests;
