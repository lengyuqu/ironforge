//! Per-file indexer abstraction.
//!
//! Two indexers exist today:
//!
//! * [`CodeIndexer`] (full-text) — the existing FTS pipeline keeps its database
//!   methods; this trait only exposes its per-file admission and extraction
//!   behaviour.
//! * [`OutlineIndexer`] — structured declarations from tree-sitter.
//!
//! The separation is deliberate so a future embedding/vector backend can be
//! added as another [`SearchIndexer`] implementation without changing callers
//! or the FTS code path. Keep implementations side-effect free in `extract`:
//! persistence stays in each indexer's own service methods.

use std::path::Path;

use crate::search::code_indexer::{self, CodeIndexer};
use crate::search::outline::{extract_outline, Language, Outline};

/// Artifact produced by the full-text indexer for a single file.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct FtsDocument {
    /// Human-readable language label stored in `code_fts.language`.
    pub language: String,
    /// File contents, indexed verbatim.
    pub content: String,
}

/// Common interface for per-file indexers.
///
/// "Can this indexer handle the file?" is separate from "extract the artifact"
/// so callers can cheaply skip unsupported paths (binary files, unknown
/// extensions, oversized outlines).
pub trait SearchIndexer {
    /// Artifact this indexer produces for one file.
    type Artifact;

    /// Stable backend name used in logs and diagnostics.
    fn name(&self) -> &'static str;

    /// Whether this indexer can handle `path`.
    fn supports(&self, path: &str) -> bool;

    /// Extract the artifact from already-read `source`.
    ///
    /// Implementations must not touch the filesystem or the database, and the
    /// outline implementation never fails: see [`extract_outline`].
    fn extract(&self, path: &str, source: &str) -> Self::Artifact;
}

impl SearchIndexer for CodeIndexer {
    type Artifact = FtsDocument;

    fn name(&self) -> &'static str {
        "fts"
    }

    fn supports(&self, path: &str) -> bool {
        // Admission is path/content based; the real content is checked again
        // when indexing. An empty probe is enough to reject binaries by
        // extension and hidden directories.
        code_indexer::should_index(Path::new(path), b"")
    }

    fn extract(&self, path: &str, source: &str) -> Self::Artifact {
        FtsDocument {
            language: code_indexer::infer_language(Path::new(path)),
            content: source.to_string(),
        }
    }
}

/// Structured-outline indexer backed by tree-sitter.
#[derive(Debug, Default, Clone, Copy)]
pub struct OutlineIndexer;

impl SearchIndexer for OutlineIndexer {
    type Artifact = Outline;

    fn name(&self) -> &'static str {
        "outline"
    }

    fn supports(&self, path: &str) -> bool {
        Language::from_path(path).is_supported()
    }

    fn extract(&self, path: &str, source: &str) -> Self::Artifact {
        extract_outline(path, source)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::outline::ItemKind;

    #[test]
    fn outline_indexer_advertises_supported_languages() {
        let indexer = OutlineIndexer;
        assert_eq!(indexer.name(), "outline");
        for path in ["a.rs", "a.ts", "a.tsx", "a.py", "a.go", "a.svelte"] {
            assert!(indexer.supports(path), "{path}");
        }
        assert!(!indexer.supports("README.md"));
        assert!(!indexer.supports("Makefile"));
    }

    #[test]
    fn outline_indexer_extracts_without_side_effects() {
        let indexer = OutlineIndexer;
        let outline = indexer.extract("lib.rs", "pub fn hello() {}\n");
        assert_eq!(outline.language, Language::Rust);
        assert_eq!(outline.items.len(), 1);
        assert_eq!(outline.items[0].kind, ItemKind::Function);
        assert!(outline.items[0].exported);
    }

    #[test]
    fn outline_indexer_is_fail_soft_for_unknown_paths() {
        let outline = OutlineIndexer.extract("notes.txt", "hello");
        assert_eq!(outline.language, Language::Other);
        assert!(outline.error.is_some());
    }
}
