//! Corpus measurement for the symbol-graph design (task T27, issue #63).
//!
//! Test-only and `#[ignore]`d on purpose: it walks a checkout, runs
//! [`extract_outline`] over every supported file and counts identifier
//! occurrences with a tree-sitter query. It prints the numbers quoted in
//! `ironforge-docs/architecture/symbol-graph-design-2026-11.md`; nothing here
//! ships in a release build and no production code is touched.
//!
//! Reproduce:
//!
//! ```text
//! cargo test -p rg-core --release measure_symbol_graph_corpus -- --ignored --nocapture
//! IRONFORGE_MEASURE_ROOT="$(go env GOROOT)/src" \
//!     cargo test -p rg-core --release measure_symbol_graph_corpus -- --ignored --nocapture
//! ```
//!
//! Set `IRONFORGE_MEASURE_DUMP_DIR=/tmp/sym` to also emit
//! `definitions.tsv` / `references.tsv` in the shape of the draft tables, which
//! is what the disk-size estimate in the design doc measures with `sqlite3`.
//!
//! Identifier counts are "identifier nodes in the AST", i.e. an upper bound on
//! the reference rows a name-based reference index could emit. The test also
//! prints `identifiers - definitions`, the count you get after subtracting the
//! one occurrence that is the declaration's own name.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use tree_sitter::{Language as TsLanguage, Parser, Query, QueryCursor, StreamingIterator};

use super::{extract_outline, Language, OutlineItem, MAX_FILE_BYTES};

/// Directories that never carry first-party source worth indexing.
const SKIP_DIRS: &[&str] = &[
    ".git",
    ".cargo",
    ".rustup",
    "target",
    "node_modules",
    "vendor",
    "dist",
    "build",
    "coverage",
    ".next",
    "__pycache__",
    ".venv",
    "venv",
];

#[derive(Default)]
struct Bucket {
    files: usize,
    source_bytes: u64,
    source_lines: u64,
    definitions: u64,
    identifiers: u64,
    parse_errors: usize,
    truncated_files: usize,
    def_per_file: Vec<u32>,
    ident_per_file: Vec<u32>,
    def_name_chars: u64,
    signature_chars: u64,
    ident_chars: u64,
    path_chars: u64,
    longest_path: usize,
    longest_identifier: usize,
}

impl Bucket {
    fn record(&mut self, file: FileFacts) {
        self.files += 1;
        self.source_bytes += file.source_bytes;
        self.source_lines += file.source_lines;
        self.definitions += file.definitions as u64;
        self.identifiers += file.identifiers as u64;
        if file.error {
            self.parse_errors += 1;
        }
        if file.truncated {
            self.truncated_files += 1;
        }
        self.def_per_file.push(file.definitions);
        self.ident_per_file.push(file.identifiers);
        self.def_name_chars += file.def_name_chars;
        self.signature_chars += file.signature_chars;
        self.ident_chars += file.ident_chars;
        self.path_chars += file.path_chars as u64;
        self.longest_path = self.longest_path.max(file.path_chars);
        self.longest_identifier = self.longest_identifier.max(file.longest_identifier);
    }

    /// Mean length of the columns that dominate a row's size.
    fn mean_def_name(&self) -> f64 {
        ratio(self.def_name_chars, self.definitions)
    }

    fn mean_signature(&self) -> f64 {
        ratio(self.signature_chars, self.definitions)
    }

    fn mean_identifier(&self) -> f64 {
        ratio(self.ident_chars, self.identifiers)
    }

    fn mean_path(&self) -> f64 {
        ratio(self.path_chars, self.files as u64)
    }

    fn references(&self) -> u64 {
        // Every definition contributes exactly one identifier occurrence that is
        // its own name; the rest are uses.
        self.identifiers.saturating_sub(self.definitions)
    }
}

fn ratio(numerator: u64, denominator: u64) -> f64 {
    if denominator == 0 {
        0.0
    } else {
        numerator as f64 / denominator as f64
    }
}

struct FileFacts {
    source_bytes: u64,
    source_lines: u64,
    definitions: u32,
    identifiers: u32,
    def_name_chars: u64,
    signature_chars: u64,
    ident_chars: u64,
    path_chars: usize,
    longest_identifier: usize,
    error: bool,
    truncated: bool,
}

