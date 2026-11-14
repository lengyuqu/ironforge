//! Structured code outline extraction (issue #39, step 1).
//!
//! This module turns a source file into a tree of declarations (functions,
//! methods, types, …) using real tree-sitter ASTs — never regular expressions.
//! It is the extraction layer only: HTTP endpoints, token budgeting and the
//! symbol graph are deliberately out of scope (task T10).
//!
//! # Fail-soft contract
//!
//! [`extract_outline`] never returns `Result` and never panics on malformed
//! input. Unsupported files, oversized files, binary content and syntax errors
//! all return an [`Outline`] with an `error` string and an empty or partial item
//! list. Callers can display the outline that did parse while surfacing the
//! error separately.
//!
//! # Limits
//!
//! * [`MAX_FILE_BYTES`] — 512 KiB. Outline extraction is meant for navigation;
//!   files larger than this are usually generated artifacts where a declaration
//!   tree is noise. Indexing should skip them instead of paying parse cost.
//! * [`MAX_ITEMS`] — 2000 items per file. A declaration-dense generated file
//!   would otherwise blow up downstream token budgets; the tail is dropped and
//!   `truncated` is set.
//! * [`MAX_SIGNATURE_CHARS`] — signatures are a single line capped at 200
//!   characters. Function bodies are never included; T10's token trimming
//!   depends on this invariant.
//!
//! # Svelte
//!
//! `tree-sitter-svelte` only ever published 0.10.2, which pins
//! `tree-sitter ^0.20.6`. Its `Language` type is incompatible with the 0.25
//! runtime (two different `tree-sitter` crates would be linked), so Svelte is a
//! documented best-effort fallback: `<script>` blocks are located textually and
//! their contents are parsed with the TypeScript grammar. Markup is not parsed;
//! script declarations are structurally accurate but Svelte-specific syntax
//! (runes, `$:` reactive statements) is opaque to the TS grammar.

mod queries;

use std::path::Path;

/// Files larger than this are skipped without parsing.
pub const MAX_FILE_BYTES: usize = 512 * 1024;

/// Maximum number of outline items returned per file (all nesting levels).
pub const MAX_ITEMS: usize = 2000;

/// Maximum length of [`OutlineItem::signature`], in characters.
pub const MAX_SIGNATURE_CHARS: usize = 200;

/// Language of an extracted outline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize)]
pub enum Language {
    Rust,
    TypeScript,
    Tsx,
    Python,
    Go,
    Svelte,
    /// Unknown/unsupported extension. `extract_outline` returns an error for
    /// these; the variant exists so the outline never lies about a real
    /// language.
    #[default]
    Other,
}

impl Language {
    /// Infer the outline language from a file path.
    ///
    /// `js`/`jsx`/`mjs`/`cjs` are parsed with the TypeScript grammar, which is
    /// a superset of JavaScript.
    pub fn from_path(path: &str) -> Self {
        let ext = Path::new(path)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        match ext.as_str() {
            "rs" => Language::Rust,
            "ts" | "mts" | "cts" | "js" | "jsx" | "mjs" | "cjs" => Language::TypeScript,
            "tsx" => Language::Tsx,
            "py" | "pyi" => Language::Python,
            "go" => Language::Go,
            "svelte" => Language::Svelte,
            _ => Language::Other,
        }
    }

    /// Stable lowercase name, suitable for JSON output and logs.
    pub fn as_str(self) -> &'static str {
        match self {
            Language::Rust => "rust",
            Language::TypeScript => "typescript",
            Language::Tsx => "tsx",
            Language::Python => "python",
            Language::Go => "go",
            Language::Svelte => "svelte",
            Language::Other => "other",
        }
    }

    /// Whether outlines can be extracted for this language.
    pub fn is_supported(self) -> bool {
        !matches!(self, Language::Other)
    }
}

/// Kind of an outline item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize)]
pub enum ItemKind {
    Function,
    Method,
    Struct,
    Enum,
    Trait,
    Impl,
    Class,
    Interface,
    Module,
    Const,
    Macro,
    Type,
    Other,
}

impl ItemKind {
    /// Stable lowercase name, suitable for JSON output and logs.
    pub fn as_str(self) -> &'static str {
        match self {
            ItemKind::Function => "function",
            ItemKind::Method => "method",
            ItemKind::Struct => "struct",
            ItemKind::Enum => "enum",
            ItemKind::Trait => "trait",
            ItemKind::Impl => "impl",
            ItemKind::Class => "class",
            ItemKind::Interface => "interface",
            ItemKind::Module => "module",
            ItemKind::Const => "const",
            ItemKind::Macro => "macro",
            ItemKind::Type => "type",
            ItemKind::Other => "other",
        }
    }
}

/// A single declaration in a source file.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct OutlineItem {
    pub name: String,
    pub kind: ItemKind,
    /// 1-based first line.
    pub start_line: u32,
    /// 1-based inclusive last line.
    pub end_line: u32,
    /// First source line of the declaration, whitespace-collapsed and capped at
    /// [`MAX_SIGNATURE_CHARS`]. Never contains the body.
    pub signature: String,
    /// Rust: `pub`; TS: `export`; Python: no leading `_`; Go: leading uppercase.
    pub exported: bool,
    pub children: Vec<OutlineItem>,
}

