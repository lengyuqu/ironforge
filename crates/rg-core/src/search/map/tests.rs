//! Unit tests for the repository-map budget trimming.

use super::*;
use crate::search::outline::{ItemKind, Language, Outline, OutlineItem};

fn item(name: &str, kind: ItemKind, exported: bool, start: u32) -> OutlineItem {
    OutlineItem {
        name: name.to_string(),
        kind,
        start_line: start,
        end_line: start + 3,
        signature: format!("pub fn {name}(a: u32) -> u32"),
        exported,
        children: Vec::new(),
    }
}

fn outline(language: Language, items: Vec<OutlineItem>) -> Outline {
    Outline {
        language,
        items,
        truncated: false,
        error: None,
    }
}

fn mapped(path: &str, items: Vec<OutlineItem>) -> MappedFile {
    let language = Language::from_path(path);
    MappedFile {
        path: path.to_string(),
        outline: outline(language, items),
    }
}

fn rust_types(count: usize) -> Vec<OutlineItem> {
    (0..count)
        .map(|n| item(&format!("Type{n}"), ItemKind::Struct, true, n as u32 + 1))
        .collect()
}

fn rust_functions(count: usize) -> Vec<OutlineItem> {
    (0..count)
        .map(|n| {
            item(
                &format!("helper{n}"),
                ItemKind::Function,
                false,
                n as u32 + 1,
            )
        })
        .collect()
}

fn many_files() -> Vec<MappedFile> {
    vec![
        mapped("crates/a/src/lib.rs", rust_types(6)),
        mapped("crates/a/src/util.rs", rust_functions(6)),
        mapped("crates/b/src/main.rs", rust_functions(4)),
        mapped("web/src/app.ts", rust_types(3)),
    ]
}

#[test]
fn empty_repository_yields_an_empty_map() {
    let map = build(
        "main",
        Some("abc".into()),
        Vec::new(),
        Vec::new(),
        DEFAULT_BUDGET,
    );
    assert!(map.directories.is_empty());
    assert!(map.files.is_empty());
    assert!(map.skipped.is_empty());
    assert!(!map.truncated);
    assert!(map.estimated_tokens <= DEFAULT_BUDGET);
    assert_eq!(map.git_ref, "main");
}

#[test]
fn only_unsupported_paths_all_go_to_skipped() {
    let skipped = vec![
        MapSkipped::new("README.md", SkipReason::UnsupportedLanguage),
        MapSkipped::new("assets/logo.png", SkipReason::UnsupportedLanguage),
    ];
    let map = build("main", None, Vec::new(), skipped, 200);
    assert!(map.files.is_empty());
    assert!(map.directories.is_empty());
    assert_eq!(map.skipped.len(), 2);
    assert_eq!(map.skipped[0].reason, "unsupported language");
    assert!(
        !map.truncated,
        "rejecting non-source paths is not truncation"
    );
}

#[test]
fn ample_budget_keeps_every_item_and_reports_no_truncation() {
    let map = build("main", None, many_files(), Vec::new(), MAX_BUDGET);
    assert!(!map.truncated);
    assert_eq!(map.files.len(), 4);
    assert_eq!(map.files.iter().map(|f| f.items.len()).sum::<usize>(), 19);
    assert!(map.files.iter().all(|f| !f.items_omitted));
    assert!(map.estimated_tokens <= MAX_BUDGET);
}

#[test]
fn directories_aggregate_counts_and_languages() {
    let map = build("main", None, many_files(), Vec::new(), MAX_BUDGET);
    let dirs: Vec<(&str, usize, Vec<&str>)> = map
        .directories
        .iter()
        .map(|d| (d.path.as_str(), d.files, d.languages.clone()))
        .collect();
    assert_eq!(
        dirs,
        vec![
            ("crates/a/src", 2, vec!["rust"]),
            ("crates/b/src", 1, vec!["rust"]),
            ("web/src", 1, vec!["typescript"]),
        ]
    );
}

#[test]
fn ranking_prefers_type_heavy_files() {
    let map = build("main", None, many_files(), Vec::new(), MAX_BUDGET);
    // Files are ordered by weight: exported structs outrank private helpers.
    assert_eq!(map.files[0].path, "crates/a/src/lib.rs");
    let weights: Vec<&str> = map.files.iter().map(|f| f.path.as_str()).collect();
    let lib = weights
        .iter()
        .position(|p| *p == "crates/a/src/lib.rs")
        .unwrap();
    let util = weights
        .iter()
        .position(|p| *p == "crates/a/src/util.rs")
        .unwrap();
    assert!(lib < util, "type-heavy file must rank first: {weights:?}");
}

#[test]
fn tiny_budget_keeps_the_directory_layer_only() {
    let map = build("main", None, many_files(), Vec::new(), MIN_BUDGET);
    assert!(map.truncated);
    assert!(map.estimated_tokens <= MIN_BUDGET);
    // Items are the first thing to go.
    assert!(map.files.iter().all(|f| f.items.is_empty()));
}