#[test]
#[ignore = "corpus measurement for the symbol-graph design (T27); pass --ignored --nocapture"]
fn measure_symbol_graph_corpus() {
    let root = measure_root();
    let mut paths = Vec::new();
    walk(&root, &mut paths);

    let mut buckets: BTreeMap<&'static str, Bucket> = BTreeMap::new();
    let mut unsupported = 0usize;
    let mut too_large = 0usize;
    let mut unreadable = 0usize;
    let mut error_paths: Vec<String> = Vec::new();
    let mut dumper = Dumper::from_env();

    for path in &paths {
        let relative = path
            .strip_prefix(&root)
            .unwrap_or(path.as_path())
            .to_string_lossy()
            .replace('\\', "/");
        let language = Language::from_path(&relative);
        if !language.is_supported() {
            unsupported += 1;
            continue;
        }
        if std::fs::metadata(path).map(|meta| meta.len()).unwrap_or(0) > MAX_FILE_BYTES as u64 {
            too_large += 1;
            continue;
        }
        let Ok(source) = std::fs::read_to_string(path) else {
            unreadable += 1;
            continue;
        };

        let outline = extract_outline(&relative, &source);
        let mut definitions = 0u32;
        let mut def_name_chars = 0u64;
        let mut signature_chars = 0u64;
        for_each_item(&outline.items, &mut |item| {
            definitions += 1;
            def_name_chars += item.name.chars().count() as u64;
            signature_chars += item.signature.chars().count() as u64;
            if let Some(dumper) = dumper.as_mut() {
                dumper.definition(&relative, item);
            }
        });

        let identifiers = count_identifiers(language, &source, &relative, dumper.as_mut());
        if outline.error.is_some() {
            error_paths.push(relative.clone());
        }
        buckets
            .entry(language.as_str())
            .or_default()
            .record(FileFacts {
                source_bytes: source.len() as u64,
                source_lines: source.bytes().filter(|byte| *byte == b'\n').count() as u64,
                definitions,
                identifiers: identifiers.count as u32,
                def_name_chars,
                signature_chars,
                ident_chars: identifiers.chars,
                path_chars: relative.chars().count(),
                longest_identifier: identifiers.longest,
                error: outline.error.is_some(),
                truncated: outline.truncated,
            });
    }

    println!("\n== symbol-graph corpus measurement ==");
    println!("root: {}", root.display());
    println!(
        "files walked: {} (supported+readable: {}, unsupported ext: {}, >{} KiB: {}, unreadable: {})",
        paths.len(),
        buckets.values().map(|bucket| bucket.files).sum::<usize>(),
        unsupported,
        MAX_FILE_BYTES / 1024,
        too_large,
        unreadable
    );

    println!("\n| language | files | source KiB | defs | identifier occurrences | references (id - defs) | defs/file median | defs/file max | ids/file median | ids/file p90 | ids/file max |");
    println!("|---|---|---|---|---|---|---|---|---|---|---|");
    let mut total = Bucket::default();
    for (language, bucket) in &buckets {
        let mut defs = bucket.def_per_file.clone();
        let mut ids = bucket.ident_per_file.clone();
        defs.sort_unstable();
        ids.sort_unstable();
        println!(
            "| {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} |",
            language,
            bucket.files,
            bucket.source_bytes / 1024,
            bucket.definitions,
            bucket.identifiers,
            bucket.references(),
            percentile(&defs, 0.50),
            defs.last().copied().unwrap_or(0),
            percentile(&ids, 0.50),
            percentile(&ids, 0.90),
            ids.last().copied().unwrap_or(0),
        );
        total.files += bucket.files;
        total.source_bytes += bucket.source_bytes;
        total.source_lines += bucket.source_lines;
        total.definitions += bucket.definitions;
        total.identifiers += bucket.identifiers;
        total.def_name_chars += bucket.def_name_chars;
        total.signature_chars += bucket.signature_chars;
        total.ident_chars += bucket.ident_chars;
        total.path_chars += bucket.path_chars;
        total.longest_path = total.longest_path.max(bucket.longest_path);
        total.longest_identifier = total.longest_identifier.max(bucket.longest_identifier);
        total.parse_errors += bucket.parse_errors;
        total.truncated_files += bucket.truncated_files;
        total.def_per_file.extend_from_slice(&bucket.def_per_file);
        total
            .ident_per_file
            .extend_from_slice(&bucket.ident_per_file);
    }
    let mut defs = total.def_per_file.clone();
    let mut ids = total.ident_per_file.clone();
    defs.sort_unstable();
    ids.sort_unstable();
    println!(
        "| **total** | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} |",
        total.files,
        total.source_bytes / 1024,
        total.definitions,
        total.identifiers,
        total.references(),
        percentile(&defs, 0.50),
        defs.last().copied().unwrap_or(0),
        percentile(&ids, 0.50),
        percentile(&ids, 0.90),
        ids.last().copied().unwrap_or(0),
    );

    println!("\n== column widths (chars, mean / max) ==");
    println!("| language | path | def name | signature | identifier |");
    println!("|---|---|---|---|---|");
    for (language, bucket) in &buckets {
        println!(
            "| {} | {:.1} / {} | {:.1} | {:.1} | {:.1} / {} |",
            language,
            bucket.mean_path(),
            bucket.longest_path,
            bucket.mean_def_name(),
            bucket.mean_signature(),
            bucket.mean_identifier(),
            bucket.longest_identifier,
        );
    }
    println!(
        "| **total** | {:.1} / {} | {:.1} | {:.1} | {:.1} / {} |",
        total.mean_path(),
        total.longest_path,
        total.mean_def_name(),
        total.mean_signature(),
        total.mean_identifier(),
        total.longest_identifier,
    );

    println!(
        "\nsource lines: {}, parse-error files: {}, truncated files: {}",
        total.source_lines, total.parse_errors, total.truncated_files
    );
    if !error_paths.is_empty() {
        println!("\nparse-error files ({}):", error_paths.len());
        for path in &error_paths {
            println!("  {path}");
        }
    }
}