/// Result of outlining one file.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Outline {
    pub language: Language,
    pub items: Vec<OutlineItem>,
    /// True when the item cap was hit and items were dropped.
    pub truncated: bool,
    /// Parse failure reason. Items may still be populated (partial parse).
    pub error: Option<String>,
}

impl Outline {
    fn failed(language: Language, error: impl Into<String>) -> Self {
        Self {
            language,
            items: Vec::new(),
            truncated: false,
            error: Some(error.into()),
        }
    }

    /// Total number of items at all nesting levels.
    pub fn item_count(&self) -> usize {
        count_items(&self.items)
    }
}

/// Extract a structured outline from `source`.
///
/// The language is inferred from `path`'s extension. Unknown extensions and
/// files over [`MAX_FILE_BYTES`] return an error without parsing. Syntax errors
/// produce a partial outline plus `error`; they never panic.
pub fn extract_outline(path: &str, source: &str) -> Outline {
    let language = Language::from_path(path);
    if !language.is_supported() {
        return Outline::failed(language, "unsupported language");
    }
    if source.len() > MAX_FILE_BYTES {
        return Outline::failed(language, "file too large");
    }
    if source.contains('\0') {
        return Outline::failed(language, "binary content");
    }

    let mut outline = match language {
        Language::Rust => queries::extract_rust(source),
        Language::TypeScript => queries::extract_typescript(source, false),
        Language::Tsx => queries::extract_typescript(source, true),
        Language::Python => queries::extract_python(source),
        Language::Go => queries::extract_go(source),
        Language::Svelte => queries::extract_svelte(source),
        Language::Other => Outline::failed(language, "unsupported language"),
    };
    outline.language = language;
    truncate_to_limit(&mut outline);
    outline
}

/// A declaration found by a tree-sitter query, before nesting is resolved.
pub(super) struct RawItem {
    pub name: String,
    pub kind: ItemKind,
    pub start_byte: usize,
    pub end_byte: usize,
    pub start_line: u32,
    pub end_line: u32,
    pub signature: String,
    pub exported: bool,
}

struct Frame {
    end_byte: usize,
    item: OutlineItem,
}

/// Nest flat query results by source range and apply the error metadata.
pub(super) fn assemble(
    language: Language,
    mut raws: Vec<RawItem>,
    error: Option<String>,
) -> Outline {
    raws.sort_by(|a, b| {
        a.start_byte
            .cmp(&b.start_byte)
            .then_with(|| b.end_byte.cmp(&a.end_byte))
    });
    raws.dedup_by(|a, b| a.start_byte == b.start_byte && a.end_byte == b.end_byte);

    let mut roots: Vec<OutlineItem> = Vec::new();
    let mut stack: Vec<Frame> = Vec::new();

    for raw in raws {
        while let Some(top) = stack.last() {
            let inside = raw.start_byte < top.end_byte && raw.end_byte <= top.end_byte;
            if inside {
                break;
            }
            if let Some(frame) = stack.pop() {
                attach(&mut stack, &mut roots, frame.item);
            }
        }
        stack.push(Frame {
            end_byte: raw.end_byte,
            item: OutlineItem {
                name: raw.name,
                kind: raw.kind,
                start_line: raw.start_line,
                end_line: raw.end_line,
                signature: raw.signature,
                exported: raw.exported,
                children: Vec::new(),
            },
        });
    }
    while let Some(frame) = stack.pop() {
        attach(&mut stack, &mut roots, frame.item);
    }

    rekind_methods(&mut roots);

    Outline {
        language,
        items: roots,
        truncated: false,
        error,
    }
}

fn attach(stack: &mut [Frame], roots: &mut Vec<OutlineItem>, item: OutlineItem) {
    match stack.last_mut() {
        Some(parent) => parent.item.children.push(item),
        None => roots.push(item),
    }
}

/// Rust functions declared inside `impl`/`trait` blocks are methods. The other
/// grammars either emit dedicated method nodes or (Python) intentionally keep
/// the language's "everything is a function" wording.
fn rekind_methods(items: &mut [OutlineItem]) {
    for item in items.iter_mut() {
        if matches!(item.kind, ItemKind::Impl | ItemKind::Trait) {
            for child in item.children.iter_mut() {
                if child.kind == ItemKind::Function {
                    child.kind = ItemKind::Method;
                }
            }
        }
        rekind_methods(&mut item.children);
    }
}

fn count_items(items: &[OutlineItem]) -> usize {
    items
        .iter()
        .map(|item| 1 + count_items(&item.children))
        .sum()
}

fn truncate_to_limit(outline: &mut Outline) {
    if count_items(&outline.items) <= MAX_ITEMS {
        return;
    }
    outline.truncated = true;
    let mut budget = MAX_ITEMS;
    truncate_items(&mut outline.items, &mut budget);
}

fn truncate_items(items: &mut Vec<OutlineItem>, budget: &mut usize) {
    let mut kept = Vec::with_capacity(items.len().min(*budget));
    for mut item in std::mem::take(items) {
        if *budget == 0 {
            break;
        }
        *budget -= 1;
        truncate_items(&mut item.children, budget);
        kept.push(item);
    }
    *items = kept;
}

#[cfg(test)]
mod tests;