#[test]
fn items_are_dropped_before_the_whole_file_is_dropped() {
    let full = build("main", None, many_files(), Vec::new(), MAX_BUDGET);
    let half = full.estimated_tokens / 2;
    let map = build("main", None, many_files(), Vec::new(), half);

    assert!(map.truncated);
    assert!(map.estimated_tokens <= half);

    // Survivors are the top of the ranking, in order: the walk starts at the
    // least informative file.
    let ranked: Vec<&str> = full.files.iter().map(|f| f.path.as_str()).collect();
    let kept: Vec<&str> = map.files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(
        kept,
        ranked[..kept.len()].to_vec(),
        "kept files must be a prefix of the ranking"
    );

    // No survivor is a bare shell without saying why.
    for file in &map.files {
        assert!(
            !file.items.is_empty() || file.items_omitted,
            "{} lost its items without the flag",
            file.path
        );
    }
    // A file removed entirely never reappears in `files`; it may be named in
    // `skipped` with reason `budget`, but only if the leftover budget covers
    // the entry (content always wins).
    for entry in &map.skipped {
        if entry.reason == "budget" {
            assert!(map.files.iter().all(|f| f.path != entry.path));
        }
    }
}

#[test]
fn a_tight_budget_keeps_a_few_complete_outlines_not_many_shells() {
    // 200 files x 8 exported structs: the whole repository cannot fit, but the
    // answer must still be a readable map, not 200 empty file entries.
    let files: Vec<MappedFile> = (0..200)
        .map(|n| mapped(&format!("src/d{}/f{n}.rs", n % 10), rust_types(8)))
        .collect();
    let map = build("main", None, files, Vec::new(), 2_000);
    assert!(map.estimated_tokens <= 2_000);
    assert!(
        !map.files.is_empty(),
        "a small budget must still buy some map"
    );
    assert!(
        map.files.iter().any(|f| !f.items.is_empty()),
        "the top file keeps its outline"
    );
    assert!(
        map.files
            .iter()
            .all(|f| !f.items.is_empty() || f.items_omitted),
        "an empty survivor must say its items were omitted"
    );
    assert!(map.files.len() < 200);
}

#[test]
fn a_file_only_leaves_the_map_after_its_items_are_gone() {
    // 120 tokens: the directory layer plus a couple of file shells fit, no
    // items do. Everything still listed lost its items first.
    let map = build("main", None, many_files(), Vec::new(), 120);
    assert!(map.estimated_tokens <= 120);
    assert!(map.truncated);
    assert!(map
        .files
        .iter()
        .all(|f| f.items.is_empty() && f.items_omitted));
    for entry in map.skipped.iter().filter(|s| s.reason == "budget") {
        assert!(
            map.files.iter().all(|f| f.path != entry.path),
            "a budget-skipped file is not also listed in files"
        );
    }
}

#[test]
fn signatures_are_never_shortened() {
    for budget in [MIN_BUDGET, 200, 400, 800, 1_600, MAX_BUDGET] {
        let map = build("main", None, many_files(), Vec::new(), budget);
        for file in &map.files {
            for got in &file.items {
                // The fixture's signature is exactly this one line; a trimmed
                // budget must drop items, never shorten the text.
                assert_eq!(
                    got.signature,
                    format!("pub fn {}(a: u32) -> u32", got.name),
                    "budget={budget} {}",
                    file.path
                );
            }
        }
    }
}

#[test]
fn estimated_tokens_never_exceed_the_budget() {
    let files = many_files();
    for budget in (MIN_BUDGET..=2_000).step_by(7) {
        let map = build("main", None, files.clone(), Vec::new(), budget);
        assert!(
            map.estimated_tokens <= budget,
            "budget={budget} estimated={}",
            map.estimated_tokens
        );
        assert_eq!(map.estimated_tokens, map.measure());
    }
}

#[test]
fn a_budget_equal_to_the_full_estimate_is_not_truncated() {
    let full = build("main", None, many_files(), Vec::new(), MAX_BUDGET);
    let exact = build(
        "main",
        None,
        many_files(),
        Vec::new(),
        full.estimated_tokens,
    );
    assert_eq!(exact.estimated_tokens, full.estimated_tokens);
    assert!(!exact.truncated);
    assert_eq!(exact.files, full.files);
}

#[test]
fn outline_errors_without_items_become_skipped_entries() {
    let broken = MappedFile {
        path: "src/generated.rs".into(),
        outline: Outline {
            language: Language::Rust,
            items: Vec::new(),
            truncated: false,
            error: Some("file too large".into()),
        },
    };
    let map = build("main", None, vec![broken], Vec::new(), DEFAULT_BUDGET);
    assert!(map.files.is_empty());
    assert_eq!(map.skipped.len(), 1);
    assert_eq!(map.skipped[0].reason, "file too large");
    assert!(!map.truncated);
}

#[test]
fn svelte_is_flagged_as_a_degraded_outline() {
    let map = build(
        "main",
        None,
        vec![mapped("web/src/App.svelte", rust_functions(1))],
        Vec::new(),
        DEFAULT_BUDGET,
    );
    let notice = map.files[0].notice.as_deref().unwrap_or_default();
    assert!(notice.contains("svelte"), "{notice}");
    assert!(notice.contains("markup"), "{notice}");
}

#[test]
fn budget_is_clamped_into_the_supported_range() {
    let low = build("main", None, Vec::new(), Vec::new(), 1);
    assert_eq!(low.budget, MIN_BUDGET);
    let high = build("main", None, Vec::new(), Vec::new(), usize::MAX);
    assert_eq!(high.budget, MAX_BUDGET);
}

#[test]
fn the_serialised_body_is_within_four_chars_per_token_of_the_estimate() {
    let map = build(
        "main",
        Some("deadbeef".into()),
        many_files(),
        Vec::new(),
        900,
    );
    let body = serde_json::to_string(&map).unwrap();
    assert_eq!(map.estimated_tokens, body.len().div_ceil(CHARS_PER_TOKEN));
}