fn measure_root() -> PathBuf {
    match std::env::var_os("IRONFORGE_MEASURE_ROOT") {
        Some(root) => PathBuf::from(root),
        // `crates/rg-core` → workspace root.
        None => Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .components()
            .collect(),
    }
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if file_type.is_dir() {
            if name.starts_with('.') || SKIP_DIRS.contains(&name.as_ref()) {
                continue;
            }
            walk(&entry.path(), out);
        } else if file_type.is_file() || file_type.is_symlink() {
            if name.starts_with('.') {
                continue;
            }
            out.push(entry.path());
        }
    }
}

fn for_each_item(items: &[OutlineItem], visit: &mut impl FnMut(&OutlineItem)) {
    for item in items {
        visit(item);
        for_each_item(&item.children, visit);
    }
}

struct IdentifierCount {
    count: usize,
    chars: u64,
    longest: usize,
}

/// Writes one TSV row per draft `symbol_definitions` / `symbol_references` row
/// so the design doc can measure real table size with `sqlite3`.
struct Dumper {
    definitions: BufWriter<File>,
    references: BufWriter<File>,
}

impl Dumper {
    fn from_env() -> Option<Self> {
        let dir = PathBuf::from(std::env::var_os("IRONFORGE_MEASURE_DUMP_DIR")?);
        std::fs::create_dir_all(&dir).ok()?;
        Some(Self {
            definitions: BufWriter::new(File::create(dir.join("definitions.tsv")).ok()?),
            references: BufWriter::new(File::create(dir.join("references.tsv")).ok()?),
        })
    }

    fn definition(&mut self, path: &str, item: &OutlineItem) {
        let _ = writeln!(
            self.definitions,
            "{}\t{}\t{}\t{}\t{}\t{}",
            clean(path),
            clean(&item.name),
            item.kind.as_str(),
            item.start_line,
            item.end_line,
            clean(&item.signature),
        );
    }

    fn reference(&mut self, path: &str, line: u32, name: &str, snippet: &str) {
        let _ = writeln!(
            self.references,
            "{}\t{}\t{}\t{}",
            clean(path),
            line,
            clean(name),
            clean(snippet),
        );
    }
}

/// Keep TSV columns single-line and tab-free.
fn clean(value: &str) -> String {
    value.replace(['\t', '\n', '\r'], " ")
}

/// Count identifier nodes in `source` with a per-language query.
///
/// Languages without an outline grammar (none today, `Other` never reaches
/// here) and query build failures fall back to zero so the corpus scan stays
/// fail-soft.
fn count_identifiers(
    language: Language,
    source: &str,
    path: &str,
    mut dumper: Option<&mut Dumper>,
) -> IdentifierCount {
    let Some((ts_language, query)) = identifier_query(language) else {
        return IdentifierCount {
            count: 0,
            chars: 0,
            longest: 0,
        };
    };
    let mut parser = Parser::new();
    if parser.set_language(ts_language).is_err() {
        return IdentifierCount {
            count: 0,
            chars: 0,
            longest: 0,
        };
    }
    let Some(tree) = parser.parse(source, None) else {
        return IdentifierCount {
            count: 0,
            chars: 0,
            longest: 0,
        };
    };

    let text = source.as_bytes();
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(query, tree.root_node(), text);
    let mut count = 0usize;
    let mut chars = 0u64;
    let mut longest = 0usize;
    let lines: Option<Vec<&str>> = dumper.as_ref().map(|_| source.lines().collect());
    while let Some(matched) = matches.next() {
        for capture in matched.captures {
            let Ok(slice) = capture.node.utf8_text(text) else {
                continue;
            };
            count += 1;
            chars += slice.chars().count() as u64;
            longest = longest.max(slice.chars().count());
            if let (Some(dumper), Some(lines)) = (dumper.as_deref_mut(), lines.as_ref()) {
                let row = capture.node.start_position().row;
                let snippet = lines.get(row).copied().unwrap_or("");
                dumper.reference(path, row as u32 + 1, slice, snippet);
            }
        }
    }
    IdentifierCount {
        count,
        chars,
        longest,
    }
}

/// Identifier node names per grammar. Missing node names make `Query::new`
/// fail, which is why each set is kept to nodes the grammar documents.
fn identifier_query(language: Language) -> Option<(&'static TsLanguage, &'static Query)> {
    static RUST: OnceLock<Option<(TsLanguage, Query)>> = OnceLock::new();
    static TS: OnceLock<Option<(TsLanguage, Query)>> = OnceLock::new();
    static TSX: OnceLock<Option<(TsLanguage, Query)>> = OnceLock::new();
    static PYTHON: OnceLock<Option<(TsLanguage, Query)>> = OnceLock::new();
    static GO: OnceLock<Option<(TsLanguage, Query)>> = OnceLock::new();

    const RUST_IDS: &str =
        "[(identifier) (type_identifier) (field_identifier) (shorthand_field_identifier)] @id";
    const TS_IDS: &str = "[(identifier) (property_identifier) (type_identifier) (shorthand_property_identifier) (shorthand_property_identifier_pattern)] @id";
    const PYTHON_IDS: &str = "(identifier) @id";
    const GO_IDS: &str =
        "[(identifier) (field_identifier) (type_identifier) (package_identifier)] @id";

    let (cell, ts_language, query_text) = match language {
        Language::Rust => (&RUST, tree_sitter_rust::LANGUAGE, RUST_IDS),
        Language::TypeScript => (&TS, tree_sitter_typescript::LANGUAGE_TYPESCRIPT, TS_IDS),
        Language::Tsx => (&TSX, tree_sitter_typescript::LANGUAGE_TSX, TS_IDS),
        // Svelte has no grammar here; the outline layer parses `<script>` as TS.
        // Counting identifiers over the whole file with the TS grammar keeps the
        // estimate in the same units, at the cost of markup noise.
        Language::Svelte => (&TS, tree_sitter_typescript::LANGUAGE_TYPESCRIPT, TS_IDS),
        Language::Python => (&PYTHON, tree_sitter_python::LANGUAGE, PYTHON_IDS),
        Language::Go => (&GO, tree_sitter_go::LANGUAGE, GO_IDS),
        Language::Other => return None,
    };

    let cached = cell.get_or_init(|| {
        let ts_language: TsLanguage = ts_language.into();
        Query::new(&ts_language, query_text)
            .ok()
            .map(|query| (ts_language, query))
    });
    cached.as_ref().map(|(language, query)| (language, query))
}

fn percentile(sorted: &[u32], p: f64) -> u32 {
    if sorted.is_empty() {
        return 0;
    }
    let index = ((sorted.len() - 1) as f64 * p).round() as usize;
    sorted[index]
}
